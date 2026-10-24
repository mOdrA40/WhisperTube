//! Publish already-flushed files before committing database/checkpoint references.
use std::{fs, io, path::Path};

pub fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Path has no parent directory"))?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    #[cfg(windows)]
    let _ = parent; // Windows publication uses MOVEFILE_WRITE_THROUGH below.
    Ok(())
}

pub fn create_dir_all(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_dir_all(parent)?;
    }
    match fs::create_dir(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

/// Only for application-owned storage leaves. Do not chmod user ancestors or
/// external export directories. Securing the root also protects legacy files
/// and files created by SQLite, yt-dlp, FFmpeg and Whisper inside it.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            create_dir_all(parent)?;
        }
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => sync_parent(path)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        // Operate on the opened directory, never follow a substituted leaf
        // symlink or change a directory belonging to a different OS account.
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(path)?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        if directory.metadata()?.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "App storage directory belongs to another OS account",
            ));
        }
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
        #[cfg(target_os = "macos")]
        remove_directory_acl(&directory)?;
        directory.sync_all()
    }
    #[cfg(not(unix))]
    create_dir_all(path)
}

#[cfg(target_os = "macos")]
fn remove_directory_acl(directory: &fs::File) -> io::Result<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    unsafe extern "C" {
        fn acl_init(count: i32) -> *mut c_void;
        fn acl_set_fd(fd: i32, acl: *mut c_void) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // Darwin extended allow entries can grant access independently of mode
    // bits. Replace inherited/old ACLs on this owned root before using it.
    // SAFETY: acl_init returns an owned empty ACL; a live fd is passed below.
    let acl = unsafe { acl_init(0) };
    if acl.is_null() {
        return Err(io::Error::last_os_error());
    }
    let status = unsafe { acl_set_fd(directory.as_raw_fd(), acl) };
    let error = (status != 0).then(io::Error::last_os_error);
    // SAFETY: release the owned ACL once on both paths.
    unsafe { acl_free(acl) };
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Both paths must be on the same filesystem. Callers flush file bytes first.
pub fn publish(temporary: &Path, target: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        publish_unix(temporary, target, sync_parent)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
        }
        fn wide(path: &Path) -> io::Result<Vec<u16>> {
            // std::fs::rename also supports long paths. Canonicalize only the
            // existing parent: the target leaf may not exist and must not be
            // followed if it is a symbolic link being replaced.
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let name = path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "Path has no file name")
            })?;
            let path = fs::canonicalize(parent)?.join(name);
            let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
            if value.contains(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Path contains NUL",
                ));
            }
            value.push(0);
            Ok(value)
        }
        let source = wide(temporary)?;
        let target = wide(target)?;
        // SAFETY: both buffers are NUL-terminated and live for this call.
        // REPLACE_EXISTING | WRITE_THROUGH; no cross-volume copy fallback.
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x1 | 0x8) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
fn publish_unix(
    temporary: &Path,
    target: &Path,
    sync: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    fs::rename(temporary, target)?;
    sync(target)?;
    if temporary.parent() != target.parent() {
        sync(temporary)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn private_storage_protects_new_and_legacy_contents_without_changing_ancestors() {
        use std::os::unix::fs::PermissionsExt;
        let parent =
            std::env::temp_dir().join(format!("whispertube-private-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        let root = parent.join("app");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        for name in [
            "result.json",
            "transcript.txt",
            "audio.wav",
            "whispertube.db",
            "whispertube.db-wal",
        ] {
            fs::write(root.join(name), b"private legacy bytes").unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o644)).unwrap();
        }
        create_private_dir_all(&root).unwrap();
        create_private_dir_all(&root).unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let job = root.join("jobs");
        create_private_dir_all(&job).unwrap();
        assert_eq!(
            fs::metadata(job).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for name in [
            "result.json",
            "transcript.txt",
            "audio.wav",
            "whispertube.db",
            "whispertube.db-wal",
        ] {
            assert_eq!(fs::read(root.join(name)).unwrap(), b"private legacy bytes");
        }
        fs::remove_dir_all(parent).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_storage_rejects_symlink_and_file_without_changing_their_targets() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let parent =
            std::env::temp_dir().join(format!("whispertube-private-link-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        let outside = parent.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        let link = parent.join("app");
        symlink(&outside, &link).unwrap();
        assert!(create_private_dir_all(&link).is_err());
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let file = parent.join("file");
        fs::write(&file, b"unchanged").unwrap();
        assert!(create_private_dir_all(&file).is_err());
        assert_eq!(fs::read(file).unwrap(), b"unchanged");
        fs::remove_dir_all(parent).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn private_storage_clears_inherited_macos_allow_acl() {
        let parent =
            std::env::temp_dir().join(format!("whispertube-private-acl-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        assert!(std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone allow list,search,readattr,readextattr,readsecurity,file_inherit,directory_inherit"])
            .arg(&parent).status().unwrap().success());
        let root = parent.join("app");
        fs::create_dir(&root).unwrap();
        let acl = |path: &Path| {
            let output = std::process::Command::new("/bin/ls")
                .arg("-lde")
                .arg(path)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        };
        assert!(acl(&root).contains("everyone"));
        create_private_dir_all(&root).unwrap();
        assert!(!acl(&root).contains("everyone"));
        assert!(acl(&parent).contains("everyone"));
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn publish_replaces_only_after_the_complete_staging_file_exists() {
        let root =
            std::env::temp_dir().join(format!("whispertube-durable-{}", uuid::Uuid::new_v4()));
        create_dir_all(&root.join("nested")).unwrap();
        let target = root.join("nested/result");
        let temporary = root.join("nested/staging");
        fs::write(&target, b"old").unwrap();
        assert!(publish(&temporary, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        fs::write(&temporary, b"new complete result").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .unwrap()
            .sync_all()
            .unwrap();
        publish(&temporary, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new complete result");
        assert!(!temporary.exists());
        #[cfg(unix)]
        assert!(sync_parent(&root.join("missing/file")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn directory_sync_is_after_rename_and_failure_is_not_acknowledged() {
        let root =
            std::env::temp_dir().join(format!("whispertube-sync-failure-{}", uuid::Uuid::new_v4()));
        create_dir_all(&root).unwrap();
        let temporary = root.join("staging");
        let target = root.join("result");
        fs::write(&temporary, b"result").unwrap();
        let error = publish_unix(&temporary, &target, |path| {
            assert_eq!(path, target);
            assert!(!temporary.exists());
            assert_eq!(fs::read(&target).unwrap(), b"result");
            Err(io::Error::other("injected directory sync failure"))
        })
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("injected directory sync failure"));
        // Rename alone is not success: callers must still roll back/withhold
        // the database/checkpoint acknowledgement when directory sync fails.
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn publication_preserves_standard_library_long_path_support() {
        let root =
            std::env::temp_dir().join(format!("whispertube-long-publish-{}", uuid::Uuid::new_v4()));
        let mut directory = root.clone();
        for _ in 0..8 {
            directory.push("long-directory-component-for-export");
        }
        create_dir_all(&directory).unwrap();
        let temporary = directory.join("staging");
        let target = directory.join("result");
        fs::write(&temporary, b"complete").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .unwrap()
            .sync_all()
            .unwrap();
        publish(&temporary, &target).unwrap();
        assert_eq!(fs::read(target).unwrap(), b"complete");
        fs::remove_dir_all(root).unwrap();
    }
}
