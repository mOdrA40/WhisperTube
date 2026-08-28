use chrono::Utc;
use regex::Regex;
use reqwest::blocking::Client;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, State};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
struct ModelSpec {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    size_mb: u64,
    sha1: &'static str,
}

const MODELS: [ModelSpec; 3] = [
    ModelSpec {
        id: "base",
        label: "Fast",
        description: "Ringan untuk CPU/laptop sederhana",
        size_mb: 142,
        sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
    },
    ModelSpec {
        id: "large-v3-turbo-q5_0",
        label: "Balanced",
        description: "Default: cepat dan akurat untuk penggunaan umum",
        size_mb: 547,
        sha1: "e050f7970618a659205450ad97eb95a18d69c9ee",
    },
    ModelSpec {
        id: "large-v3-q5_0",
        label: "Accurate",
        description: "Akurasi tinggi, lebih berat dan lambat",
        size_mb: 1100,
        sha1: "e6e2ed78495d403bef4b7cff42ef4aaadcfea8de",
    },
];

#[derive(Default)]
struct AppState {
    active_pid: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemStatus {
    yt_dlp: bool,
    ffmpeg: bool,
    cpu_engine: bool,
    cuda_engine: bool,
    nvidia: bool,
    gpu_name: Option<String>,
    cpu_threads: usize,
    recommendation: String,
    recommended_model_id: String,
    recommended_backend: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelInfo {
    id: String,
    label: String,
    description: String,
    size_mb: u64,
    installed: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VideoMetadata {
    id: String,
    title: String,
    channel: String,
    duration: f64,
    thumbnail: Option<String>,
    webpage_url: String,
    availability: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    stage: String,
    percent: f64,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadPayload {
    id: String,
    percent: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Segment {
    from: String,
    to: String,
    text: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscriptResult {
    history_id: i64,
    title: String,
    channel: String,
    language: String,
    duration: f64,
    model: String,
    backend: String,
    segments: Vec<Segment>,
    text: String,
    txt_path: String,
    srt_path: String,
    vtt_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: i64,
    title: String,
    channel: String,
    source_url: String,
    created_at: String,
    duration: f64,
    language: String,
    model: String,
    backend: String,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TranscriptRequest {
    url: String,
    title: String,
    channel: String,
    duration: f64,
    browser: String,
    backend: String,
    language: String,
    model_id: String,
    keep_audio: bool,
}

fn emit_progress(app: &AppHandle, stage: &str, percent: f64, message: impl Into<String>) {
    let _ = app.emit(
        "job-progress",
        ProgressPayload {
            stage: stage.to_string(),
            percent: percent.clamp(0.0, 100.0),
            message: message.into(),
        },
    );
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Tidak bisa menentukan app data directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat app data directory: {e}"))?;
    Ok(dir)
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("models");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder models: {e}"))?;
    Ok(dir)
}

fn runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let platform = "windows";
    #[cfg(target_os = "macos")]
    let platform = "macos";
    #[cfg(target_os = "linux")]
    let platform = "linux";

    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(platform))
    } else {
        Ok(app
            .path()
            .resource_dir()
            .map_err(|e| format!("Gagal menemukan resource directory: {e}"))?
            .join("runtime")
            .join(platform))
    }
}

fn exe_name(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn tool_path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(runtime_dir(app)?.join(exe_name(name)))
}

fn engine_path(app: &AppHandle, backend: &str) -> Result<PathBuf, String> {
    Ok(runtime_dir(app)?
        .join(backend)
        .join(exe_name("whisper-cli")))
}

fn model_spec(id: &str) -> Result<ModelSpec, String> {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .ok_or_else(|| format!("Model tidak dikenal: {id}"))
}

fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let _ = model_spec(id)?;
    Ok(models_dir(app)?.join(format!("ggml-{id}.bin")))
}

fn validate_youtube_url(raw: &str) -> Result<String, String> {
    let parsed = Url::parse(raw).map_err(|_| "URL tidak valid.".to_string())?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("URL harus menggunakan http/https.".into());
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = host == "youtu.be"
        || host == "youtube.com"
        || host == "www.youtube.com"
        || host == "m.youtube.com"
        || host == "music.youtube.com";
    if !allowed {
        return Err("Saat ini input URL dibatasi ke domain YouTube resmi.".into());
    }
    Ok(parsed.to_string())
}

fn browser_args(browser: &str) -> Result<Vec<String>, String> {
    match browser {
        "none" | "" => Ok(vec![]),
        "chrome" | "edge" | "firefox" | "brave" => Ok(vec![
            "--cookies-from-browser".into(),
            browser.to_string(),
        ]),
        _ => Err("Browser session tidak didukung.".into()),
    }
}

fn detect_nvidia() -> (bool, Option<String>) {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() {
                (false, None)
            } else {
                (true, Some(text.lines().next().unwrap_or_default().trim().to_string()))
            }
        }
        _ => (false, None),
    }
}

fn init_db(app: &AppHandle) -> Result<PathBuf, String> {
    let db_path = app_data_dir(app)?.join("whispertube.db");
    let conn = Connection::open(&db_path).map_err(|e| format!("Gagal membuka database: {e}"))?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            channel TEXT NOT NULL,
            source_url TEXT NOT NULL,
            created_at TEXT NOT NULL,
            duration REAL NOT NULL,
            language TEXT NOT NULL,
            model TEXT NOT NULL,
            backend TEXT NOT NULL,
            result_path TEXT NOT NULL
        );
        ",
    )
    .map_err(|e| format!("Gagal inisialisasi database: {e}"))?;
    Ok(db_path)
}

