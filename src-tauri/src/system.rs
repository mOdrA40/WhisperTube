use std::{
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use std::fs;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tauri::AppHandle;

use crate::{
    accelerators, models,
    paths::{engine_path, is_regular_file, runtime_dir, tool_path},
    types::{ComputeDeviceInfo, SystemStatus},
};

const USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const MIN_AVAILABLE_MEMORY_AFTER_START_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SYSTEM_PROBE_STDOUT_BYTES: usize = 1024 * 1024;

static NVIDIA_SMI_PROGRAM: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static NVIDIA_SELECTED_DEVICE: OnceLock<Mutex<Option<usize>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub total_memory_mb: Option<u64>,
    pub free_memory_mb: Option<u64>,
    pub device_index: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct UsageSnapshot {
    pub cpu_usage_percent: Option<f64>,
    pub gpu_usage_percent: Option<f64>,
    pub available_memory_bytes: Option<u64>,
    pub process_memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct UsageTarget {
    pub backend: String,
    pub device_index: Option<usize>,
}

pub struct UsageMonitor {
    snapshot: Arc<Mutex<UsageSnapshot>>,
    target: Arc<Mutex<Option<UsageTarget>>>,
    process_id: Arc<Mutex<Option<u32>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub struct ProcessMonitorGuard<'a> {
    monitor: &'a UsageMonitor,
    process_id: u32,
}

#[derive(Default)]
struct CpuSampler {
    #[cfg(target_os = "windows")]
    previous: Option<CpuTimes>,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
struct CpuTimes {
    idle: u64,
    total: u64,
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemTimes(
        idle_time: *mut FileTime,
        kernel_time: *mut FileTime,
        user_time: *mut FileTime,
    ) -> i32;
}

impl UsageMonitor {
    pub fn start() -> Self {
        let snapshot = Arc::new(Mutex::new(UsageSnapshot::default()));
        let target = Arc::new(Mutex::new(None));
        let process_id = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_snapshot = Arc::clone(&snapshot);
        let thread_target = Arc::clone(&target);
        let thread_process_id = Arc::clone(&process_id);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let mut cpu_sampler = CpuSampler::default();
            let mut memory_sampler = System::new();
            while !thread_stop.load(Ordering::SeqCst) {
                let target = thread_target.lock().ok().and_then(|value| value.clone());
                let process_id = thread_process_id.lock().ok().and_then(|value| *value);
                let current = sample_usage_uncached(
                    &mut cpu_sampler,
                    &mut memory_sampler,
                    target.as_ref(),
                    process_id,
                );
                if let Ok(mut cached) = thread_snapshot.lock() {
                    *cached = current;
                }
                let started = Instant::now();
                while started.elapsed() < USAGE_SAMPLE_INTERVAL {
                    if thread_stop.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        });
        Self {
            snapshot,
            target,
            process_id,
            stop,
            thread: Some(thread),
        }
    }

    pub fn snapshot(&self) -> UsageSnapshot {
        self.snapshot
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    pub fn set_target(&self, backend: &str, device_index: Option<usize>) {
        if let Ok(mut target) = self.target.lock() {
            *target = Some(UsageTarget {
                backend: backend.to_string(),
                device_index,
            });
        }
    }

    pub fn track_process(&self, process_id: u32) -> ProcessMonitorGuard<'_> {
        if let Ok(mut current) = self.process_id.lock() {
            *current = Some(process_id);
        }
        ProcessMonitorGuard {
            monitor: self,
            process_id,
        }
    }

    pub fn memory_pressure(&self) -> Option<String> {
        memory_pressure_message(&self.snapshot())
    }
}

impl Drop for ProcessMonitorGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut current) = self.monitor.process_id.lock() {
            if *current == Some(self.process_id) {
                *current = None;
            }
        }
    }
}

