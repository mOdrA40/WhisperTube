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
