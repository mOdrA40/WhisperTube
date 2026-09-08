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

pub fn reset(app: &AppHandle) -> Result<(), String> {
    let root = paths::app_data_dir(app)?;

    let mut errors = Vec::new();
    for path in ["models", "jobs", "runtime"] {
        if let Err(error) = remove_dir_if_present(&root.join(path)) {
            errors.push(error);
        }
    }
    if let Err(error) = remove_file_if_present(&root.join("whispertube.db")) {
        errors.push(error);
    }

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

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Reset data tidak selesai sepenuhnya: {}",
            errors.join("; ")
        ))
    }
}