#[tauri::command]
fn system_status(app: AppHandle) -> Result<SystemStatus, String> {
    let runtime = runtime_dir(&app)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?.exists();
    let ffmpeg = tool_path(&app, "ffmpeg")?.exists();
    let cpu_engine = engine_path(&app, "cpu")?.exists();
    let cuda_engine = engine_path(&app, "cuda")?.exists();
    let (nvidia, gpu_name) = detect_nvidia();
    let cpu_threads = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);

    let (recommendation, recommended_model_id, recommended_backend) = if nvidia && cuda_engine {
        (
            format!(
                "{} terdeteksi. Auto akan memakai CUDA; Balanced direkomendasikan.",
                gpu_name.clone().unwrap_or_else(|| "NVIDIA GPU".into())
            ),
            "large-v3-turbo-q5_0".to_string(),
            "auto".to_string(),
        )
    } else if nvidia && !cuda_engine {
        (
            "NVIDIA terdeteksi, tetapi CUDA engine belum dipasang. CPU fallback aktif.".into(),
            if cpu_threads >= 12 { "large-v3-turbo-q5_0" } else { "base" }.to_string(),
            "auto".to_string(),
        )
    } else if cpu_threads >= 12 {
        (
            "CPU cukup kuat. Mulai dengan Balanced; gunakan Fast jika terlalu lambat.".into(),
            "large-v3-turbo-q5_0".to_string(),
            "auto".to_string(),
        )
    } else {
        (
            "Gunakan Fast untuk mesin ini agar responsif.".into(),
            "base".to_string(),
            "auto".to_string(),
        )
    };

    let _ = runtime; // menjaga error path tervalidasi
    Ok(SystemStatus {
        yt_dlp,
        ffmpeg,
        cpu_engine,
        cuda_engine,
        nvidia,
        gpu_name,
        cpu_threads,
        recommendation,
        recommended_model_id,
        recommended_backend,
    })
}

#[tauri::command]
fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    let dir = models_dir(&app)?;
    Ok(MODELS
        .iter()
        .map(|m| ModelInfo {
            id: m.id.into(),
            label: m.label.into(),
            description: m.description.into(),
            size_mb: m.size_mb,
            installed: dir.join(format!("ggml-{}.bin", m.id)).exists(),
        })
        .collect())
}

