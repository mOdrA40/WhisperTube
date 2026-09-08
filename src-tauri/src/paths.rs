use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

use crate::models::model_spec;

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Tidak bisa menentukan app data directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat app data directory: {e}"))?;
    Ok(dir)
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("models");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder models: {e}"))?;
    Ok(dir)
}

pub fn jobs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("jobs");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder jobs: {e}"))?;
    Ok(dir)
}

pub fn runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let platform = runtime_platform();

    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(platform))
    } else {
        Ok(app
            .path()
            .resource_dir()
            .map_err(|e| format!("Gagal menemukan resource directory: {e}"))?
            .join("runtime")
            .join(platform))
    }
}

fn runtime_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    return "windows";
    #[cfg(target_os = "macos")]
    return "macos";
    #[cfg(target_os = "linux")]
    return "linux";
    #[allow(unreachable_code)]
    "unknown"
}

pub fn user_runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("runtime").join(runtime_platform());
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder runtime user: {e}"))?;
    Ok(dir)
}

fn exe_name(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub fn tool_path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(runtime_dir(app)?.join(exe_name(name)))
}

pub fn is_regular_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

pub fn clear_invalid_runtime_destination(
    destination: &Path,
    executable: &str,
) -> Result<(), String> {
    if !destination.exists() {
        return Ok(());
    }
    if is_regular_file(&destination.join(executable)) {
        return Err(
            "Runtime baru saja dipasang oleh proses lain. Klik Re-check components.".into(),
        );
    }

    let metadata = fs::symlink_metadata(destination)
        .map_err(|error| format!("Gagal memeriksa runtime lama yang tidak valid: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(
            "Runtime lama tidak valid berupa symbolic link dan tidak dapat diganti otomatis."
                .into(),
        );
    }
    if metadata.is_dir() {
        fs::remove_dir_all(destination).map_err(|error| {
            format!("Gagal membersihkan runtime lama yang tidak valid: {error}")
        })?;
    } else {
        fs::remove_file(destination).map_err(|error| {
            format!("Gagal membersihkan runtime lama yang tidak valid: {error}")
        })?;
    }
    Ok(())
}

pub fn engine_path(app: &AppHandle, backend: &str) -> Result<PathBuf, String> {
    let user_path = user_runtime_dir(app)?
        .join(backend)
        .join(exe_name("whisper-cli"));
    if is_regular_file(&user_path) {
        return Ok(user_path);
    }
    Ok(runtime_dir(app)?
        .join(backend)
        .join(exe_name("whisper-cli")))
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let _ = model_spec(id)?;
    Ok(models_dir(app)?.join(format!("ggml-{id}.bin")))
}

#[cfg(test)]
mod tests {
    use super::{clear_invalid_runtime_destination, is_regular_file};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "whispertube-paths-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn invalid_runtime_directory_is_not_treated_as_an_engine_and_is_replaced() {
        let root = temporary_path("invalid-runtime");
        let destination = root.join("vulkan");
        fs::create_dir_all(destination.join("whisper-cli.exe"))
            .expect("test runtime directory should be created");

        assert!(!is_regular_file(&destination.join("whisper-cli.exe")));
        clear_invalid_runtime_destination(&destination, "whisper-cli.exe")
            .expect("invalid runtime directory should be removable");
        assert!(!destination.exists());

        fs::create_dir_all(&destination).expect("test runtime directory should be created");
        fs::write(destination.join("whisper-cli.exe"), [])
            .expect("empty test runtime should be created");
        assert!(!is_regular_file(&destination.join("whisper-cli.exe")));
        clear_invalid_runtime_destination(&destination, "whisper-cli.exe")
            .expect("empty runtime should be removable");
        assert!(!destination.exists());

        fs::remove_dir_all(root).expect("test directory should be removed");
    }
}
