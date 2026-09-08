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
pub async fn system_status(app: AppHandle) -> Result<SystemStatus, String> {
    tokio::task::spawn_blocking(move || system::system_status(&app))
        .await
        .map_err(|error| format!("Pemeriksaan sistem gagal dijalankan: {error}"))?
}

#[tauri::command]
pub async fn list_browsers() -> Result<Vec<crate::types::BrowserInfo>, String> {
    tokio::task::spawn_blocking(browsers::discover_browsers)
        .await
        .map_err(|error| format!("Pencarian browser gagal dijalankan: {error}"))
}

#[tauri::command]
pub async fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    tokio::task::spawn_blocking(move || models::list_models(&app))
        .await
        .map_err(|error| format!("Pembacaan model gagal dijalankan: {error}"))?
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
pub async fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    let operation = OperationGuard::reserve_model_delete(&state, model_id.clone())?;
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        models::delete_model(&app, &model_id)
    })
    .await
    .map_err(|error| format!("Penghapusan model gagal dijalankan: {error}"))?
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
pub async fn list_history(
    app: AppHandle,
    state: State<'_, AppState>,
    before_id: Option<i64>,
) -> Result<HistoryPageResult, String> {
    let operation = OperationGuard::reserve_history_read(&state)?;
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::list_history(&app, before_id)
    })
    .await
    .map_err(|error| format!("Pembacaan history gagal dijalankan: {error}"))?
}

#[tauri::command]
pub async fn load_history(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<TranscriptResult, String> {
    let operation = OperationGuard::reserve_history_read(&state)?;
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::load_history(&app, id)
    })
    .await
    .map_err(|error| format!("Pemuatan history gagal dijalankan: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_history(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<(), String> {
    let operation = OperationGuard::reserve_history_delete(&state)?;
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::delete_history(&app, &ids)
    })
    .await
    .map_err(|error| format!("Penghapusan history gagal dijalankan: {error}"))?
}

#[tauri::command]
pub async fn copy_export(
    app: AppHandle,
    state: State<'_, AppState>,
    source: String,
    target: String,
) -> Result<(), String> {
    let operation = OperationGuard::reserve_history_export(&state)?;
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::copy_export(&app, &source, &target)
    })
    .await
    .map_err(|error| format!("Export history gagal dijalankan: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub fn reveal_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let _operation = OperationGuard::reserve_history_reveal(&state)?;
    history::reveal_audio(&app, &path)
}
