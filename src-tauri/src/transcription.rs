use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::{
    checkpoint,
    cookies::CookieJar,
    history, models,
    paths::{engine_path_cancelable, is_regular_file, jobs_dir, model_path, tool_path},
    process, resources,
    sources::{
        local::{local_path, validate_source},
        source_js_runtime_args, validate_media_duration,
    },
    system::{
        configure_cuda_command, cuda_driver_is_compatible, detect_gpu, detect_nvidia,
        detect_nvidia_device, detect_nvidia_driver_version, ensure_cuda_install_supported,
        UsageMonitor,
    },
    types::{
        ComputeDeviceInfo, ProgressPayload, Segment, TranscriptRequest, TranscriptResult,
        MAX_TRANSCRIPT_RESULT_BYTES, MAX_TRANSCRIPT_SEGMENTS, MAX_TRANSCRIPT_TEXT_BYTES,
    },
};

#[derive(Clone, Copy)]
struct JobContext<'a> {
    app: &'a AppHandle,
    usage: &'a UsageMonitor,
    active_pid: &'a Arc<Mutex<Option<u32>>>,
    cancelled: &'a Arc<AtomicBool>,
    capability_probe: &'a Arc<Mutex<HashSet<String>>>,
    progress_start: f64,
    progress_end: f64,
}

struct ProgressUpdate<'a> {
    stage: &'a str,
    message_code: &'static str,
    percent: f64,
    message: String,
    backend: Option<&'a str>,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    network_bytes_per_second: Option<u64>,
}

struct WhisperConfig<'a> {
    language: &'a str,
    backend: &'a str,
    compute_device_id: Option<&'a str>,
    model_id: &'a str,
}

impl JobContext<'_> {
    fn emit(&self, update: ProgressUpdate<'_>) {
        let usage_snapshot = self.usage.snapshot();
        let _ = self.app.emit(
            "job-progress",
            ProgressPayload {
                stage: update.stage.to_string(),
                message_code: update.message_code.into(),
                percent: if update.stage == "transcribing" {
                    self.progress_start
                        + (self.progress_end - self.progress_start)
                            * update.percent.clamp(0.0, 100.0)
                            / 100.0
                } else {
                    update.percent.clamp(0.0, 100.0)
                },
                message: update.message,
                backend: update.backend.map(str::to_string),
                downloaded_bytes: update.downloaded_bytes,
                total_bytes: update.total_bytes,
                network_bytes_per_second: update.network_bytes_per_second,
                cpu_usage_percent: usage_snapshot.cpu_usage_percent,
                gpu_usage_percent: usage_snapshot.gpu_usage_percent,
            },
        );
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

fn process_failed(cancelled: &Arc<AtomicBool>, stderr: String, stage: &str) -> String {
    if cancelled.load(Ordering::SeqCst) {
        "Job dibatalkan.".into()
    } else if stderr.trim().is_empty() {
        format!("Process {stage} gagal tanpa detail error.")
    } else {
        format!("{stage}: {}", stderr.trim())
    }
}

fn normalize_utf8_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn normalize_output_utf8(path: &Path, max_bytes: u64) -> Result<(), String> {
    let bytes = read_bounded_file(path, max_bytes, "output Whisper")?;
    let normalized = normalize_utf8_bytes(&bytes);
    if normalized.as_bytes() != bytes.as_slice() {
        fs::write(path, normalized.as_bytes())
            .map_err(|e| format!("Gagal menormalkan output Whisper: {e}"))?;
    }
    Ok(())
}

fn read_bounded_file(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("Gagal membuka {label}: {e}"))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024) as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Gagal membaca {label}: {e}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("{label} melebihi batas ukuran aman."));
    }
    Ok(bytes)
}

struct JobDirectoryGuard {
    path: PathBuf,
    marker: PathBuf,
    committed: bool,
    preserve_on_failure: bool,
}

impl JobDirectoryGuard {
    fn create(path: PathBuf) -> Result<Self, String> {
        crate::durable::create_private_dir_all(&path)
            .map_err(|e| format!("Gagal membuat job directory: {e}"))?;
        let marker = path.join(".in-progress");
        if let Err(error) = File::create(&marker) {
            let _ = fs::remove_dir_all(&path);
            return Err(format!("Gagal membuat marker job: {error}"));
        }
        Ok(Self {
            path,
            marker,
            committed: false,
            preserve_on_failure: false,
        })
    }

    fn resume(path: PathBuf) -> Result<Self, String> {
        if !path.is_dir() {
            return Err("Folder cache audio tidak valid.".into());
        }
        crate::durable::create_private_dir_all(&path)
            .map_err(|e| format!("Gagal mengamankan folder cache audio: {e}"))?;
        let marker = path.join(".in-progress");
        if !fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.is_file()) {
            return Err("Cache audio tidak memiliki marker job yang valid.".into());
        }
        Ok(Self {
            path,
            marker,
            committed: false,
            preserve_on_failure: true,
        })
    }

    fn preserve_on_failure(&mut self) {
        self.preserve_on_failure = true;
    }

    fn commit(mut self) {
        self.committed = true;
        if let Err(error) = fs::remove_file(&self.marker) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("WhisperTube job marker cleanup warning: {error}");
            }
        }
    }
}

impl Drop for JobDirectoryGuard {
    fn drop(&mut self) {
        if !self.committed && !self.preserve_on_failure {
            if let Err(error) = fs::remove_dir_all(&self.path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "WhisperTube failed job cleanup warning ({}): {error}",
                        self.path.display()
                    );
                }
            }
        }
        history::invalidate_job_storage_cache();
    }
}

fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let result = (|| -> Result<(), String> {
        let mut file = File::create(&temporary)
            .map_err(|e| format!("Gagal membuat file sementara result: {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("Gagal menulis result sementara: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Gagal flush result sementara: {e}"))?;
        drop(file);
        crate::durable::publish(&temporary, path)
            .map_err(|e| format!("Gagal mengaktifkan result.json: {e}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_file_if_present(path: &Path, label: &str) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Gagal menghapus {label}: {error}")),
    }
}

#[derive(Deserialize, Serialize)]
struct AudioCacheMetadata {
    #[serde(default)]
    audio_recovered: bool,
    version: u8,
    url: String,
    #[serde(default)]
    title: String,
    duration: f64,
    #[serde(default = "audio_ready_default")]
    audio_ready: bool,
}

fn audio_ready_default() -> bool {
    true
}

fn write_audio_cache_metadata(
    job_dir: &Path,
    request: &TranscriptRequest,
    audio_ready: bool,
) -> Result<(), String> {
    write_audio_cache_metadata_with_recovery(job_dir, request, audio_ready, false)
}

fn write_audio_cache_metadata_with_recovery(
    job_dir: &Path,
    request: &TranscriptRequest,
    audio_ready: bool,
    audio_recovered: bool,
) -> Result<(), String> {
    let metadata = AudioCacheMetadata {
        audio_recovered,
        version: AUDIO_CACHE_METADATA_VERSION,
        url: request.url.clone(),
        title: request.title.clone(),
        duration: request.duration,
        audio_ready,
    };
    let bytes = serde_json::to_vec(&metadata)
        .map_err(|error| format!("Gagal serialize metadata cache audio: {error}"))?;
    write_file_atomically(&job_dir.join(AUDIO_CACHE_METADATA_FILE), &bytes)
}

fn read_audio_cache_metadata(path: &Path) -> Option<AudioCacheMetadata> {
    if !is_regular_file(path) {
        return None;
    }
    let bytes =
        read_bounded_file(path, MAX_AUDIO_CACHE_METADATA_BYTES, "metadata cache audio").ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn cached_job_source(job_dir: &Path) -> Option<(String, String)> {
    let metadata = read_audio_cache_metadata(&job_dir.join(AUDIO_CACHE_METADATA_FILE))?;
    Some((metadata.title.chars().take(256).collect(), metadata.url))
}

fn find_reusable_audio_job(
    jobs_dir: &Path,
    request: &mut TranscriptRequest,
    committed_jobs: &HashSet<PathBuf>,
) -> Result<Option<(PathBuf, bool)>, String> {
    let job = find_reusable_audio_job_for_source(
        jobs_dir,
        &request.url,
        request.duration,
        committed_jobs,
    )?;
    Ok(job.map(|(path, cache)| {
        if cache.audio_ready {
            // Checkpoints belong to the measured audio duration, which may
            // differ slightly from the metadata sent again by the frontend.
            request.duration = cache.duration;
        }
        (path, cache.audio_ready)
    }))
}

fn find_reusable_audio_job_for_source(
    jobs_dir: &Path,
    url: &str,
    duration: f64,
    committed_jobs: &HashSet<PathBuf>,
) -> Result<Option<(PathBuf, AudioCacheMetadata)>, String> {
    let canonical_root = fs::canonicalize(jobs_dir)
        .map_err(|error| format!("Gagal memvalidasi folder cache audio: {error}"))?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&canonical_root)
        .map_err(|error| format!("Gagal membaca folder cache audio: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Gagal membaca cache audio: {error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Gagal membaca tipe cache audio: {error}"))?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let canonical_job_dir = match fs::canonicalize(&path) {
            Ok(path) if path.parent() == Some(canonical_root.as_path()) => path,
            _ => continue,
        };
        if !fs::symlink_metadata(canonical_job_dir.join(".in-progress"))
            .is_ok_and(|metadata| metadata.is_file())
        {
            continue;
        }
        let Some(cache) =
            read_audio_cache_metadata(&canonical_job_dir.join(AUDIO_CACHE_METADATA_FILE))
        else {
            continue;
        };
        if !matches!(cache.version, 1 | AUDIO_CACHE_METADATA_VERSION)
            || validate_source(&cache.url).ok().as_deref() != Some(url)
            || !cache.duration.is_finite()
            || cache.duration <= 0.0
            || cache.duration > MAX_MEDIA_DURATION_SECONDS
            || (cache.duration - duration).abs() > MEDIA_DURATION_TOLERANCE_SECONDS
        {
            continue;
        }
        // SQLite decides whether the job committed. A result file alone may
        // have been written immediately before a transaction rolled back.
        if committed_jobs.contains(&canonical_job_dir) {
            continue;
        }
        if cache.audio_ready {
            let audio = canonical_job_dir.join("audio.wav");
            if checkpoint::PcmAudio::inspect(&audio, cache.duration, || false).is_err() {
                continue;
            }
        }
        let modified = fs::metadata(&canonical_job_dir)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, canonical_job_dir, cache));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, path, cache)| (path, cache)))
}

fn resume_stage_in(
    root: &Path,
    url: &str,
    duration: f64,
    committed: &HashSet<PathBuf>,
) -> Result<Option<crate::types::TranscriptionResumeStage>, String> {
    use crate::types::TranscriptionResumeStage;
    let Some((job, cache)) = find_reusable_audio_job_for_source(root, url, duration, committed)?
    else {
        return Ok(None);
    };
    if cache.audio_ready {
        return Ok(Some(TranscriptionResumeStage::Audio));
    }
    for entry in fs::read_dir(&job).map_err(|e| format!("Gagal membaca progres unduhan: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca progres unduhan: {e}"))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|e| format!("Gagal memeriksa progres unduhan: {e}"))?;
        if entry.file_name().to_string_lossy().starts_with("source.")
            && metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() > 0
        {
            return Ok(Some(TranscriptionResumeStage::Download));
        }
    }
    Ok(None)
}

// Advisory UI lookup: use the pipeline's own candidate selection, without
// hashing full audio or promising any completed inference checkpoints. The
// pipeline revalidates audio and checkpoint settings when the user continues.
pub fn resume_stage(
    app: &AppHandle,
    url: &str,
    duration: f64,
) -> Result<Option<crate::types::TranscriptionResumeStage>, String> {
    let url = validate_source(url)?;
    validate_media_duration(duration, false)?;
    resume_stage_in(
        &jobs_dir(app)?,
        &url,
        duration,
        &history::referenced_job_directories(app)?,
    )
}

const MAX_CAPTURED_STDERR_BYTES: usize = 64 * 1024;
pub(crate) const MAX_MEDIA_DURATION_SECONDS: f64 = 8.0 * 60.0 * 60.0;
pub(crate) const MEDIA_DURATION_TOLERANCE_SECONDS: f64 = 2.0;
const MAX_MEDIA_DOWNLOAD_BYTES: &str = "4G";
const MAX_MEDIA_DOWNLOAD_BYTES_VALUE: u64 = 4 * 1024 * 1024 * 1024;
const AUDIO_CACHE_METADATA_FILE: &str = ".audio-cache.json";
const AUDIO_CACHE_METADATA_VERSION: u8 = 2;
const MAX_AUDIO_CACHE_METADATA_BYTES: u64 = 64 * 1024;
const DISK_SAFETY_BUFFER_BYTES: u64 = 512 * 1024 * 1024;
const DISK_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const PROCESS_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const CPU_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const MINIMUM_AUDIO_DURATION_SECONDS: f64 = 1.0;
const MAX_REQUEST_TITLE_BYTES: usize = 4 * 1024;
const MAX_REQUEST_CHANNEL_BYTES: usize = 2 * 1024;
const MAX_REQUEST_LANGUAGE_BYTES: usize = 16;
const MAX_REQUEST_DEVICE_ID_BYTES: usize = 64;
const MAX_TIMESTAMP_BYTES: usize = 64;

