use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

use crate::{
    history, models,
    state::AppState,
    system, transcription,
    types::{
        HistoryItem, ModelInfo, SystemStatus, TranscriptRequest, TranscriptResult, VideoMetadata,
    },
    youtube,
};

#[tauri::command]
pub fn system_status(app: AppHandle) -> Result<SystemStatus, String> {
    system::system_status(&app)
}

#[tauri::command]
pub fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    models::list_models(&app)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn download_model(app: AppHandle, model_id: String) -> Result<(), String> {
    models::download_model(app, model_id).await
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
) -> Result<VideoMetadata, String> {
    youtube::inspect_youtube(app, url, browser).await
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
