use serde_json::Value;
use std::process::Command;
use tauri::AppHandle;
use url::Url;

use crate::{browsers::browser_args, paths::tool_path, types::VideoMetadata};

pub fn validate_youtube_url(raw: &str) -> Result<String, String> {
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

fn run_output(mut command: Command) -> Result<std::process::Output, String> {
    command
        .output()
        .map_err(|e| format!("Gagal menjalankan process: {e}"))
}

pub async fn inspect_youtube(
    app: AppHandle,
    url: String,
    browser: String,
    profile: Option<String>,
) -> Result<VideoMetadata, String> {
    let safe_url = validate_youtube_url(&url)?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    if !yt_dlp.exists() {
        return Err("yt-dlp belum terpasang. Jalankan scripts/setup-windows.ps1.".into());
    }
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(yt_dlp);
        command.args([
            "--ignore-config",
            "--dump-single-json",
            "--skip-download",
            "--no-playlist",
            "--no-warnings",
        ]);
        command.args(browser_args(&browser, profile.as_deref())?);
        command.arg(&safe_url);
        let output = run_output(command)?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if error.is_empty() {
                "YouTube metadata gagal dibaca. Jika ini video Member, pilih browser yang sudah login.".into()
            } else {
                format!("yt-dlp: {error}")
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
