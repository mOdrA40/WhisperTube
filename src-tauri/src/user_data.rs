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
    for path in ["models", "jobs", "runtime"] {
        if let Err(error) = remove_dir_if_present(&root.join(path)) {
            errors.push(error);
        }
    }
    if let Err(error) = remove_file_if_present(&root.join("whispertube.db")) {
        errors.push(error);
    }
    errors
}

pub fn reset(app: &AppHandle) -> Result<(), String> {
    let root = paths::app_data_dir(app)?;
    let mut errors = cleanup_targets(&root);

    // Rebuild the baseline even when cleanup was only partially successful. This
    // keeps history queries and future downloads usable after a locked/denied path.
    if let Err(error) = history::init_db(app) {
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