fn validate_transcript_request(request: &TranscriptRequest) -> Result<(), String> {
    if request.title.len() > MAX_REQUEST_TITLE_BYTES {
        return Err("Judul video terlalu panjang.".into());
    }
    if request.channel.len() > MAX_REQUEST_CHANNEL_BYTES {
        return Err("Nama channel terlalu panjang.".into());
    }
    if request.language.len() > MAX_REQUEST_LANGUAGE_BYTES
        || !matches!(
            request.language.as_str(),
            "auto" | "id" | "en" | "zh" | "ja" | "ko"
        )
    {
        return Err("Bahasa transkripsi tidak didukung.".into());
    }
    if !matches!(
        request.backend.as_str(),
        "auto" | "cpu" | "cuda" | "metal" | "vulkan"
    ) {
        return Err("Compute backend tidak dikenal.".into());
    }
    if request
        .compute_device_id
        .as_deref()
        .is_some_and(|value| value.len() > MAX_REQUEST_DEVICE_ID_BYTES)
    {
        return Err("Target compute terlalu panjang.".into());
    }
    if request
        .cookies_path
        .as_deref()
        .is_some_and(|value| value.len() > 32 * 1024)
    {
        return Err("Path cookies.txt terlalu panjang.".into());
    }
    if crate::qwen::is_qwen(&request.model_id)
        && !crate::qwen::supported_backends().contains(&request.backend.as_str())
    {
        return Err("Backend Qwen tidak tersedia pada platform ini. Pilih backend yang didukung runtime Qwen.".into());
    }
    Ok(())
}

fn pcm_wav_bytes(duration: f64) -> u64 {
    (duration.max(0.0) + MEDIA_DURATION_TOLERANCE_SECONDS)
        .min(MAX_MEDIA_DURATION_SECONDS)
        .ceil() as u64
        * 16_000
        * 2
        + 44
}

fn validate_audio_wav(path: &Path, duration: f64) -> Result<u64, String> {
    if !is_regular_file(path) {
        return Err("Audio hasil konversi tidak ditemukan.".into());
    }
    let size = fs::metadata(path)
        .map_err(|error| format!("Gagal membaca ukuran WAV hasil konversi: {error}"))?
        .len();
    if size <= 44 {
        return Err("Audio hasil konversi kosong atau tidak lengkap.".into());
    }
    if size > pcm_wav_bytes(duration).saturating_add(4096) {
        return Err("WAV hasil konversi melebihi batas durasi media yang diizinkan.".into());
    }
    Ok(size)
}

fn reconciled_audio_duration(audio: &checkpoint::PcmAudio, source: f64) -> (f64, Option<f64>) {
    let actual = audio.duration_seconds();
    // A shorter audio track is legitimate. Never claim it covered the source
    // metadata's full duration: retain that discrepancy for the result UI.
    let source = ((source - actual).abs() > MEDIA_DURATION_TOLERANCE_SECONDS).then_some(source);
    (actual, source)
}

fn media_disk_reservation(duration: f64, local_audio: bool) -> u64 {
    let download_bytes = if local_audio {
        0
    } else {
        MAX_MEDIA_DOWNLOAD_BYTES_VALUE
    };
    download_bytes
        .saturating_add(pcm_wav_bytes(duration))
        .saturating_add(DISK_SAFETY_BUFFER_BYTES)
}

fn remaining_disk_reservation(already_written: u64, maximum: u64, future: u64) -> u64 {
    maximum
        .saturating_sub(already_written)
        .saturating_add(future)
        .saturating_add(DISK_SAFETY_BUFFER_BYTES)
}

fn directory_file_bytes(path: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    for entry in fs::read_dir(path).map_err(|e| format!("Gagal memeriksa pemakaian disk: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry pemakaian disk: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Gagal membaca tipe entry pemakaian disk: {e}"))?;
        if file_type.is_symlink() {
            return Err("Folder job berisi symbolic link yang tidak diizinkan.".into());
        }
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Gagal membaca metadata pemakaian disk: {e}"))?;
        if file_type.is_file() {
            total = total.saturating_add(metadata.len());
        } else if file_type.is_dir() {
            total = total.saturating_add(directory_file_bytes(&entry.path())?);
        }
    }
    Ok(total)
}

fn ensure_job_storage_quota(app: &AppHandle) -> Result<(), String> {
    let used = history::job_storage_bytes(app)?;
    ensure_job_storage_budget(used, history::MAX_JOB_STORAGE_BYTES)
}

pub(crate) fn ensure_job_storage_budget(used: u64, limit: u64) -> Result<(), String> {
    if used >= limit {
        return Err(format!(
            "storage_quota_exceeded:Penyimpanan job WhisperTube sudah melewati batas {:.1} GB.",
            limit as f64 / 1024_f64.powi(3),
        ));
    }
    Ok(())
}

fn ffmpeg_target_duration(duration: f64) -> f64 {
    // Read beyond the accepted window. Capping at the metadata duration would
    // silently truncate a longer track and produce a seemingly complete WAV.
    // The extra second is a sentinel: validators reject it before readiness.
    (duration + MEDIA_DURATION_TOLERANCE_SECONDS + 1.0).clamp(
        MINIMUM_AUDIO_DURATION_SECONDS,
        MAX_MEDIA_DURATION_SECONDS + 1.0,
    )
}

fn whisper_inactivity_timeout(backend: &str) -> std::time::Duration {
    if backend == "cpu" {
        CPU_INACTIVITY_TIMEOUT
    } else {
        PROCESS_INACTIVITY_TIMEOUT
    }
}

fn whisper_thread_count(available: usize, backend: &str) -> usize {
    if backend == "cpu" {
        available.clamp(1, 16)
    } else {
        available.clamp(1, 12)
    }
}

fn configure_vulkan_command(command: &mut Command) {
    // AMD Windows drivers, including Radeon 780M, can expose cooperative
    // matrices while failing during Whisper inference through the Vulkan
    // shader path. Keep Vulkan cross-vendor compatibility ahead of that
    // optional optimization; the engine still uses the selected GPU.
    command.env("GGML_VK_DISABLE_COOPMAT", "1");
}

fn media_command_error(
    error: String,
    program: &std::ffi::OsStr,
    stderr: &[u8],
    cancelled: bool,
) -> String {
    if crate::io_cancel::is_cancellation_error(&error) {
        return error;
    }
    let detail = String::from_utf8_lossy(stderr);
    let label = Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy();
    if cancelled || detail.trim().is_empty() {
        format!("{label}: {error}")
    } else {
        format!("{label}: {error} Detail: {}", detail.trim())
    }
}

fn run_media_command(
    context: &JobContext,
    command: &mut Command,
    primary: process::OutputStream,
    inactivity_timeout: std::time::Duration,
    timeout: Option<std::time::Duration>,
    mut on_line: impl FnMut(&str) -> Result<(), String>,
) -> Result<(ExitStatus, String), String> {
    let mut lines = process::OutputLines::default();
    let mut stderr = Vec::with_capacity(MAX_CAPTURED_STDERR_BYTES);
    let mut truncated = false;
    let mut last_activity = Instant::now();
    let mut monitor = None;
    let mut active = None;
    let outcome = process::run_piped(
        command,
        timeout,
        Some(context.cancelled),
        |pid| {
            active = Some(process::ActiveProcessGuard::new(context.active_pid, pid)?);
            monitor = Some(context.usage.track_process(pid));
            Ok(())
        },
        |event| {
            match event {
                process::OutputEvent::Data(stream, bytes) => {
                    if stream == process::OutputStream::Stderr {
                        let retained = bytes
                            .len()
                            .min(MAX_CAPTURED_STDERR_BYTES.saturating_sub(stderr.len()));
                        stderr.extend_from_slice(&bytes[..retained]);
                        truncated |= retained < bytes.len();
                    }
                    if stream == primary {
                        last_activity = Instant::now();
                        lines.push(bytes, |line| on_line(&line))?;
                    }
                }
                process::OutputEvent::Tick => {
                    if let Some(error) = context.usage.memory_pressure() {
                        return Err(error);
                    }
                    if last_activity.elapsed() >= inactivity_timeout {
                        return Err(format!(
                            "Process tidak menghasilkan progress selama {} detik dan dihentikan.",
                            inactivity_timeout.as_secs()
                        ));
                    }
                }
            }
            Ok(())
        },
    );
    drop(active);
    let status = outcome.map_err(|error| {
        media_command_error(
            error,
            command.get_program(),
            &stderr,
            context.is_cancelled(),
        )
    })?;
    if let Some(line) = lines.finish() {
        on_line(&line)?;
    }
    let mut stderr = String::from_utf8_lossy(&stderr).into_owned();
    if truncated {
        stderr.push_str("\n[stderr dipotong karena terlalu panjang]");
    }
    Ok((status, stderr))
}

fn download_needs_duration_probe(value: &str, inspected: f64) -> Result<bool, String> {
    let Some(duration) = value
        .parse::<f64>()
        .ok()
        .filter(|duration| duration.is_finite() && *duration > 0.0)
    else {
        return Ok(true);
    };
    validate_media_duration(duration, false)?;
    if (duration - inspected).abs() > MEDIA_DURATION_TOLERANCE_SECONDS {
        return Err(
            "Durasi audio berubah setelah pemeriksaan. Periksa ulang URL sebelum transkripsi."
                .into(),
        );
    }
    Ok(false)
}

