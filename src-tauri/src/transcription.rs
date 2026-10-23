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
    sources::{source_js_runtime_args, validate_media_duration, validate_media_url},
    system::{
        cuda_driver_is_compatible, detect_gpu, detect_nvidia, detect_nvidia_device,
        detect_nvidia_driver_version, ensure_cuda_install_supported, UsageMonitor,
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
        crate::durable::create_dir_all(&path)
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
    let metadata = AudioCacheMetadata {
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
    request: &TranscriptRequest,
    committed_jobs: &HashSet<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    find_reusable_audio_job_for_source(jobs_dir, &request.url, request.duration, committed_jobs)
}

fn find_reusable_audio_job_for_source(
    jobs_dir: &Path,
    url: &str,
    duration: f64,
    committed_jobs: &HashSet<PathBuf>,
) -> Result<Option<PathBuf>, String> {
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
            || validate_media_url(&cache.url).ok().as_deref() != Some(url)
            || !cache.duration.is_finite()
            || cache.duration <= 0.0
            || cache.duration > MAX_MEDIA_DURATION_SECONDS
            || (cache.duration - duration).abs() > 1.0
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
            if checkpoint::PcmAudio::inspect(&audio, duration, || false).is_err() {
                continue;
            }
        }
        let modified = fs::metadata(&canonical_job_dir)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, canonical_job_dir));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    Ok(candidates.into_iter().next().map(|(_, path)| path))
}

