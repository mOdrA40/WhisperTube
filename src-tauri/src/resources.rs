use std::path::{Path, PathBuf};
#[cfg(windows)]
use sysinfo::Disks;
use sysinfo::System;

pub fn available_memory_bytes() -> u64 {
    let mut system = System::new();
    system.refresh_memory();
    system.available_memory()
}

pub fn available_memory_mb() -> u64 {
    available_memory_bytes() / 1024 / 1024
}

#[cfg(windows)]
pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    let path = existing_disk_path(path).ok()?;
    let disks = Disks::new_with_refreshed_list();
    let mounts = disks
        .iter()
        .map(|disk| disk.mount_point().to_path_buf())
        .collect::<Vec<_>>();
    matching_mount_index(&path, &mounts).map(|index| disks[index].available_space())
}

#[cfg(unix)]
pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    unix_available_disk_bytes(&existing_disk_path(path).ok()?).ok()
}

#[cfg(unix)]
fn unix_available_disk_bytes(path: &Path) -> std::io::Result<u64> {
    use std::{ffi::CString, io, mem::MaybeUninit, os::unix::ffi::OsStrExt};

    let path = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: the terminated path and writable statvfs buffer live through the call.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statvfs initialized the output on success.
    let stats = unsafe { stats.assume_init() };
    let available = u128::from(stats.f_frsize) * u128::from(stats.f_bavail);
    Ok(u64::try_from(available).unwrap_or(u64::MAX))
}

