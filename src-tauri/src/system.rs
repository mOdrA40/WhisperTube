use std::process::Command;
use tauri::AppHandle;

use crate::{
    models,
    paths::{engine_path, runtime_dir, tool_path},
    types::SystemStatus,
};

#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub total_memory_mb: Option<u64>,
    pub free_memory_mb: Option<u64>,
}

impl GpuInfo {
    pub fn available_memory_mb(&self) -> Option<u64> {
        self.free_memory_mb.or(self.total_memory_mb)
    }
}

fn format_vram(memory_mb: Option<u64>) -> String {
    memory_mb
        .map(|value| format!("{value} MB VRAM bebas"))
        .unwrap_or_else(|| "VRAM tidak terbaca".into())
}

pub fn detect_nvidia() -> Option<GpuInfo> {
    let mut candidates = vec!["nvidia-smi".to_string()];
    #[cfg(target_os = "windows")]
    {
        candidates.push(r"C:\Windows\System32\nvidia-smi.exe".into());
        candidates.push(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe".into());
    }

    let output = candidates.into_iter().find_map(|program| {
        Command::new(program)
            .args([
                "--query-gpu=name,memory.total,memory.free",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .ok()
            .filter(|out| out.status.success())
    });
    match output {
        Some(out) => {
            let line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            let mut fields = line.split(',').map(str::trim);
            let name = fields.next().unwrap_or_default().to_string();
            if name.is_empty() {
                return None;
            }
            Some(GpuInfo {
                name,
                total_memory_mb: fields.next().and_then(|value| value.parse().ok()),
                free_memory_mb: fields.next().and_then(|value| value.parse().ok()),
            })
        }
        _ => None,
    }
}

pub fn system_status(app: &AppHandle) -> Result<SystemStatus, String> {
    let runtime = runtime_dir(app)?;
    let yt_dlp = tool_path(app, "yt-dlp")?.exists();
    let ffmpeg = tool_path(app, "ffmpeg")?.exists();
    let cpu_engine = engine_path(app, "cpu")?.exists();
    let cuda_engine = engine_path(app, "cuda")?.exists();
    let gpu = detect_nvidia();
    let nvidia = gpu.is_some();
    let gpu_name = gpu.as_ref().map(|info| info.name.clone());
    let gpu_memory_mb = gpu.as_ref().and_then(|info| info.total_memory_mb);
    let gpu_free_memory_mb = gpu.as_ref().and_then(|info| info.free_memory_mb);
    let available_vram_mb = gpu.as_ref().and_then(GpuInfo::available_memory_mb);
    let cpu_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let recommended_id = models::recommended_model_id(available_vram_mb);

    let (recommendation, recommended_model_id, recommended_backend) = if nvidia && cuda_engine {
        (
            format!(
                "{} terdeteksi dengan {}. Auto akan memakai CUDA; {} direkomendasikan.",
                gpu_name.clone().unwrap_or_else(|| "NVIDIA GPU".into()),
                format_vram(gpu_free_memory_mb),
                recommended_id,
            ),
            recommended_id,
            "auto".to_string(),
        )
    } else if nvidia && !cuda_engine {
        (
            format!(
                "NVIDIA terdeteksi ({}). Pasang CUDA acceleration agar tidak memakai CPU.",
                format_vram(gpu_free_memory_mb),
            ),
            recommended_id,
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
        gpu_memory_mb,
        gpu_free_memory_mb,
        cpu_threads,
        recommendation,
        recommended_model_id,
        recommended_backend,
    })
}