fn memory_pressure_message(snapshot: &UsageSnapshot) -> Option<String> {
    let available = snapshot.available_memory_bytes?;
    if available >= MIN_AVAILABLE_MEMORY_AFTER_START_BYTES {
        return None;
    }
    let process_detail = snapshot
        .process_memory_bytes
        .map(|memory| {
            format!(
                " Proses Whisper memakai sekitar {:.1} MiB.",
                memory as f64 / 1024.0 / 1024.0
            )
        })
        .unwrap_or_default();
    Some(format!(
            "Transkripsi dihentikan karena RAM tersedia turun di bawah batas aman ({:.1} MiB tersedia).{} Tutup aplikasi lain lalu coba lagi.",
            available as f64 / 1024.0 / 1024.0,
            process_detail
        ))
}

impl Drop for UsageMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl GpuInfo {
    pub fn available_memory_mb(&self) -> Option<u64> {
        self.free_memory_mb
    }
}

fn format_vram(memory_mb: Option<u64>) -> String {
    memory_mb
        .map(|value| format!("{value} MB VRAM bebas"))
        .unwrap_or_else(|| "VRAM tidak terbaca".into())
}

pub fn detect_nvidia() -> Option<GpuInfo> {
    let devices = detect_nvidia_devices();
    let gpu = select_nvidia_device(&devices);
    set_selected_nvidia_device(gpu.as_ref());
    gpu
}

pub fn detect_nvidia_device(device_index: usize) -> Option<GpuInfo> {
    detect_nvidia_devices()
        .into_iter()
        .find(|device| device.device_index == Some(device_index))
}

pub fn detect_nvidia_devices() -> Vec<GpuInfo> {
    let Some(program) = nvidia_smi_program() else {
        return Vec::new();
    };
    let output = {
        let mut command = Command::new(&program);
        crate::process::hide_console(&mut command);
        command.args([
            "--query-gpu=index,name,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ]);
        command_output_with_timeout(command, Duration::from_secs(2))
    };
    match output {
        Some(out) => parse_nvidia_devices(&out.stdout),
        None => {
            clear_nvidia_smi_program();
            Vec::new()
        }
    }
}

fn select_nvidia_device(devices: &[GpuInfo]) -> Option<GpuInfo> {
    devices.iter().cloned().fold(None, |best, candidate| {
        let candidate_memory = candidate
            .free_memory_mb
            .or(candidate.total_memory_mb)
            .unwrap_or(0);
        let best_memory = best
            .as_ref()
            .and_then(|value: &GpuInfo| value.free_memory_mb.or(value.total_memory_mb))
            .unwrap_or(0);
        if best.is_none() || candidate_memory > best_memory {
            Some(candidate)
        } else {
            best
        }
    })
}

fn set_selected_nvidia_device(gpu: Option<&GpuInfo>) {
    if let Ok(mut selected) = NVIDIA_SELECTED_DEVICE
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *selected = gpu.and_then(|value| value.device_index);
    }
}

fn parse_nvidia_devices(output: &[u8]) -> Vec<GpuInfo> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            let device_index = fields.next()?.parse().ok();
            let name = fields.next()?.to_string();
            (!name.is_empty()).then(|| GpuInfo {
                name,
                total_memory_mb: fields.next().and_then(|value| value.parse().ok()),
                free_memory_mb: fields.next().and_then(|value| value.parse().ok()),
                device_index,
            })
        })
        .collect()
}

#[cfg(test)]
fn parse_nvidia_gpus(output: &[u8]) -> Option<GpuInfo> {
    select_nvidia_device(&parse_nvidia_devices(output))
}

