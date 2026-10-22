use std::{fs, path::Path};

use tauri::AppHandle;

use crate::{history, models, paths};

fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Gagal menghapus {}: {error}", path.display())),
    }
}

fn remove_dir_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Gagal menghapus {}: {error}", path.display())),
    }
}

fn cleanup_targets(root: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    for path in ["models", "jobs", "runtime", "cookie-jars"] {
        if let Err(error) = remove_dir_if_present(&root.join(path)) {
            errors.push(error);
        }
    }
    // Do not remove recovery files when the database itself could not be
    // removed (for example another handle still owns it).
    if let Err(error) = remove_file_if_present(&root.join("whispertube.db")) {
        errors.push(error);
    } else {
        for name in [
            "whispertube.db-journal",
            "whispertube.db-wal",
            "whispertube.db-shm",
        ] {
            if let Err(error) = remove_file_if_present(&root.join(name)) {
                errors.push(error);
            }
        }
    }
    errors
}

fn ensure_recovery_files_removed(root: &Path) -> Result<(), String> {
    for name in [
        "whispertube.db-journal",
        "whispertube.db-wal",
        "whispertube.db-shm",
    ] {
        match fs::symlink_metadata(root.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Gagal memeriksa file recovery {name}: {error}")),
            Ok(_) => return Err(format!("Database belum dibuat ulang karena {name} belum terhapus. Tutup aplikasi lain lalu ulangi reset.")),
        }
    }
    Ok(())
}

pub fn reset(app: &AppHandle) -> Result<(), String> {
    let root = paths::app_data_dir(app)?;
    let mut errors = cleanup_targets(&root);

    // Rebuild the baseline even when cleanup was only partially successful. This
    // keeps history queries and future downloads usable after a locked/denied path.
    // Reopening a new database with an undeleted old journal could recover
    // records the user just asked us to remove. Leave it closed until retry.
    if let Err(error) =
        ensure_recovery_files_removed(&root).and_then(|()| history::init_db(app).map(|_| ()))
    {
        errors.push(error);
    }
    if let Err(error) = paths::models_dir(app) {
        errors.push(error);
    }
    if let Err(error) = paths::jobs_dir(app) {
        errors.push(error);
    }
    models::clear_verified_models();
    history::invalidate_job_storage_cache();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Reset data tidak selesai sepenuhnya: {}",
            errors.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_targets;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn cleanup_continues_after_one_target_fails() {
        let root = std::env::temp_dir().join(format!("whispertube-reset-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("jobs")).unwrap();
        fs::write(root.join("models"), b"not a directory").unwrap();
        fs::write(root.join("jobs").join("audio.wav"), b"audio").unwrap();

        let errors = cleanup_targets(&root);

        assert_eq!(errors.len(), 1);
        assert!(root.join("models").exists());
        assert!(!root.join("jobs").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_removes_owned_database_sidecars_and_preserves_external_files() {
        let root = std::env::temp_dir().join(format!("whispertube-reset-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("cookie-jars/jar")).unwrap();
        fs::write(root.join("cookie-jars/jar/cookies.txt"), b"internal copy").unwrap();
        for name in [
            "whispertube.db",
            "whispertube.db-journal",
            "whispertube.db-wal",
            "whispertube.db-shm",
        ] {
            fs::write(root.join(name), b"database fixture").unwrap();
        }
        fs::write(root.join("cookies.txt"), b"original export").unwrap();
        fs::create_dir(root.join("WebView")).unwrap();
        assert!(cleanup_targets(&root).is_empty());
        assert!(cleanup_targets(&root).is_empty());
        for name in [
            "cookie-jars",
            "whispertube.db",
            "whispertube.db-journal",
            "whispertube.db-wal",
            "whispertube.db-shm",
        ] {
            assert!(!root.join(name).exists(), "{name}");
        }
        assert_eq!(
            fs::read(root.join("cookies.txt")).unwrap(),
            b"original export"
        );
        assert!(root.join("WebView").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_database_removal_preserves_recovery_files() {
        let root = std::env::temp_dir().join(format!("whispertube-reset-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("whispertube.db")).unwrap();
        fs::write(root.join("whispertube.db-journal"), b"recovery").unwrap();
        assert!(!cleanup_targets(&root).is_empty());
        assert_eq!(
            fs::read(root.join("whispertube.db-journal")).unwrap(),
            b"recovery"
        );
        assert!(super::ensure_recovery_files_removed(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_removes_symlink_without_following_target() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("whispertube-symlink-test-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("whispertube-symlink-target-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        symlink(&outside, root.join("models")).unwrap();

        let errors = cleanup_targets(&root);

        assert!(errors.is_empty());
        assert!(!root.join("models").exists());
        assert!(outside.join("keep.txt").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