fn verify_sha1(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| format!("Gagal membuka file untuk verifikasi: {e}"))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| format!("Gagal membaca file: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "Checksum model tidak cocok. Expected {expected}, actual {actual}. File dihapus demi keamanan."
        ));
    }
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
async fn download_model(app: AppHandle, model_id: String) -> Result<(), String> {
    let spec = model_spec(&model_id)?;
    let dest = model_path(&app, &model_id)?;
    let app_for_task = app.clone();
    tokio::task::spawn_blocking(move || {
        if dest.exists() {
            return Ok(());
        }
        let temp = dest.with_extension("bin.download");
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin?download=true",
            spec.id
        );
        let client = Client::builder()
            .user_agent("WhisperTube/0.1")
            .build()
            .map_err(|e| format!("Gagal membuat HTTP client: {e}"))?;
        let mut response = client
            .get(url)
            .send()
            .map_err(|e| format!("Gagal mengunduh model: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("Server model mengembalikan HTTP {}", response.status()));
        }
        let total = response.content_length().unwrap_or(spec.size_mb * 1024 * 1024);
        let mut file = File::create(&temp).map_err(|e| format!("Gagal membuat file model: {e}"))?;
        let mut downloaded = 0u64;
        let mut buffer = [0u8; 1024 * 256];
        loop {
            let n = response
                .read(&mut buffer)
                .map_err(|e| format!("Download model terputus: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[..n])
                .map_err(|e| format!("Gagal menulis model: {e}"))?;
            downloaded += n as u64;
            let percent = if total > 0 {
                downloaded as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let _ = app_for_task.emit(
                "model-download",
                ModelDownloadPayload {
                    id: spec.id.into(),
                    percent: percent.clamp(0.0, 100.0),
                },
            );
        }
        drop(file);
        if let Err(e) = verify_sha1(&temp, spec.sha1) {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        fs::rename(&temp, &dest).map_err(|e| format!("Gagal memasang model: {e}"))?;
        let _ = app_for_task.emit(
            "model-download",
            ModelDownloadPayload {
                id: spec.id.into(),
                percent: 100.0,
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| format!("Download task gagal: {e}"))?
}

#[tauri::command(rename_all = "camelCase")]
fn delete_model(app: AppHandle, model_id: String) -> Result<(), String> {
    let path = model_path(&app, &model_id)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("Gagal menghapus model: {e}"))?;
    }
    Ok(())
}

fn run_output(mut command: Command) -> Result<std::process::Output, String> {
    command
        .output()
        .map_err(|e| format!("Gagal menjalankan process: {e}"))
}

#[tauri::command]
async fn inspect_youtube(app: AppHandle, url: String, browser: String) -> Result<VideoMetadata, String> {
    let safe_url = validate_youtube_url(&url)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    if !yt_dlp.exists() {
        return Err("yt-dlp belum terpasang. Jalankan scripts/setup-windows.ps1.".into());
    }
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(yt_dlp);
        cmd.args(["--ignore-config", "--dump-single-json", "--skip-download", "--no-playlist", "--no-warnings"]);
        cmd.args(browser_args(&browser)?);
        cmd.arg(&safe_url);
        let output = run_output(cmd)?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "YouTube metadata gagal dibaca. Jika ini video Member, pilih browser yang sudah login.".into()
            } else {
                format!("yt-dlp: {err}")
            });
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Output metadata yt-dlp tidak valid: {e}"))?;
        Ok(VideoMetadata {
            id: value.get("id").and_then(Value::as_str).unwrap_or("unknown").into(),
            title: value.get("title").and_then(Value::as_str).unwrap_or("Untitled video").into(),
            channel: value
                .get("channel")
                .or_else(|| value.get("uploader"))
                .and_then(Value::as_str)
                .unwrap_or("Unknown channel")
                .into(),
            duration: value.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
            thumbnail: value.get("thumbnail").and_then(Value::as_str).map(str::to_string),
            webpage_url: value
                .get("webpage_url")
                .and_then(Value::as_str)
                .unwrap_or(&safe_url)
                .to_string(),
            availability: value.get("availability").and_then(Value::as_str).map(str::to_string),
        })
    })
    .await
    .map_err(|e| format!("Metadata task gagal: {e}"))?
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

fn run_download(
    app: &AppHandle,
    request: &TranscriptRequest,
    job_dir: &Path,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let yt_dlp = tool_path(app, "yt-dlp")?;
    let template = job_dir.join("source.%(ext)s");
    let mut cmd = Command::new(yt_dlp);
    cmd.args([
        "--ignore-config",
        "--no-playlist",
        "--newline",
        "--quiet",
        "--progress",
        "--progress-template",
        "download:WT_PROGRESS=%(progress._percent_str)s",
        "-f",
        "bestaudio/best",
        "-o",
    ])
    .arg(&template)
    .args(browser_args(&request.browser)?)
    .arg(&request.url)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .stdin(Stdio::null());

    let mut child = cmd.spawn().map_err(|e| format!("Gagal menjalankan yt-dlp: {e}"))?;
    set_active_pid(active_pid, Some(child.id()));
    let stdout = child.stdout.take().ok_or("Tidak bisa membaca progress yt-dlp")?;
    let percent_re = Regex::new(r"WT_PROGRESS=\s*([0-9.]+)%").unwrap();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(caps) = percent_re.captures(&line) {
            if let Ok(p) = caps[1].parse::<f64>() {
                emit_progress(app, "downloading", p, "Mengunduh best available audio dari YouTube…");
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().map_err(|e| format!("Gagal menunggu yt-dlp: {e}"))?;
    set_active_pid(active_pid, None);
    if !status.success() {
        return Err(process_failed(cancelled, stderr, "Download"));
    }

    let mut candidates = fs::read_dir(job_dir)
        .map_err(|e| format!("Gagal membaca folder job: {e}"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with("source.") && !s.ends_with(".part") && !s.ends_with(".ytdl"))
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
    input: &Path,
    output: &Path,
    duration: f64,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), String> {
    let ffmpeg = tool_path(app, "ffmpeg")?;
    let mut child = Command::new(ffmpeg)
        .args(["-y", "-nostats", "-loglevel", "error", "-progress", "pipe:1", "-i"])
        .arg(input)
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Gagal menjalankan FFmpeg: {e}"))?;
    set_active_pid(active_pid, Some(child.id()));
    let stdout = child.stdout.take().ok_or("Tidak bisa membaca progress FFmpeg")?;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(raw) = line.strip_prefix("out_time_us=") {
            if let Ok(us) = raw.parse::<f64>() {
                let seconds = us / 1_000_000.0;
                let p = if duration > 0.0 { seconds / duration * 100.0 } else { 0.0 };
                emit_progress(app, "converting", p, "Konversi ke PCM 16 kHz mono…");
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().map_err(|e| format!("Gagal menunggu FFmpeg: {e}"))?;
    set_active_pid(active_pid, None);
    if !status.success() {
        return Err(process_failed(cancelled, stderr, "FFmpeg"));
    }
    Ok(())
}

fn choose_backend(app: &AppHandle, requested: &str) -> Result<(String, PathBuf), String> {
    let cpu = engine_path(app, "cpu")?;
    let cuda = engine_path(app, "cuda")?;
    let (nvidia, _) = detect_nvidia();
    match requested {
        "cpu" => {
            if !cpu.exists() {
                Err("CPU whisper engine belum terpasang.".into())
            } else {
                Ok(("cpu".into(), cpu))
            }
        }
        "cuda" => {
            if !nvidia {
                Err("CUDA dipilih tetapi NVIDIA GPU/driver tidak terdeteksi.".into())
            } else if !cuda.exists() {
                Err("CUDA engine belum terpasang. Jalankan scripts/install-cuda-engine.ps1.".into())
            } else {
                Ok(("cuda".into(), cuda))
            }
        }
        "auto" => {
            if nvidia && cuda.exists() {
                Ok(("cuda".into(), cuda))
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
    wav: &Path,
    output_prefix: &Path,
    model: &Path,
    language: &str,
    backend: &str,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, String> {
    let (resolved_backend, engine) = choose_backend(app, backend)?;
    let threads = std::thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(4)
        .clamp(1, 12);
    let mut cmd = Command::new(engine);
    cmd.args(["-m"])
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
        cmd.arg("-ng");
    }
    let mut child = cmd.spawn().map_err(|e| format!("Gagal menjalankan whisper.cpp: {e}"))?;
    set_active_pid(active_pid, Some(child.id()));
    let stderr_pipe = child.stderr.take().ok_or("Tidak bisa membaca progress whisper.cpp")?;
    let progress_re = Regex::new(r"progress\s*=\s*([0-9]+)%").unwrap();
    let mut stderr_all = String::new();
    for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
        stderr_all.push_str(&line);
        stderr_all.push('\n');
        if let Some(caps) = progress_re.captures(&line) {
            if let Ok(p) = caps[1].parse::<f64>() {
                emit_progress(
                    app,
                    "transcribing",
                    p,
                    format!("Whisper sedang bekerja via {}…", resolved_backend.to_uppercase()),
                );
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
    }
    let status = child.wait().map_err(|e| format!("Gagal menunggu whisper.cpp: {e}"))?;
    set_active_pid(active_pid, None);
    if !status.success() {
        return Err(process_failed(cancelled, stderr_all, "Whisper"));
    }
    Ok(resolved_backend)
}

fn parse_whisper_result(path: &Path) -> Result<(String, Vec<Segment>, String), String> {
    let file = File::open(path).map_err(|e| format!("Output JSON Whisper tidak ditemukan: {e}"))?;
    let value: Value = serde_json::from_reader(file).map_err(|e| format!("Output JSON Whisper rusak: {e}"))?;
    let language = value
        .get("result")
        .and_then(|v| v.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let mut segments = Vec::new();
    if let Some(items) = value.get("transcription").and_then(Value::as_array) {
        for item in items {
            let timestamps = item.get("timestamps");
            let from = timestamps
                .and_then(|t| t.get("from"))
                .and_then(Value::as_str)
                .unwrap_or("00:00:00,000")
                .to_string();
            let to = timestamps
                .and_then(|t| t.get("to"))
                .and_then(Value::as_str)
                .unwrap_or("00:00:00,000")
                .to_string();
            let text = item.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if !text.is_empty() {
                segments.push(Segment { from, to, text });
            }
        }
    }
    let text = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n");
    Ok((language, segments, text))
}

fn save_history_record(
    app: &AppHandle,
    request: &TranscriptRequest,
    language: &str,
    backend: &str,
    result_path: &Path,
) -> Result<i64, String> {
    let db_path = init_db(app)?;
    let conn = Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
    conn.execute(
        "INSERT INTO history (title, channel, source_url, created_at, duration, language, model, backend, result_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            request.title,
            request.channel,
            request.url,
            Utc::now().to_rfc3339(),
            request.duration,
            language,
            request.model_id,
            backend,
            result_path.to_string_lossy().to_string(),
        ],
    )
    .map_err(|e| format!("Gagal menyimpan history: {e}"))?;
    Ok(conn.last_insert_rowid())
}

fn pipeline(
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
    let jobs_dir = app_data_dir(&app)?.join("jobs");
    fs::create_dir_all(&jobs_dir).map_err(|e| format!("Gagal membuat folder jobs: {e}"))?;
    let job_dir = jobs_dir.join(Uuid::new_v4().to_string());
    fs::create_dir_all(&job_dir).map_err(|e| format!("Gagal membuat job directory: {e}"))?;

    emit_progress(&app, "downloading", 0.0, "Menyiapkan download…");
    let source = run_download(&app, &request, &job_dir, &active_pid, &cancelled)?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }

    let wav = job_dir.join("audio.wav");
    emit_progress(&app, "converting", 0.0, "Menormalisasi audio untuk Whisper…");
    run_ffmpeg(&app, &source, &wav, request.duration, &active_pid, &cancelled)?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }

    let output_prefix = job_dir.join("transcript");
    emit_progress(&app, "transcribing", 0.0, "Memuat model Whisper…");
    let resolved_backend = run_whisper(
        &app,
        &wav,
        &output_prefix,
        &model,
        &request.language,
        &request.backend,
        &active_pid,
        &cancelled,
    )?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Job dibatalkan.".into());
    }

    emit_progress(&app, "finalizing", 92.0, "Merapikan transcript dan menyimpan history…");
    let json_path = output_prefix.with_extension("json");
    let txt_path = output_prefix.with_extension("txt");
    let srt_path = output_prefix.with_extension("srt");
    let vtt_path = output_prefix.with_extension("vtt");
    let (language, segments, text) = parse_whisper_result(&json_path)?;
    let result_store_path = job_dir.join("result.json");
    let history_id = save_history_record(&app, &request, &language, &resolved_backend, &result_store_path)?;

    let result = TranscriptResult {
        history_id,
        title: request.title.clone(),
        channel: request.channel.clone(),
        language,
        duration: request.duration,
        model: request.model_id.clone(),
        backend: resolved_backend,
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
    emit_progress(&app, "done", 100.0, "Transkripsi selesai.");
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
async fn start_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TranscriptRequest,
) -> Result<TranscriptResult, String> {
    let active_pid = state.active_pid.clone();
    let cancelled = state.cancelled.clone();
    if active_pid.lock().map(|g| g.is_some()).unwrap_or(false) {
        return Err("Masih ada job yang sedang berjalan.".into());
    }
    tokio::task::spawn_blocking(move || pipeline(app, active_pid, cancelled, request))
        .await
        .map_err(|e| format!("Transcription task gagal: {e}"))?
}

#[tauri::command]
fn cancel_job(state: State<'_, AppState>) -> Result<(), String> {
    state.cancelled.store(true, Ordering::SeqCst);
    let guard = state.active_pid.lock().map_err(|_| "State process terkunci")?;
    let pid = *guard;
    drop(guard);
    if let Some(pid) = pid {
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    Ok(())
}

#[tauri::command]
fn list_history(app: AppHandle) -> Result<Vec<HistoryItem>, String> {
    let db_path = init_db(&app)?;
    let conn = Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, channel, source_url, created_at, duration, language, model, backend
             FROM history ORDER BY id DESC LIMIT 100",
        )
        .map_err(|e| format!("Gagal membaca history: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                title: row.get(1)?,
                channel: row.get(2)?,
                source_url: row.get(3)?,
                created_at: row.get(4)?,
                duration: row.get(5)?,
                language: row.get(6)?,
                model: row.get(7)?,
                backend: row.get(8)?,
            })
        })
        .map_err(|e| format!("Gagal query history: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Gagal decode history: {e}"))
}

#[tauri::command]
fn load_history(app: AppHandle, id: i64) -> Result<TranscriptResult, String> {
    let db_path = init_db(&app)?;
    let conn = Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
    let result_path: String = conn
        .query_row("SELECT result_path FROM history WHERE id = ?1", [id], |row| row.get(0))
        .map_err(|e| format!("History tidak ditemukan: {e}"))?;
    let bytes = fs::read(&result_path).map_err(|e| format!("File transcript history tidak ditemukan: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("File history rusak: {e}"))
}

#[tauri::command]
fn copy_export(app: AppHandle, source: String, target: String) -> Result<(), String> {
    let src = PathBuf::from(&source);
    if !src.exists() {
        return Err("File export sumber sudah tidak tersedia.".into());
    }
    let canonical_src = fs::canonicalize(&src).map_err(|e| format!("Gagal memvalidasi source export: {e}"))?;
    let canonical_data = fs::canonicalize(app_data_dir(&app)?).map_err(|e| format!("Gagal memvalidasi app data: {e}"))?;
    if !canonical_src.starts_with(&canonical_data) {
        return Err("Source export berada di luar storage WhisperTube.".into());
    }
    fs::copy(canonical_src, target).map_err(|e| format!("Gagal menyimpan export: {e}"))?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            init_db(&handle).map_err(std::io::Error::other)?;
            models_dir(&handle).map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_status,
            list_models,
            download_model,
            delete_model,
            inspect_youtube,
            start_transcription,
            cancel_job,
            list_history,
            load_history,
            copy_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperTube");
}