fn nvidia_smi_program() -> Option<String> {
    let cache = NVIDIA_SMI_PROGRAM.get_or_init(|| Mutex::new(None));
    if let Ok(cached) = cache.lock() {
        if cached.is_some() {
            return cached.clone();
        }
    }
    let mut candidates = vec!["nvidia-smi".to_string()];
    #[cfg(target_os = "windows")]
    {
        candidates.push(r"C:\Windows\System32\nvidia-smi.exe".into());
        candidates.push(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe".into());
    }
    let found = candidates.into_iter().find(|program| {
        let mut command = Command::new(program);
        crate::process::hide_console(&mut command);
        command.arg("--version");
        command_output_with_timeout(command, Duration::from_secs(2)).is_some()
    });
    if let Ok(mut cached) = cache.lock() {
        *cached = found;
        return cached.clone();
    }
    None
}

fn clear_nvidia_smi_program() {
    if let Some(cache) = NVIDIA_SMI_PROGRAM.get() {
        if let Ok(mut cached) = cache.lock() {
            *cached = None;
        }
    }
}

pub fn refresh_hardware_detection() {
    clear_nvidia_smi_program();
}

fn vulkan_vendor(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("nvidia") {
        "nvidia"
    } else if lower.contains("amd") || lower.contains("radeon") {
        "amd"
    } else if lower.contains("intel") {
        "intel"
    } else {
        "unknown"
    }
}

fn is_software_vulkan_device(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "llvmpipe",
        "lavapipe",
        "swiftshader",
        "software rasterizer",
        "software adapter",
        "microsoft basic render",
        "warp",
        "virtual display",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn parse_vulkan_devices(stderr: &str) -> Vec<ComputeDeviceInfo> {
    stderr
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("ggml_vulkan: ")?;
            let (index, rest) = rest.split_once(" = ")?;
            let device_index = index.trim().parse::<usize>().ok()?;
            let (name, capabilities) = rest.split_once(" | ").unwrap_or((rest, ""));
            let name = name.trim();
            if name.is_empty() || is_software_vulkan_device(name) {
                return None;
            }
            Some(ComputeDeviceInfo {
                id: format!("vulkan:{device_index}"),
                backend: "vulkan".into(),
                name: name.into(),
                vendor: vulkan_vendor(name).into(),
                device_index: Some(device_index),
                integrated: capabilities.contains("uma: 1"),
                total_memory_mb: None,
                free_memory_mb: None,
            })
        })
        .collect()
}

pub fn detect_vulkan_devices_from_engine(engine: &Path) -> Vec<ComputeDeviceInfo> {
    if !is_regular_file(engine) {
        return Vec::new();
    }
    let mut command = Command::new(engine);
    command.arg("--help");
    crate::process::run_with_timeout_captured(&mut command, Duration::from_secs(3), 256 * 1024)
        .map(|(_, stderr)| parse_vulkan_devices(&stderr))
        .unwrap_or_default()
}

pub fn detect_vulkan_devices(app: &AppHandle) -> Vec<ComputeDeviceInfo> {
    let Ok(engine) = engine_path(app, "vulkan") else {
        return Vec::new();
    };
    detect_vulkan_devices_from_engine(&engine)
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
    let mut command = Command::new("powershell.exe");
    crate::process::hide_console(&mut command);
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance -ClassName Win32_VideoController | Select-Object -ExpandProperty Name",
    ]);
    let output = command_output_with_timeout(command, Duration::from_secs(3))?;
    let name = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(clean_gpu_name)?;
    Some(GpuInfo {
        name,
        total_memory_mb: None,
        free_memory_mb: None,
        device_index: None,
    })
}

#[cfg(target_os = "macos")]
fn detect_generic_gpu() -> Option<GpuInfo> {
    let mut command = Command::new("system_profiler");
    crate::process::hide_console(&mut command);
    command.args(["SPDisplaysDataType", "-detailLevel", "basic"]);
    let name = command_output_with_timeout(command, Duration::from_secs(5))
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
        device_index: None,
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
            device_index: None,
        });
    }
    let output = command_output_with_timeout(Command::new("lspci"), Duration::from_secs(2))?;
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
            device_index: None,
        })
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn detect_generic_gpu() -> Option<GpuInfo> {
    None
}

pub fn detect_gpu() -> Option<GpuInfo> {
    detect_nvidia().or_else(detect_generic_gpu)
}

