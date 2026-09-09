use tauri::{AppHandle, State};

use crate::{
    accelerators, history, models, sources,
    state::{AppState, JobGuard, OperationGuard},
    system, transcription,
    types::{
        ErrorPayload, HistoryPageResult, ModelInfo, SystemStatus, TranscriptRequest,
        TranscriptResult, VideoMetadata,
    },
    user_data,
};

type CommandResult<T> = Result<T, ErrorPayload>;

#[tauri::command]
pub async fn system_status(app: AppHandle) -> CommandResult<SystemStatus> {
    Ok(
        tokio::task::spawn_blocking(move || system::system_status(&app))
            .await
            .map_err(|error| format!("Pemeriksaan sistem gagal dijalankan: {error}"))??,
    )
}

#[tauri::command]
pub async fn list_models(app: AppHandle) -> CommandResult<Vec<ModelInfo>> {
    Ok(
        tokio::task::spawn_blocking(move || models::list_models(&app))
            .await
            .map_err(|error| format!("Pembacaan model gagal dijalankan: {error}"))??,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
    compute_device_id: Option<String>,
) -> CommandResult<()> {
    models::ensure_download_supported(&app, &model_id, compute_device_id.as_deref())?;
    let _operation = OperationGuard::reserve_model_download(&state, model_id.clone())?;
    let result = models::download_model(app, model_id, state.model_cancelled.clone()).await;
    Ok(result?)
}

#[tauri::command]
pub async fn install_cuda_engine(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
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
    Ok(result?)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_accelerator(
    app: AppHandle,
    state: State<'_, AppState>,
    backend: String,
) -> CommandResult<()> {
    let _operation = OperationGuard::reserve_runtime_install(&state, backend.clone())?;
    let result = accelerators::install(app, backend, state.runtime_cancelled.clone()).await;
    Ok(result?)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> CommandResult<()> {
    let operation = OperationGuard::reserve_model_delete(&state, model_id.clone())?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        models::delete_model(&app, &model_id)
    })
    .await
    .map_err(|error| format!("Penghapusan model gagal dijalankan: {error}"))??)
}

#[tauri::command]
pub async fn reset_user_data(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let operation = OperationGuard::reserve_data_reset(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        user_data::reset(&app)
    })
    .await
    .map_err(|error| format!("Reset data gagal dijalankan: {error}"))??)
}

#[tauri::command]
pub fn begin_app_update(state: State<'_, AppState>) -> CommandResult<()> {
    Ok(OperationGuard::reserve_app_update(&state)?)
}

#[tauri::command]
pub fn end_app_update(state: State<'_, AppState>) -> CommandResult<()> {
    Ok(OperationGuard::release_app_update(&state)?)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn inspect_media(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    cookies_path: Option<String>,
) -> CommandResult<VideoMetadata> {
    let _operation = OperationGuard::reserve_inspecting(&state)?;
    Ok(sources::inspect_media(
        app,
        url,
        cookies_path,
        state.active_pid.clone(),
        state.inspect_cancelled.clone(),
    )
    .await?)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn start_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    request: TranscriptRequest,
) -> CommandResult<TranscriptResult> {
    let job_guard = JobGuard::reserve(&state)?;
    let active_pid = state.active_pid.clone();
    let cancelled = state.cancelled.clone();
    let capability_probe = state.capability_probe.clone();
    Ok(tokio::task::spawn_blocking(move || {
        job_guard.mark_running()?;
        transcription::pipeline(app, active_pid, cancelled, capability_probe, request)
    })
    .await
    .map_err(|e| format!("Transcription task gagal: {e}"))??)
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>) -> CommandResult<()> {
    OperationGuard::request_cancel(&state)?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn list_history(
    app: AppHandle,
    state: State<'_, AppState>,
    before_id: Option<i64>,
) -> CommandResult<HistoryPageResult> {
    let operation = OperationGuard::reserve_history_read(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::list_history(&app, before_id)
    })
    .await
    .map_err(|error| format!("Pembacaan history gagal dijalankan: {error}"))??)
}

#[tauri::command]
pub async fn load_history(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<TranscriptResult> {
    let operation = OperationGuard::reserve_history_read(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::load_history(&app, id)
    })
    .await
    .map_err(|error| format!("Pemuatan history gagal dijalankan: {error}"))??)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn delete_history(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> CommandResult<()> {
    let operation = OperationGuard::reserve_history_delete(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::delete_history(&app, &ids)
    })
    .await
    .map_err(|error| format!("Penghapusan history gagal dijalankan: {error}"))??)
}

#[tauri::command]
pub async fn copy_export(
    app: AppHandle,
    state: State<'_, AppState>,
    source: String,
    target: String,
) -> CommandResult<()> {
    let operation = OperationGuard::reserve_history_export(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        history::copy_export(&app, &source, &target)
    })
    .await
    .map_err(|error| format!("Export history gagal dijalankan: {error}"))??)
}

#[tauri::command(rename_all = "camelCase")]
pub fn reveal_audio(app: AppHandle, state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let _operation = OperationGuard::reserve_history_reveal(&state)?;
    Ok(history::reveal_audio(&app, &path)?)
}
