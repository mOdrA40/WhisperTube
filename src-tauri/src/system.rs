use std::process::Command;

#[cfg(target_os = "linux")]
use std::fs;
use tauri::AppHandle;

use crate::{
    accelerators, models,
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

fn clean_gpu_name(value: &str) -> Option<String> {
    let name = value.trim();
    if name.is_empty() {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let ignored = [
        "microsoft basic display",
        "remote display",
        "virtual display",
        "indirect display",
        "software adapter",
    ];
    if ignored.iter().any(|item| lower.contains(item)) {
        return None;
    }
    Some(name.to_string())
}

#[cfg(target_os = "windows")]
fn detect_generic_gpu() -> Option<GpuInfo> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance -ClassName Win32_VideoController | Select-Object -ExpandProperty Name",
        ])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(clean_gpu_name)?;
    Some(GpuInfo {
        name,
        total_memory_mb: None,
        free_memory_mb: None,
    })
}

#[cfg(target_os = "macos")]
fn detect_generic_gpu() -> Option<GpuInfo> {
    let name = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-detailLevel", "basic"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .and_then(|output| {
            output.lines().find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.trim() == "Chipset Model")
                    .and_then(|(_, value)| clean_gpu_name(value))
            })
        })
        .unwrap_or_else(|| "Mac graphics processor".into());
    Some(GpuInfo {
        name,
        total_memory_mb: None,
        free_memory_mb: None,
    })
}

#[cfg(target_os = "linux")]
fn detect_generic_gpu() -> Option<GpuInfo> {
    let drm_root = fs::read_dir("/sys/class/drm").ok()?;
    for entry in drm_root.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with("card") || file_name.contains('-') {
            continue;
        }
        let vendor = fs::read_to_string(entry.path().join("device/vendor"))
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        let name = match vendor.as_deref() {
            Some("0x10de") => "NVIDIA GPU",
            Some("0x1002") => "AMD GPU",
            Some("0x8086") => "Intel GPU",
            Some(_) => "Compatible GPU",
            None => continue,
        };
        return Some(GpuInfo {
            name: name.into(),
            total_memory_mb: None,
            free_memory_mb: None,
        });
    }
    let output = Command::new("lspci").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("vga")
                || lower.contains("3d controller")
                || lower.contains("display controller")
        })
        .and_then(clean_gpu_name)
        .map(|name| GpuInfo {
            name,
            total_memory_mb: None,
            free_memory_mb: None,
        })
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn detect_generic_gpu() -> Option<GpuInfo> {
    None
}

pub fn detect_gpu() -> Option<GpuInfo> {
    detect_nvidia().or_else(detect_generic_gpu)
}

pub fn system_status(app: &AppHandle) -> Result<SystemStatus, String> {
    let runtime = runtime_dir(app)?;
    let yt_dlp = tool_path(app, "yt-dlp")?.exists();
    let ffmpeg = tool_path(app, "ffmpeg")?.exists();
    let cpu_engine = engine_path(app, "cpu")?.exists();
    let cuda_engine = engine_path(app, "cuda")?.exists();
    let nvidia_gpu = detect_nvidia();
    let gpu = nvidia_gpu.clone().or_else(detect_generic_gpu);
    let nvidia = nvidia_gpu.is_some();
    let cuda_supported = cfg!(all(target_os = "windows", target_arch = "x86_64")) && nvidia;
    let gpu_name = gpu.as_ref().map(|info| info.name.clone());
    let gpu_memory_mb = gpu.as_ref().and_then(|info| info.total_memory_mb);
    let gpu_free_memory_mb = gpu.as_ref().and_then(|info| info.free_memory_mb);
    let available_vram_mb = gpu.as_ref().and_then(GpuInfo::available_memory_mb);
    let cpu_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let recommended_id = models::recommended_model_id(available_vram_mb);

    let (recommendation, recommended_model_id, recommended_backend) = if nvidia
        && cuda_supported
        && cuda_engine
    {
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
    } else if nvidia && cuda_supported && !cuda_engine {
        (
            format!(
                "NVIDIA terdeteksi ({}). Pasang CUDA acceleration agar tidak memakai CPU.",
                format_vram(gpu_free_memory_mb),
            ),
            recommended_id,
            "auto".to_string(),
        )
    } else if nvidia && !cuda_supported {
        (
            "NVIDIA terdeteksi, tetapi CUDA package ini hanya tersedia di Windows. Gunakan accelerator yang sesuai atau CPU.".into(),
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
        cuda_supported,
        accelerators: accelerators::catalog(app, gpu.is_some())?,
    })
}