fn capture_system_probe_stdout(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut captured = Vec::with_capacity(MAX_SYSTEM_PROBE_STDOUT_BYTES.min(64 * 1024));
    let mut buffer = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("Gagal membaca output system probe: {error}"))?;
        if read == 0 {
            break;
        }
        let remaining = MAX_SYSTEM_PROBE_STDOUT_BYTES.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    if truncated {
        Err("Output system probe terlalu besar.".into())
    } else {
        Ok(captured)
    }
}

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> Option<Output> {
    crate::process::hide_console(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let stdout_reader = std::thread::spawn(move || capture_system_probe_stdout(stdout));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_reader.join().ok()?.ok()?;
                return status.success().then_some(Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                crate::process::terminate_process_tree(child.id());
                let _ = child.wait();
                let _ = stdout_reader.join();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                crate::process::terminate_process_tree(child.id());
                let _ = child.wait();
                let _ = stdout_reader.join();
                return None;
            }
        }
    }
}

fn parse_percent(output: &[u8]) -> Option<f64> {
    String::from_utf8_lossy(output)
        .lines()
        .find_map(|line| line.trim().trim_end_matches('%').parse::<f64>().ok())
        .map(|value| value.clamp(0.0, 100.0))
}

#[cfg(target_os = "windows")]
fn file_time_to_u64(value: FileTime) -> u64 {
    (u64::from(value.high) << 32) | u64::from(value.low)
}

#[cfg(target_os = "windows")]
fn read_cpu_times() -> Option<CpuTimes> {
    let mut idle = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    let success = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0;
    if !success {
        return None;
    }
    Some(CpuTimes {
        idle: file_time_to_u64(idle),
        total: file_time_to_u64(kernel).saturating_add(file_time_to_u64(user)),
    })
}

#[cfg(target_os = "windows")]
fn sample_cpu_usage(sampler: &mut CpuSampler) -> Option<f64> {
    let current = read_cpu_times()?;
    let previous = sampler.previous.replace(current)?;
    let total_delta = current.total.saturating_sub(previous.total);
    let idle_delta = current.idle.saturating_sub(previous.idle).min(total_delta);
    if total_delta == 0 {
        return None;
    }
    Some(((total_delta - idle_delta) as f64 / total_delta as f64 * 100.0).clamp(0.0, 100.0))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn sample_cpu_usage(_sampler: &mut CpuSampler) -> Option<f64> {
    let mut command = Command::new("ps");
    crate::process::hide_console(&mut command);
    command.args(["-A", "-o", "%cpu="]);
    let output = command_output_with_timeout(command, Duration::from_secs(2))?;
    let total: f64 = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .sum();
    let cores = std::thread::available_parallelism()
        .map(|value| value.get() as f64)
        .unwrap_or(1.0);
    Some((total / cores).clamp(0.0, 100.0))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn sample_cpu_usage(_sampler: &mut CpuSampler) -> Option<f64> {
    None
}

#[cfg(target_os = "windows")]
fn sample_gpu_usage(target: Option<&UsageTarget>) -> Option<f64> {
    if target.is_none_or(|value| value.backend == "cpu") {
        return None;
    }
    if target.is_some_and(|value| value.backend == "cuda") {
        if let Some(program) = nvidia_smi_program() {
            let mut command = Command::new(&program);
            crate::process::hide_console(&mut command);
            if let Some(index) = target.and_then(|value| value.device_index) {
                command.arg(format!("--id={index}"));
            }
            command.args([
                "--query-gpu=utilization.gpu",
                "--format=csv,noheader,nounits",
            ]);
            if let Some(value) = command_output_with_timeout(command, Duration::from_secs(2))
                .and_then(|output| parse_percent(&output.stdout))
            {
                return Some(value);
            }
        }
        return None;
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_gpu_busy_percent() -> Option<f64> {
    let mut primary_card_count = 0usize;
    let mut card_values = Vec::new();
    for entry in fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        primary_card_count = primary_card_count.saturating_add(1);
        let device = entry.path().join("device");
        let direct = fs::read_to_string(device.join("gpu_busy_percent"))
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok());
        let intel = fs::read_dir(device.join("gt"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|gt| fs::read_to_string(gt.path().join("busy_percent")).ok())
            .filter_map(|value| value.trim().parse::<f64>().ok())
            .max_by(|left, right| left.total_cmp(right));
        if let Some(value) = direct.or(intel) {
            card_values.push(value.clamp(0.0, 100.0));
        }
    }
    // A single readable metric is not enough: a hybrid system may expose a
    // metric for only one of several adapters. Without a PCI/Vulkan mapping,
    // report unavailable rather than attaching that value to the wrong GPU.
    (primary_card_count == 1 && card_values.len() == 1).then(|| card_values[0])
}

#[cfg(target_os = "linux")]
fn sample_gpu_usage(target: Option<&UsageTarget>) -> Option<f64> {
    if target.is_none_or(|value| value.backend == "cpu") {
        return None;
    }
    // Vulkan indices do not map portably to DRM cards. The helper therefore
    // reports a value only when the host exposes exactly one primary adapter.
    linux_gpu_busy_percent()
}

#[cfg(any(
    target_os = "macos",
    not(any(target_os = "windows", target_os = "linux"))
))]
fn sample_gpu_usage(_target: Option<&UsageTarget>) -> Option<f64> {
    // macOS has no stable, non-privileged GPU utilization API that can be
    // mapped to the selected Metal device without guessing.
    None
}

