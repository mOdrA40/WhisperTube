use regex::Regex;
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::{
    browsers::browser_args,
    history, models,
    paths::{engine_path, jobs_dir, model_path, tool_path},
    system::{detect_gpu, detect_nvidia, UsageMonitor},
    types::{ProgressPayload, Segment, TranscriptRequest, TranscriptResult},
    youtube::validate_youtube_url,
};

fn emit_progress(
    app: &AppHandle,
    usage: &UsageMonitor,
    stage: &str,
    percent: f64,
    message: impl Into<String>,
    backend: Option<&str>,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    network_bytes_per_second: Option<u64>,
) {
    let usage_snapshot = usage.snapshot();
    let _ = app.emit(
        "job-progress",
        ProgressPayload {
            stage: stage.to_string(),
            percent: percent.clamp(0.0, 100.0),
            message: message.into(),
            backend: backend.map(str::to_string),
            downloaded_bytes,
            total_bytes,
            network_bytes_per_second,
            cpu_usage_percent: usage_snapshot.cpu_usage_percent,
            gpu_usage_percent: usage_snapshot.gpu_usage_percent,
        },
    );
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

fn normalize_output_utf8(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("Gagal membaca output Whisper: {e}"))?;
    let normalized = normalize_utf8_bytes(&bytes);
    if normalized.as_bytes() != bytes.as_slice() {
        fs::write(path, normalized.as_bytes())
            .map_err(|e| format!("Gagal menormalkan output Whisper: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod output_tests {
    use super::normalize_utf8_bytes;

    #[test]
    fn replaces_invalid_utf8_without_panicking() {
        assert_eq!(
            normalize_utf8_bytes(b"text: \xAE misalnya"),
            "text: � misalnya"
        );
    }
}

fn run_download(
    app: &AppHandle,
    usage: &UsageMonitor,
    request: &TranscriptRequest,
    job_dir: &Path,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let yt_dlp = tool_path(app, "yt-dlp")?;
    let template = job_dir.join("source.%(ext)s");
    let mut command = Command::new(yt_dlp);
    command
        .args([
            "--ignore-config",
            "--no-playlist",
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
        .args(browser_args(
            &request.browser,
            request.browser_profile.as_deref(),
        )?)
        .arg(&request.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Gagal menjalankan yt-dlp: {e}"))?;
    set_active_pid(active_pid, Some(child.id()));
    let stdout = child
        .stdout
        .take()
        .ok_or("Tidak bisa membaca progress yt-dlp")?;
    let progress_re =
        Regex::new(r"WT_PROGRESS=\s*([0-9.]+)%\|([0-9]+|NA)\|([0-9]+|NA)\|([0-9]+|NA)").unwrap();
    let download_started_at = Instant::now();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(caps) = progress_re.captures(&line) {
            let downloaded_bytes = caps.get(2).and_then(|value| value.as_str().parse().ok());
            let total_bytes = caps
                .get(3)
                .and_then(|value| value.as_str().parse().ok())
                .or_else(|| caps.get(4).and_then(|value| value.as_str().parse().ok()));
            let network_bytes_per_second = downloaded_bytes.and_then(|downloaded| {
                let elapsed = download_started_at.elapsed().as_secs_f64();
                (elapsed > 0.0).then(|| (downloaded as f64 / elapsed) as u64)
            });
            if let Ok(percent) = caps[1].parse::<f64>() {
                emit_progress(
                    app,
                    usage,
                    "downloading",
                    percent,
                    "Mengunduh best available audio dari YouTube…",
                    None,
                    downloaded_bytes,
                    total_bytes,
                    network_bytes_per_second,
                );
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
    }
    let mut stderr = String::new();
    if let Some(mut error) = child.stderr.take() {
        let _ = error.read_to_string(&mut stderr);
    }
    let status = child
        .wait()
        .map_err(|e| format!("Gagal menunggu yt-dlp: {e}"))?;
    set_active_pid(active_pid, None);
    if !status.success() {
        return Err(process_failed(cancelled, stderr, "Download"));
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
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| "yt-dlp selesai tetapi file audio sumber tidak ditemukan.".into())
}

fn run_ffmpeg(
    app: &AppHandle,
    usage: &UsageMonitor,
    input: &Path,
    output: &Path,
    duration: f64,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), String> {
    let ffmpeg = tool_path(app, "ffmpeg")?;
    let mut child = Command::new(ffmpeg)
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
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Gagal menjalankan FFmpeg: {e}"))?;
    set_active_pid(active_pid, Some(child.id()));
    let stdout = child
        .stdout
        .take()
        .ok_or("Tidak bisa membaca progress FFmpeg")?;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(raw) = line.strip_prefix("out_time_us=") {
            if let Ok(microseconds) = raw.parse::<f64>() {
                let seconds = microseconds / 1_000_000.0;
                let percent = if duration > 0.0 {
                    seconds / duration * 100.0
                } else {
                    0.0
                };
                emit_progress(
                    app,
                    usage,
                    "converting",
                    percent,
                    "Konversi ke PCM 16 kHz mono…",
                    None,
                    None,
                    None,
                    None,
                );
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
    }
    let mut stderr = String::new();
    if let Some(mut error) = child.stderr.take() {
        let _ = error.read_to_string(&mut stderr);
    }
    let status = child
        .wait()
        .map_err(|e| format!("Gagal menunggu FFmpeg: {e}"))?;
    set_active_pid(active_pid, None);
    if !status.success() {
        return Err(process_failed(cancelled, stderr, "FFmpeg"));
    }
    Ok(())
}

fn choose_backend(app: &AppHandle, requested: &str) -> Result<(String, PathBuf), String> {
    let cpu = engine_path(app, "cpu")?;
    let cuda = engine_path(app, "cuda")?;
    let metal = engine_path(app, "metal")?;
    let vulkan = engine_path(app, "vulkan")?;
    let nvidia = detect_nvidia().is_some();
    let gpu_detected = detect_gpu().is_some();
    match requested {
        "cpu" => {
            if !cpu.exists() {
                Err("CPU whisper engine belum terpasang.".into())
            } else {
                Ok(("cpu".into(), cpu))
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
                Ok(("cuda".into(), cuda))
            }
        }
        "metal" => {
            if !cfg!(target_os = "macos") {
                Err("Apple Metal hanya tersedia di macOS.".into())
            } else if !metal.exists() {
                Err("Metal engine belum terpasang. Install Apple Metal dari Settings terlebih dahulu.".into())
            } else {
                Ok(("metal".into(), metal))
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
                Ok(("vulkan".into(), vulkan))
            }
        }
        "auto" => {
            if nvidia && cfg!(all(target_os = "windows", target_arch = "x86_64")) && cuda.exists() {
                Ok(("cuda".into(), cuda))
            } else if cfg!(target_os = "macos") && metal.exists() {
                Ok(("metal".into(), metal))
            } else if cfg!(any(target_os = "windows", target_os = "linux"))
                && gpu_detected
                && vulkan.exists()
            {
                Ok(("vulkan".into(), vulkan))
            } else if nvidia && cfg!(all(target_os = "windows", target_arch = "x86_64")) {
                Err("NVIDIA GPU terdeteksi tetapi CUDA engine belum terpasang. Pasang CUDA acceleration terlebih dahulu.".into())
            } else if cpu.exists() {
                Ok(("cpu".into(), cpu))
            } else {
                Err("Tidak ada whisper engine yang siap digunakan.".into())
            }
        }
        _ => Err("Compute backend tidak dikenal.".into()),
    }
}

fn run_whisper(
    app: &AppHandle,
    usage: &UsageMonitor,
    wav: &Path,
    output_prefix: &Path,
    model: &Path,
    language: &str,
    backend: &str,
    model_id: &str,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let (resolved_backend, engine) = choose_backend(app, backend)?;
    if resolved_backend == "cuda" {
        let gpu = detect_nvidia().ok_or_else(|| {
            "NVIDIA GPU tidak lagi terdeteksi. Pilih CPU atau periksa driver.".to_string()
        })?;
        models::ensure_vram_available(model_id, gpu.available_memory_mb())?;
    }
    let threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
        .clamp(1, 12);
    let mut command = Command::new(engine);
    command
        .args(["-m"])
        .arg(model)
        .args(["-f"])
        .arg(wav)
        .args(["-l", language, "-t"])
        .arg(threads.to_string())
        .args(["-pp", "-ojf", "-otxt", "-osrt", "-ovtt", "-of"])
        .arg(output_prefix)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if resolved_backend == "cpu" {
        command.arg("-ng");
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Gagal menjalankan whisper.cpp: {e}"))?;
    set_active_pid(active_pid, Some(child.id()));
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or("Tidak bisa membaca progress whisper.cpp")?;
    let progress_re = Regex::new(r"progress\s*=\s*([0-9]+)%").unwrap();
    let mut stderr_all = String::new();
    for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
        stderr_all.push_str(&line);
        stderr_all.push('\n');
        if let Some(caps) = progress_re.captures(&line) {
            if let Ok(percent) = caps[1].parse::<f64>() {
                emit_progress(
                    app,
                    usage,
                    "transcribing",
                    percent,
                    format!(
                        "Whisper sedang bekerja via {}…",
                        resolved_backend.to_uppercase()
                    ),
                    Some(&resolved_backend),
                    None,
                    None,
                    None,
                );
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("Gagal menunggu whisper.cpp: {e}"))?;
    set_active_pid(active_pid, None);
    if !status.success() {
        return Err(process_failed(cancelled, stderr_all, "Whisper"));
    }
    Ok(resolved_backend)
}

fn parse_whisper_result(path: &Path) -> Result<(String, Vec<Segment>, String), String> {
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
    request: TranscriptRequest,
) -> Result<TranscriptResult, String> {
    cancelled.store(false, Ordering::SeqCst);
    validate_youtube_url(&request.url)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    let ffmpeg = tool_path(&app, "ffmpeg")?;
    if !yt_dlp.exists() || !ffmpeg.exists() {
        return Err("Runtime belum lengkap. Jalankan scripts/setup-windows.ps1.".into());
    }
    let model = model_path(&app, &request.model_id)?;
    if !model.exists() {
        return Err("Model belum diunduh. Unduh model dari UI terlebih dahulu.".into());
    }
    let jobs_dir = jobs_dir(&app)?;
    let job_dir = jobs_dir.join(Uuid::new_v4().to_string());
    fs::create_dir_all(&job_dir).map_err(|e| format!("Gagal membuat job directory: {e}"))?;
    let usage_monitor = UsageMonitor::start();

    emit_progress(
        &app,
        &usage_monitor,
        "downloading",
        0.0,
        "Menyiapkan download…",
        None,
        None,
        None,
        None,
    );
    let source = run_download(
        &app,
        &usage_monitor,
        &request,
        &job_dir,
        &active_pid,
        &cancelled,
    )?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }

    let wav = job_dir.join("audio.wav");
    emit_progress(
        &app,
        &usage_monitor,
        "converting",
        0.0,
        "Menormalisasi audio untuk Whisper…",
        None,
        None,
        None,
        None,
    );
    run_ffmpeg(
        &app,
        &usage_monitor,
        &source,
        &wav,
        request.duration,
        &active_pid,
        &cancelled,
    )?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }

    let output_prefix = job_dir.join("transcript");
    emit_progress(
        &app,
        &usage_monitor,
        "transcribing",
        0.0,
        "Memuat model Whisper…",
        None,
        None,
        None,
        None,
    );
    let resolved_backend = run_whisper(
        &app,
        &usage_monitor,
        &wav,
        &output_prefix,
        &model,
        &request.language,
        &request.backend,
        &request.model_id,
        &active_pid,
        &cancelled,
    )?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }

    for extension in ["json", "txt", "srt", "vtt"] {
        normalize_output_utf8(&output_prefix.with_extension(extension))?;
    }

    emit_progress(
        &app,
        &usage_monitor,
        "finalizing",
        92.0,
        "Merapikan transcript dan menyimpan history…",
        Some(&resolved_backend),
        None,
        None,
        None,
    );
    let json_path = output_prefix.with_extension("json");
    let txt_path = output_prefix.with_extension("txt");
    let srt_path = output_prefix.with_extension("srt");
    let vtt_path = output_prefix.with_extension("vtt");
    let (language, segments, text) = parse_whisper_result(&json_path)?;
    let result_store_path = job_dir.join("result.json");
    let history_id = history::save_history_record(
        &app,
        &request,
        &language,
        &resolved_backend,
        &result_store_path,
    )?;
    let result = TranscriptResult {
        history_id,
        title: request.title.clone(),
        channel: request.channel.clone(),
        language,
        duration: request.duration,
        model: request.model_id.clone(),
        backend: resolved_backend.clone(),
        segments,
        text,
        txt_path: txt_path.to_string_lossy().to_string(),
        srt_path: srt_path.to_string_lossy().to_string(),
        vtt_path: vtt_path.to_string_lossy().to_string(),
    };
    fs::write(
        &result_store_path,
        serde_json::to_vec_pretty(&result).map_err(|e| format!("Gagal serialize hasil: {e}"))?,
    )
    .map_err(|e| format!("Gagal menyimpan result.json: {e}"))?;

    if !request.keep_audio {
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(&wav);
    }
    emit_progress(
        &app,
        &usage_monitor,
        "done",
        100.0,
        "Transkripsi selesai.",
        Some(&resolved_backend),
        None,
        None,
        None,
    );
    Ok(result)
}