// Capacity belongs to the filesystem reached by the OS, including linked
// ancestors. Future files/directories use their nearest existing ancestor.
fn existing_disk_path(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    for ancestor in absolute.ancestors() {
        match std::fs::canonicalize(ancestor) {
            Ok(path) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A dangling link is not an output that can be created here.
                if std::fs::symlink_metadata(ancestor).is_ok_and(|meta| meta.is_symlink()) {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Storage ancestor tidak ditemukan",
    ))
}

// Comparison only: keep the original path for all filesystem operations.
// canonicalize() adds verbatim prefixes on Windows, including to cached jobs.
#[cfg(any(windows, test))]
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

#[cfg(any(windows, test))]
fn matching_mount_index(path: &Path, mounts: &[PathBuf]) -> Option<usize> {
    let path = mount_comparison_path(path);
    mounts
        .iter()
        .enumerate()
        .filter(|(_, mount)| path.starts_with(mount_comparison_path(mount)))
        .max_by_key(|(_, mount)| mount_comparison_path(mount).as_os_str().len())
        .map(|(index, _)| index)
}

#[cfg(windows)]
pub fn require_disk_allocations(allocations: &[(&Path, u64)], purpose: &str) -> Result<(), String> {
    let disks = Disks::new_with_refreshed_list();
    let mounts = disks
        .iter()
        .map(|disk| disk.mount_point().to_path_buf())
        .collect::<Vec<_>>();
    let required_by_disk = disk_allocations_by_mount(allocations, &mounts)?;

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

#[cfg(unix)]
pub fn require_disk_allocations(allocations: &[(&Path, u64)], purpose: &str) -> Result<(), String> {
    use std::{collections::BTreeMap, os::unix::fs::MetadataExt};

    struct Allocation {
        path: PathBuf,
        available: u64,
        required: u64,
    }

    let mut filesystems = BTreeMap::new();
    for (path, required) in allocations {
        let path = existing_disk_path(path)
            .map_err(|error| format!("Gagal menentukan filesystem tujuan: {error}"))?;
        let device = std::fs::metadata(&path)
            .map_err(|error| format!("Gagal membaca filesystem tujuan: {error}"))?
            .dev();
        let available = unix_available_disk_bytes(&path)
            .map_err(|error| format!("Ruang disk tersedia tidak dapat diperiksa: {error}"))?;
        let allocation = filesystems.entry(device).or_insert(Allocation {
            path,
            available,
            required: 0,
        });
        allocation.available = allocation.available.min(available);
        allocation.required = allocation.required.saturating_add(*required);
    }
    for allocation in filesystems.into_values() {
        if allocation.available < allocation.required {
            return Err(format!(
                "Ruang disk tidak cukup untuk {purpose} pada {}. Diperlukan sekitar {:.1} GB, tersedia {:.1} GB.",
                allocation.path.display(),
                allocation.required as f64 / 1024_f64.powi(3),
                allocation.available as f64 / 1024_f64.powi(3),
            ));
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn disk_allocations_by_mount(
    allocations: &[(&Path, u64)],
    mounts: &[PathBuf],
) -> Result<Vec<u64>, String> {
    let mut required_by_disk = vec![0u64; mounts.len()];
    for (path, required) in allocations {
        let resolved = existing_disk_path(path)
            .map_err(|error| format!("Gagal menentukan filesystem tujuan: {error}"))?;
        let index = matching_mount_index(&resolved, mounts)
            .ok_or_else(|| "Ruang disk tersedia tidak dapat diperiksa.".to_string())?;
        required_by_disk[index] = required_by_disk[index].saturating_add(*required);
    }
    Ok(required_by_disk)
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
    fn disk_identity_resolves_junctions_before_mount_selection() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("disk-junction-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("volume")).unwrap();
        let volume = fs::canonicalize(root.join("volume")).unwrap();
        let alias = root.join("alias");
        let output = std::process::Command::new("pwsh")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:WHISPERTUBE_JUNCTION_PATH -Target $env:WHISPERTUBE_JUNCTION_TARGET | Out-Null"])
            .env("WHISPERTUBE_JUNCTION_PATH", &alias)
            // New-Item stores a verbatim \\?\ target as an invalid junction.
            // Use its normal Win32 spelling; compare the resolved identity below.
            .env("WHISPERTUBE_JUNCTION_TARGET", root.join("volume"))
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::canonicalize(&alias).unwrap(), volume);
        let future = alias.join("future/audio.wav");
        assert_eq!(super::existing_disk_path(&future).unwrap(), volume);
        assert_eq!(
            super::disk_allocations_by_mount(
                &[(&future, 7), (&volume.join("model.bin"), 9)],
                &[fs::canonicalize(&root).unwrap(), volume],
            )
            .unwrap(),
            vec![0, 16]
        );
        assert_eq!(
            fs::canonicalize(&root).unwrap().parent(),
            Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn disk_identity_resolves_linked_ancestors_and_groups_future_allocations() {
        use std::{fs, os::unix::fs::symlink};
        let root = std::env::temp_dir().join(format!("disk-alias-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("volume")).unwrap();
        let canonical_root = fs::canonicalize(&root).unwrap();
        let volume = canonical_root.join("volume");
        let alias = root.join("alias");
        symlink(&volume, &alias).unwrap();
        let future = alias.join("new/job/audio.wav");
        assert_eq!(super::existing_disk_path(&future).unwrap(), volume);
        assert_eq!(
            super::disk_allocations_by_mount(
                &[(&future, 11), (&volume.join("model.bin"), 13)],
                &[canonical_root, volume.clone()],
            )
            .unwrap(),
            vec![0, 24]
        );
        symlink(root.join("absent"), root.join("broken")).unwrap();
        assert!(super::existing_disk_path(&root.join("broken/file")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

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

    #[cfg(unix)]
    #[test]
    fn disk_guards_add_allocations_resolved_to_the_same_filesystem() {
        let directory = tempfile::tempdir().unwrap();
        let future = directory.path().join("audio.wav");
        let available = super::available_disk_bytes(&future).unwrap();
        assert!(available > 4);
        let single = available - available / 4;
        super::require_disk(&future, single, "test").unwrap();
        let error = super::require_disk_allocations(
            &[(&future, single), (directory.path(), single)],
            "test",
        )
        .unwrap_err();
        assert!(error.contains("Ruang disk tidak cukup"));
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
