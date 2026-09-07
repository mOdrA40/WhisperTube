use std::path::{Path, PathBuf};
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

fn matching_mount_index(path: &Path, mounts: &[PathBuf]) -> Option<usize> {
    mounts
        .iter()
        .enumerate()
        .filter(|(_, mount)| path.starts_with(mount))
        .max_by_key(|(_, mount)| mount.as_os_str().len())
        .map(|(index, _)| index)
}

pub fn require_disk_allocations(allocations: &[(&Path, u64)], purpose: &str) -> Result<(), String> {
    let disks = Disks::new_with_refreshed_list();
    let mounts = disks
        .iter()
        .map(|disk| disk.mount_point().to_path_buf())
        .collect::<Vec<_>>();
    let mut required_by_disk = vec![0u64; disks.len()];

    for (path, required) in allocations {
        let index = matching_mount_index(path, &mounts)
            .ok_or_else(|| "Ruang disk tersedia tidak dapat diperiksa.".to_string())?;
        required_by_disk[index] = required_by_disk[index].saturating_add(*required);
    }

    for (index, required) in required_by_disk.into_iter().enumerate() {
        if required == 0 {
            continue;
        }
        let available = disks[index].available_space();
        if available < required {
            return Err(format!(
                "Ruang disk tidak cukup untuk {purpose} pada {}. Diperlukan sekitar {:.1} GB, tersedia {:.1} GB.",
                mounts[index].display(),
                required as f64 / 1024_f64.powi(3),
                available as f64 / 1024_f64.powi(3),
            ));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::matching_mount_index;
    use std::path::PathBuf;

    #[test]
    fn disk_matching_prefers_the_most_specific_mount() {
        let mounts = vec![
            PathBuf::from("/"),
            PathBuf::from("/mnt"),
            PathBuf::from("/mnt/data"),
        ];
        assert_eq!(
            matching_mount_index(PathBuf::from("/mnt/data/jobs/file").as_path(), &mounts),
            Some(2)
        );
    }
}
