use std::path::{Path, PathBuf};
use sysinfo::{Disks, System};

pub fn available_memory_bytes() -> u64 {
    let mut system = System::new();
    system.refresh_memory();
    system.available_memory()
}

pub fn available_memory_mb() -> u64 {
    available_memory_bytes() / 1024 / 1024
}

pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    let mounts = disks
        .iter()
        .map(|disk| disk.mount_point().to_path_buf())
        .collect::<Vec<_>>();
    matching_mount_index(path, &mounts).map(|index| disks[index].available_space())
}

// Comparison only: keep the original path for all filesystem operations.
// canonicalize() adds verbatim prefixes on Windows, including to cached jobs.
fn mount_comparison_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::{
            ffi::OsString,
            path::{Component, Prefix},
        };
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let mut normalized = match prefix.kind() {
                Prefix::VerbatimDisk(drive) | Prefix::Disk(drive) => {
                    PathBuf::from(format!("{}:\\", drive.to_ascii_uppercase() as char))
                }
                Prefix::VerbatimUNC(server, share) | Prefix::UNC(server, share) => {
                    let mut value = OsString::from("\\\\");
                    value.push(server);
                    value.push("\\");
                    value.push(share);
                    PathBuf::from(value)
                }
                _ => return path.into(),
            };
            for component in components {
                if !matches!(component, Component::RootDir) {
                    normalized.push(component.as_os_str());
                }
            }
            return normalized;
        }
    }
    path.into()
}

fn matching_mount_index(path: &Path, mounts: &[PathBuf]) -> Option<usize> {
    let path = mount_comparison_path(path);
    mounts
        .iter()
        .enumerate()
        .filter(|(_, mount)| path.starts_with(mount_comparison_path(mount)))
        .max_by_key(|(_, mount)| mount_comparison_path(mount).as_os_str().len())
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

    #[cfg(windows)]
    #[test]
    fn verbatim_paths_match_drive_and_unc_mounts_without_requiring_a_file() {
        for (path, mount) in [
            (r"\\?\C:\jobs\new\audio.wav", r"C:\"),
            (r"c:\jobs\new\audio.wav", r"\\?\C:\"),
            (r"\\?\UNC\server\share\new\audio.wav", r"\\server\share\"),
        ] {
            assert_eq!(
                matching_mount_index(PathBuf::from(path).as_path(), &[mount.into()]),
                Some(0)
            );
        }
        let mounts = vec![PathBuf::from(r"C:\"), PathBuf::from(r"C:\mounted")];
        assert_eq!(
            matching_mount_index(PathBuf::from(r"\\?\C:\mounted\job").as_path(), &mounts),
            Some(1)
        );
        assert_eq!(
            matching_mount_index(
                PathBuf::from(r"\\?\C:\mounted-other\job").as_path(),
                &mounts
            ),
            Some(0)
        );
        // Verbatim prefixes must not make a root appear more specific than
        // a short nested mount using the ordinary prefix.
        assert_eq!(
            matching_mount_index(
                PathBuf::from(r"\\?\C:\m\job").as_path(),
                &[r"\\?\C:\".into(), r"C:\m".into()]
            ),
            Some(1)
        );
    }

    #[cfg(windows)]
    #[test]
    fn disk_guards_accept_a_canonical_retry_directory_and_a_future_output() {
        let directory = std::env::temp_dir();
        let canonical = std::fs::canonicalize(&directory).unwrap();
        let raw = super::available_disk_bytes(&directory).unwrap();
        assert!(raw > 0);
        assert!(super::available_disk_bytes(&canonical).is_some());
        let future = canonical.join("whispertube-future-output.wav");
        super::require_disk(&future, 1, "test").unwrap();
        super::require_disk_allocations(&[(&future, 1), (&directory, 1)], "test").unwrap();
    }

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
