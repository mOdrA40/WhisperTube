use std::{
    path::Path,
    process::{Command, Output},
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
    paths::{engine_available, engine_path, is_regular_file, runtime_dir, tool_path},
    types::{ComputeDeviceInfo, SystemStatus},
};

const USAGE_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const MIN_AVAILABLE_MEMORY_AFTER_START_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SYSTEM_PROBE_STDOUT_BYTES: usize = 1024 * 1024;
pub const MIN_CUDA_DRIVER_VERSION: &str = "528.33";

static NVIDIA_SMI_PROGRAM: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static NVIDIA_SELECTED_DEVICE: OnceLock<Mutex<Option<usize>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub total_memory_mb: Option<u64>,
    pub free_memory_mb: Option<u64>,
    pub uuid: Option<String>,
    /// Physical index reported by NVIDIA Management Library / nvidia-smi.
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
    #[cfg(target_os = "linux")]
    linux: LinuxCpuSampler,
}

// /proc/stat aggregate counters already include all CPUs. Guest time is
// included in user/nice, so only the first eight counters are summed.
#[cfg(any(target_os = "linux", test))]
#[derive(Default)]
struct LinuxCpuSampler {
    previous: Option<[u64; 8]>,
}

#[cfg(any(target_os = "linux", test))]
impl LinuxCpuSampler {
    fn sample(&mut self, stat: &str) -> Option<f64> {
        let current = stat.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != "cpu" {
                return None;
            }
            let mut counters = [0; 8];
            let mut count = 0;
            for (slot, field) in counters.iter_mut().zip(fields) {
                *slot = field.parse::<u64>().ok()?;
                count += 1;
            }
            (count >= 4).then_some(counters)
        });
        let previous = std::mem::replace(&mut self.previous, current)?;
        let current = current?;
        let mut deltas = [0; 8];
        for ((delta, current), previous) in deltas.iter_mut().zip(current).zip(previous) {
            // A reset, rollover or decreasing iowait invalidates this interval.
            *delta = current.checked_sub(previous)?;
        }
        let total = deltas.into_iter().try_fold(0u64, u64::checked_add)?;
        let idle = deltas[3].checked_add(deltas[4])?;
        if total == 0 {
            return None;
        }
        Some(((total - idle) as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
    }
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
    detect_nvidia_device_from(&detect_nvidia_devices(), device_index)
}

fn detect_nvidia_device_from(devices: &[GpuInfo], device_index: usize) -> Option<GpuInfo> {
    devices
        .iter()
        .find(|device| device.device_index == Some(device_index))
        .cloned()
}

pub fn detect_nvidia_devices() -> Vec<GpuInfo> {
    let (devices, _) = detect_nvidia_snapshot();
    cuda_visible_nvidia_devices(&devices)
}

fn detect_nvidia_snapshot() -> (Vec<GpuInfo>, Option<String>) {
    let Some(program) = nvidia_smi_program() else {
        return (Vec::new(), None);
    };
    let output = {
        let mut command = Command::new(&program);
        crate::process::hide_console(&mut command);
        command.args([
            "--query-gpu=index,name,uuid,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ]);
        command_output_with_timeout(command, Duration::from_secs(2))
    };
    match output {
        Some(out) => parse_nvidia_snapshot(&out.stdout),
        None => {
            clear_nvidia_smi_program();
            (Vec::new(), None)
        }
    }
}

pub fn detect_nvidia_driver_version() -> Option<String> {
    detect_nvidia_snapshot().1
}

fn parse_nvidia_snapshot(output: &[u8]) -> (Vec<GpuInfo>, Option<String>) {
    parse_nvidia_snapshot_output(output)
}

fn parse_nvidia_snapshot_output(output: &[u8]) -> (Vec<GpuInfo>, Option<String>) {
    let driver = String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.split(',').nth(5).map(str::trim))
        .find(|value| parse_driver_version(value).is_some())
        .map(str::to_string);
    (parse_nvidia_devices(output), driver)
}

fn parse_driver_version(value: &str) -> Option<(u32, u32, u32)> {
    let mut parts = value.trim().split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next().unwrap_or("0").parse().ok()?,
    ))
}

pub fn cuda_driver_is_compatible(version: &str) -> bool {
    let Some(actual) = parse_driver_version(version) else {
        return false;
    };
    let required = parse_driver_version(MIN_CUDA_DRIVER_VERSION).expect("minimum driver is valid");
    actual >= required
}