fn sample_usage_uncached(
    cpu_sampler: &mut CpuSampler,
    memory_sampler: &mut System,
    target: Option<&UsageTarget>,
    process_id: Option<u32>,
) -> UsageSnapshot {
    memory_sampler.refresh_memory();
    let process_memory_bytes = process_id.and_then(|process_id| {
        let pid = Pid::from_u32(process_id);
        memory_sampler.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        memory_sampler.process(pid).map(|process| process.memory())
    });
    UsageSnapshot {
        cpu_usage_percent: sample_cpu_usage(cpu_sampler),
        gpu_usage_percent: sample_gpu_usage(target),
        available_memory_bytes: Some(memory_sampler.available_memory()),
        process_memory_bytes,
    }
}

pub fn system_status(app: &AppHandle) -> Result<SystemStatus, String> {
    refresh_hardware_detection();
    let runtime = runtime_dir(app)?;
    let yt_dlp = crate::paths::is_regular_file(&tool_path(app, "yt-dlp")?);
    let ffmpeg = crate::paths::is_regular_file(&tool_path(app, "ffmpeg")?);
    let cpu_engine = crate::paths::is_regular_file(&engine_path(app, "cpu")?);
    let cuda_engine = crate::paths::is_regular_file(&engine_path(app, "cuda")?);
    let nvidia_devices = detect_nvidia_devices();
    let nvidia_gpu = select_nvidia_device(&nvidia_devices);
    set_selected_nvidia_device(nvidia_gpu.as_ref());
    let gpu = nvidia_gpu.clone().or_else(detect_generic_gpu);
    let nvidia = nvidia_gpu.is_some();
    let cuda_supported = cfg!(all(target_os = "windows", target_arch = "x86_64")) && nvidia;
    let gpu_name = gpu.as_ref().map(|info| info.name.clone());
    let gpu_memory_mb = gpu.as_ref().and_then(|info| info.total_memory_mb);
    let gpu_free_memory_mb = gpu.as_ref().and_then(|info| info.free_memory_mb);
    let available_vram_mb = gpu.as_ref().and_then(GpuInfo::available_memory_mb);
    let accelerators = accelerators::catalog(app, gpu.is_some())?;
    let job_storage_bytes = crate::history::job_storage_bytes(app)?;
    let vulkan_installed = accelerators
        .iter()
        .any(|accelerator| accelerator.backend == "vulkan" && accelerator.installed);
    let mut compute_devices = Vec::new();
    if cuda_supported && cuda_engine {
        compute_devices.extend(nvidia_devices.iter().filter_map(|device| {
            let index = device.device_index?;
            Some(ComputeDeviceInfo {
                id: format!("cuda:{index}"),
                backend: "cuda".into(),
                name: device.name.clone(),
                vendor: "nvidia".into(),
                device_index: Some(index),
                integrated: false,
                total_memory_mb: device.total_memory_mb,
                free_memory_mb: device.free_memory_mb,
            })
        }));
    }
    if vulkan_installed {
        compute_devices.extend(detect_vulkan_devices(app));
    }
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
    } else if nvidia && cuda_supported && !cuda_engine && vulkan_installed {
        (
            format!(
                "NVIDIA terdeteksi ({}). Auto akan mencoba Vulkan; pasang CUDA jika ingin backend CUDA.",
                format_vram(gpu_free_memory_mb),
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
        accelerators,
        compute_devices,
        job_storage_bytes,
        job_storage_limit_bytes: crate::history::MAX_JOB_STORAGE_BYTES,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        capture_system_probe_stdout, memory_pressure_message, parse_nvidia_gpus,
        parse_vulkan_devices, UsageMonitor, UsageSnapshot, MAX_SYSTEM_PROBE_STDOUT_BYTES,
    };
    use std::io::Cursor;

    #[test]
    fn selects_nvidia_device_with_the_most_free_memory() {
        let output = b"0,Small GPU,4096,512\n1,Large GPU,12288,8192\n";
        let gpu = parse_nvidia_gpus(output).expect("GPU should be parsed");
        assert_eq!(gpu.name, "Large GPU");
        assert_eq!(gpu.device_index, Some(1));
        assert_eq!(gpu.free_memory_mb, Some(8192));
    }

    #[test]
    fn keeps_first_device_when_memory_is_unavailable_or_tied() {
        let output = b"0,First GPU,N/A,N/A\n1,Second GPU,N/A,N/A\n";
        let gpu = parse_nvidia_gpus(output).expect("GPU should be parsed");
        assert_eq!(gpu.name, "First GPU");
        assert_eq!(gpu.device_index, Some(0));
    }

    #[test]
    fn parses_vulkan_devices_and_marks_integrated_gpu() {
        let output = "ggml_vulkan: Found 2 Vulkan devices:\n\
            ggml_vulkan: 0 = AMD Radeon 780M Graphics (AMD proprietary driver) | uma: 1 | fp16: 1\n\
            ggml_vulkan: 1 = NVIDIA GeForce RTX 4050 Laptop GPU (NVIDIA) | uma: 0 | fp16: 1\n";
        let devices = parse_vulkan_devices(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "vulkan:0");
        assert_eq!(devices[0].vendor, "amd");
        assert!(devices[0].integrated);
        assert_eq!(devices[1].vendor, "nvidia");
        assert!(!devices[1].integrated);
    }

    #[test]
    fn filters_software_vulkan_devices() {
        let output = "ggml_vulkan: 0 = llvmpipe (LLVM 17.0.6, 256 bits) | uma: 1 | fp16: 1\n\
            ggml_vulkan: 1 = AMD Radeon 780M Graphics (AMD proprietary driver) | uma: 1 | fp16: 1\n";
        let devices = parse_vulkan_devices(output);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "vulkan:1");
    }

    #[test]
    fn system_probe_output_is_bounded() {
        let output = vec![b'x'; MAX_SYSTEM_PROBE_STDOUT_BYTES + 1];
        assert!(capture_system_probe_stdout(Cursor::new(output)).is_err());
    }

    #[test]
    fn reports_memory_pressure_below_safe_floor() {
        let message = memory_pressure_message(&UsageSnapshot {
            available_memory_bytes: Some(128 * 1024 * 1024),
            process_memory_bytes: Some(512 * 1024 * 1024),
            ..UsageSnapshot::default()
        })
        .expect("low available memory should be reported");
        assert!(message.contains("RAM tersedia turun"));
        assert!(message.contains("512.0 MiB"));
    }

    #[test]
    fn process_monitor_guard_clears_the_tracked_pid() {
        let monitor = UsageMonitor::start();
        monitor.set_target("vulkan", Some(0));
        {
            let _guard = monitor.track_process(1234);
            assert_eq!(*monitor.process_id.lock().unwrap(), Some(1234));
        }
        assert_eq!(*monitor.process_id.lock().unwrap(), None);
    }
}