fn resume_stage_in(
    root: &Path,
    url: &str,
    duration: f64,
    committed: &HashSet<PathBuf>,
) -> Result<Option<crate::types::TranscriptionResumeStage>, String> {
    use crate::types::TranscriptionResumeStage;
    let Some(job) = find_reusable_audio_job_for_source(root, url, duration, committed)? else {
        return Ok(None);
    };
    if read_audio_cache_metadata(&job.join(AUDIO_CACHE_METADATA_FILE))
        .is_some_and(|cache| cache.audio_ready)
    {
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
    let url = validate_media_url(url)?;
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
    Ok(())
}

fn pcm_wav_bytes(duration: f64) -> u64 {
    duration.max(0.0).ceil() as u64 * 16_000 * 2 + 44
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

fn media_disk_reservation(duration: f64) -> u64 {
    MAX_MEDIA_DOWNLOAD_BYTES_VALUE
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
    duration.max(MINIMUM_AUDIO_DURATION_SECONDS)
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
        let detail = String::from_utf8_lossy(&stderr);
        let label = Path::new(command.get_program())
            .file_name()
            .unwrap_or(command.get_program())
            .to_string_lossy();
        if context.is_cancelled() || detail.trim().is_empty() {
            format!("{label}: {error}")
        } else {
            format!("{label}: {error} Detail: {}", detail.trim())
        }
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

fn run_download(
    context: &JobContext,
    request: &TranscriptRequest,
    job_dir: &Path,
) -> Result<PathBuf, String> {
    let yt_dlp = tool_path(context.app, "yt-dlp")?;
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
        .arg(format!(
            "!is_live & duration <= {}",
            MAX_MEDIA_DURATION_SECONDS as u64
        ))
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
    Ok(source)
}

fn download_speed(raw: &str) -> Option<u64> {
    raw.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as u64)
}

fn run_ffmpeg(
    context: &JobContext,
    input: &Path,
    output: &Path,
    duration: f64,
) -> Result<(), String> {
    let ffmpeg = tool_path(context.app, "ffmpeg")?;
    let mut command = Command::new(ffmpeg);
    process::hide_console(&mut command);
    command
        .args([
            "-y",
            "-nostats",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-i",
        ])
        .arg(input)
        .args(["-t"])
        .arg(format!("{:.3}", ffmpeg_target_duration(duration)))
        .args(["-af", "apad=whole_dur=1"])
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
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
        return Err(process_failed(context.cancelled, stderr, "FFmpeg"));
    }
    Ok(())
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
        if backend == "vulkan" {
            configure_vulkan_command(&mut command);
        }
        command
            .args(["-m"])
            .arg(model)
            .args(["-f"])
            .arg(&wav)
            .args(["-l", "auto", "-t", "1", "-nt", "-np"]);
        if backend == "vulkan" {
            command.arg("-nfa");
        }
        if let Some(device_index) = device_index {
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
        Ok(())
    })();
    let _ = fs::remove_dir_all(&probe_dir);
    result
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
                Err("CUDA dipilih tetapi NVIDIA GPU/driver tidak terdeteksi.".into())
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
                run_cuda_probe_with_vram_guard(model_id, &gpu, || {
                    ensure_backend_probe(context, "cuda", &cuda, model, gpu.device_index)
                })?;
                Ok(("cuda".into(), cuda, None, gpu.device_index))
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
                    format!("Vulkan dipilih tetapi capability probe gagal: {error}")
                })?;
                Ok(("vulkan".into(), vulkan, None, Some(device_index)))
            }
        }
        "auto" => {
            let nvidia = detect_nvidia().is_some();
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
    }
    context.usage.set_target(&resolved_backend, device_index);
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
        if let Some(device_index) = device_index {
            command.arg("-dev").arg(device_index.to_string());
        }
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
    Ok(resolved_backend)
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
    request.url = validate_media_url(&request.url)?;
    validate_transcript_request(&request)?;
    validate_media_duration(request.duration, false)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    let ffmpeg = tool_path(&app, "ffmpeg")?;
    if !is_regular_file(&yt_dlp) || !is_regular_file(&ffmpeg) {
        return Err("Runtime belum lengkap. Jalankan scripts/setup-windows.ps1.".into());
    }
    let model = model_path(&app, &request.model_id)?;
    if !is_regular_file(&model) {
        return Err("Model belum diunduh. Unduh model dari UI terlebih dahulu.".into());
    }
    models::verify_model_file(&model, &request.model_id, &cancelled)?;
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
    let reusable_job_dir = find_reusable_audio_job(&jobs_dir, &request, &committed_jobs)?;
    let audio_ready = reusable_job_dir
        .as_ref()
        .and_then(|path| read_audio_cache_metadata(&path.join(AUDIO_CACHE_METADATA_FILE)))
        .is_some_and(|cache| cache.audio_ready);
    if !audio_ready {
        crate::resources::require_memory(inference_required, "transkripsi")?;
    }
    let disk_reservation = if audio_ready {
        DISK_SAFETY_BUFFER_BYTES
    } else if let Some(path) = &reusable_job_dir {
        media_disk_reservation(request.duration)
            .saturating_sub(directory_file_bytes(path)?)
            .max(DISK_SAFETY_BUFFER_BYTES)
    } else {
        media_disk_reservation(request.duration)
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
        let source = run_download(&context, &request, &job_dir)?;
        if context.is_cancelled() {
            return Err("Job dibatalkan.".into());
        }

        context.emit(ProgressUpdate {
            stage: "converting",
            message_code: "normalizing_audio",
            percent: 0.0,
            message: "Menormalisasi audio untuk Whisper…".into(),
            backend: None,
            downloaded_bytes: None,
            total_bytes: None,
            network_bytes_per_second: None,
        });
        run_ffmpeg(&context, &source, &wav, request.duration)?;
        ensure_job_storage_quota(&app)?;
        validate_audio_wav(&wav, request.duration)?;
        if context.is_cancelled() {
            return Err("Job dibatalkan.".into());
        }
        // Commit readiness only after FFmpeg succeeded and the WAV validates.
        // A killed conversion is never eligible as completed audio.
        checkpoint::PcmAudio::inspect(&wav, request.duration, || context.is_cancelled())?;
        write_audio_cache_metadata(&job_dir, &request, true)?;
    };
    // Retry cleanup even when a completed WAV bypasses download/conversion.
    history::cleanup_source_audio(&job_dir)?;

    let output_prefix = job_dir.join("transcript");
    context.emit(ProgressUpdate {
        stage: "transcribing",
        message_code: "loading_model",
        percent: 0.0,
        message: "Memuat model Whisper…".into(),
        backend: None,
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    let audio = checkpoint::PcmAudio::open(&wav, request.duration, || context.is_cancelled())?;
    let mut resolved_engine = None;
    let completed = checkpoint::transcribe(
        &audio,
        &job_dir,
        &request,
        |chunk_wav, prefix, language, span| {
            let required = fs::metadata(chunk_wav)
                .map_err(|e| format!("Gagal membaca potongan audio: {e}"))?.len()
                .saturating_mul(2).saturating_add(model_bytes).saturating_add(512 * 1024 * 1024);
            resources::require_memory(required, "inference potongan Whisper")?;
            let chunk_context = JobContext { progress_start: span.0, progress_end: span.1, ..context };
            let backend = run_whisper(
                &chunk_context, chunk_wav, prefix, &model,
                WhisperConfig { language, backend: &request.backend,
                    compute_device_id: request.compute_device_id.as_deref(), model_id: &request.model_id },
                &mut resolved_engine,
            )?;
            normalize_output_utf8(&prefix.with_extension("json"), checkpoint::MAX_CHECKPOINT_BYTES)?;
            let (language, segments, _) = parse_whisper_result(&prefix.with_extension("json"))?;
            Ok(checkpoint::ChunkOutput { language, backend, segments })
        },
        || context.is_cancelled(),
        |percent, reused| {
            context.emit(ProgressUpdate {
                stage: "transcribing",
                message_code: if reused { "resuming_transcription" } else { "checkpoint_progress" },
                percent,
                message: if reused { "Melanjutkan hasil bagian yang sudah tersimpan.".into() }
                    else { "Memproses audio per bagian; progres selesai disimpan otomatis.".into() },
                backend: None, downloaded_bytes: None, total_bytes: None, network_bytes_per_second: None,
            });
        },
    ).map_err(|error| format!("{error} Audio dan checkpoint yang berhasil disimpan tetap tersedia. Percobaan dengan sumber dan pengaturan yang sama dapat memakai ulang progres tersebut."))?;
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
        transcript_available: true,
        history_id: 0,
        title: request.title.clone(),
        channel: request.channel.clone(),
        language: language.clone(),
        duration: request.duration,
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
    let history_id = history::save_history_record_with(
        &app,
        &request,
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
    use crate::types::TranscriptRequest;
    use std::{
        collections::HashSet,
        fs,
        sync::atomic::{AtomicBool, Ordering},
        sync::{Arc, Mutex},
    };
    use uuid::Uuid;

    #[test]
    fn replaces_invalid_utf8_without_panicking() {
        assert_eq!(
            normalize_utf8_bytes(b"text: \xAE misalnya"),
            "text: � misalnya"
        );
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
            find_reusable_audio_job(&root, &matching_request, &HashSet::new()).unwrap(),
            Some(fs::canonicalize(&job).unwrap())
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
            find_reusable_audio_job(&root, &other_request, &HashSet::new()).unwrap(),
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
            find_reusable_audio_job(&root, &no_keep_request, &HashSet::new()).unwrap(),
            Some(fs::canonicalize(&job).unwrap())
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
            find_reusable_audio_job(&root, &no_keep_request, &HashSet::new()).unwrap(),
            Some(fs::canonicalize(&job).unwrap())
        );
        fs::write(job.join("audio.wav"), saved_audio).unwrap();
        write_audio_cache_metadata(&job, &matching_request, true).unwrap();
        // A result written before a rolled-back commit must still resume.
        fs::write(job.join("result.json"), b"uncommitted result").unwrap();
        assert_eq!(
            find_reusable_audio_job(&root, &no_keep_request, &HashSet::new()).unwrap(),
            Some(fs::canonicalize(&job).unwrap())
        );
        // A committed row, even with a marker left by a crash, prevents duplication.
        let committed = HashSet::from([fs::canonicalize(&job).unwrap()]);
        assert_eq!(stage(&matching_request.url, &committed), None);
        assert_eq!(
            find_reusable_audio_job(&root, &no_keep_request, &committed).unwrap(),
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
    fn cuda_vram_guard_runs_before_capability_probe() {
        let probe_called = Arc::new(AtomicBool::new(false));
        let probe_called_by_closure = Arc::clone(&probe_called);
        let gpu = crate::system::GpuInfo {
            name: "Test NVIDIA".into(),
            total_memory_mb: Some(8192),
            free_memory_mb: Some(1024),
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
    fn short_audio_is_padded_to_the_safe_minimum_duration() {
        assert_eq!(ffmpeg_target_duration(0.25), 1.0);
        assert_eq!(ffmpeg_target_duration(12.0), 12.0);
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