pub fn ensure_cuda_install_supported() -> Result<(), String> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err("Download CUDA otomatis saat ini hanya didukung di Windows x64.".into());
    }
    if detect_nvidia_snapshot().0.is_empty() {
        return Err("NVIDIA GPU/driver tidak terdeteksi. CUDA engine tidak perlu dipasang.".into());
    }
    let Some(driver_version) = detect_nvidia_driver_version() else {
        return Err(
            "Versi driver NVIDIA tidak dapat diverifikasi. Update driver NVIDIA lalu coba lagi."
                .into(),
        );
    };
    if !cuda_driver_is_compatible(&driver_version) {
        return Err(format!(
            "Driver NVIDIA {driver_version} terlalu lama untuk CUDA 12.x. Minimal driver Windows yang didukung adalah {MIN_CUDA_DRIVER_VERSION}. Update driver sebelum mengunduh CUDA."
        ));
    }
    Ok(())
}

fn select_nvidia_device(devices: &[GpuInfo]) -> Option<GpuInfo> {
    ordered_nvidia_devices(devices).into_iter().next()
}

fn select_nvidia_display_device(devices: &[GpuInfo]) -> Option<GpuInfo> {
    devices.iter().cloned().max_by_key(|device| {
        (
            device.free_memory_mb.is_some(),
            device.free_memory_mb.unwrap_or(0),
            device.total_memory_mb.unwrap_or(0),
        )
    })
}

pub fn cuda_devices_by_preference() -> Vec<GpuInfo> {
    ordered_nvidia_devices(&detect_nvidia_devices())
}

pub fn configure_cuda_command(command: &mut Command, gpu: &GpuInfo) -> Result<(), String> {
    let uuid = gpu
        .uuid
        .as_deref()
        .filter(|uuid| is_cuda_gpu_uuid(uuid))
        .ok_or_else(|| format!("UUID NVIDIA untuk {} tidak dapat diverifikasi.", gpu.name))?;
    // Restrict this child process to the selected physical GPU. Its CUDA
    // ordinal is then always zero, regardless of CUDA_VISIBLE_DEVICES ordering.
    command.env("CUDA_VISIBLE_DEVICES", uuid);
    Ok(())
}

fn nvidia_compute_capability_command(program: &str, gpu: &GpuInfo) -> Result<Command, String> {
    let uuid = gpu
        .uuid
        .as_deref()
        .filter(|uuid| is_cuda_gpu_uuid(uuid))
        .ok_or("UUID NVIDIA tidak valid untuk pemeriksaan compute capability.")?;
    let mut command = Command::new(program);
    crate::process::hide_console(&mut command);
    // NVML physical indices and CUDA ordinals can differ. Query the same UUID
    // that configure_cuda_command exposes to the inference child.
    command.arg(format!("--id={uuid}")).args([
        "--query-gpu=uuid,compute_cap",
        "--format=csv,noheader,nounits",
    ]);
    Ok(command)
}

fn parse_nvidia_compute_capability(output: &[u8], uuid: &str) -> Option<(u32, u32)> {
    let output = std::str::from_utf8(output).ok()?;
    let mut lines = output.trim().lines();
    let (actual_uuid, capability) = lines.next()?.split_once(',')?;
    if lines.next().is_some() || !actual_uuid.trim().eq_ignore_ascii_case(uuid) {
        return None;
    }
    let (major, minor) = capability.trim().split_once('.')?;
    if [major, minor]
        .iter()
        .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let (major, minor) = (major.parse().ok()?, minor.parse().ok()?);
    (major > 0 && minor < 10).then_some((major, minor))
}

pub fn nvidia_compute_capability(gpu: &GpuInfo) -> Result<(u32, u32), String> {
    let program = nvidia_smi_program().ok_or("nvidia-smi tidak tersedia.")?;
    let command = nvidia_compute_capability_command(&program, gpu)?;
    let output = command_output_with_timeout(command, Duration::from_secs(2)).ok_or_else(|| {
        format!(
            "Gagal membaca compute capability NVIDIA untuk {}.",
            gpu.name
        )
    })?;
    parse_nvidia_compute_capability(&output.stdout, gpu.uuid.as_deref().unwrap()).ok_or_else(|| {
        format!(
            "Compute capability NVIDIA untuk {} tidak dapat diverifikasi.",
            gpu.name
        )
    })
}

fn ordered_nvidia_devices(devices: &[GpuInfo]) -> Vec<GpuInfo> {
    let mut visible = devices.to_vec();
    visible.sort_by(|left, right| {
        right
            .free_memory_mb
            .is_some()
            .cmp(&left.free_memory_mb.is_some())
            .then_with(|| {
                right
                    .free_memory_mb
                    .unwrap_or(0)
                    .cmp(&left.free_memory_mb.unwrap_or(0))
            })
            .then_with(|| left.device_index.cmp(&right.device_index))
    });
    visible
}

fn set_selected_nvidia_device(gpu: Option<&GpuInfo>) {
    if let Ok(mut selected) = NVIDIA_SELECTED_DEVICE
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *selected = gpu.and_then(|value| value.device_index);
    }
}

