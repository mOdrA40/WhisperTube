use std::process::{Command, Stdio};
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, State};

use crate::{
    accelerators, browsers, history, models,
    state::AppState,
    system, transcription,
    types::{
        HistoryItem, ModelInfo, SystemStatus, TranscriptRequest, TranscriptResult, VideoMetadata,
    },
    youtube,
};

fn begin_runtime_install(
    state: &AppState,
    operation: &str,
) -> Result<Arc<std::sync::atomic::AtomicBool>, String> {
    let mut active = state
        .runtime_installing
        .lock()
        .map_err(|_| "State runtime terkunci")?;
    if let Some(current) = active.as_deref() {
        return Err(format!("Installer runtime {current} sedang berjalan."));
    }
    *active = Some(operation.to_string());
    state.runtime_cancelled.store(false, Ordering::SeqCst);
    Ok(state.runtime_cancelled.clone())
}

fn end_runtime_install(state: &AppState, operation: &str) {
    if let Ok(mut active) = state.runtime_installing.lock() {
        if active.as_deref() == Some(operation) {
            *active = None;
        }
    }
}

#[tauri::command]
pub fn system_status(app: AppHandle) -> Result<SystemStatus, String> {
    system::system_status(&app)
}

#[tauri::command]
pub fn list_browsers() -> Vec<crate::types::BrowserInfo> {
    browsers::discover_browsers()
}

#[tauri::command]
pub fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    models::list_models(&app)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn download_model(app: AppHandle, model_id: String) -> Result<(), String> {
    models::download_model(app, model_id).await
}

#[tauri::command]
pub async fn install_cuda_engine(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err(
            "CUDA engine otomatis saat ini hanya tersedia pada build Windows yang didukung.".into(),
        );
    }
    if system::detect_nvidia().is_none() {
        return Err("NVIDIA GPU/driver tidak terdeteksi. CUDA engine tidak perlu dipasang.".into());
    }
    let operation = "cuda";
    let cancelled = begin_runtime_install(&state, operation)?;
    let result = models::install_cuda_engine(app, cancelled).await;
    end_runtime_install(&state, operation);
    result
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_accelerator(
    app: AppHandle,
    state: State<'_, AppState>,
    backend: String,
) -> Result<(), String> {
    let operation = backend.clone();
    let cancelled = begin_runtime_install(&state, &operation)?;
    let result = accelerators::install(app, backend, cancelled).await;
    end_runtime_install(&state, &operation);
    result
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_model(app: AppHandle, model_id: String) -> Result<(), String> {
    models::delete_model(&app, &model_id)
}

#[tauri::command]
pub async fn inspect_youtube(
    app: AppHandle,
    url: String,
    browser: String,
    profile: Option<String>,
) -> Result<VideoMetadata, String> {
    youtube::inspect_youtube(app, url, browser, profile).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn start_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TranscriptRequest,
) -> Result<TranscriptResult, String> {
    let active_pid = state.active_pid.clone();
    let cancelled = state.cancelled.clone();
    if active_pid
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
    {
        return Err("Masih ada job yang sedang berjalan.".into());
    }
    tokio::task::spawn_blocking(move || {
        transcription::pipeline(app, active_pid, cancelled, request)
    })
    .await
    .map_err(|e| format!("Transcription task gagal: {e}"))?
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>) -> Result<(), String> {
    state.cancelled.store(true, Ordering::SeqCst);
    state.runtime_cancelled.store(true, Ordering::SeqCst);
    let guard = state
        .active_pid
        .lock()
        .map_err(|_| "State process terkunci")?;
    let pid = *guard;
    drop(guard);
    if let Some(pid) = pid {
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn list_history(app: AppHandle) -> Result<Vec<HistoryItem>, String> {
    history::list_history(&app)
}

#[tauri::command]
pub fn load_history(app: AppHandle, id: i64) -> Result<TranscriptResult, String> {
    history::load_history(&app, id)
}

#[tauri::command]
pub fn copy_export(app: AppHandle, source: String, target: String) -> Result<(), String> {
    history::copy_export(&app, &source, &target)
}
