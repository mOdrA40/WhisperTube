use regex::Regex;
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Instant,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::{
    browsers::cookie_args,
    history, models,
    paths::{engine_path, jobs_dir, model_path, tool_path},
    process, resources,
    sources::{js_runtime_args, validate_media_duration, validate_media_url},
    state::VulkanProbeResult,
    system::{detect_gpu, detect_nvidia, UsageMonitor},
    types::{
        ProgressPayload, Segment, TranscriptRequest, TranscriptResult, MAX_TRANSCRIPT_RESULT_BYTES,
        MAX_TRANSCRIPT_SEGMENTS, MAX_TRANSCRIPT_TEXT_BYTES,
    },
};

struct JobContext<'a> {
    app: &'a AppHandle,
    usage: &'a UsageMonitor,
    active_pid: &'a Arc<Mutex<Option<u32>>>,
    cancelled: &'a Arc<AtomicBool>,
    vulkan_probe: &'a Arc<Mutex<Option<VulkanProbeResult>>>,
}

struct ProgressUpdate<'a> {
    stage: &'a str,
    percent: f64,
    message: String,
    backend: Option<&'a str>,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    network_bytes_per_second: Option<u64>,
}

impl JobContext<'_> {
    fn emit(&self, update: ProgressUpdate<'_>) {
        let usage_snapshot = self.usage.snapshot();
        let _ = self.app.emit(
            "job-progress",
            ProgressPayload {
                stage: update.stage.to_string(),
                percent: update.percent.clamp(0.0, 100.0),
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

fn set_active_pid(active_pid: &Arc<Mutex<Option<u32>>>, pid: Option<u32>) {
    if let Ok(mut slot) = active_pid.lock() {
        *slot = pid;
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
    let size = fs::metadata(path)
        .map_err(|e| format!("Gagal membaca ukuran output Whisper: {e}"))?
        .len();
    if size > max_bytes {
        return Err(format!(
            "Output Whisper {} melebihi batas ukuran aman.",
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("file")
                .to_uppercase()
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("Gagal membaca output Whisper: {e}"))?;
    let normalized = normalize_utf8_bytes(&bytes);
    if normalized.as_bytes() != bytes.as_slice() {
        fs::write(path, normalized.as_bytes())
            .map_err(|e| format!("Gagal menormalkan output Whisper: {e}"))?;
    }
    Ok(())
}

struct JobDirectoryGuard {
    path: PathBuf,
    marker: PathBuf,
    committed: bool,
}

impl JobDirectoryGuard {
    fn create(path: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&path).map_err(|e| format!("Gagal membuat job directory: {e}"))?;
        let marker = path.join(".in-progress");
        if let Err(error) = File::create(&marker) {
            let _ = fs::remove_dir_all(&path);
            return Err(format!("Gagal membuat marker job: {error}"));
        }
        Ok(Self {
            path,
            marker,
            committed: false,
        })
    }

    fn commit(mut self) {
        self.committed = true;
        let _ = fs::remove_file(&self.marker);
    }
}

impl Drop for JobDirectoryGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
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
        fs::rename(&temporary, path).map_err(|e| format!("Gagal mengaktifkan result.json: {e}"))?;
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

const MAX_CAPTURED_STDERR_BYTES: usize = 64 * 1024;
pub(crate) const MAX_MEDIA_DURATION_SECONDS: f64 = 2.0 * 60.0 * 60.0;
const MAX_MEDIA_DOWNLOAD_BYTES: &str = "4G";
const MAX_MEDIA_DOWNLOAD_BYTES_VALUE: u64 = 4 * 1024 * 1024 * 1024;
const DISK_SAFETY_BUFFER_BYTES: u64 = 512 * 1024 * 1024;
const DISK_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const PROCESS_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const CPU_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const PROCESS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const MINIMUM_AUDIO_DURATION_SECONDS: f64 = 1.0;

fn capture_stderr(mut reader: impl Read) -> Result<String, String> {
    let mut captured = Vec::with_capacity(MAX_CAPTURED_STDERR_BYTES);
    let mut buffer = [0u8; 8 * 1024];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("Gagal membaca stderr process: {error}")),
        };
        if read == 0 {
            break;
        }
        let remaining = MAX_CAPTURED_STDERR_BYTES.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    let mut output = String::from_utf8_lossy(&captured).into_owned();
    if truncated {
        output.push_str("\n[stderr dipotong karena terlalu panjang]");
    }
    Ok(output)
}

fn drain_stderr(stderr: ChildStderr) -> JoinHandle<Result<String, String>> {
    std::thread::spawn(move || capture_stderr(stderr))
}

fn join_stderr(handle: JoinHandle<Result<String, String>>) -> Result<String, String> {
    handle
        .join()
        .map_err(|_| "Reader stderr process gagal.".to_string())?
}

fn pcm_wav_bytes(duration: f64) -> u64 {
    duration.max(0.0).ceil() as u64 * 16_000 * 2 + 44
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

fn ffmpeg_target_duration(duration: f64) -> f64 {
    duration.max(MINIMUM_AUDIO_DURATION_SECONDS)
}

fn spawn_line_reader<R: Read + Send + 'static>(
    reader: R,
    label: &'static str,
) -> (Receiver<Result<String, String>>, JoinHandle<()>) {
    let (sender, receiver) = mpsc::sync_channel(128);
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let line = line.map_err(|error| format!("Gagal membaca output {label}: {error}"));
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    (receiver, reader)
}

fn wait_for_child_after_stream_closed(
    child: &mut Child,
    cancelled: &AtomicBool,
    last_activity: Instant,
    inactivity_timeout: std::time::Duration,
    label: &str,
) -> Result<(), String> {
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(error) => return Err(format!("Gagal memantau {label}: {error}")),
        }
        if last_activity.elapsed() >= inactivity_timeout {
            return Err(format!(
                "Process {label} tidak menghasilkan progress selama {} detik dan dihentikan.",
                inactivity_timeout.as_secs()
            ));
        }
        std::thread::sleep(PROCESS_POLL_INTERVAL);
    }
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

fn run_download(
    context: &JobContext,
    request: &TranscriptRequest,
    job_dir: &Path,
) -> Result<PathBuf, String> {
    let yt_dlp = tool_path(context.app, "yt-dlp")?;
    let template = job_dir.join("source.%(ext)s");
    let mut command = Command::new(yt_dlp);
    process::hide_console(&mut command);
    let cookie_args = cookie_args(
        &request.browser,
        request.browser_profile.as_deref(),
        request.cookies_path.as_deref(),
    )
    .map_err(|error| {
        if request.cookies_path.is_some() {
            format!("media_source_cookie_file:{error}")
        } else {
            error
        }
    })?;
    command
        .args(["--ignore-config", "--no-playlist", "--match-filter"])
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
            "download:WT_PROGRESS=%(progress._percent_str)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s",
            "-f",
            "bestaudio/best",
            "-o",
        ])
        .arg(&template)
        .args(js_runtime_args())
        .args(cookie_args)
        .arg(&request.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Gagal menjalankan yt-dlp: {e}"))?;
    let child_pid = child.id();
    set_active_pid(context.active_pid, Some(child_pid));
    if context.is_cancelled() {
        process::terminate_child(&mut child);
    }
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            process::terminate_child(&mut child);
            let _ = child.wait();
            set_active_pid(context.active_pid, None);
            return Err("Tidak bisa membaca error yt-dlp.".into());
        }
    };
    let stderr_reader = drain_stderr(stderr);
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            process::terminate_child(&mut child);
            let _ = child.wait();
            set_active_pid(context.active_pid, None);
            let _ = join_stderr(stderr_reader);
            return Err("Tidak bisa membaca progress yt-dlp".into());
        }
    };
    let progress_re =
        Regex::new(r"WT_PROGRESS=\s*([0-9.]+)%\|([0-9]+|NA)\|([0-9]+|NA)\|([0-9]+|NA)").unwrap();
    let download_started_at = Instant::now();
    let (stdout_receiver, stdout_reader) = spawn_line_reader(stdout, "progress yt-dlp");
    let mut last_activity = Instant::now();
    let mut last_disk_check = Instant::now() - DISK_CHECK_INTERVAL;
    let stdout_result = (|| -> Result<(), String> {
        loop {
            match stdout_receiver.recv_timeout(PROCESS_POLL_INTERVAL) {
                Ok(line) => {
                    let line = line?;
                    last_activity = Instant::now();
                    if let Some(caps) = progress_re.captures(&line) {
                        let downloaded_bytes =
                            caps.get(2).and_then(|value| value.as_str().parse().ok());
                        let total_bytes = caps
                            .get(3)
                            .and_then(|value| value.as_str().parse().ok())
                            .or_else(|| caps.get(4).and_then(|value| value.as_str().parse().ok()));
                        let network_bytes_per_second = downloaded_bytes.and_then(|downloaded| {
                            let elapsed = download_started_at.elapsed().as_secs_f64();
                            (elapsed > 0.0).then(|| (downloaded as f64 / elapsed) as u64)
                        });
                        if let Ok(percent) = caps[1].parse::<f64>() {
                            context.emit(ProgressUpdate {
                                stage: "downloading",
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
                            return Err(
                                "Download media melebihi batas ukuran yang diizinkan.".into()
                            );
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
                        last_disk_check = Instant::now();
                    }
                    if context.is_cancelled() {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if context.is_cancelled() {
                        break;
                    }
                    if child
                        .try_wait()
                        .map_err(|e| format!("Gagal memantau yt-dlp: {e}"))?
                        .is_some()
                    {
                        break;
                    }
                    if last_activity.elapsed() >= PROCESS_INACTIVITY_TIMEOUT {
                        return Err(format!(
                            "Process Download tidak menghasilkan progress selama {} detik dan dihentikan.",
                            PROCESS_INACTIVITY_TIMEOUT.as_secs()
                        ));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    wait_for_child_after_stream_closed(
                        &mut child,
                        context.cancelled,
                        last_activity,
                        PROCESS_INACTIVITY_TIMEOUT,
                        "Download",
                    )?;
                    break;
                }
            }
        }
        Ok(())
    })();
    if stdout_result.is_err() || context.is_cancelled() {
        process::terminate_child(&mut child);
    }
    drop(stdout_receiver);
    let _ = stdout_reader.join();
    let status = child
        .wait()
        .map_err(|e| format!("Gagal menunggu yt-dlp: {e}"));
    set_active_pid(context.active_pid, None);
    let stderr = join_stderr(stderr_reader)?;
    stdout_result?;
    let status = status?;
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

fn run_ffmpeg(
    context: &JobContext,
    input: &Path,
    output: &Path,
    duration: f64,
) -> Result<(), String> {
    let ffmpeg = tool_path(context.app, "ffmpeg")?;
    let mut command = Command::new(ffmpeg);
    process::hide_console(&mut command);
    let mut child = command
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
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Gagal menjalankan FFmpeg: {e}"))?;
    let child_pid = child.id();
    set_active_pid(context.active_pid, Some(child_pid));
    if context.is_cancelled() {
        process::terminate_child(&mut child);
    }
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            process::terminate_child(&mut child);
            let _ = child.wait();
            set_active_pid(context.active_pid, None);
            return Err("Tidak bisa membaca error FFmpeg.".into());
        }
    };
    let stderr_reader = drain_stderr(stderr);
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            process::terminate_child(&mut child);
            let _ = child.wait();
            set_active_pid(context.active_pid, None);
            let _ = join_stderr(stderr_reader);
            return Err("Tidak bisa membaca progress FFmpeg".into());
        }
    };
    let (stdout_receiver, stdout_reader) = spawn_line_reader(stdout, "progress FFmpeg");
    let mut last_activity = Instant::now();
    let mut last_disk_check = Instant::now() - DISK_CHECK_INTERVAL;
    let stdout_result = (|| -> Result<(), String> {
        loop {
            match stdout_receiver.recv_timeout(PROCESS_POLL_INTERVAL) {
                Ok(line) => {
                    let line = line?;
                    last_activity = Instant::now();
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
                                "WAV hasil konversi melebihi batas durasi media yang diizinkan."
                                    .into(),
                            );
                        }
                        resources::require_disk(
                            output_dir,
                            remaining_disk_reservation(written, maximum_wav, 0),
                            "konversi media",
                        )?;
                        last_disk_check = Instant::now();
                    }
                    if context.is_cancelled() {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if context.is_cancelled() {
                        break;
                    }
                    if child
                        .try_wait()
                        .map_err(|e| format!("Gagal memantau FFmpeg: {e}"))?
                        .is_some()
                    {
                        break;
                    }
                    if last_activity.elapsed() >= PROCESS_INACTIVITY_TIMEOUT {
                        return Err(format!(
                            "Process FFmpeg tidak menghasilkan progress selama {} detik dan dihentikan.",
                            PROCESS_INACTIVITY_TIMEOUT.as_secs()
                        ));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    wait_for_child_after_stream_closed(
                        &mut child,
                        context.cancelled,
                        last_activity,
                        PROCESS_INACTIVITY_TIMEOUT,
                        "FFmpeg",
                    )?;
                    break;
                }
            }
        }
        Ok(())
    })();
    if stdout_result.is_err() || context.is_cancelled() {
        process::terminate_child(&mut child);
    }
    drop(stdout_receiver);
    let _ = stdout_reader.join();
    let status = child
        .wait()
        .map_err(|e| format!("Gagal menunggu FFmpeg: {e}"));
    set_active_pid(context.active_pid, None);
    let stderr = join_stderr(stderr_reader)?;
    stdout_result?;
    let status = status?;
    if !status.success() {
        return Err(process_failed(context.cancelled, stderr, "FFmpeg"));
    }
    Ok(())
}