fn cuda_visible_nvidia_devices(devices: &[GpuInfo]) -> Vec<GpuInfo> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    if let Some(mask) = std::env::var_os("CUDA_VISIBLE_DEVICES") {
        // Some Windows drivers treat an empty value as unset. Preserve the
        // documented empty-mask behavior before asking the driver to enumerate.
        if mask.is_empty() {
            return Vec::new();
        }
        // CUDA interprets numeric masks in its own enumeration order, including
        // CUDA_DEVICE_ORDER. NVML indices cannot resolve these ordinals.
        return match cuda_visible_device_uuids() {
            Ok(uuids) => nvidia_devices_for_cuda_uuids(devices, Some(&uuids)),
            Err(error) => {
                eprintln!("WhisperTube: visibilitas perangkat CUDA tidak dapat diperiksa: {error}");
                Vec::new()
            }
        };
    }
    nvidia_devices_for_cuda_uuids(devices, None)
}

fn nvidia_devices_for_cuda_uuids(
    devices: &[GpuInfo],
    visible_uuids: Option<&[String]>,
) -> Vec<GpuInfo> {
    let valid = |device: &&GpuInfo| {
        device.device_index.is_some() && device.uuid.as_deref().is_some_and(is_cuda_gpu_uuid)
    };
    let Some(visible_uuids) = visible_uuids else {
        return devices.iter().filter(valid).cloned().collect();
    };
    visible_uuids
        .iter()
        .filter_map(|uuid| {
            let mut matches = devices.iter().filter(valid).filter(|device| {
                device
                    .uuid
                    .as_deref()
                    .is_some_and(|physical_uuid| physical_uuid.eq_ignore_ascii_case(uuid))
            });
            let device = matches.next()?;
            matches.next().is_none().then(|| device.clone())
        })
        .collect()
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn cuda_visible_device_uuids() -> Result<Vec<String>, String> {
    use std::ffi::c_void;

    type CuInit = unsafe extern "system" fn(u32) -> i32;
    type CuDeviceGetCount = unsafe extern "system" fn(*mut i32) -> i32;
    type CuDeviceGet = unsafe extern "system" fn(*mut i32, i32) -> i32;
    type CuDeviceGetUuid = unsafe extern "system" fn(*mut [u8; 16], i32) -> i32;
    type FarProc = unsafe extern "system" fn() -> isize;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> Option<FarProc>;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }

    struct DriverLibrary(*mut c_void);
    impl Drop for DriverLibrary {
        fn drop(&mut self) {
            // SAFETY: this handle came from LoadLibraryExW and is released once.
            unsafe { FreeLibrary(self.0) };
        }
    }

    unsafe fn symbol<T: Copy>(module: *mut c_void, name: &'static [u8]) -> Result<T, String> {
        // SAFETY: each caller supplies a NUL-terminated exported CUDA API name.
        let address = unsafe { GetProcAddress(module, name.as_ptr()) }
            .ok_or_else(|| "CUDA Driver API yang diperlukan tidak tersedia.".to_string())?;
        // SAFETY: callers declare the documented CUDA signature for each symbol;
        // all of these Windows function pointer types have the same size.
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }

    let name = "nvcuda.dll\0".encode_utf16().collect::<Vec<_>>();
    // SAFETY: name is terminated UTF-16. LOAD_LIBRARY_SEARCH_SYSTEM32 prevents
    // a writable application/PATH directory from supplying the driver DLL.
    let handle = unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), 0x0000_0800) };
    if handle.is_null() {
        return Err("Driver nvcuda.dll tidak tersedia di System32.".into());
    }
    let _library = DriverLibrary(handle);
    // SAFETY: signatures match the CUDA Driver API, including the 16-byte CUuuid.
    let (init, get_count, get_device, get_uuid) = unsafe {
        (
            symbol::<CuInit>(handle, b"cuInit\0")?,
            symbol::<CuDeviceGetCount>(handle, b"cuDeviceGetCount\0")?,
            symbol::<CuDeviceGet>(handle, b"cuDeviceGet\0")?,
            symbol::<CuDeviceGetUuid>(handle, b"cuDeviceGetUuid_v2\0")?,
        )
    };
    // SAFETY: initialization takes no pointers; flags must be zero.
    let status = unsafe { init(0) };
    if status == 100 {
        return Ok(Vec::new()); // CUDA_ERROR_NO_DEVICE, including a masked device set.
    }
    if status != 0 {
        return Err(format!("cuInit gagal (kode {status})."));
    }
    let mut count = 0;
    // SAFETY: count is a writable i32 output.
    let status = unsafe { get_count(&mut count) };
    if status != 0 || count < 0 {
        return Err(format!("cuDeviceGetCount gagal (kode {status})."));
    }
    let mut uuids = Vec::new();
    for ordinal in 0..count {
        let mut device = 0;
        let mut uuid = [0u8; 16];
        // SAFETY: both outputs have the documented sizes and remain valid.
        let status = unsafe { get_device(&mut device, ordinal) };
        if status != 0 {
            return Err(format!("cuDeviceGet gagal (kode {status})."));
        }
        // SAFETY: uuid is writable for all 16 bytes of CUuuid.
        let status = unsafe { get_uuid(&mut uuid, device) };
        if status != 0 {
            return Err(format!("cuDeviceGetUuid_v2 gagal (kode {status})."));
        }
        uuids.push(format!("GPU-{}", uuid::Uuid::from_bytes(uuid)));
    }
    Ok(uuids)
}

