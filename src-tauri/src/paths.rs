use std::{fs, path::PathBuf};
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
    #[cfg(target_os = "windows")]
    let platform = "windows";
    #[cfg(target_os = "macos")]
    let platform = "macos";
    #[cfg(target_os = "linux")]
    let platform = "linux";

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

pub fn engine_path(app: &AppHandle, backend: &str) -> Result<PathBuf, String> {
    Ok(runtime_dir(app)?
        .join(backend)
        .join(exe_name("whisper-cli")))
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let _ = model_spec(id)?;
    Ok(models_dir(app)?.join(format!("ggml-{id}.bin")))
}