const VULKAN_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn write_vulkan_probe_wav(path: &Path) -> Result<(), String> {
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

fn run_vulkan_probe(context: &JobContext, engine: &Path, model: &Path) -> Result<(), String> {
    let probe_dir =
        std::env::temp_dir().join(format!("whispertube-vulkan-probe-{}", Uuid::new_v4()));
    fs::create_dir_all(&probe_dir)
        .map_err(|e| format!("Gagal membuat folder probe Vulkan: {e}"))?;
    let wav = probe_dir.join("silence.wav");
    let output = probe_dir.join("probe");
    let result = (|| -> Result<(), String> {
        write_vulkan_probe_wav(&wav)?;
        let mut command = Command::new(engine);
        process::hide_console(&mut command);
        command
            .args(["-m"])
            .arg(model)
            .args(["-f"])
            .arg(&wav)
            .args(["-l", "auto", "-t", "1", "-nt", "-np", "-nfa", "-of"])
            .arg(&output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| format!("engine Vulkan tidak bisa dijalankan: {e}"))?;
        let child_pid = child.id();
        set_active_pid(context.active_pid, Some(child_pid));
        let stderr = match child.stderr.take() {
            Some(stderr) => drain_stderr(stderr),
            None => {
                process::terminate_child(&mut child);
                let _ = child.wait();
                set_active_pid(context.active_pid, None);
                return Err(
                    "engine Vulkan tidak menyediakan stderr untuk capability probe.".into(),
                );
            }
        };
        let started = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if context.is_cancelled() => {
                    process::terminate_child(&mut child);
                    let _ = child.wait();
                    set_active_pid(context.active_pid, None);
                    let _ = join_stderr(stderr);
                    return Err("Job dibatalkan.".into());
                }
                Ok(None) if started.elapsed() >= VULKAN_PROBE_TIMEOUT => {
                    process::terminate_child(&mut child);
                    let _ = child.wait();
                    set_active_pid(context.active_pid, None);
                    let stderr = join_stderr(stderr)?;
                    let detail = if stderr.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" Detail: {}", stderr.trim())
                    };
                    return Err(format!(
                        "capability probe Vulkan melewati batas waktu.{detail}"
                    ));
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(error) => {
                    process::terminate_child(&mut child);
                    let _ = child.wait();
                    set_active_pid(context.active_pid, None);
                    let _ = join_stderr(stderr);
                    return Err(format!(
                        "Status capability probe Vulkan tidak bisa dibaca: {error}"
                    ));
                }
            }
        };
        set_active_pid(context.active_pid, None);
        let stderr = join_stderr(stderr)?;
        if !status.success() {
            return Err(process_failed(
                context.cancelled,
                stderr,
                "Capability probe Vulkan",
            ));
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&probe_dir);
    result
}