fn is_cuda_gpu_uuid(value: &str) -> bool {
    cuda_gpu_uuid_suffix(value).is_some_and(|suffix| {
        suffix.len() >= 8
            && suffix
                .chars()
                .all(|character| character.is_ascii_hexdigit() || character == '-')
    })
}

fn cuda_gpu_uuid_suffix(value: &str) -> Option<&str> {
    let prefix = value.get(..4)?;
    prefix.eq_ignore_ascii_case("GPU-").then(|| &value[4..])
}

fn parse_nvidia_devices(output: &[u8]) -> Vec<GpuInfo> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            let device_index = fields.next()?.parse().ok();
            let name = fields.next()?.to_string();
            let uuid = fields.next().map(str::to_string);
            let total_memory_mb = fields.next().and_then(|value| value.parse().ok());
            let free_memory_mb = fields.next().and_then(|value| value.parse().ok());
            (!name.is_empty()).then_some(GpuInfo {
                name,
                total_memory_mb,
                free_memory_mb,
                uuid,
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

fn vulkan_probe_command(engine: &Path) -> Command {
    let mut command = Command::new(engine);
    crate::process::hide_console(&mut command);
    command
}

pub fn detect_vulkan_devices_from_engine(engine: &Path) -> Vec<ComputeDeviceInfo> {
    if !is_regular_file(engine) {
        return Vec::new();
    }
    let mut command = vulkan_probe_command(engine);
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
    #[repr(C)]
    struct DisplayDevice {
        size: u32,
        name: [u16; 32],
        description: [u16; 128],
        flags: u32,
        id: [u16; 128],
        key: [u16; 128],
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumDisplayDevicesW(
            device: *const u16,
            index: u32,
            output: *mut DisplayDevice,
            flags: u32,
        ) -> i32;
    }
    for index in 0.. {
        let mut device = DisplayDevice {
            size: std::mem::size_of::<DisplayDevice>() as u32,
            name: [0; 32],
            description: [0; 128],
            flags: 0,
            id: [0; 128],
            key: [0; 128],
        };
        // SAFETY: cb and the repr(C) layout match DISPLAY_DEVICEW. A null input
        // enumerates adapters, and output points to a complete writable struct.
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), index, &mut device, 0) } == 0 {
            return None;
        }
        if device.flags & 0x8 != 0 {
            continue;
        } // DISPLAY_DEVICE_MIRRORING_DRIVER
        let length = device
            .description
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(device.description.len());
        if let Some(name) = clean_gpu_name(&String::from_utf16_lossy(&device.description[..length]))
        {
            return Some(GpuInfo {
                name,
                total_memory_mb: None,
                free_memory_mb: None,
                uuid: None,
                device_index: None,
            });
        }
    }
    None
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
        uuid: None,
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
            uuid: None,
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
            uuid: None,
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

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> Option<Output> {
    let mut stdout = Vec::with_capacity(64 * 1024);
    let status = crate::process::run_piped(
        &mut command,
        Some(timeout),
        None,
        |_| Ok(()),
        |event| {
            if let crate::process::OutputEvent::Data(crate::process::OutputStream::Stdout, bytes) =
                event
            {
                if stdout.len().saturating_add(bytes.len()) > MAX_SYSTEM_PROBE_STDOUT_BYTES {
                    return Err("Output system probe terlalu besar.".into());
                }
                stdout.extend_from_slice(bytes);
            }
            Ok(())
        },
    )
    .ok()?;
    status.success().then_some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
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

#[cfg(target_os = "linux")]
fn sample_cpu_usage(sampler: &mut CpuSampler) -> Option<f64> {
    match fs::read_to_string("/proc/stat") {
        Ok(stat) => sampler.linux.sample(&stat),
        Err(_) => {
            // Do not reuse an old baseline after an unavailable sample.
            sampler.linux.previous = None;
            None
        }
    }
}

#[cfg(target_os = "macos")]
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

fn non_nvidia_recommendation(
    model_id: String,
    cpu_threads: usize,
    gpu_ready: bool,
) -> (String, String, String) {
    let message = if gpu_ready {
        format!("Accelerator GPU siap. Auto akan mencoba GPU; {model_id} direkomendasikan.")
    } else if cpu_threads >= 12 {
        "CPU cukup kuat. Mulai dengan Balanced; gunakan Fast jika terlalu lambat.".into()
    } else {
        "Gunakan Fast untuk mesin ini agar responsif.".into()
    };
    (message, model_id, "auto".into())
}

pub fn system_status(app: &AppHandle) -> Result<SystemStatus, String> {
    let runtime = runtime_dir(app)?;
    let yt_dlp = crate::paths::is_regular_file(&tool_path(app, "yt-dlp")?);
    let ffmpeg = crate::paths::is_regular_file(&tool_path(app, "ffmpeg")?);
    let cpu_engine = engine_available(app, "cpu")?;
    let cuda_engine = engine_available(app, "cuda")?;
    let (nvidia_devices, nvidia_driver_version) = detect_nvidia_snapshot();
    let nvidia_gpu = select_nvidia_display_device(&nvidia_devices);
    let cuda_devices = cuda_visible_nvidia_devices(&nvidia_devices);
    let cuda_gpu = select_nvidia_device(&cuda_devices);
    set_selected_nvidia_device(cuda_gpu.as_ref().or(nvidia_gpu.as_ref()));
    let gpu = cuda_gpu
        .clone()
        .or_else(|| nvidia_gpu.clone())
        .or_else(detect_generic_gpu);
    let nvidia = nvidia_gpu.is_some();
    let cuda_supported = cfg!(all(target_os = "windows", target_arch = "x86_64")) && nvidia;
    let nvidia_driver_version = nvidia.then_some(nvidia_driver_version).flatten();
    let cuda_installable = cuda_supported
        && nvidia_driver_version
            .as_deref()
            .is_some_and(cuda_driver_is_compatible);
    let gpu_name = gpu.as_ref().map(|info| info.name.clone());
    let gpu_memory_mb = gpu.as_ref().and_then(|info| info.total_memory_mb);
    let gpu_free_memory_mb = gpu.as_ref().and_then(|info| info.free_memory_mb);
    let available_memory_mb = crate::resources::available_memory_mb();
    let available_vram_mb = cuda_gpu
        .as_ref()
        .or(gpu.as_ref())
        .and_then(GpuInfo::available_memory_mb);
    let vulkan_engine = if gpu.is_some() && cfg!(any(target_os = "windows", target_os = "linux")) {
        Some(engine_path(app, "vulkan")?)
    } else {
        None
    };
    let accelerators = accelerators::catalog(app, gpu.is_some(), vulkan_engine.as_deref())?;
    let job_storage_bytes = crate::history::job_storage_bytes(app)?;
    let vulkan_installed = accelerators
        .iter()
        .any(|accelerator| accelerator.backend == "vulkan" && accelerator.installed);
    let mut compute_devices = Vec::new();
    if cuda_installable && cuda_engine {
        compute_devices.extend(cuda_devices.iter().filter_map(|device| {
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
        if let Some(engine) = &vulkan_engine {
            compute_devices.extend(detect_vulkan_devices_from_engine(engine));
        }
    }
    let cpu_threads = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let has_discrete_vulkan = compute_devices
        .iter()
        .any(|device| device.backend == "vulkan" && !device.integrated);
    let metal_installed = accelerators
        .iter()
        .any(|accelerator| accelerator.backend == "metal" && accelerator.installed);
    let recommended_id = models::recommended_model_id_for_system(
        available_vram_mb,
        nvidia && cuda_installable && cuda_engine && cuda_gpu.is_some(),
        has_discrete_vulkan,
        metal_installed,
        cpu_threads,
    );

    let (recommendation, recommended_model_id, recommended_backend) = if nvidia
        && cuda_installable
        && cuda_engine
        && cuda_gpu.is_some()
    {
        (
            format!(
                "{} terdeteksi dengan {}. Auto akan mencoba CUDA; {} direkomendasikan.",
                gpu_name.clone().unwrap_or_else(|| "NVIDIA GPU".into()),
                format_vram(cuda_gpu.as_ref().and_then(GpuInfo::available_memory_mb)),
                recommended_id,
            ),
            recommended_id,
            "auto".to_string(),
        )
    } else if nvidia && cuda_supported && !cuda_installable {
        (
            format!(
                "NVIDIA terdeteksi, tetapi driver {} tidak memenuhi syarat CUDA 12.x (minimal {}). Update driver sebelum memasang CUDA.",
                nvidia_driver_version.as_deref().unwrap_or("tidak terbaca"),
                MIN_CUDA_DRIVER_VERSION,
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
    } else if nvidia && cuda_supported && cuda_engine && cuda_gpu.is_none() {
        (
            "NVIDIA terdeteksi, tetapi CUDA tidak menampilkan perangkat yang dapat dipetakan ke engine. Auto akan mencoba Vulkan atau CPU.".into(),
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
    } else {
        non_nvidia_recommendation(
            recommended_id,
            cpu_threads,
            has_discrete_vulkan || metal_installed,
        )
    };

    let _ = runtime;
    Ok(SystemStatus {
        qwen_runtime: crate::qwen::installed(app),
        qwen_backends: crate::qwen::supported_backends(),
        yt_dlp,
        ffmpeg,
        cpu_engine,
        cuda_engine,
        nvidia,
        gpu_name,
        gpu_memory_mb,
        gpu_free_memory_mb,
        available_memory_mb,
        nvidia_driver_version,
        cuda_installable,
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
    #[test]
    fn compute_capability_query_uses_physical_uuid_instead_of_a_cuda_ordinal() {
        let gpu = super::GpuInfo {
            name: "Selected GPU".into(),
            total_memory_mb: Some(8192),
            free_memory_mb: Some(6144),
            uuid: Some("GPU-aaaaaaaa".into()),
            device_index: Some(3),
        };
        let command = super::nvidia_compute_capability_command("nvidia-smi", &gpu).unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                "--id=GPU-aaaaaaaa",
                "--query-gpu=uuid,compute_cap",
                "--format=csv,noheader,nounits",
            ]
        );
        for uuid in [None, Some("3"), Some("--query-gpu=name")] {
            let mut invalid = gpu.clone();
            invalid.uuid = uuid.map(str::to_owned);
            assert!(super::nvidia_compute_capability_command("nvidia-smi", &invalid).is_err());
        }
    }

    #[test]
    fn compute_capability_requires_one_valid_result_for_the_selected_uuid() {
        for (output, expected) in [
            ("GPU-aaaaaaaa, 6.1\n", (6, 1)),
            ("GPU-AAAAAAAA, 7.5\r\n", (7, 5)),
            ("GPU-aaaaaaaa, 8.9\n", (8, 9)),
            ("GPU-aaaaaaaa, 12.0\n", (12, 0)),
        ] {
            assert_eq!(
                super::parse_nvidia_compute_capability(output.as_bytes(), "GPU-aaaaaaaa"),
                Some(expected)
            );
        }
        for output in [
            "",
            "GPU-bbbbbbbb, 8.9",
            "GPU-aaaaaaaa, N/A",
            "GPU-aaaaaaaa, 8",
            "GPU-aaaaaaaa, 0.0",
            "GPU-aaaaaaaa, 8.10",
            "GPU-aaaaaaaa, 8.9.0",
            "GPU-aaaaaaaa, 8.9, extra",
            "GPU-aaaaaaaa, -8.9",
            "GPU-aaaaaaaa, 8.+9",
            "GPU-aaaaaaaa, 8.9\nGPU-bbbbbbbb, 9.0",
        ] {
            assert_eq!(
                super::parse_nvidia_compute_capability(output.as_bytes(), "GPU-aaaaaaaa"),
                None,
                "{output}"
            );
        }
    }

    #[test]
    fn combined_nvidia_query_preserves_devices_and_driver_validation() {
        let output = b"0, GPU A, GPU-aaaaaaaa, 8192, 0, 528.33\n1, GPU B, GPU-bbbbbbbb, 16384, 8192, 528.33\n";
        let (devices, driver) = super::parse_nvidia_snapshot_output(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].free_memory_mb, Some(0));
        assert_eq!(
            super::select_nvidia_device(&devices).unwrap().device_index,
            Some(1)
        );
        assert_eq!(devices[1].uuid.as_deref(), Some("GPU-bbbbbbbb"));
        assert_eq!(driver.as_deref(), Some("528.33"));
        assert!(super::cuda_driver_is_compatible(driver.as_deref().unwrap()));
        let (_, driver) =
            super::parse_nvidia_snapshot_output(b"0, GPU, GPU-aaaaaaaa, 8192, 4096, N/A\n");
        assert!(driver.is_none());
    }

    #[test]
    fn cuda_uuid_visibility_preserves_physical_ids_when_cuda_ordinals_differ() {
        let devices = super::parse_nvidia_devices(
            b"0, GPU A, GPU-aaaaaaaa, 8192, 7000, 528.33\n1, GPU B, GPU-bbbbbbbb, 16384, 6000, 528.33\n",
        );
        // CUDA ordinal 0 is NVML index 1, e.g. under FASTEST_FIRST. The driver
        // resolves the mask; only its returned UUIDs may identify physical GPUs.
        let visible = super::nvidia_devices_for_cuda_uuids(
            &devices,
            Some(&["GPU-bbbbbbbb".into(), "GPU-aaaaaaaa".into()]),
        );
        assert_eq!(
            visible
                .iter()
                .map(|device| device.device_index)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(0)]
        );
        assert_eq!(
            super::nvidia_devices_for_cuda_uuids(&devices, Some(&["GPU-bbbbbbbb".into()]))[0]
                .device_index,
            Some(1)
        );
    }

    #[test]
    fn cuda_uuid_visibility_empty_or_unmappable_fails_closed() {
        let devices = super::parse_nvidia_devices(
            b"0, GPU A, GPU-aaaaaaaa, 8192, 7000, 528.33\n1, GPU B, GPU-bbbbbbbb, 16384, 6000, 528.33\n",
        );
        assert!(super::nvidia_devices_for_cuda_uuids(&devices, Some(&[])).is_empty());
        assert!(
            super::nvidia_devices_for_cuda_uuids(&devices, Some(&["GPU-cccccccc".into()]),)
                .is_empty()
        );
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn native_cuda_driver_uuids_match_physical_inventory() {
        let (devices, driver) = super::detect_nvidia_snapshot();
        if devices.is_empty()
            || !driver
                .as_deref()
                .is_some_and(super::cuda_driver_is_compatible)
        {
            return;
        }
        let uuids =
            super::cuda_visible_device_uuids().expect("compatible CUDA driver should enumerate");
        let matched = super::nvidia_devices_for_cuda_uuids(&devices, Some(&uuids));
        assert_eq!(
            matched.len(),
            uuids.len(),
            "CUDA UUID did not map to NVML inventory"
        );
        for gpu in &matched {
            let capability = super::nvidia_compute_capability(gpu)
                .expect("compatible NVIDIA driver should report compute capability by UUID");
            eprintln!(
                "{}: compute capability {}.{}",
                gpu.name, capability.0, capability.1
            );
        }
        let visible = super::cuda_visible_nvidia_devices(&devices);
        if std::env::var_os("CUDA_VISIBLE_DEVICES").is_some_and(|mask| mask.is_empty()) {
            assert!(visible.is_empty());
        }
        eprintln!(
            "Physical NVIDIA GPUs: {}; CUDA-visible GPUs: {}",
            devices.len(),
            visible.len()
        );
    }

    #[test]
    fn cuda_child_is_restricted_to_the_selected_gpu_uuid() {
        let gpu = super::GpuInfo {
            name: "Selected GPU".into(),
            total_memory_mb: Some(8192),
            free_memory_mb: Some(4096),
            uuid: Some("GPU-aaaaaaaa".into()),
            device_index: Some(3),
        };
        let mut command = std::process::Command::new("whisper-cli");

        super::configure_cuda_command(&mut command, &gpu).unwrap();

        let selected_uuid = command
            .get_envs()
            .find(|(key, _)| key == &std::ffi::OsStr::new("CUDA_VISIBLE_DEVICES"))
            .and_then(|(_, value)| value)
            .expect("CUDA child should receive the selected GPU UUID");
        assert_eq!(selected_uuid, std::ffi::OsStr::new("GPU-aaaaaaaa"));
    }

    #[test]
    fn known_free_vram_is_ranked_before_unknown_free_vram() {
        let devices = super::parse_nvidia_devices(
            b"0, Unknown-free GPU, GPU-aaaaaaaa, 24576, N/A, 528.33\n1, Usable GPU, GPU-bbbbbbbb, 12288, 8192, 528.33\n",
        );
        let ordered = super::ordered_nvidia_devices(&devices);
        assert_eq!(ordered[0].name, "Usable GPU");
        assert_eq!(ordered[1].name, "Unknown-free GPU");
        assert_eq!(
            super::select_nvidia_device(&devices)
                .unwrap()
                .free_memory_mb,
            Some(8192)
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_display_adapter_probe_handles_present_or_headless_sessions() {
        if let Some(gpu) = super::detect_generic_gpu() {
            assert!(!gpu.name.is_empty());
            assert_eq!(super::clean_gpu_name(&gpu.name), Some(gpu.name.clone()));
        }
    }
    #[test]
    fn non_nvidia_status_preserves_gpu_model_policy_with_fewer_cpu_threads() {
        for (vulkan, metal, threads, expected) in [
            (true, false, 8, "large-v3-turbo-q5_0"),
            (false, true, 8, "large-v3-turbo-q5_0"),
            (false, false, 8, "base"),
            (false, false, 12, "large-v3-turbo-q5_0"),
        ] {
            let model =
                crate::models::recommended_model_id_for_system(None, false, vulkan, metal, threads);
            let (message, actual, backend) =
                super::non_nvidia_recommendation(model, threads, vulkan || metal);
            assert_eq!(actual, expected);
            assert_eq!(backend, "auto");
            if vulkan || metal {
                assert!(message.contains("GPU siap"));
            }
        }
    }
    use super::{
        cuda_driver_is_compatible, memory_pressure_message, parse_nvidia_gpus,
        parse_vulkan_devices, UsageMonitor, UsageSnapshot,
    };

    #[test]
    fn linux_cpu_usage_uses_interval_aggregate_and_excludes_guest_time() {
        let mut sampler = super::LinuxCpuSampler::default();
        assert_eq!(
            sampler.sample("cpu 100 0 0 900 0 0 0 0 100 0\ncpu0 100 0 0 900"),
            None
        );
        assert_eq!(sampler.sample("cpu 150 0 0 950 0 0 0 0 150 0"), Some(50.0));
        assert_eq!(sampler.sample("cpu 150 0 0 1050 0 0 0 0"), Some(0.0));
        assert_eq!(sampler.sample("cpu 250 0 0 1050 0 0 0 0"), Some(100.0));
    }

    #[test]
    fn linux_cpu_usage_rebaselines_after_missing_invalid_or_reset_counters() {
        let mut sampler = super::LinuxCpuSampler::default();
        assert_eq!(sampler.sample("cpu 100 0 0 100 10 0 0 0"), None);
        assert_eq!(sampler.sample("cpu 100 0 0 100 10 0 0 0"), None);
        assert_eq!(sampler.sample("cpu 110 0 0 110 9 0 0 0"), None);
        assert!(sampler
            .sample("cpu 120 0 0 120 9 0 0 0")
            .unwrap()
            .is_finite());
        for invalid in [
            "",
            "cpu0 1 2 3 4",
            "cpu 1 2 3",
            "cpu 1 2 3 -1",
            "cpu 1 2 3 nope",
        ] {
            assert_eq!(sampler.sample(invalid), None);
            assert_eq!(sampler.sample("cpu 1 0 0 1"), None);
            assert_eq!(sampler.sample("cpu 2 0 0 2"), Some(50.0));
        }
        assert_eq!(sampler.sample("cpu 0 0 0 0"), None);
        assert_eq!(sampler.sample("cpu 1 0 0 1"), Some(50.0));
        assert_eq!(
            sampler.sample(&format!("cpu {} 0 0 {}", u64::MAX, u64::MAX)),
            None
        );
        assert_eq!(sampler.sample("cpu 0 0 0 0"), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn vulkan_probe_child_has_no_console_and_captures_stderr() {
        const CHILD: &str = "WHISPERTUBE_VULKAN_CONSOLE_TEST_CHILD";
        if let Some(mode) = std::env::var_os(CHILD) {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetConsoleWindow() -> *mut std::ffi::c_void;
            }
            assert!(unsafe { GetConsoleWindow() }.is_null());
            eprintln!("hidden-probe-ok");
            if mode == "sleep" {
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
            return;
        }
        let mut command = super::vulkan_probe_command(&std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "system::tests::vulkan_probe_child_has_no_console_and_captures_stderr",
                "--nocapture",
            ])
            .env(CHILD, "1");
        let (status, stderr) = crate::process::run_with_timeout_captured(
            &mut command,
            std::time::Duration::from_secs(10),
            4096,
        )
        .unwrap();
        assert!(status.success(), "{stderr}");
        assert!(stderr.contains("hidden-probe-ok"));

        command.env(CHILD, "sleep");
        let error = crate::process::run_with_timeout_captured(
            &mut command,
            std::time::Duration::from_millis(250),
            4096,
        )
        .unwrap_err();
        assert!(error.contains("melewati batas waktu"));

        let cancelled = std::sync::atomic::AtomicBool::new(true);
        let error = crate::process::run_with_timeout_captured_cancelable(
            &mut command,
            std::time::Duration::from_secs(10),
            4096,
            &cancelled,
        )
        .unwrap_err();
        assert!(error.contains("dibatalkan"));
    }

    #[test]
    fn selects_nvidia_device_with_the_most_free_memory() {
        let output = b"0,Small GPU,GPU-aaaaaaaa,4096,512\n1,Large GPU,GPU-bbbbbbbb,12288,8192\n";
        let gpu = parse_nvidia_gpus(output).expect("GPU should be parsed");
        assert_eq!(gpu.name, "Large GPU");
        assert_eq!(gpu.device_index, Some(1));
        assert_eq!(gpu.free_memory_mb, Some(8192));
    }

    #[test]
    fn keeps_first_device_when_memory_is_unavailable_or_tied() {
        let output = b"0,First GPU,GPU-aaaaaaaa,N/A,N/A\n1,Second GPU,GPU-bbbbbbbb,N/A,N/A\n";
        let gpu = parse_nvidia_gpus(output).expect("GPU should be parsed");
        assert_eq!(gpu.name, "First GPU");
        assert_eq!(gpu.device_index, Some(0));
    }

    #[test]
    fn enforces_cuda_driver_minimum() {
        assert!(!cuda_driver_is_compatible("528.32"));
        assert!(cuda_driver_is_compatible("528.33"));
        assert!(cuda_driver_is_compatible("551.61"));
        assert!(!cuda_driver_is_compatible("not-a-version"));
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
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::tests::captured_probe_child",
                "--nocapture",
            ])
            .env("WHISPERTUBE_CAPTURE_CHILD", "large-stdout");
        assert!(
            super::command_output_with_timeout(command, std::time::Duration::from_secs(8))
                .is_none()
        );
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
