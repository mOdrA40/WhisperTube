use std::path::Path;
use sysinfo::{Disks, System};

pub fn available_memory_bytes() -> u64 {
    let mut system = System::new();
    system.refresh_memory();
    system.available_memory()
}

pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

pub fn require_memory(required: u64, purpose: &str) -> Result<(), String> {
    let available = available_memory_bytes();
    if available < required {
        return Err(format!(
            "RAM tersedia tidak cukup untuk {purpose}. Diperlukan sekitar {:.1} GB, tersedia {:.1} GB.",
            required as f64 / 1024_f64.powi(3),
            available as f64 / 1024_f64.powi(3),
        ));
    }
    Ok(())
}

pub fn require_disk(path: &Path, required: u64, purpose: &str) -> Result<(), String> {
    let available = available_disk_bytes(path)
        .ok_or_else(|| "Ruang disk tersedia tidak dapat diperiksa.".to_string())?;
    if available < required {
        return Err(format!(
            "Ruang disk tidak cukup untuk {purpose}. Diperlukan sekitar {:.1} GB, tersedia {:.1} GB.",
            required as f64 / 1024_f64.powi(3),
            available as f64 / 1024_f64.powi(3),
        ));
    }
    Ok(())
}