fn ensure_vulkan_probe(context: &JobContext, engine: &Path, model: &Path) -> Result<(), String> {
    let signature = vulkan_probe_signature(engine, model);
    run_cached_vulkan_probe(context.vulkan_probe, signature, || {
        run_vulkan_probe(context, engine, model)
    })
}

fn run_cached_vulkan_probe<F>(
    cache: &Arc<Mutex<Option<VulkanProbeResult>>>,
    signature: String,
    probe: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if cache
        .lock()
        .map_err(|_| "Cache probe Vulkan terkunci")?
        .as_ref()
        .is_some_and(|cached| cached.signature == signature)
    {
        return Ok(());
    }

    let result = probe();
    if result.is_ok() {
        *cache.lock().map_err(|_| "Cache probe Vulkan terkunci")? =
            Some(VulkanProbeResult { signature });
    }
    result
}

fn choose_backend(
    context: &JobContext,
    requested: &str,
    model: &Path,
) -> Result<(String, PathBuf, Option<String>, Option<usize>), String> {
    let app = context.app;
    let cpu = engine_path(app, "cpu")?;
    let cuda = engine_path(app, "cuda")?;
    let metal = engine_path(app, "metal")?;
    let vulkan = engine_path(app, "vulkan")?;
    let nvidia_gpu = detect_nvidia();
    let nvidia = nvidia_gpu.is_some();
    let gpu_detected = nvidia || detect_gpu().is_some();
    match requested {
        "cpu" => {
            if !cpu.exists() {
                Err("CPU whisper engine belum terpasang.".into())
            } else {
                Ok(("cpu".into(), cpu, None, None))
            }
        }
        "cuda" => {
            if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
                Err("CUDA hanya tersedia pada build Windows yang didukung.".into())
            } else if !nvidia {
                Err("CUDA dipilih tetapi NVIDIA GPU/driver tidak terdeteksi.".into())
            } else if !cuda.exists() {
                Err("CUDA engine belum terpasang. Install CUDA acceleration dari Settings terlebih dahulu.".into())
            } else {
                Ok((
                    "cuda".into(),
                    cuda,
                    None,
                    nvidia_gpu.as_ref().and_then(|gpu| gpu.device_index),
                ))
            }
        }
        "metal" => {
            if !cfg!(target_os = "macos") {
                Err("Apple Metal hanya tersedia di macOS.".into())
            } else if !metal.exists() {
                Err("Metal engine belum terpasang. Install Apple Metal dari Settings terlebih dahulu.".into())
            } else {
                Ok(("metal".into(), metal, None, None))
            }
        }
        "vulkan" => {
            if !cfg!(any(target_os = "windows", target_os = "linux")) {
                Err("Vulkan accelerator saat ini tersedia di Windows/Linux.".into())
            } else if !gpu_detected {
                Err("Vulkan dipilih tetapi GPU tidak terdeteksi pada perangkat ini.".into())
            } else if !vulkan.exists() {
                Err(
                    "Vulkan engine belum terpasang. Install Vulkan dari Settings terlebih dahulu."
                        .into(),
                )
            } else {
                ensure_vulkan_probe(context, &vulkan, model).map_err(|error| {
                    format!("Vulkan dipilih tetapi capability probe gagal: {error}")
                })?;
                Ok(("vulkan".into(), vulkan, None, None))
            }
        }
        "auto" => {
            if nvidia && cfg!(all(target_os = "windows", target_arch = "x86_64")) && cuda.exists() {
                Ok((
                    "cuda".into(),
                    cuda,
                    None,
                    nvidia_gpu.as_ref().and_then(|gpu| gpu.device_index),
                ))
            } else if cfg!(target_os = "macos") && metal.exists() {
                Ok(("metal".into(), metal, None, None))
            } else if cfg!(any(target_os = "windows", target_os = "linux"))
                && gpu_detected
                && vulkan.exists()
            {
                match ensure_vulkan_probe(context, &vulkan, model) {
                    Ok(()) => Ok(("vulkan".into(), vulkan, None, None)),
                    Err(error) if cpu.exists() => Ok((
                        "cpu".into(),
                        cpu,
                        Some(format!(
                            "Vulkan tidak lolos capability probe ({error}); transkripsi dilanjutkan dengan CPU."
                        )),
                        None,
                    )),
                    Err(error) => Err(format!(
                        "Vulkan tidak lolos capability probe dan CPU engine tidak tersedia: {error}"
                    )),
                }
            } else if nvidia && cfg!(all(target_os = "windows", target_arch = "x86_64")) {
                Err("NVIDIA GPU terdeteksi tetapi CUDA engine belum terpasang. Pasang CUDA acceleration terlebih dahulu.".into())
            } else if cpu.exists() {
                Ok(("cpu".into(), cpu, None, None))
            } else {
                Err("Tidak ada whisper engine yang siap digunakan.".into())
            }
        }
        _ => Err("Compute backend tidak dikenal.".into()),
    }
}

