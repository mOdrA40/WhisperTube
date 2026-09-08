use tauri::{AppHandle, State};

use crate::{
    accelerators, browsers, history, models, sources,
    state::{AppState, JobGuard, OperationGuard},
    system, transcription,
    types::{
        HistoryPageResult, ModelInfo, SystemStatus, TranscriptRequest, TranscriptResult,
        VideoMetadata,
    },
    user_data,
};

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
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
    compute_device_id: Option<String>,
) -> Result<(), String> {
    models::ensure_download_supported(&app, &model_id, compute_device_id.as_deref())?;
    let _operation = OperationGuard::reserve_model_download(&state, model_id.clone())?;
    let result = models::download_model(app, model_id, state.model_cancelled.clone()).await;
    result
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
    let _operation = OperationGuard::reserve_runtime_install(&state, "cuda".into())?;
    let result = models::install_cuda_engine(app, state.runtime_cancelled.clone()).await;
    result
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_accelerator(
    app: AppHandle,
    state: State<'_, AppState>,
    backend: String,
) -> Result<(), String> {
    let _operation = OperationGuard::reserve_runtime_install(&state, backend.clone())?;
    let result = accelerators::install(app, backend, state.runtime_cancelled.clone()).await;
    result
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    let _operation = OperationGuard::reserve_model_delete(&state, model_id.clone())?;
    models::delete_model(&app, &model_id)
}

#[tauri::command]
pub async fn reset_user_data(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let operation = OperationGuard::reserve_data_reset(&state)?;
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        user_data::reset(&app)
    })
    .await
    .map_err(|error| format!("Reset data gagal dijalankan: {error}"))?
}

#[tauri::command]
pub fn begin_app_update(state: State<'_, AppState>) -> Result<(), String> {
    OperationGuard::reserve_app_update(&state)
}

#[tauri::command]
pub fn end_app_update(state: State<'_, AppState>) -> Result<(), String> {
    OperationGuard::release_app_update(&state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn inspect_media(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    browser: String,
    profile: Option<String>,
    cookies_path: Option<String>,
) -> Result<VideoMetadata, String> {
    let _operation = OperationGuard::reserve_inspecting(&state)?;
    sources::inspect_media(
        app,
        url,
        browser,
        profile,
        cookies_path,
        state.active_pid.clone(),
        state.inspect_cancelled.clone(),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn start_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TranscriptRequest,
) -> Result<TranscriptResult, String> {
    let job_guard = JobGuard::reserve(&state)?;
    let active_pid = state.active_pid.clone();
    let cancelled = state.cancelled.clone();
    let vulkan_probe = state.vulkan_probe.clone();
    tokio::task::spawn_blocking(move || {
        job_guard.mark_running()?;
        transcription::pipeline(app, active_pid, cancelled, vulkan_probe, request)
    })
    .await
    .map_err(|e| format!("Transcription task gagal: {e}"))?
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>) -> Result<(), String> {
    OperationGuard::request_cancel(&state)?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn list_history(app: AppHandle, before_id: Option<i64>) -> Result<HistoryPageResult, String> {
    history::list_history(&app, before_id)
}

#[tauri::command]
pub fn load_history(app: AppHandle, id: i64) -> Result<TranscriptResult, String> {
    history::load_history(&app, id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_history(app: AppHandle, ids: Vec<i64>) -> Result<(), String> {
    history::delete_history(&app, &ids)
}

#[tauri::command]
pub fn copy_export(app: AppHandle, source: String, target: String) -> Result<(), String> {
    history::copy_export(&app, &source, &target)
}

#[tauri::command(rename_all = "camelCase")]
pub fn reveal_audio(app: AppHandle, path: String) -> Result<(), String> {
    history::reveal_audio(&app, &path)
}
