use std::process::Command;
use tauri::AppHandle;

use crate::{
    paths::{engine_path, runtime_dir, tool_path},
    types::SystemStatus,
};

pub fn detect_nvidia() -> (bool, Option<String>) {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if text.is_empty() {
                (false, None)
            } else {
                (
                    true,
                    Some(text.lines().next().unwrap_or_default().trim().to_string()),
                )
            }
        }
        _ => (false, None),
    }
}

pub fn system_status(app: &AppHandle) -> Result<SystemStatus, String> {
    let runtime = runtime_dir(app)?;
    let yt_dlp = tool_path(app, "yt-dlp")?.exists();
    let ffmpeg = tool_path(app, "ffmpeg")?.exists();
    let cpu_engine = engine_path(app, "cpu")?.exists();
    let cuda_engine = engine_path(app, "cuda")?.exists();
    let (nvidia, gpu_name) = detect_nvidia();
    let cpu_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);

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
            if cpu_threads >= 12 {
                "large-v3-turbo-q5_0"
            } else {
                "base"
            }
            .to_string(),
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

    let _ = runtime;
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