fn run_whisper(
    context: &JobContext,
    wav: &Path,
    output_prefix: &Path,
    model: &Path,
    language: &str,
    backend: &str,
    model_id: &str,
) -> Result<String, String> {
    let (resolved_backend, engine, fallback_message, device_index) =
        choose_backend(context, backend, model)?;
    if let Some(message) = fallback_message {
        context.emit(ProgressUpdate {
            stage: "transcribing",
            percent: 0.0,
            message,
            backend: Some("cpu"),
            downloaded_bytes: None,
            total_bytes: None,
            network_bytes_per_second: None,
        });
    }
    if resolved_backend == "cuda" {
        let gpu = detect_nvidia().ok_or_else(|| {
            "NVIDIA GPU tidak lagi terdeteksi. Pilih CPU atau periksa driver.".to_string()
        })?;
        models::ensure_vram_available(model_id, gpu.available_memory_mb())?;
    }
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
        .args(["-l", language, "-t"])
        .arg(threads.to_string())
        .args(["-pp", "-oj", "-otxt", "-osrt", "-ovtt", "-of"])
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
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Gagal menjalankan whisper.cpp: {e}"))?;
    let child_pid = child.id();
    set_active_pid(context.active_pid, Some(child_pid));
    if context.is_cancelled() {
        process::terminate_child(&mut child);
    }
    let stderr_pipe = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            process::terminate_child(&mut child);
            let _ = child.wait();
            set_active_pid(context.active_pid, None);
            return Err("Tidak bisa membaca progress whisper.cpp".into());
        }
    };
    let progress_re = Regex::new(r"progress\s*=\s*([0-9]+)%").unwrap();
    let mut stderr_all = Vec::with_capacity(MAX_CAPTURED_STDERR_BYTES);
    let mut stderr_truncated = false;
    let (stderr_receiver, stderr_reader) = spawn_line_reader(stderr_pipe, "progress whisper.cpp");
    let mut last_activity = Instant::now();
    let inactivity_timeout = whisper_inactivity_timeout(&resolved_backend);
    let stderr_result = (|| -> Result<(), String> {
        loop {
            match stderr_receiver.recv_timeout(PROCESS_POLL_INTERVAL) {
                Ok(line) => {
                    let line = line?;
                    last_activity = Instant::now();
                    let line_bytes = line.as_bytes();
                    let remaining = MAX_CAPTURED_STDERR_BYTES.saturating_sub(stderr_all.len());
                    let retained = remaining.min(line_bytes.len());
                    stderr_all.extend_from_slice(&line_bytes[..retained]);
                    if retained < line_bytes.len() || stderr_all.len() == MAX_CAPTURED_STDERR_BYTES
                    {
                        stderr_truncated = true;
                    } else {
                        stderr_all.push(b'\n');
                    }
                    if let Some(caps) = progress_re.captures(&line) {
                        if let Ok(percent) = caps[1].parse::<f64>() {
                            context.emit(ProgressUpdate {
                                stage: "transcribing",
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
                    if context.is_cancelled() {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if context.is_cancelled() {
                        break;
                    }
                    if child
                        .try_wait()
                        .map_err(|e| format!("Gagal memantau whisper.cpp: {e}"))?
                        .is_some()
                    {
                        break;
                    }
                    if last_activity.elapsed() >= inactivity_timeout {
                        return Err(format!(
                            "Process Whisper tidak menghasilkan progress selama {} detik dan dihentikan.",
                            inactivity_timeout.as_secs()
                        ));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    wait_for_child_after_stream_closed(
                        &mut child,
                        context.cancelled,
                        last_activity,
                        inactivity_timeout,
                        "Whisper",
                    )?;
                    break;
                }
            }
        }
        Ok(())
    })();
    if stderr_result.is_err() || context.is_cancelled() {
        process::terminate_child(&mut child);
    }
    drop(stderr_receiver);
    let _ = stderr_reader.join();
    let status = child
        .wait()
        .map_err(|e| format!("Gagal menunggu whisper.cpp: {e}"));
    set_active_pid(context.active_pid, None);
    stderr_result?;
    let status = status?;
    if !status.success() {
        let mut stderr = String::from_utf8_lossy(&stderr_all).into_owned();
        if stderr_truncated {
            stderr.push_str("\n[stderr dipotong karena terlalu panjang]");
        }
        return Err(process_failed(context.cancelled, stderr, "Whisper"));
    }
    Ok(resolved_backend)
}

fn parse_whisper_result(path: &Path) -> Result<(String, Vec<Segment>, String), String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("Gagal membaca ukuran output JSON Whisper: {e}"))?
        .len();
    if size > MAX_TRANSCRIPT_RESULT_BYTES {
        return Err("Output JSON Whisper melebihi batas ukuran aman.".into());
    }
    let file = File::open(path).map_err(|e| format!("Output JSON Whisper tidak ditemukan: {e}"))?;
    let value: Value =
        serde_json::from_reader(file).map_err(|e| format!("Output JSON Whisper rusak: {e}"))?;
    let language = value
        .get("result")
        .and_then(|result| result.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let mut segments = Vec::new();
    let mut total_text_bytes = 0usize;
    if let Some(items) = value.get("transcription").and_then(Value::as_array) {
        for item in items {
            let timestamps = item.get("timestamps");
            let from = timestamps
                .and_then(|value| value.get("from"))
                .and_then(Value::as_str)
                .unwrap_or("00:00:00,000")
                .to_string();
            let to = timestamps
                .and_then(|value| value.get("to"))
                .and_then(Value::as_str)
                .unwrap_or("00:00:00,000")
                .to_string();
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
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
    vulkan_probe: Arc<Mutex<Option<VulkanProbeResult>>>,
    request: TranscriptRequest,
) -> Result<TranscriptResult, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }
    validate_media_url(&request.url)?;
    validate_media_duration(request.duration, false)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    let ffmpeg = tool_path(&app, "ffmpeg")?;
    if !yt_dlp.exists() || !ffmpeg.exists() {
        return Err("Runtime belum lengkap. Jalankan scripts/setup-windows.ps1.".into());
    }
    let model = model_path(&app, &request.model_id)?;
    if !model.exists() {
        return Err("Model belum diunduh. Unduh model dari UI terlebih dahulu.".into());
    }
    models::verify_model_file(&model, &request.model_id)?;
    let jobs_dir = jobs_dir(&app)?;
    let duration_seconds = request.duration.max(0.0).ceil() as u64;
    let inference_bytes = duration_seconds.saturating_mul(16_000).saturating_mul(4);
    let model_bytes = model.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let inference_required = inference_bytes
        .saturating_add(model_bytes)
        .saturating_add(512 * 1024 * 1024);
    crate::resources::require_memory(inference_required, "transkripsi")?;
    crate::resources::require_disk(
        &jobs_dir,
        media_disk_reservation(request.duration),
        "download dan konversi media",
    )?;
    let job_dir = jobs_dir.join(Uuid::new_v4().to_string());
    let job_guard = JobDirectoryGuard::create(job_dir.clone())?;
    let usage_monitor = UsageMonitor::start();
    let context = JobContext {
        app: &app,
        usage: &usage_monitor,
        active_pid: &active_pid,
        cancelled: &cancelled,
        vulkan_probe: &vulkan_probe,
    };

    context.emit(ProgressUpdate {
        stage: "downloading",
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

    let wav = job_dir.join("audio.wav");
    context.emit(ProgressUpdate {
        stage: "converting",
        percent: 0.0,
        message: "Menormalisasi audio untuk Whisper…".into(),
        backend: None,
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    run_ffmpeg(&context, &source, &wav, request.duration)?;
    remove_file_if_present(&source, "audio sumber")?;
    if context.is_cancelled() {
        return Err("Job dibatalkan.".into());
    }
    let actual_wav_bytes = fs::metadata(&wav)
        .map_err(|e| format!("Gagal membaca ukuran WAV hasil konversi: {e}"))?
        .len();
    if actual_wav_bytes > pcm_wav_bytes(request.duration).saturating_add(4096) {
        return Err("WAV hasil konversi melebihi batas durasi media yang diizinkan.".into());
    }
    let actual_inference_required = actual_wav_bytes
        .saturating_mul(2)
        .saturating_add(model_bytes)
        .saturating_add(512 * 1024 * 1024);
    crate::resources::require_memory(actual_inference_required, "inference Whisper")?;

    let output_prefix = job_dir.join("transcript");
    context.emit(ProgressUpdate {
        stage: "transcribing",
        percent: 0.0,
        message: "Memuat model Whisper…".into(),
        backend: None,
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    let resolved_backend = run_whisper(
        &context,
        &wav,
        &output_prefix,
        &model,
        &request.language,
        &request.backend,
        &request.model_id,
    )?;
    if context.is_cancelled() {
        return Err("Job dibatalkan.".into());
    }

    for extension in ["json", "txt", "srt", "vtt"] {
        normalize_output_utf8(
            &output_prefix.with_extension(extension),
            MAX_TRANSCRIPT_RESULT_BYTES,
        )?;
    }

    context.emit(ProgressUpdate {
        stage: "finalizing",
        percent: 92.0,
        message: "Merapikan transcript dan menyimpan history…".into(),
        backend: Some(&resolved_backend),
        downloaded_bytes: None,
        total_bytes: None,
        network_bytes_per_second: None,
    });
    let json_path = output_prefix.with_extension("json");
    let txt_path = output_prefix.with_extension("txt");
    let srt_path = output_prefix.with_extension("srt");
    let vtt_path = output_prefix.with_extension("vtt");
    let (language, segments, text) = parse_whisper_result(&json_path)?;
    let result_store_path = job_dir.join("result.json");
    if !request.keep_audio {
        remove_file_if_present(&wav, "WAV hasil konversi")?;
    }
    let mut result = TranscriptResult {
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

    context.emit(ProgressUpdate {
        stage: "done",
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
    use super::{
        capture_stderr, ffmpeg_target_duration, normalize_utf8_bytes, remaining_disk_reservation,
        run_cached_vulkan_probe, whisper_inactivity_timeout, whisper_thread_count,
        JobDirectoryGuard, CPU_INACTIVITY_TIMEOUT, DISK_SAFETY_BUFFER_BYTES,
        MAX_CAPTURED_STDERR_BYTES, PROCESS_INACTIVITY_TIMEOUT,
    };
    use std::{
        io::Cursor,
        process::{Command, Stdio},
        sync::atomic::AtomicBool,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
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
    fn stderr_is_fully_drained_while_capture_is_bounded() {
        let bytes = vec![b'x'; MAX_CAPTURED_STDERR_BYTES + 4096];
        let mut reader = Cursor::new(bytes.clone());
        let output = capture_stderr(&mut reader).unwrap();
        assert_eq!(reader.position(), bytes.len() as u64);
        assert!(output.starts_with(&"x".repeat(MAX_CAPTURED_STDERR_BYTES)));
        assert!(output.ends_with("[stderr dipotong karena terlalu panjang]"));
    }

    #[test]
    fn transient_vulkan_probe_failure_is_retried_and_not_cached() {
        let cache = Arc::new(Mutex::new(None));
        let mut attempts = 0;
        let signature = "engine:model".to_string();

        let first = run_cached_vulkan_probe(&cache, signature.clone(), || {
            attempts += 1;
            Err("Job dibatalkan.".into())
        });
        let second = run_cached_vulkan_probe(&cache, signature, || {
            attempts += 1;
            Err("capability probe Vulkan melewati batas waktu.".into())
        });

        assert!(first.is_err());
        assert!(second.is_err());
        assert_eq!(attempts, 2);
        assert!(cache.lock().unwrap().is_none());
    }

    #[test]
    fn successful_vulkan_probe_is_cached() {
        let cache = Arc::new(Mutex::new(None));
        let mut attempts = 0;
        let signature = "engine:model".to_string();

        run_cached_vulkan_probe(&cache, signature.clone(), || {
            attempts += 1;
            Ok(())
        })
        .unwrap();
        run_cached_vulkan_probe(&cache, signature, || {
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

    #[test]
    fn closed_progress_stream_still_waits_for_normal_process_exit() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let cancelled = AtomicBool::new(false);

        super::wait_for_child_after_stream_closed(
            &mut child,
            &cancelled,
            Instant::now(),
            Duration::from_secs(5),
            "test",
        )
        .unwrap();
        assert!(child.wait().unwrap().success());
    }
}