fn run_download(
    context: &JobContext,
    request: &TranscriptRequest,
    job_dir: &Path,
) -> Result<(PathBuf, bool), String> {
    let yt_dlp = tool_path(context.app, "yt-dlp")?;
    let probe_eligible = crate::sources::supports_duration_probe(&request.url);
    // Fail closed if the marker is absent. A known extractor duration keeps
    // the existing long-VOD path, without an extra full decode under 10 seconds.
    let mut needs_duration_probe = probe_eligible;
    let template = job_dir.join("source.%(ext)s");
    let mut command = Command::new(yt_dlp);
    process::hide_console(&mut command);
    let cookie_jar =
        CookieJar::prepare(context.app, request.cookies_path.as_deref()).map_err(|error| {
            if request.cookies_path.is_some() {
                format!("media_source_cookie_file:{error}")
            } else {
                error
            }
        })?;
    command
        .args(["--ignore-config", "--continue", "--part", "--no-playlist", "--match-filter"])
        .arg(crate::sources::download_media_filter(&request.url))
        .args([
            "--max-filesize",
            MAX_MEDIA_DOWNLOAD_BYTES,
            "--newline",
            "--quiet",
            "--progress",
            "--progress-template",
            "download:WT_PROGRESS=%(progress._percent_str)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s",
            "-f",
            "bestaudio/best",
            "-o",
        ])
        .arg(&template)
        .args(source_js_runtime_args(&request.url, context.active_pid, context.cancelled)?)
        .args(cookie_jar.args())
        .args(if probe_eligible {
            vec!["--no-simulate", "--print", "before_dl:WT_SOURCE_DURATION=%(duration|NA)s"]
        } else { vec![] })
        .arg(&request.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let progress_re =
        Regex::new(r"WT_PROGRESS=\s*([0-9.]+)%\|([0-9]+|NA)\|([0-9]+|NA)\|([0-9]+|NA)\|([^\s|]+)")
            .unwrap();
    let mut last_disk_check = Instant::now() - DISK_CHECK_INTERVAL;
    let (status, stderr) = run_media_command(
        context,
        &mut command,
        process::OutputStream::Stdout,
        PROCESS_INACTIVITY_TIMEOUT,
        None,
        |line| {
            if probe_eligible {
                if let Some(value) = line.strip_prefix("WT_SOURCE_DURATION=") {
                    needs_duration_probe = download_needs_duration_probe(value, request.duration)?;
                }
            }
            if let Some(caps) = progress_re.captures(line) {
                let downloaded_bytes = caps.get(2).and_then(|value| value.as_str().parse().ok());
                let total_bytes = caps
                    .get(3)
                    .and_then(|value| value.as_str().parse().ok())
                    .or_else(|| caps.get(4).and_then(|value| value.as_str().parse().ok()));
                // yt-dlp subtracts the resume offset when calculating speed.
                // downloaded_bytes includes cached bytes and cannot be used
                // as the numerator of this invocation's transfer rate.
                let network_bytes_per_second =
                    caps.get(5).and_then(|value| download_speed(value.as_str()));
                if let Ok(percent) = caps[1].parse::<f64>() {
                    context.emit(ProgressUpdate {
                        stage: "downloading",
                        message_code: "downloading_audio",
                        percent,
                        message: "Mengunduh best available audio dari YouTube…".into(),
                        backend: None,
                        downloaded_bytes,
                        total_bytes,
                        network_bytes_per_second,
                    });
                }
            }
            if last_disk_check.elapsed() >= DISK_CHECK_INTERVAL {
                let written = directory_file_bytes(job_dir)?;
                if written > MAX_MEDIA_DOWNLOAD_BYTES_VALUE {
                    return Err("Download media melebihi batas ukuran yang diizinkan.".into());
                }
                resources::require_disk(
                    job_dir,
                    remaining_disk_reservation(
                        written,
                        MAX_MEDIA_DOWNLOAD_BYTES_VALUE,
                        pcm_wav_bytes(request.duration),
                    ),
                    "download dan konversi media",
                )?;
                ensure_job_storage_quota(context.app)?;
                last_disk_check = Instant::now();
            }
            Ok(())
        },
    )?;
    if !status.success() {
        return Err(process_failed(context.cancelled, stderr, "Download"));
    }

    let mut candidates = fs::read_dir(job_dir)
        .map_err(|e| format!("Gagal membaca folder job: {e}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.starts_with("source.")
                        && !name.ends_with(".part")
                        && !name.ends_with(".ytdl")
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    candidates.sort();
    let source = candidates
        .into_iter()
        .next()
        .ok_or("yt-dlp selesai tetapi file audio sumber tidak ditemukan.")?;
    let source_size = fs::metadata(&source)
        .map_err(|e| format!("Gagal membaca ukuran audio sumber: {e}"))?
        .len();
    if source_size > MAX_MEDIA_DOWNLOAD_BYTES_VALUE {
        return Err("Audio sumber melebihi batas ukuran download yang diizinkan.".into());
    }
    Ok((source, needs_duration_probe))
}

fn download_speed(raw: &str) -> Option<u64> {
    raw.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as u64)
}

fn ffmpeg_command(ffmpeg: &Path, input: &Path, output: &Path, duration: f64) -> Command {
    ffmpeg_decode_command(ffmpeg, input, output, duration, false)
}

fn ffmpeg_decode_command(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    duration: f64,
    recovering: bool,
) -> Command {
    let mut command = Command::new(ffmpeg);
    crate::sources::local::configure_decode(&mut command, recovering);
    process::hide_console(&mut command);
    command
        .args([
            "-y",
            "-nostdin",
            "-nostats",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(input)
        // Match the first audio stream inspected by the duration probe.
        .args(["-map", "0:a:0", "-t"])
        .arg(format!("{:.3}", ffmpeg_target_duration(duration)))
        .args(["-af", "apad=whole_dur=1"])
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    command
}

fn run_ffmpeg(
    context: &JobContext,
    input: &Path,
    output: &Path,
    duration: f64,
    allow_recovery: bool,
) -> Result<bool, String> {
    let ffmpeg = tool_path(context.app, "ffmpeg")?;
    for recovering in [false, true] {
        let mut command = if recovering {
            ffmpeg_decode_command(&ffmpeg, input, output, duration, true)
        } else {
            ffmpeg_command(&ffmpeg, input, output, duration)
        };
        let mut last_disk_check = Instant::now() - DISK_CHECK_INTERVAL;
        let (status, stderr) = run_media_command(
            context,
            &mut command,
            process::OutputStream::Stdout,
            PROCESS_INACTIVITY_TIMEOUT,
            None,
            |line| {
                if let Some(raw) = line.strip_prefix("out_time_us=") {
                    if let Ok(microseconds) = raw.parse::<f64>() {
                        let seconds = microseconds / 1_000_000.0;
                        let percent = if duration > 0.0 {
                            seconds / duration * 100.0
                        } else {
                            0.0
                        };
                        context.emit(ProgressUpdate {
                            stage: "converting",
                            message_code: "converting_audio",
                            percent,
                            message: "Konversi ke PCM 16 kHz mono…".into(),
                            backend: None,
                            downloaded_bytes: None,
                            total_bytes: None,
                            network_bytes_per_second: None,
                        });
                    }
                }
                if last_disk_check.elapsed() >= DISK_CHECK_INTERVAL {
                    let output_dir = output.parent().unwrap_or_else(|| Path::new("."));
                    let written = fs::metadata(output)
                        .map(|metadata| metadata.len())
                        .unwrap_or(0);
                    let maximum_wav = pcm_wav_bytes(duration);
                    if written > maximum_wav.saturating_add(4096) {
                        return Err(
                            "WAV hasil konversi melebihi batas durasi media yang diizinkan.".into(),
                        );
                    }
                    resources::require_disk(
                        output_dir,
                        remaining_disk_reservation(written, maximum_wav, 0),
                        "konversi media",
                    )?;
                    ensure_job_storage_quota(context.app)?;
                    last_disk_check = Instant::now();
                }
                Ok(())
            },
        )?;
        if !status.success() {
            if !recovering
                && allow_recovery
                && crate::sources::local::can_recover_mp3(input, &stderr)
            {
                crate::io_cancel::check(context.cancelled)?;
                continue;
            }
            return Err(process_failed(context.cancelled, stderr, "FFmpeg"));
        }
        return Ok(recovering);
    }
    unreachable!("both decode attempts return or fail")
}

const CAPABILITY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn write_probe_wav(path: &Path) -> Result<(), String> {
    const SAMPLE_RATE: u32 = 16_000;
    const CHANNELS: u16 = 1;
    const BITS_PER_SAMPLE: u16 = 16;
    const SAMPLE_COUNT: u32 = SAMPLE_RATE;
    let data_bytes = SAMPLE_COUNT * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE / 8);
    let mut file =
        File::create(path).map_err(|e| format!("Gagal membuat input probe Vulkan: {e}"))?;
    file.write_all(b"RIFF")
        .and_then(|_| file.write_all(&(36 + data_bytes).to_le_bytes()))
        .and_then(|_| file.write_all(b"WAVEfmt "))
        .and_then(|_| file.write_all(&16u32.to_le_bytes()))
        .and_then(|_| file.write_all(&1u16.to_le_bytes()))
        .and_then(|_| file.write_all(&CHANNELS.to_le_bytes()))
        .and_then(|_| file.write_all(&SAMPLE_RATE.to_le_bytes()))
        .and_then(|_| {
            file.write_all(
                &(SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS_PER_SAMPLE / 8)).to_le_bytes(),
            )
        })
        .and_then(|_| file.write_all(&(CHANNELS * (BITS_PER_SAMPLE / 8)).to_le_bytes()))
        .and_then(|_| file.write_all(&BITS_PER_SAMPLE.to_le_bytes()))
        .and_then(|_| file.write_all(b"data"))
        .and_then(|_| file.write_all(&data_bytes.to_le_bytes()))
        .and_then(|_| file.write_all(&vec![0u8; data_bytes as usize]))
        .map_err(|e| format!("Gagal menulis input probe Vulkan: {e}"))
}

fn vulkan_probe_signature(engine: &Path, model: &Path) -> String {
    fn file_signature(path: &Path) -> String {
        let metadata = fs::metadata(path);
        let length = metadata.as_ref().map(|value| value.len()).unwrap_or(0);
        let modified = metadata
            .and_then(|value| value.modified())
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| format!("{}:{}", value.as_secs(), value.subsec_nanos()))
            .unwrap_or_else(|| "unknown".into());
        format!("{}:{length}:{modified}", path.display())
    }

    format!(
        "engine={}|model={}",
        file_signature(engine),
        file_signature(model)
    )
}

fn run_engine_probe(
    context: &JobContext,
    backend: &str,
    engine: &Path,
    model: &Path,
    device_index: Option<usize>,
) -> Result<(), String> {
    let probe_dir =
        std::env::temp_dir().join(format!("whispertube-{backend}-probe-{}", Uuid::new_v4()));
    fs::create_dir_all(&probe_dir)
        .map_err(|e| format!("Gagal membuat folder probe {backend}: {e}"))?;
    let wav = probe_dir.join("silence.wav");
    let output = probe_dir.join("probe");
    let result = (|| -> Result<(), String> {
        write_probe_wav(&wav)?;
        let mut command = Command::new(engine);
        process::hide_console(&mut command);
        let probe_device_index = if backend == "cuda" {
            let index = device_index.ok_or_else(|| {
                "Capability probe CUDA tidak menerima indeks perangkat NVIDIA.".to_string()
            })?;
            let gpu = detect_nvidia_device(index)
                .ok_or_else(|| format!("NVIDIA device {index} tidak lagi terdeteksi."))?;
            configure_cuda_command(&mut command, &gpu)?;
            Some(0)
        } else {
            device_index
        };
        if backend == "vulkan" {
            configure_vulkan_command(&mut command);
        }
        command
            .args(["-m"])
            .arg(model)
            .args(["-f"])
            .arg(&wav)
            // Keep -np off: the pinned CLI disables the whisper log callback
            // with that flag, including the GPU backend attestation we need.
            .args(["-l", "auto", "-t", "1", "-nt"]);
        if backend == "vulkan" {
            command.arg("-nfa");
        }
        if let Some(device_index) = probe_device_index {
            command.arg("-dev").arg(device_index.to_string());
        }
        command
            .args(["-of"])
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let (status, stderr) = run_media_command(
            context,
            &mut command,
            process::OutputStream::Stderr,
            CAPABILITY_PROBE_TIMEOUT,
            Some(CAPABILITY_PROBE_TIMEOUT),
            |_| Ok(()),
        )?;
        if !status.success() {
            return Err(process_failed(
                context.cancelled,
                stderr,
                &format!("Capability probe {backend}"),
            ));
        }
        require_gpu_backend_attestation(&stderr, backend, probe_device_index)?;
        Ok(())
    })();
    if let Err(error) = fs::remove_dir_all(&probe_dir) {
        eprintln!("WhisperTube backend probe cleanup warning: {error}");
    }
    result
}

pub(crate) fn require_gpu_backend_attestation(
    stderr: &str,
    backend: &str,
    device_index: Option<usize>,
) -> Result<(), String> {
    let backend = backend.to_ascii_lowercase();
    let backend_prefix = if backend == "metal" { "mtl" } else { &backend };
    let expected_device = device_index.map(|index| format!("{backend_prefix}{index}"));
    let backend_name = stderr.lines().find_map(|line| {
        let (_, suffix) = line.split_once("whisper_backend_init_gpu: using ")?;
        let (name, _) = suffix.split_once(" backend")?;
        let name = name.trim();
        let normalized = name.to_ascii_lowercase();
        let matches_requested = expected_device.as_deref().map_or_else(
            || normalized.starts_with(backend_prefix),
            |expected| normalized == expected,
        );
        matches_requested.then(|| name.to_string())
    });
    let Some(backend_name) = backend_name else {
        return Err(format!(
            "Whisper {backend} selesai tanpa konfirmasi bahwa backend GPU {backend} dipakai."
        ));
    };
    if stderr.to_ascii_lowercase().contains(&format!(
        "failed to initialize {} backend",
        backend_name.to_ascii_lowercase()
    )) {
        return Err(format!(
            "Whisper {backend} gagal menginisialisasi perangkat GPU {backend_name}."
        ));
    }
    Ok(())
}

fn ensure_vulkan_probe(
    context: &JobContext,
    engine: &Path,
    model: &Path,
    device_index: usize,
) -> Result<(), String> {
    let signature = format!(
        "backend=vulkan|{}|device={device_index}",
        vulkan_probe_signature(engine, model)
    );
    run_cached_probe(context.capability_probe, signature, || {
        run_engine_probe(context, "vulkan", engine, model, Some(device_index))
    })
}

fn ensure_backend_probe(
    context: &JobContext,
    backend: &str,
    engine: &Path,
    model: &Path,
    device_index: Option<usize>,
) -> Result<(), String> {
    let signature = format!(
        "backend={backend}|{}|device={device_index:?}",
        vulkan_probe_signature(engine, model)
    );
    run_cached_probe(context.capability_probe, signature, || {
        run_engine_probe(context, backend, engine, model, device_index)
    })
}

fn run_cached_probe<F>(
    cache: &Arc<Mutex<HashSet<String>>>,
    signature: String,
    probe: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if cache
        .lock()
        .map_err(|_| "Cache capability probe terkunci")?
        .contains(&signature)
    {
        return Ok(());
    }

    let result = probe();
    if result.is_ok() {
        cache
            .lock()
            .map_err(|_| "Cache capability probe terkunci")?
            .insert(signature);
    }
    result
}

fn requested_device_index(
    requested_device_id: Option<&str>,
    backend: &str,
) -> Result<Option<usize>, String> {
    let Some(requested_device_id) = requested_device_id else {
        return Ok(None);
    };
    if requested_device_id == backend {
        return Ok(None);
    }
    let prefix = format!("{backend}:");
    let value = requested_device_id.strip_prefix(&prefix).ok_or_else(|| {
        format!("Target compute {requested_device_id} tidak cocok dengan backend {backend}.")
    })?;
    value
        .parse::<usize>()
        .map(Some)
        .map_err(|_| format!("Index device compute tidak valid: {requested_device_id}."))
}

fn vulkan_device_indices(devices: &[ComputeDeviceInfo]) -> Vec<usize> {
    let mut indexed = devices
        .iter()
        .filter_map(|device| device.device_index.map(|index| (device.integrated, index)))
        .collect::<Vec<_>>();
    indexed.sort_by_key(|(integrated, _)| *integrated);

    indexed
        .into_iter()
        .map(|(_, index)| index)
        .collect::<Vec<_>>()
}

fn ensure_vulkan_host_memory(
    devices: &[ComputeDeviceInfo],
    model_id: &str,
    wav: &Path,
    device_index: usize,
) -> Result<(), String> {
    let Some(device) = devices
        .iter()
        .find(|device| device.device_index == Some(device_index))
    else {
        return Ok(());
    };

    // Vulkan does not expose a portable, trustworthy VRAM budget through the
    // whisper.cpp device listing. UMA/iGPU devices share system RAM, so apply
    // a conservative host-memory guard instead of inventing dedicated VRAM.
    if device.integrated {
        let spec = models::model_spec(model_id)?;
        let wav_bytes = fs::metadata(wav)
            .map_err(|error| format!("Gagal membaca ukuran audio untuk guard Vulkan: {error}"))?
            .len();
        let required = spec
            .vram_required_mb
            .saturating_mul(1024 * 1024)
            .saturating_add(wav_bytes.saturating_mul(2))
            .saturating_add(512 * 1024 * 1024);
        resources::require_memory(required, "inference Vulkan pada GPU shared-memory")?;
    }
    Ok(())
}

fn probe_vulkan_devices(
    context: &JobContext,
    engine: &Path,
    model: &Path,
    model_id: &str,
    wav: &Path,
    devices: &[ComputeDeviceInfo],
    requested_index: Option<usize>,
) -> Result<usize, String> {
    let device_indices = requested_index
        .map(|index| vec![index])
        .unwrap_or_else(|| vulkan_device_indices(devices));
    if device_indices.is_empty() {
        return Err("Tidak ada device Vulkan nyata yang lolos enumerasi.".into());
    }
    let mut errors = Vec::new();
    for device_index in &device_indices {
        if let Err(error) = ensure_vulkan_host_memory(devices, model_id, wav, *device_index) {
            errors.push(format!("device {device_index}: {error}"));
            continue;
        }
        match ensure_vulkan_probe(context, engine, model, *device_index) {
            Ok(()) => return Ok(*device_index),
            Err(error) => errors.push(format!("device {device_index}: {error}")),
        }
    }
    Err(errors.join("; "))
}

fn run_cuda_probe_with_vram_guard<F>(
    model_id: &str,
    gpu: &crate::system::GpuInfo,
    probe: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    models::ensure_vram_available(model_id, gpu.available_memory_mb())?;
    probe()
}

type BackendSelection = (String, PathBuf, Option<String>, Option<usize>);

fn try_cuda_devices<T>(
    devices: &[crate::system::GpuInfo],
    mut attempt: impl FnMut(&crate::system::GpuInfo) -> Result<T, String>,
    mut is_cancelled: impl FnMut() -> bool,
    failures: &mut Vec<String>,
) -> Result<Option<T>, String> {
    for gpu in devices {
        if is_cancelled() {
            return Err("Transkripsi dibatalkan.".into());
        }
        match attempt(gpu) {
            Ok(selection) => return Ok(Some(selection)),
            Err(error) if is_cancelled() => return Err(error),
            Err(error) => failures.push(format!(
                "cuda:{} ({}): {error}",
                gpu.device_index
                    .map(|index| index.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                gpu.name
            )),
        }
    }
    Ok(None)
}

struct ResolvedEngine {
    selection: BackendSelection,
    runtime: crate::runtime_snapshot::RuntimeSnapshot,
}

fn choose_backend(
    context: &JobContext,
    requested: &str,
    requested_device_id: Option<&str>,
    model: &Path,
    wav: &Path,
    model_id: &str,
) -> Result<BackendSelection, String> {
    let app = context.app;
    match requested {
        "cpu" => {
            let cpu = engine_path_cancelable(app, "cpu", context.cancelled)?;
            if !is_regular_file(&cpu) {
                Err("CPU whisper engine belum terpasang.".into())
            } else {
                Ok(("cpu".into(), cpu, None, None))
            }
        }
        "cuda" => {
            let nvidia = detect_nvidia().is_some();
            let cuda = engine_path_cancelable(app, "cuda", context.cancelled)?;
            if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
                Err("CUDA hanya tersedia pada build Windows yang didukung.".into())
            } else if !nvidia {
                Err("CUDA tidak melihat perangkat NVIDIA yang dapat dipetakan. Periksa visibilitas GPU dan driver.".into())
            } else if let Err(error) = ensure_cuda_install_supported() {
                Err(error)
            } else if !is_regular_file(&cuda) {
                Err("CUDA engine belum terpasang. Install CUDA acceleration dari Settings terlebih dahulu.".into())
            } else {
                let device_index = requested_device_index(requested_device_id, "cuda")?;
                let gpu = match device_index {
                    Some(index) => detect_nvidia_device(index)
                        .ok_or_else(|| format!("NVIDIA device {index} tidak lagi terdeteksi."))?,
                    None => detect_nvidia()
                        .ok_or_else(|| "NVIDIA GPU/driver tidak terdeteksi.".to_string())?,
                };
                let device_index = gpu.device_index.ok_or_else(|| {
                    format!(
                        "NVIDIA device {} tidak memiliki indeks perangkat.",
                        gpu.name
                    )
                })?;
                run_cuda_probe_with_vram_guard(model_id, &gpu, || {
                    ensure_backend_probe(context, "cuda", &cuda, model, Some(device_index))
                })?;
                Ok(("cuda".into(), cuda, None, Some(device_index)))
            }
        }
        "metal" => {
            let metal = engine_path_cancelable(app, "metal", context.cancelled)?;
            if !cfg!(target_os = "macos") {
                Err("Apple Metal hanya tersedia di macOS.".into())
            } else if !is_regular_file(&metal) {
                Err("Metal engine belum terpasang. Install Apple Metal dari Settings terlebih dahulu.".into())
            } else {
                ensure_backend_probe(context, "metal", &metal, model, None)?;
                Ok(("metal".into(), metal, None, None))
            }
        }
        "vulkan" => {
            let gpu_detected = detect_nvidia().is_some() || detect_gpu().is_some();
            let vulkan = engine_path_cancelable(app, "vulkan", context.cancelled)?;
            if !cfg!(any(target_os = "windows", target_os = "linux")) {
                Err("Vulkan accelerator saat ini tersedia di Windows/Linux.".into())
            } else if !gpu_detected {
                Err("Vulkan dipilih tetapi GPU tidak terdeteksi pada perangkat ini.".into())
            } else if !is_regular_file(&vulkan) {
                Err(
                    "Vulkan engine belum terpasang. Install Vulkan dari Settings terlebih dahulu."
                        .into(),
                )
            } else {
                let requested_device_index = requested_device_index(requested_device_id, "vulkan")?;
                let devices = crate::system::detect_vulkan_devices_from_engine(&vulkan);
                let device_index = probe_vulkan_devices(
                    context,
                    &vulkan,
                    model,
                    model_id,
                    wav,
                    &devices,
                    requested_device_index,
                )
                .map_err(|error| {
                    if crate::io_cancel::is_cancellation_error(&error) {
                        error
                    } else {
                        format!("Vulkan dipilih tetapi capability probe gagal: {error}")
                    }
                })?;
                Ok(("vulkan".into(), vulkan, None, Some(device_index)))
            }
        }
        "auto" => {
            let cuda_devices = crate::system::cuda_devices_by_preference();
            let nvidia = !cuda_devices.is_empty();
            let gpu_detected = nvidia || detect_gpu().is_some();
            let mut candidates = Vec::new();
            if cfg!(all(target_os = "windows", target_arch = "x86_64"))
                && nvidia
                && detect_nvidia_driver_version()
                    .as_deref()
                    .is_some_and(cuda_driver_is_compatible)
            {
                candidates.push("cuda");
            }
            if cfg!(target_os = "macos") {
                candidates.push("metal");
            }
            if cfg!(any(target_os = "windows", target_os = "linux")) && gpu_detected {
                candidates.push("vulkan");
            }
            candidates.push("cpu");
            let mut failures = Vec::new();
            for backend in candidates {
                if context.is_cancelled() {
                    return Err("Transkripsi dibatalkan.".into());
                }
                if backend == "cuda" {
                    let selection = try_cuda_devices(
                        &cuda_devices,
                        |gpu| {
                            let device_index = gpu.device_index.ok_or_else(|| {
                                format!(
                                    "NVIDIA device {} tidak memiliki indeks perangkat.",
                                    gpu.name
                                )
                            })?;
                            let device_id = format!("cuda:{device_index}");
                            let (backend, engine, _, device) = choose_backend(
                                context,
                                "cuda",
                                Some(&device_id),
                                model,
                                wav,
                                model_id,
                            )?;
                            Ok((backend, engine, device))
                        },
                        || context.is_cancelled(),
                        &mut failures,
                    )?;
                    if let Some((backend, engine, device)) = selection {
                        let message = (!failures.is_empty())
                            .then(|| format!("Auto beralih ke {backend}: {}", failures.join("; ")));
                        return Ok((backend, engine, message, device));
                    }
                    continue;
                }
                // Resolve and hash only the candidate we are about to use.
                // Explicit selection keeps its normal integrity/VRAM/probe guards.
                match choose_backend(context, backend, None, model, wav, model_id) {
                    Ok((backend, engine, _, device)) => {
                        let message = (!failures.is_empty())
                            .then(|| format!("Auto beralih ke {backend}: {}", failures.join("; ")));
                        return Ok((backend, engine, message, device));
                    }
                    Err(error) if context.is_cancelled() => return Err(error),
                    Err(error) => failures.push(format!("{backend}: {error}")),
                }
            }
            Err(format!(
                "Tidak ada whisper engine yang siap digunakan: {}",
                failures.join("; ")
            ))
        }
        _ => Err("Compute backend tidak dikenal.".into()),
    }
}

fn run_whisper(
    context: &JobContext,
    wav: &Path,
    output_prefix: &Path,
    model: &Path,
    config: WhisperConfig<'_>,
    resolved: &mut Option<ResolvedEngine>,
) -> Result<String, String> {
    let (resolved_backend, engine, fallback_message, device_index) = match resolved {
        Some(selection) => {
            selection.runtime.validate(context.cancelled)?;
            selection.selection.clone()
        }
        None => {
            let selection = choose_backend(
                context,
                config.backend,
                config.compute_device_id,
                model,
                wav,
                config.model_id,
            )?;
            let runtime = crate::paths::snapshot_runtime(
                context.app,
                &selection.0,
                &selection.1,
                context.cancelled,
            )?;
            *resolved = Some(ResolvedEngine {
                selection: selection.clone(),
                runtime,
            });
            selection
        }
    };
    if let Some(message) = fallback_message {
        context.emit(ProgressUpdate {
            stage: "transcribing",
            message_code: "backend_fallback",
            percent: 0.0,
            message,
            backend: Some(&resolved_backend),
            downloaded_bytes: None,
            total_bytes: None,
            network_bytes_per_second: None,
        });
    }
    let mut usage_device_index = device_index;
    let mut selected_cuda_gpu = None;
    if resolved_backend == "cuda" {
        let gpu = match device_index {
            Some(index) => detect_nvidia_device(index).ok_or_else(|| {
                format!(
                    "NVIDIA device {index} tidak lagi terdeteksi. Pilih CPU atau periksa driver."
                )
            })?,
            None => detect_nvidia().ok_or_else(|| {
                "NVIDIA GPU tidak lagi terdeteksi. Pilih CPU atau periksa driver.".to_string()
            })?,
        };
        models::ensure_vram_available(config.model_id, gpu.available_memory_mb())?;
        usage_device_index = gpu.device_index;
        selected_cuda_gpu = Some(gpu);
    }
    context
        .usage
        .set_target(&resolved_backend, usage_device_index);
    let available_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    let threads = whisper_thread_count(available_threads, &resolved_backend);
    let mut command = Command::new(engine);
    process::hide_console(&mut command);
    command
        .args(["-m"])
        .arg(model)
        .args(["-f"])
        .arg(wav)
        .args(["-l", config.language, "-t"])
        .arg(threads.to_string())
        .args(["-pp", "-oj", "-of"])
        .arg(output_prefix)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if resolved_backend == "cpu" {
        command.arg("-ng");
    } else if resolved_backend == "cuda" {
        let gpu = selected_cuda_gpu
            .as_ref()
            .ok_or_else(|| "CUDA tidak memiliki perangkat NVIDIA terpilih.".to_string())?;
        configure_cuda_command(&mut command, gpu)?;
        command.arg("-dev").arg("0");
    } else if resolved_backend == "vulkan" {
        configure_vulkan_command(&mut command);
        // Keep the actual job on the same conservative Vulkan path as the
        // capability probe. Some AMD drivers are unstable with flash-attn.
        command.arg("-nfa");
        if let Some(device_index) = device_index {
            command.arg("-dev").arg(device_index.to_string());
        }
    }
    let progress_re = Regex::new(r"progress\s*=\s*([0-9]+)%").unwrap();
    let (status, stderr) = run_media_command(
        context,
        &mut command,
        process::OutputStream::Stderr,
        whisper_inactivity_timeout(&resolved_backend),
        None,
        |line| {
            if let Some(caps) = progress_re.captures(line) {
                if let Ok(percent) = caps[1].parse::<f64>() {
                    context.emit(ProgressUpdate {
                        stage: "transcribing",
                        message_code: "whisper_via",
                        percent,
                        message: format!(
                            "Whisper sedang bekerja via {}…",
                            resolved_backend.to_uppercase()
                        ),
                        backend: Some(&resolved_backend),
                        downloaded_bytes: None,
                        total_bytes: None,
                        network_bytes_per_second: None,
                    });
                }
            }
            Ok(())
        },
    )?;
    if !status.success() {
        return Err(process_failed(context.cancelled, stderr, "Whisper"));
    }
    if resolved_backend != "cpu" {
        let engine_device_index = if resolved_backend == "cuda" {
            Some(0)
        } else {
            device_index
        };
        require_gpu_backend_attestation(&stderr, &resolved_backend, engine_device_index)?;
    }
    Ok(resolved_backend)
}

fn run_qwen(
    context: &JobContext,
    wav: &Path,
    prefix: &Path,
    model: &Path,
    config: WhisperConfig<'_>,
    runtime: &crate::runtime_snapshot::RuntimeSnapshot,
) -> Result<checkpoint::ChunkOutput, String> {
    runtime.validate(context.cancelled)?;
    if config.backend == "vulkan" {
        return Err(
            "Qwen belum mendukung pilihan Vulkan di WhisperTube. Pilih Auto, CUDA, atau CPU."
                .into(),
        );
    }
    let mut gpu = None;
    let mut cuda_runtime = None;
    let mut cuda_engine = None;
    let qwen_backends = crate::qwen::supported_backends();
    if qwen_backends.contains(&"cuda")
        && matches!(config.backend, "cuda" | "auto")
        && detect_nvidia_driver_version()
            .as_deref()
            .is_some_and(cuda_driver_is_compatible)
    {
        let index = if config.backend == "cuda" {
            requested_device_index(config.compute_device_id, "cuda")?
        } else {
            None
        };
        let candidate = match index {
            Some(index) => detect_nvidia_device(index),
            None => detect_nvidia(),
        };
        let engine = engine_path_cancelable(context.app, "cuda", context.cancelled)?;
        let candidate = candidate.filter(|candidate| {
            is_regular_file(&engine)
                && models::ensure_vram_available(config.model_id, candidate.available_memory_mb())
                    .is_ok()
        });
        if let Some(candidate) = crate::qwen::select_cuda_candidate(
            config.backend,
            candidate,
            crate::system::nvidia_compute_capability,
        )? {
            cuda_runtime = Some(crate::paths::snapshot_runtime(
                context.app,
                "cuda",
                &engine,
                context.cancelled,
            )?);
            cuda_engine = Some(engine);
            gpu = Some(candidate);
        }
    }
    let backend = match config.backend {
        "cuda" if gpu.is_none() => return Err("CUDA Qwen belum siap atau VRAM bebas kurang. Pasang akselerasi NVIDIA, tutup aplikasi GPU lain, atau pilih CPU.".into()),
        "cuda" => "cuda",
        "auto" if gpu.is_some() => "cuda",
        "metal" if qwen_backends.contains(&"metal") => "metal",
        "auto" if qwen_backends.contains(&"metal") => "metal",
        "auto" | "cpu" => "cpu",
        _ => return Err("Backend Qwen tidak tersedia pada platform ini.".into()),
    };
    let library = crate::qwen::library_path(context.app)?;
    let folder = library.parent().ok_or("Folder runtime Qwen invalid.")?;
    let mut command =
        Command::new(std::env::current_exe().map_err(|e| format!("Executable worker Qwen: {e}"))?);
    command
        .arg("--qwen-worker")
        .arg(&library)
        .arg(model)
        .arg(wav)
        .arg(prefix.with_extension("json"))
        .arg(backend)
        .arg(config.language)
        .current_dir(folder);
    let mut search = vec![folder.to_path_buf()];
    if let Some(engine) = &cuda_engine {
        search.push(engine.parent().ok_or("Folder CUDA invalid.")?.to_path_buf());
    }
    search.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    command.env(
        "PATH",
        std::env::join_paths(search).map_err(|e| format!("Path worker Qwen: {e}"))?,
    );
    // Dynamic modules must come only from the verified runtime, never from a
    // developer's global GGML override or an arbitrary working directory.
    command.env_remove("GGML_BACKEND_PATH");
    command.env_remove("TRANSCRIBE_BACKENDS");
    if let Some(gpu) = &gpu {
        configure_cuda_command(&mut command, gpu)?;
    }
    context
        .usage
        .set_target(backend, gpu.as_ref().and_then(|gpu| gpu.device_index));
    let (status, stderr) = run_media_command(
        context,
        &mut command,
        process::OutputStream::Stderr,
        whisper_inactivity_timeout(backend),
        None,
        |line| {
            if let Some(percent) = line
                .strip_prefix("QWEN_PROGRESS ")
                .and_then(|s| s.parse::<f64>().ok())
            {
                context.emit(ProgressUpdate {
                    stage: "transcribing",
                    message_code: "checkpoint_progress",
                    percent,
                    message: format!("Qwen sedang bekerja via {}…", backend.to_uppercase()),
                    backend: Some(backend),
                    downloaded_bytes: None,
                    total_bytes: None,
                    network_bytes_per_second: None,
                });
            }
            Ok(())
        },
    )?;
    // Retain dependency file handles until the child and its pipes have exited.
    drop(cuda_runtime);
    if !status.success() {
        return Err(process_failed(context.cancelled, stderr, "Qwen"));
    }
    let bytes = read_bounded_file(
        &prefix.with_extension("json"),
        checkpoint::MAX_CHECKPOINT_BYTES,
        "hasil Qwen",
    )?;
    let output: crate::qwen::WorkerOutput =
        serde_json::from_slice(&bytes).map_err(|e| format!("Hasil worker Qwen invalid: {e}"))?;
    if output.backend != backend {
        return Err("Backend hasil Qwen berbeda dari perangkat yang diminta.".into());
    }
    Ok(checkpoint::ChunkOutput {
        language: output.language,
        backend: output.backend,
        segments: output.segments,
    })
}

#[derive(Deserialize)]
struct WhisperOutput {
    #[serde(default)]
    result: Option<WhisperResultMetadata>,
    #[serde(default)]
    transcription: Vec<WhisperOutputSegment>,
}

#[derive(Deserialize)]
struct WhisperResultMetadata {
    #[serde(default)]
    language: Option<String>,
}

#[derive(Deserialize)]
struct WhisperOutputSegment {
    #[serde(default)]
    timestamps: Option<WhisperTimestamps>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct WhisperTimestamps {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

fn parse_whisper_result(path: &Path) -> Result<(String, Vec<Segment>, String), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Gagal membaca ukuran output JSON Whisper: {error}"))?;
    if metadata.len() > MAX_TRANSCRIPT_RESULT_BYTES {
        return Err("Output JSON Whisper melebihi batas ukuran aman.".into());
    }
    let file =
        File::open(path).map_err(|error| format!("Gagal membuka output JSON Whisper: {error}"))?;
    let mut bounded_reader = file.take(MAX_TRANSCRIPT_RESULT_BYTES.saturating_add(1));
    let mut deserializer = serde_json::Deserializer::from_reader(&mut bounded_reader);
    let output = WhisperOutput::deserialize(&mut deserializer)
        .map_err(|error| format!("Output JSON Whisper rusak: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("Output JSON Whisper memiliki data tambahan: {error}"))?;
    drop(deserializer);
    if bounded_reader.limit() == 0 {
        return Err("Output JSON Whisper melebihi batas ukuran aman.".into());
    }

    let language = output
        .result
        .and_then(|result| result.language)
        .unwrap_or_else(|| "unknown".into());
    if language.len() > MAX_REQUEST_LANGUAGE_BYTES {
        return Err("Bahasa hasil Whisper terlalu panjang.".into());
    }
    let mut segments = Vec::new();
    let mut total_text_bytes = 0usize;
    for item in output.transcription {
        let from = item
            .timestamps
            .as_ref()
            .and_then(|timestamps| timestamps.from.as_deref())
            .unwrap_or("00:00:00,000")
            .to_string();
        let to = item
            .timestamps
            .as_ref()
            .and_then(|timestamps| timestamps.to.as_deref())
            .unwrap_or("00:00:00,000")
            .to_string();
        if from.len() > MAX_TIMESTAMP_BYTES || to.len() > MAX_TIMESTAMP_BYTES {
            return Err("Timestamp hasil Whisper terlalu panjang.".into());
        }
        let text = item.text.unwrap_or_default().trim().to_string();
        if !text.is_empty() {
            if segments.len() >= MAX_TRANSCRIPT_SEGMENTS {
                return Err("Output Whisper memiliki terlalu banyak segment.".into());
            }
            total_text_bytes = total_text_bytes
                .saturating_add(text.len())
                .saturating_add(usize::from(!segments.is_empty()));
            if total_text_bytes > MAX_TRANSCRIPT_TEXT_BYTES {
                return Err("Teks output Whisper melebihi batas ukuran aman.".into());
            }
            segments.push(Segment { from, to, text });
        }
    }
    let text = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok((language, segments, text))
}

fn inference_error(error: String) -> String {
    if crate::io_cancel::is_cancellation_error(&error) {
        error
    } else {
        format!("{error} Audio dan checkpoint yang berhasil disimpan tetap tersedia. Percobaan dengan sumber dan pengaturan yang sama dapat memakai ulang progres tersebut.")
    }
}

pub fn pipeline(
    app: AppHandle,
    active_pid: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
    capability_probe: Arc<Mutex<HashSet<String>>>,
    mut request: TranscriptRequest,
) -> Result<TranscriptResult, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }
    request.url = validate_source(&request.url)?;
    let local_audio = local_path(&request.url)?;
    validate_transcript_request(&request)?;
    validate_media_duration(request.duration, false)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    let ffmpeg = tool_path(&app, "ffmpeg")?;
    if (local_audio.is_none() && !is_regular_file(&yt_dlp)) || !is_regular_file(&ffmpeg) {
        return Err("Runtime belum lengkap. Jalankan scripts/setup-windows.ps1.".into());
    }
    let model = model_path(&app, &request.model_id)?;
    if !is_regular_file(&model) {
        return Err("Model belum diunduh. Unduh model dari UI terlebih dahulu.".into());
    }
    models::verify_model_file(&model, &request.model_id, &cancelled)?;
    let qwen_runtime = if crate::qwen::is_qwen(&request.model_id) {
        Some(crate::qwen::snapshot(&app, &cancelled)?)
    } else {
        None
    };
    let jobs_dir = jobs_dir(&app)?;
    let duration_seconds = request.duration.max(0.0).ceil() as u64;
    let inference_bytes = duration_seconds
        .min(checkpoint::CHUNK_SECONDS + 1)
        .saturating_mul(16_000)
        .saturating_mul(4);
    let model_bytes = model
        .metadata()
        .map_err(|error| format!("Gagal membaca ukuran model untuk guard RAM: {error}"))?
        .len();
    let inference_required = inference_bytes
        .saturating_add(model_bytes)
        .saturating_add(512 * 1024 * 1024);
    let committed_jobs = history::referenced_job_directories(&app)?;
    let (reusable_job_dir, audio_ready) =
        match find_reusable_audio_job(&jobs_dir, &mut request, &committed_jobs)? {
            Some((path, ready)) => (Some(path), ready),
            None => (None, false),
        };
    if !audio_ready {
        crate::resources::require_memory(inference_required, "transkripsi")?;
    }
    let disk_reservation = if audio_ready {
        DISK_SAFETY_BUFFER_BYTES
    } else if local_audio.is_some() {
        // The original stays outside the job directory. A partial conversion
        // is overwritten, so reserve the full WAV even when resuming a job.
        media_disk_reservation(request.duration, true)
    } else if let Some(path) = &reusable_job_dir {
        media_disk_reservation(request.duration, false)
            .saturating_sub(directory_file_bytes(path)?)
            .max(DISK_SAFETY_BUFFER_BYTES)
    } else {
        media_disk_reservation(request.duration, false)
    };
    crate::resources::require_disk(&jobs_dir, disk_reservation, "transkripsi")?;
    ensure_job_storage_quota(&app)?;
    let (job_dir, mut job_guard) = match reusable_job_dir {
        Some(path) => (path.clone(), JobDirectoryGuard::resume(path)?),
        None => {
            let path = jobs_dir.join(Uuid::new_v4().to_string());
            let guard = JobDirectoryGuard::create(path.clone())?;
            (path, guard)
        }
    };
    // Preserve stage files on failure/cancel regardless of the final keep-audio
    // preference. Startup expires abandoned jobs after 24 hours.
    if !audio_ready {
        write_audio_cache_metadata(&job_dir, &request, false)?;
    }
    job_guard.preserve_on_failure();
    let usage_monitor = UsageMonitor::start();
    let context = JobContext {
        app: &app,
        usage: &usage_monitor,
        active_pid: &active_pid,
        cancelled: &cancelled,
        capability_probe: &capability_probe,
        progress_start: 0.0,
        progress_end: 100.0,
    };

    let wav = job_dir.join("audio.wav");
    if audio_ready {
        context.emit(ProgressUpdate {
            stage: "converting",
            message_code: "reusing_audio",
            percent: 100.0,
            message: "Memakai audio tersimpan; download dan konversi dilewati.".into(),
            backend: None,
            downloaded_bytes: None,
            total_bytes: None,
            network_bytes_per_second: None,
        });
        validate_audio_wav(&wav, request.duration)?;
    } else {
        if local_audio.is_none() {
            context.emit(ProgressUpdate {
                stage: "downloading",
                message_code: "preparing_download",
                percent: 0.0,
                message: "Menyiapkan download…".into(),
                backend: None,
                downloaded_bytes: None,
                total_bytes: None,
                network_bytes_per_second: None,
            });
        }
        let allow_recovery = local_audio.is_some();
        let (source, needs_duration_probe) = match local_audio {
            Some(path) => (path, false),
            None => run_download(&context, &request, &job_dir)?,
        };
        if context.is_cancelled() {
            return Err("Job dibatalkan.".into());
        }

        if needs_duration_probe {
            let actual =
                crate::sources::probe_local_duration(&ffmpeg, &source, &active_pid, &cancelled)?;
            if (actual - request.duration).abs() > MEDIA_DURATION_TOLERANCE_SECONDS {
                return Err("Durasi audio berubah setelah pemeriksaan. Periksa ulang URL sebelum transkripsi.".into());
            }
            // Metadata can round seconds or include slightly different codec
            // padding. Convert the measured audio length, never truncate it to
            // the earlier estimate after accepting this small difference.
            request.duration = actual;
            write_audio_cache_metadata(&job_dir, &request, false)?;
        }

        context.emit(ProgressUpdate {
            stage: "converting",
            message_code: "normalizing_audio",
            percent: 0.0,
            message: "Menormalisasi audio untuk transkripsi…".into(),
            backend: None,
            downloaded_bytes: None,
            total_bytes: None,
            network_bytes_per_second: None,
        });
        let audio_recovered =
            run_ffmpeg(&context, &source, &wav, request.duration, allow_recovery)?;
        local_path(&request.url)?;
        ensure_job_storage_quota(&app)?;
        validate_audio_wav(&wav, request.duration)?;
        if context.is_cancelled() {
            return Err("Job dibatalkan.".into());
        }
        // Commit readiness only after FFmpeg succeeded and the WAV validates.
        // A killed conversion is never eligible as completed audio.
        checkpoint::PcmAudio::inspect(&wav, request.duration, || context.is_cancelled())?;
        write_audio_cache_metadata_with_recovery(&job_dir, &request, true, audio_recovered)?;
    };
    // Retry cleanup even when a completed WAV bypasses download/conversion.
    history::cleanup_source_audio(&job_dir)?;

    let output_prefix = job_dir.join("transcript");
    context.emit(ProgressUpdate {
        stage: "transcribing",
        message_code: "loading_model",
        percent: 0.0,
        message: "Memuat model transkripsi…".into(),
        backend: None,
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    let audio = checkpoint::PcmAudio::open(&wav, request.duration, || context.is_cancelled())?;
    let (audio_duration, source_duration) = reconciled_audio_duration(&audio, request.duration);
    let mut resolved_engine = None;
    let completed = checkpoint::transcribe(
        &audio,
        &job_dir,
        &request,
        |chunk_wav, prefix, language, span| {
            let required = fs::metadata(chunk_wav)
                .map_err(|e| format!("Gagal membaca potongan audio: {e}"))?
                .len()
                .saturating_mul(2)
                .saturating_add(model_bytes)
                .saturating_add(512 * 1024 * 1024);
            resources::require_memory(required, "inference potongan transkripsi")?;
            let chunk_context = JobContext {
                progress_start: span.0,
                progress_end: span.1,
                ..context
            };
            if let Some(runtime) = &qwen_runtime {
                return run_qwen(
                    &chunk_context,
                    chunk_wav,
                    prefix,
                    &model,
                    WhisperConfig {
                        language,
                        backend: &request.backend,
                        compute_device_id: request.compute_device_id.as_deref(),
                        model_id: &request.model_id,
                    },
                    runtime,
                );
            }
            let backend = run_whisper(
                &chunk_context,
                chunk_wav,
                prefix,
                &model,
                WhisperConfig {
                    language,
                    backend: &request.backend,
                    compute_device_id: request.compute_device_id.as_deref(),
                    model_id: &request.model_id,
                },
                &mut resolved_engine,
            )?;
            normalize_output_utf8(
                &prefix.with_extension("json"),
                checkpoint::MAX_CHECKPOINT_BYTES,
            )?;
            let (language, segments, _) = parse_whisper_result(&prefix.with_extension("json"))?;
            Ok(checkpoint::ChunkOutput {
                language,
                backend,
                segments,
            })
        },
        || context.is_cancelled(),
        |percent, reused| {
            context.emit(ProgressUpdate {
                stage: "transcribing",
                message_code: if reused {
                    "resuming_transcription"
                } else {
                    "checkpoint_progress"
                },
                percent,
                message: if reused {
                    "Melanjutkan hasil bagian yang sudah tersimpan.".into()
                } else {
                    "Memproses audio per bagian; progres selesai disimpan otomatis.".into()
                },
                backend: None,
                downloaded_bytes: None,
                total_bytes: None,
                network_bytes_per_second: None,
            });
        },
    )
    .map_err(inference_error)?;
    if context.is_cancelled() {
        return Err("Job dibatalkan.".into());
    }
    checkpoint::write_exports(&output_prefix, &completed)?;
    let resolved_backend = completed.backend;
    let language = completed.language;
    let segments = completed.segments;
    let text = completed.text;
    context.emit(ProgressUpdate {
        stage: "finalizing",
        message_code: "finalizing_result",
        percent: 92.0,
        message: "Merapikan transcript dan menyimpan history…".into(),
        backend: Some(&resolved_backend),
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    let txt_path = output_prefix.with_extension("txt");
    let srt_path = output_prefix.with_extension("srt");
    let vtt_path = output_prefix.with_extension("vtt");
    ensure_job_storage_quota(&app)?;
    let result_store_path = job_dir.join("result.json");
    let mut result = TranscriptResult {
        audio_recovered: read_audio_cache_metadata(&job_dir.join(AUDIO_CACHE_METADATA_FILE))
            .ok_or("Metadata cache audio tidak dapat dibaca saat menyimpan hasil.")?
            .audio_recovered,
        transcript_available: true,
        history_id: 0,
        title: request.title.clone(),
        channel: request.channel.clone(),
        language: language.clone(),
        duration: audio_duration,
        source_duration,
        model: request.model_id.clone(),
        backend: resolved_backend.clone(),
        segments,
        text,
        txt_path: txt_path.to_string_lossy().to_string(),
        srt_path: srt_path.to_string_lossy().to_string(),
        vtt_path: vtt_path.to_string_lossy().to_string(),
        audio_path: request
            .keep_audio
            .then(|| wav.to_string_lossy().to_string()),
    };
    // Keep cache/checkpoint identity tied to the inspected source, but persist
    // the duration actually transcribed in both the result and SQLite.
    let mut history_request = request.clone();
    history_request.duration = audio_duration;
    let history_id = history::save_history_record_with(
        &app,
        &history_request,
        &language,
        &resolved_backend,
        &result_store_path,
        |history_id| {
            result.history_id = history_id;
            let bytes = serde_json::to_vec_pretty(&result)
                .map_err(|e| format!("Gagal serialize hasil: {e}"))?;
            if bytes.len() as u64 > MAX_TRANSCRIPT_RESULT_BYTES {
                return Err("Hasil transcript melebihi batas ukuran aman.".into());
            }
            write_file_atomically(&result_store_path, &bytes)
        },
    )?;

    // History is committed: cleanup errors must not turn it into a failed job.
    if let Err(error) = checkpoint::cleanup(&job_dir) {
        eprintln!("WhisperTube checkpoint cleanup warning: {error}");
    }
    if request.keep_audio {
        if let Err(error) = history::record_retained_audio_cleanup(&job_dir) {
            eprintln!("WhisperTube retained audio cleanup receipt warning: {error}");
        }
    } else {
        if let Err(error) = remove_file_if_present(&wav, "WAV hasil konversi") {
            eprintln!("WhisperTube audio cleanup warning: {error}");
        }
    }
    context.emit(ProgressUpdate {
        stage: "done",
        message_code: "complete",
        percent: 100.0,
        message: "Transkripsi selesai.".into(),
        backend: Some(&resolved_backend),
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    result.history_id = history_id;
    job_guard.commit();
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn media_command_cancellation_remains_recognizable_for_every_pipeline_worker() {
        for program in [
            "yt-dlp.exe",
            "ffmpeg.exe",
            "whisper-cli.exe",
            "whispertube.exe",
        ] {
            let program = std::ffi::OsStr::new(program);
            let cancelled = super::media_command_error(
                "Proses dibatalkan.".into(),
                program,
                b"native log",
                true,
            );
            assert_eq!(super::inference_error(cancelled), "Proses dibatalkan.");
            for token in [false, true] {
                let failure = super::media_command_error(
                    "watchdog timeout".into(),
                    program,
                    b"native log",
                    token,
                );
                assert!(!crate::io_cancel::is_cancellation_error(&failure));
                assert!(failure.contains("watchdog timeout"));
                assert!(failure.contains(program.to_str().unwrap()));
                assert_eq!(failure.contains("native log"), !token);
            }
        }
    }

    #[test]
    fn cancellation_keeps_its_identity_while_inference_failures_keep_resume_context() {
        for error in [
            "Job dibatalkan.",
            "Transkripsi dibatalkan.",
            "Proses dibatalkan.",
        ] {
            assert_eq!(super::inference_error(error.into()), error);
        }
        let error = super::inference_error("Qwen: output repetition".into());
        assert!(error.starts_with("Qwen: output repetition"));
        assert!(error.contains("checkpoint"));
        for error in [
            "disk full",
            "log: Proses dibatalkan.",
            "Proses dibatalkan. Detail: failure",
        ] {
            assert!(!crate::io_cancel::is_cancellation_error(error));
        }
    }

    #[test]
    fn downloaded_duration_preserves_known_long_vod_and_verifies_unknown_or_changed_sources() {
        assert!(!super::download_needs_duration_probe("7200.25", 7200.0).unwrap());
        assert!(!super::download_needs_duration_probe("61.97", 61.97).unwrap());
        for missing in ["NA", "0", "-1", "NaN", "inf", "", "not-a-duration"] {
            assert!(
                super::download_needs_duration_probe(missing, 61.97).unwrap(),
                "{missing}"
            );
        }
        assert!(super::download_needs_duration_probe("28801", 28801.0).is_err());
        assert!(super::download_needs_duration_probe("600", 61.97)
            .unwrap_err()
            .contains("berubah"));
    }
    #[test]
    fn download_speed_uses_the_upstream_transfer_rate_not_cached_bytes() {
        assert_eq!(super::download_speed("1048576.5"), Some(1_048_576));
        assert_eq!(super::download_speed("1.048576e6"), Some(1_048_576));
        assert_eq!(super::download_speed("0"), Some(0));
        for unavailable in ["NA", "None", "nan", "inf", "-1"] {
            assert_eq!(super::download_speed(unavailable), None);
        }
    }
    use super::{
        ffmpeg_target_duration, find_reusable_audio_job, normalize_utf8_bytes,
        parse_whisper_result, remaining_disk_reservation, run_cached_probe,
        run_cuda_probe_with_vram_guard, validate_transcript_request, vulkan_device_indices,
        whisper_inactivity_timeout, whisper_thread_count, write_audio_cache_metadata,
        ComputeDeviceInfo, JobDirectoryGuard, CPU_INACTIVITY_TIMEOUT, DISK_SAFETY_BUFFER_BYTES,
        PROCESS_INACTIVITY_TIMEOUT,
    };
    use crate::{checkpoint, types::TranscriptRequest};
    use std::{
        collections::HashSet,
        fs,
        sync::atomic::{AtomicBool, Ordering},
        sync::{Arc, Mutex},
    };
    use uuid::Uuid;

    #[test]
    fn decoded_audio_duration_is_truthful_and_short_tracks_are_explicit() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("whispertube-duration-{}", Uuid::new_v4()));
        let _guard = JobDirectoryGuard::create(root.clone()).unwrap();
        let wav = root.join("audio.wav");
        let mut file = fs::File::create(&wav).unwrap();
        checkpoint::write_wav_header(&mut file, 32_000).unwrap();
        file.write_all(&vec![0; 32_000]).unwrap();
        drop(file);
        let audio = checkpoint::PcmAudio::open(&wav, 60.0, || false).unwrap();
        assert_eq!(
            super::reconciled_audio_duration(&audio, 60.0),
            (1.0, Some(60.0))
        );
        // Rounding/padding differences do not falsely label normal audio.
        assert_eq!(super::reconciled_audio_duration(&audio, 1.25), (1.0, None));
        assert_eq!(super::reconciled_audio_duration(&audio, 1.0), (1.0, None));
        // The tolerance also applies to sub-second source metadata; the old
        // minimum-padding allowance must not widen the accepted window.
        let mut file = fs::File::create(&wav).unwrap();
        checkpoint::write_wav_header(&mut file, 3 * 32_000).unwrap();
        file.write_all(&vec![0; 3 * 32_000]).unwrap();
        drop(file);
        assert!(checkpoint::PcmAudio::inspect(&wav, 0.25, || false).is_err());
    }

    #[test]
    fn conversion_restricts_input_protocols_before_opening_the_local_media() {
        use std::path::Path;
        let command = super::ffmpeg_command(
            Path::new("ffmpeg"),
            Path::new("source with spaces.wav"),
            Path::new("audio.wav"),
            1.0,
        );
        let args = command.get_args().collect::<Vec<_>>();
        let policy = args
            .iter()
            .position(|arg| *arg == "-protocol_whitelist")
            .unwrap();
        let input = args.iter().position(|arg| *arg == "-i").unwrap();
        assert_eq!(args[policy + 1], "file");
        assert!(policy < input);
        assert_eq!(args[input + 1], "source with spaces.wav");
    }

    #[test]
    #[ignore = "Requires prepared platform FFmpeg runtime; run explicitly with --ignored"]
    fn native_multitrack_conversion_uses_the_inspected_audio_track() {
        let platform = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let ffmpeg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(platform)
            .join(crate::paths::exe_name("ffmpeg"));
        assert!(
            ffmpeg.is_file(),
            "Install the runtime before this native QA test"
        );
        let root = std::env::temp_dir().join(format!("whispertube-multitrack-{}", Uuid::new_v4()));
        let _guard = JobDirectoryGuard::create(root.clone()).unwrap();
        let input = root.join("multitrack.mp4");
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut generate = std::process::Command::new(&ffmpeg);
        generate
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo:d=4",
                "-map",
                "0:a",
                "-map",
                "1:a",
                "-c:a",
                "aac",
                "-disposition:a:0",
                "0",
                "-disposition:a:1",
                "default",
            ])
            .arg(&input);
        let (status, stderr) = crate::process::run_with_timeout_captured_cancelable(
            &mut generate,
            std::time::Duration::from_secs(10),
            4096,
            &cancelled,
        )
        .unwrap();
        assert!(status.success(), "{stderr}");
        let (duration, recovered) = crate::sources::probe_import_duration(
            &ffmpeg,
            &input,
            &Arc::new(Mutex::new(None)),
            &cancelled,
        )
        .unwrap();
        assert!(!recovered);
        assert!((duration - 2.0).abs() < 0.1);
        for recovering in [false, true] {
            let output = root.join(format!("audio-{recovering}.wav"));
            let mut command =
                super::ffmpeg_decode_command(&ffmpeg, &input, &output, duration, recovering);
            let (status, stderr) = crate::process::run_with_timeout_captured_cancelable(
                &mut command,
                std::time::Duration::from_secs(10),
                4096,
                &cancelled,
            )
            .unwrap();
            assert!(status.success(), "{stderr}");
            let audio = checkpoint::PcmAudio::inspect(&output, duration, || false).unwrap();
            assert!((audio.duration_seconds() - duration).abs() < 0.1);
            // Check actual PCM content: selecting the default second track
            // would produce silence instead of the inspected first-track tone.
            let wav = fs::read(&output).unwrap();
            let audible_samples = wav[wav.len() - 16_000..]
                .chunks_exact(2)
                .filter(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 1000)
                .count();
            assert!(
                audible_samples > 1000,
                "The inspected tone must remain audible"
            );
        }
    }

    #[test]
    #[ignore = "Requires prepared platform FFmpeg runtime; run explicitly with --ignored"]
    fn native_ffmpeg_accepts_short_track_but_rejects_corrupt_partial_decode() {
        use std::io::Write;
        let platform = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let ffmpeg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(platform)
            .join(crate::paths::exe_name("ffmpeg"));
        assert!(
            ffmpeg.is_file(),
            "Install the runtime before this native QA test"
        );
        let root =
            std::env::temp_dir().join(format!("whispertube-ffmpeg-duration-{}", Uuid::new_v4()));
        let _guard = JobDirectoryGuard::create(root.clone()).unwrap();
        for corrupt in [false, true] {
            let input = root.join("source.wav");
            let output = root.join("audio.wav");
            let mut file = fs::File::create(&input).unwrap();
            checkpoint::write_wav_header(&mut file, if corrupt { 60 * 32_000 } else { 32_000 })
                .unwrap();
            file.write_all(&vec![0; 32_000]).unwrap();
            drop(file);
            let mut command = super::ffmpeg_command(&ffmpeg, &input, &output, 60.0);
            let (status, _) = crate::process::run_with_timeout_captured_cancelable(
                &mut command,
                std::time::Duration::from_secs(10),
                4096,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(status.success(), !corrupt, "corrupt={corrupt}");
            if !corrupt {
                let (_, source) = crate::sources::local::local_source(&input).unwrap();
                let local = crate::sources::local::local_path(&source).unwrap().unwrap();
                let duration = crate::sources::probe_local_duration(
                    &ffmpeg,
                    &local,
                    &Arc::new(Mutex::new(None)),
                    &Arc::new(AtomicBool::new(false)),
                )
                .unwrap();
                assert!((duration - 1.0).abs() < 0.01);
                let audio = checkpoint::PcmAudio::open(&output, 60.0, || false).unwrap();
                assert_eq!(
                    super::reconciled_audio_duration(&audio, 60.0),
                    (1.0, Some(60.0))
                );
            }
        }
        // A longer source must not be cut to the metadata duration and then
        // accepted as complete. The bounded sentinel is rejected instead.
        let input = root.join("long-source.wav");
        let output = root.join("long-audio.wav");
        let mut file = fs::File::create(&input).unwrap();
        checkpoint::write_wav_header(&mut file, 4 * 32_000).unwrap();
        file.write_all(&vec![0; 4 * 32_000]).unwrap();
        drop(file);
        let mut command = super::ffmpeg_command(&ffmpeg, &input, &output, 1.0);
        let (status, _) = crate::process::run_with_timeout_captured_cancelable(
            &mut command,
            std::time::Duration::from_secs(10),
            4096,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(status.success());
        assert!(super::validate_audio_wav(&output, 1.0).is_err());
        assert!(checkpoint::PcmAudio::inspect(&output, 1.0, || false).is_err());
    }

    #[test]
    fn replaces_invalid_utf8_without_panicking() {
        assert_eq!(
            normalize_utf8_bytes(b"text: \xAE misalnya"),
            "text: � misalnya"
        );
    }

    #[test]
    fn recovered_audio_warning_survives_cache_serialization() {
        let root = std::env::temp_dir().join(format!("whispertube-recovery-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let request = TranscriptRequest {
            url: "https://www.youtube.com/watch?v=test".into(),
            title: "test".into(),
            channel: "test".into(),
            duration: 1.0,
            cookies_path: None,
            backend: "cpu".into(),
            compute_device_id: None,
            language: "id".into(),
            model_id: "base".into(),
            keep_audio: false,
        };
        super::write_audio_cache_metadata_with_recovery(&root, &request, true, true).unwrap();
        let cache =
            super::read_audio_cache_metadata(&root.join(super::AUDIO_CACHE_METADATA_FILE)).unwrap();
        assert!(cache.audio_ready && cache.audio_recovered);
        let old: super::AudioCacheMetadata = serde_json::from_str(
            r#"{"version":2,"url":"url","title":"title","duration":1,"audio_ready":true}"#,
        )
        .unwrap();
        assert!(!old.audio_recovered);
        let old_result = serde_json::json!({
            "historyId": 1, "title": "test", "channel": "local", "language": "id", "duration": 1,
            "model": "base", "backend": "cpu", "segments": [], "text": "test",
            "txtPath": "", "srtPath": "", "vttPath": "", "audioPath": null
        });
        let mut result: crate::types::TranscriptResult =
            serde_json::from_value(old_result).unwrap();
        assert!(!result.audio_recovered);
        result.audio_recovered = true;
        let saved = serde_json::to_vec(&result).unwrap();
        assert!(
            serde_json::from_slice::<crate::types::TranscriptResult>(&saved)
                .unwrap()
                .audio_recovered
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "Requires prepared platform FFmpeg runtime; run explicitly with --ignored"]
    fn native_mp3_recovery_preserves_readable_audio_and_rejects_invalid_files() {
        use std::io::Write;
        use std::{
            path::{Path, PathBuf},
            process::Command,
        };
        let platform = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(platform)
            .join(crate::paths::exe_name("ffmpeg"));
        assert!(ffmpeg.is_file());
        let root =
            std::env::temp_dir().join(format!("whispertube-mp3-recovery-{}", Uuid::new_v4()));
        let _guard = JobDirectoryGuard::create(root.clone()).unwrap();
        let input = root.join("recording.mp3");
        let mut generate = Command::new(&ffmpeg);
        generate
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "320k",
            ])
            .arg(&input);
        assert!(generate.status().unwrap().success());
        let tracker = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let clean =
            crate::sources::probe_import_duration(&ffmpeg, &input, &tracker, &cancelled).unwrap();
        assert!(!clean.1);
        fs::OpenOptions::new()
            .append(true)
            .open(&input)
            .unwrap()
            .write_all(&[0u8; 1024])
            .unwrap();
        assert!(
            crate::sources::probe_local_duration(&ffmpeg, &input, &tracker, &cancelled).is_err()
        );
        let (duration, recovered) =
            crate::sources::probe_import_duration(&ffmpeg, &input, &tracker, &cancelled).unwrap();
        assert!(recovered);
        assert!((duration - clean.0).abs() < 0.1);
        let output = root.join("audio.wav");
        let mut convert = super::ffmpeg_decode_command(&ffmpeg, &input, &output, duration, true);
        let (status, _) = crate::process::run_with_timeout_captured_cancelable(
            &mut convert,
            std::time::Duration::from_secs(10),
            4096,
            &cancelled,
        )
        .unwrap();
        assert!(status.success());
        super::validate_audio_wav(&output, duration).unwrap();
        checkpoint::PcmAudio::inspect(&output, duration, || false).unwrap();
        assert!(
            crate::sources::probe_local_duration(&ffmpeg, &output, &tracker, &cancelled).is_ok()
        );
        // One bad packet exceeds the recovery limit in a very short recording.
        let short = root.join("too-many-errors.mp3");
        let mut generate = std::process::Command::new(&ffmpeg);
        generate
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&input)
            .args(["-t", "0.1", "-c:a", "libmp3lame", "-b:a", "320k"])
            .arg(&short);
        assert!(generate.status().unwrap().success());
        fs::OpenOptions::new()
            .append(true)
            .open(&short)
            .unwrap()
            .write_all(&[0u8; 1024])
            .unwrap();
        assert!(
            crate::sources::probe_import_duration(&ffmpeg, &short, &tracker, &cancelled).is_err()
        );
        fs::write(&input, b"not audio").unwrap();
        assert!(
            crate::sources::probe_import_duration(&ffmpeg, &input, &tracker, &cancelled).is_err()
        );
        cancelled.store(true, Ordering::SeqCst);
        assert!(
            crate::sources::probe_import_duration(&ffmpeg, &output, &tracker, &cancelled)
                .unwrap_err()
                .contains("dibatalkan")
        );
        assert_eq!(*tracker.lock().unwrap(), None);

        // Optional private QA input is read only and never copied into the repo.
        if let Some(path) = std::env::var_os("WHISPERTUBE_QA_LOCAL_MP3") {
            cancelled.store(false, Ordering::SeqCst);
            let path = PathBuf::from(path);
            let (duration, recovered) =
                crate::sources::probe_import_duration(&ffmpeg, &path, &tracker, &cancelled)
                    .unwrap();
            assert!(recovered && duration > 0.0);
            let mut convert = super::ffmpeg_decode_command(&ffmpeg, &path, &output, duration, true);
            let (status, _) = crate::process::run_with_timeout_captured_cancelable(
                &mut convert,
                std::time::Duration::from_secs(60),
                4096,
                &cancelled,
            )
            .unwrap();
            assert!(status.success());
            super::validate_audio_wav(&output, duration).unwrap();
            checkpoint::PcmAudio::inspect(&output, duration, || false).unwrap();
        }
    }

    #[test]
    fn parses_whisper_result_with_bounded_typed_deserialization() {
        let path = std::env::temp_dir().join(format!("whispertube-result-{}.json", Uuid::new_v4()));
        fs::write(
            &path,
            br#"{"result":{"language":"en"},"transcription":[{"timestamps":{"from":"00:00:00,000","to":"00:00:01,000"},"text":" hello "}]}"#,
        )
        .unwrap();

        let parsed = parse_whisper_result(&path).expect("bounded result should parse");
        assert_eq!(parsed.0, "en");
        assert_eq!(parsed.1.len(), 1);
        assert_eq!(parsed.2, "hello");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn job_directory_is_removed_when_pipeline_does_not_commit() {
        let path = std::env::temp_dir().join(format!("whispertube-job-test-{}", Uuid::new_v4()));
        {
            let guard = JobDirectoryGuard::create(path.clone()).unwrap();
            assert!(path.join(".in-progress").exists());
            drop(guard);
        }
        assert!(!path.exists());
    }

    #[test]
    fn job_directory_with_saved_audio_is_preserved_for_retry() {
        let path =
            std::env::temp_dir().join(format!("whispertube-job-cache-test-{}", Uuid::new_v4()));
        {
            let mut guard = JobDirectoryGuard::create(path.clone()).unwrap();
            fs::write(path.join("audio.wav"), b"cached audio").unwrap();
            guard.preserve_on_failure();
        }
        assert!(path.exists());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn resume_restores_measured_duration_and_reuses_completed_checkpoints() {
        for measured in [620.03, 621.5, 622.0, 618.5] {
            let root =
                std::env::temp_dir().join(format!("whispertube-duration-{}", Uuid::new_v4()));
            let _root_guard = JobDirectoryGuard::create(root.clone()).unwrap();
            let job = root.join("saved-job");
            let _job_guard = JobDirectoryGuard::create(job.clone()).unwrap();
            let mut request = TranscriptRequest {
                url: "https://www.linkedin.com/posts/resume-test".into(),
                title: "Cached video".into(),
                channel: "Test channel".into(),
                duration: measured,
                cookies_path: None,
                backend: "cpu".into(),
                compute_device_id: None,
                language: "id".into(),
                model_id: "base".into(),
                keep_audio: false,
            };
            let wav = job.join("audio.wav");
            let bytes = (measured * checkpoint::SAMPLE_RATE as f64).round() as u64 * 2;
            let mut file = fs::File::create(&wav).unwrap();
            checkpoint::write_wav_header(&mut file, bytes).unwrap();
            file.set_len(bytes + 44).unwrap();
            drop(file);
            write_audio_cache_metadata(&job, &request, true).unwrap();
            let audio = checkpoint::PcmAudio::open(&wav, measured, || false).unwrap();
            let output = || checkpoint::ChunkOutput {
                language: "id".into(),
                backend: "cpu".into(),
                segments: vec![crate::types::Segment {
                    from: "00:00:00,000".into(),
                    to: "00:00:01,000".into(),
                    text: "saved chunk".into(),
                }],
            };
            let mut calls = 0;
            assert!(checkpoint::transcribe(
                &audio,
                &job,
                &request,
                |_, _, _, _| {
                    calls += 1;
                    if calls == 2 {
                        Err("inference interrupted".into())
                    } else {
                        Ok(output())
                    }
                },
                || false,
                |_, _| {},
            )
            .is_err());
            drop(audio);

            // The frontend retries with its original inspection metadata.
            request.duration = 620.0;
            assert_eq!(
                super::resume_stage_in(&root, &request.url, request.duration, &HashSet::new())
                    .unwrap(),
                Some(crate::types::TranscriptionResumeStage::Audio)
            );
            let (resumed_job, ready) =
                find_reusable_audio_job(&root, &mut request, &HashSet::new())
                    .unwrap()
                    .unwrap();
            assert!(ready);
            assert_eq!(request.duration.to_bits(), measured.to_bits());
            super::validate_audio_wav(&resumed_job.join("audio.wav"), request.duration).unwrap();
            let audio = checkpoint::PcmAudio::open(
                &resumed_job.join("audio.wav"),
                request.duration,
                || false,
            )
            .unwrap();
            let mut rerun = 0;
            let mut reused = 0;
            checkpoint::transcribe(
                &audio,
                &resumed_job,
                &request,
                |_, _, _, _| {
                    rerun += 1;
                    Ok(output())
                },
                || false,
                |_, saved| reused += usize::from(saved),
            )
            .unwrap();
            assert_eq!(reused, 1, "measured duration {measured}");
            assert_eq!(rerun, 2, "measured duration {measured}");
        }
    }

    #[test]
    fn resume_preserves_inspected_duration_until_audio_is_ready_and_rejects_changed_sources() {
        let root = std::env::temp_dir().join(format!("whispertube-duration-{}", Uuid::new_v4()));
        let _root_guard = JobDirectoryGuard::create(root.clone()).unwrap();
        let job = root.join("saved-job");
        let _job_guard = JobDirectoryGuard::create(job.clone()).unwrap();
        let mut request = TranscriptRequest {
            url: "https://www.linkedin.com/posts/resume-test".into(),
            title: "Cached video".into(),
            channel: "Test channel".into(),
            duration: 621.5,
            cookies_path: None,
            backend: "cpu".into(),
            compute_device_id: None,
            language: "id".into(),
            model_id: "base".into(),
            keep_audio: false,
        };
        write_audio_cache_metadata(&job, &request, false).unwrap();
        request.duration = 620.0;
        let (_, ready) = find_reusable_audio_job(&root, &mut request, &HashSet::new())
            .unwrap()
            .unwrap();
        assert!(!ready);
        assert_eq!(request.duration, 620.0);
        for changed in [619.499, 623.501] {
            request.duration = changed;
            assert!(
                find_reusable_audio_job(&root, &mut request, &HashSet::new())
                    .unwrap()
                    .is_none()
            );
            assert_eq!(request.duration, changed);
        }
    }

    #[test]
    fn finds_matching_saved_audio_job_without_reusing_another_video() {
        let root =
            std::env::temp_dir().join(format!("whispertube-audio-cache-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let job = root.join("saved-job");
        {
            let mut guard = JobDirectoryGuard::create(job.clone()).unwrap();
            let mut audio = fs::File::create(job.join("audio.wav")).unwrap();
            crate::checkpoint::write_wav_header(&mut audio, 32_000).unwrap();
            audio.set_len(32_044).unwrap();
            drop(audio);
            let request = TranscriptRequest {
                url: "https://www.youtube.com/watch?v=cache-test".into(),
                title: "Cached video".into(),
                channel: "Test channel".into(),
                duration: 60.0,
                cookies_path: None,
                backend: "cpu".into(),
                compute_device_id: None,
                language: "auto".into(),
                model_id: "base".into(),
                keep_audio: true,
            };
            write_audio_cache_metadata(&job, &request, true).unwrap();
            guard.preserve_on_failure();
        }
        let matching_request = TranscriptRequest {
            url: "https://www.youtube.com/watch?v=cache-test".into(),
            title: "Cached video".into(),
            channel: "Test channel".into(),
            duration: 60.0,
            cookies_path: None,
            backend: "cpu".into(),
            compute_device_id: None,
            language: "auto".into(),
            model_id: "base".into(),
            keep_audio: true,
        };
        let other_request = TranscriptRequest {
            url: "https://www.youtube.com/watch?v=other-video".into(),
            ..matching_request.clone()
        };

        assert_eq!(
            find_reusable_audio_job(&root, &mut matching_request.clone(), &HashSet::new()).unwrap(),
            Some((fs::canonicalize(&job).unwrap(), true))
        );
        use crate::types::TranscriptionResumeStage;
        let stage = |url: &str, committed: &HashSet<std::path::PathBuf>| {
            super::resume_stage_in(&root, url, matching_request.duration, committed).unwrap()
        };
        assert_eq!(
            stage(&matching_request.url, &HashSet::new()),
            Some(TranscriptionResumeStage::Audio)
        );
        assert_eq!(stage(&other_request.url, &HashSet::new()), None);
        assert_eq!(
            find_reusable_audio_job(&root, &mut other_request.clone(), &HashSet::new()).unwrap(),
            None
        );
        // Resume is independent of the preference to keep audio after success.
        let no_keep_request = TranscriptRequest {
            keep_audio: false,
            ..matching_request.clone()
        };
        // Legacy audio-only caches remain reusable after upgrading.
        fs::write(
            job.join(super::AUDIO_CACHE_METADATA_FILE),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "url": matching_request.url, "duration": matching_request.duration
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            find_reusable_audio_job(&root, &mut no_keep_request.clone(), &HashSet::new()).unwrap(),
            Some((fs::canonicalize(&job).unwrap(), true))
        );
        // An interrupted download has metadata but no completed WAV yet.
        let saved_audio = fs::read(job.join("audio.wav")).unwrap();
        fs::remove_file(job.join("audio.wav")).unwrap();
        write_audio_cache_metadata(&job, &matching_request, false).unwrap();
        // Metadata alone is not saved work: a cancellation before download
        // writes bytes must not turn the UI action into Continue.
        assert_eq!(stage(&matching_request.url, &HashSet::new()), None);
        fs::write(job.join("source.webm.part"), b"partial download").unwrap();
        assert_eq!(
            stage(&matching_request.url, &HashSet::new()),
            Some(TranscriptionResumeStage::Download)
        );
        fs::write(job.join("source.webm.part"), b"").unwrap();
        assert_eq!(stage(&matching_request.url, &HashSet::new()), None);
        assert_eq!(
            find_reusable_audio_job(&root, &mut no_keep_request.clone(), &HashSet::new()).unwrap(),
            Some((fs::canonicalize(&job).unwrap(), false))
        );
        fs::write(job.join("audio.wav"), saved_audio).unwrap();
        write_audio_cache_metadata(&job, &matching_request, true).unwrap();
        // A result written before a rolled-back commit must still resume.
        fs::write(job.join("result.json"), b"uncommitted result").unwrap();
        assert_eq!(
            find_reusable_audio_job(&root, &mut no_keep_request.clone(), &HashSet::new()).unwrap(),
            Some((fs::canonicalize(&job).unwrap(), true))
        );
        // A committed row, even with a marker left by a crash, prevents duplication.
        let committed = HashSet::from([fs::canonicalize(&job).unwrap()]);
        assert_eq!(stage(&matching_request.url, &committed), None);
        assert_eq!(
            find_reusable_audio_job(&root, &mut no_keep_request.clone(), &committed).unwrap(),
            None
        );
        fs::write(job.join("audio.wav"), b"truncated WAV").unwrap();
        assert_eq!(stage(&matching_request.url, &HashSet::new()), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn vulkan_auto_prefers_discrete_devices_before_integrated_devices() {
        let devices = vec![
            ComputeDeviceInfo {
                id: "vulkan:0".into(),
                backend: "vulkan".into(),
                name: "AMD iGPU".into(),
                vendor: "amd".into(),
                device_index: Some(0),
                integrated: true,
                total_memory_mb: None,
                free_memory_mb: None,
            },
            ComputeDeviceInfo {
                id: "vulkan:1".into(),
                backend: "vulkan".into(),
                name: "NVIDIA dGPU".into(),
                vendor: "nvidia".into(),
                device_index: Some(1),
                integrated: false,
                total_memory_mb: None,
                free_memory_mb: None,
            },
        ];

        assert_eq!(vulkan_device_indices(&devices), vec![1, 0]);
    }

    #[test]
    fn transcript_request_rejects_untrusted_enum_and_text_values() {
        let request = TranscriptRequest {
            url: "https://youtube.com/watch?v=test".into(),
            title: "x".repeat(4 * 1024 + 1),
            channel: "channel".into(),
            duration: 60.0,
            cookies_path: None,
            backend: "cpu".into(),
            compute_device_id: None,
            language: "en".into(),
            model_id: "base".into(),
            keep_audio: false,
        };
        assert!(validate_transcript_request(&request).is_err());

        let mut invalid_language = request.clone();
        invalid_language.title = "title".into();
        invalid_language.language = "shell-command".into();
        assert!(validate_transcript_request(&invalid_language).is_err());
    }

    #[test]
    fn qwen_request_validation_rejects_unsupported_backends_before_pipeline_work() {
        let mut request = TranscriptRequest {
            url: "https://youtube.com/watch?v=test".into(),
            title: "title".into(),
            channel: "channel".into(),
            duration: 60.0,
            cookies_path: None,
            backend: "auto".into(),
            compute_device_id: None,
            language: "auto".into(),
            model_id: crate::qwen::MODEL_ID.into(),
            keep_audio: false,
        };
        let supported = crate::qwen::supported_backends();
        for backend in ["auto", "cpu", "cuda", "metal", "vulkan"] {
            request.backend = backend.into();
            assert_eq!(
                validate_transcript_request(&request).is_ok(),
                supported.contains(&backend),
                "{backend}"
            );
        }
        request.model_id = "base".into();
        // General Whisper validation must not inherit the Qwen-only restriction.
        for backend in ["auto", "cpu", "cuda", "metal", "vulkan"] {
            request.backend = backend.into();
            assert!(validate_transcript_request(&request).is_ok(), "{backend}");
        }
    }

    #[test]
    fn cuda_vram_guard_runs_before_capability_probe() {
        let probe_called = Arc::new(AtomicBool::new(false));
        let probe_called_by_closure = Arc::clone(&probe_called);
        let gpu = crate::system::GpuInfo {
            name: "Test NVIDIA".into(),
            total_memory_mb: Some(8192),
            free_memory_mb: Some(1024),
            uuid: Some("GPU-aaaaaaaa".into()),
            device_index: Some(0),
        };

        let error = run_cuda_probe_with_vram_guard("large-v3-q5_0", &gpu, || {
            probe_called_by_closure.store(true, Ordering::SeqCst);
            Ok(())
        })
        .expect_err("low VRAM should stop the probe");

        assert!(error.contains("membutuhkan"));
        assert!(!probe_called.load(Ordering::SeqCst));
    }

    #[test]
    fn transient_vulkan_probe_failure_is_retried_and_not_cached() {
        let cache = Arc::new(Mutex::new(HashSet::new()));
        let mut attempts = 0;
        let signature = "engine:model".to_string();

        let first = run_cached_probe(&cache, signature.clone(), || {
            attempts += 1;
            Err("Job dibatalkan.".into())
        });
        let second = run_cached_probe(&cache, signature, || {
            attempts += 1;
            Err("capability probe Vulkan melewati batas waktu.".into())
        });

        assert!(first.is_err());
        assert!(second.is_err());
        assert_eq!(attempts, 2);
        assert!(cache.lock().unwrap().is_empty());
    }

    #[test]
    fn capability_probe_requires_the_requested_gpu_backend_to_initialize() {
        let cuda_zero = "whisper_backend_init_gpu: using CUDA0 backend\n";
        let cuda_one = "whisper_backend_init_gpu: using CUDA1 backend\n";
        assert!(super::require_gpu_backend_attestation(cuda_zero, "cuda", Some(0)).is_ok());
        assert!(super::require_gpu_backend_attestation(cuda_one, "cuda", Some(1)).is_ok());
        assert!(super::require_gpu_backend_attestation(cuda_zero, "cuda", Some(1)).is_err());
        assert!(super::require_gpu_backend_attestation(
            "whisper_backend_init_gpu: no GPU found\n",
            "cuda",
            Some(0)
        )
        .is_err());
        assert!(super::require_gpu_backend_attestation(
            "whisper_backend_init_gpu: using CUDA0 backend\nwhisper_backend_init_gpu: failed to initialize CUDA0 backend\n",
            "cuda",
            Some(0)
        )
        .is_err());
        assert!(super::require_gpu_backend_attestation(cuda_zero, "vulkan", Some(0)).is_err());
        assert!(super::require_gpu_backend_attestation(
            "whisper_backend_init_gpu: using MTL0 backend\n",
            "metal",
            None,
        )
        .is_ok());
        assert!(super::require_gpu_backend_attestation(
            "whisper_backend_init_gpu: using MTL0 backend\nwhisper_backend_init_gpu: failed to initialize MTL0 backend\n", "metal", None,
        ).is_err());
        let vulkan_one = "whisper_backend_init_gpu: using Vulkan1 backend\n";
        assert!(super::require_gpu_backend_attestation(vulkan_one, "vulkan", Some(1)).is_ok());
        assert!(super::require_gpu_backend_attestation(vulkan_one, "vulkan", Some(0)).is_err());
        assert!(super::require_gpu_backend_attestation(
            "whisper_backend_init_gpu: using Vulkan1 backend\nwhisper_backend_init_gpu: failed to initialize Vulkan1 backend\n", "vulkan", Some(1),
        ).is_err());
        for backend in ["metal", "vulkan"] {
            assert!(super::require_gpu_backend_attestation(
                "whisper_backend_init_gpu: no GPU found\n",
                backend,
                None,
            )
            .is_err());
        }
    }

    #[test]
    fn auto_continues_to_another_cuda_device_after_a_probe_failure() {
        let devices = vec![
            crate::system::GpuInfo {
                name: "First GPU".into(),
                total_memory_mb: Some(8192),
                free_memory_mb: Some(4096),
                uuid: Some("GPU-aaaaaaaa".into()),
                device_index: Some(2),
            },
            crate::system::GpuInfo {
                name: "Second GPU".into(),
                total_memory_mb: Some(12288),
                free_memory_mb: Some(8192),
                uuid: Some("GPU-bbbbbbbb".into()),
                device_index: Some(0),
            },
        ];
        let mut attempts = Vec::new();
        let mut failures = Vec::new();
        let selected = super::try_cuda_devices(
            &devices,
            |gpu| {
                let index = gpu.device_index.unwrap();
                attempts.push(index);
                if index == 2 {
                    Err("capability probe failed".into())
                } else {
                    Ok(gpu.name.clone())
                }
            },
            || false,
            &mut failures,
        )
        .unwrap();

        assert_eq!(attempts, vec![2, 0]);
        assert_eq!(selected.as_deref(), Some("Second GPU"));
        assert_eq!(
            failures,
            vec!["cuda:2 (First GPU): capability probe failed"]
        );
    }

    #[test]
    fn successful_vulkan_probe_is_cached() {
        let cache = Arc::new(Mutex::new(HashSet::new()));
        let mut attempts = 0;
        let signature = "engine:model".to_string();

        run_cached_probe(&cache, signature.clone(), || {
            attempts += 1;
            Ok(())
        })
        .unwrap();
        run_cached_probe(&cache, signature, || {
            attempts += 1;
            Ok(())
        })
        .unwrap();

        assert_eq!(attempts, 1);
    }

    #[test]
    fn conversion_limit_includes_a_sentinel_beyond_the_accepted_duration() {
        assert_eq!(ffmpeg_target_duration(0.25), 3.25);
        assert_eq!(ffmpeg_target_duration(12.0), 15.0);
        assert_eq!(
            ffmpeg_target_duration(super::MAX_MEDIA_DURATION_SECONDS),
            super::MAX_MEDIA_DURATION_SECONDS + 1.0
        );
    }

    #[test]
    fn local_audio_reserves_wav_and_safety_buffer_without_a_download() {
        for duration in [1.0, 60.0, super::MAX_MEDIA_DURATION_SECONDS] {
            let local = super::media_disk_reservation(duration, true);
            assert_eq!(
                local,
                super::pcm_wav_bytes(duration) + DISK_SAFETY_BUFFER_BYTES
            );
            assert!(local < 1024 * 1024 * 1024 + DISK_SAFETY_BUFFER_BYTES);
            assert_eq!(
                super::media_disk_reservation(duration, false),
                local + super::MAX_MEDIA_DOWNLOAD_BYTES_VALUE
            );
        }
    }

    #[test]
    fn periodic_disk_reservation_counts_only_remaining_bytes() {
        let required = remaining_disk_reservation(600, 1_000, 200);
        assert_eq!(required, 400 + 200 + DISK_SAFETY_BUFFER_BYTES);
    }

    #[test]
    fn cpu_watchdog_allows_longer_silent_work() {
        assert_eq!(whisper_inactivity_timeout("cpu"), CPU_INACTIVITY_TIMEOUT);
        assert_eq!(
            whisper_inactivity_timeout("vulkan"),
            PROCESS_INACTIVITY_TIMEOUT
        );
    }

    #[test]
    fn cpu_thread_selection_uses_more_cores_without_becoming_unbounded() {
        assert_eq!(whisper_thread_count(4, "cpu"), 4);
        assert_eq!(whisper_thread_count(32, "cpu"), 16);
        assert_eq!(whisper_thread_count(32, "cuda"), 12);
    }
}
