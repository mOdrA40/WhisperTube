use tauri::{AppHandle, State};

use crate::{
    accelerators, history, job_cache, models, sources,
    state::{AppState, JobGuard, OperationGuard},
    system, transcription,
    types::{
        ErrorPayload, HistoryDeleteOutcome, HistoryPageResult, ModelInfo, SystemStatus,
        TranscriptRequest, TranscriptResult, VideoMetadata,
    },
    user_data,
};

type CommandResult<T> = Result<T, ErrorPayload>;

// Keep the reservation inside the blocking worker even if its awaiting IPC
// future is dropped. A cancel during preflight must prevent the transfer.
async fn run_preflight(
    operation: OperationGuard,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    preflight: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<OperationGuard, String> {
    tokio::task::spawn_blocking(move || {
        crate::io_cancel::check(&cancelled)?;
        let result = preflight();
        crate::io_cancel::check(&cancelled)?;
        result?;
        Ok(operation)
    })
    .await
    .map_err(|error| format!("Pemeriksaan awal operasi gagal dijalankan: {error}"))?
}

#[tauri::command]
pub async fn history_files(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<Vec<crate::types::HistoryFileInfo>> {
    let operation = OperationGuard::reserve_history_read(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        crate::history_files::list(&app, id)
    })
    .await
    .map_err(|e| format!("Pembacaan file history gagal: {e}"))??)
}

#[tauri::command]
pub async fn delete_history_files(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    scope: crate::types::HistoryFileScope,
) -> CommandResult<crate::types::HistoryFilesDeleteOutcome> {
    let operation = OperationGuard::reserve_history_delete(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        crate::history_files::delete(&app, &ids, scope)
    })
    .await
    .map_err(|e| format!("Penghapusan file history gagal: {e}"))??)
}

#[tauri::command]
pub fn reveal_history_file(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    kind: crate::types::HistoryFileKind,
) -> CommandResult<()> {
    let _operation = OperationGuard::reserve_history_reveal(&state)?;
    Ok(crate::history_files::reveal(&app, id, kind)?)
}

#[tauri::command]
pub async fn list_job_cache(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Vec<crate::types::JobCacheEntry>> {
    let operation = OperationGuard::reserve_history_read(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        job_cache::list(&app)
    })
    .await
    .map_err(|e| format!("Pembacaan cache job gagal: {e}"))??)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn discard_job_cache(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let operation = OperationGuard::reserve_history_delete(&state)?;
    Ok(tokio::task::spawn_blocking(move || {
        let _operation = operation;
        job_cache::discard(&app, &id)
    })
    .await
    .map_err(|e| format!("Pembersihan cache job gagal: {e}"))??)
}

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
    let operation = OperationGuard::reserve_model_download(&state, model_id.clone())?;
    let preflight_app = app.clone();
    let preflight_model = model_id.clone();
    let _operation = run_preflight(operation, state.model_cancelled.clone(), move || {
        models::ensure_download_supported(
            &preflight_app,
            &preflight_model,
            compute_device_id.as_deref(),
        )
    })
    .await?;
    let result = models::download_model(app, model_id, state.model_cancelled.clone()).await;
    Ok(result?)
}

#[tauri::command]
pub async fn install_cuda_engine(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let operation = OperationGuard::reserve_runtime_install(&state, "cuda".into())?;
    let _operation = run_preflight(
        operation,
        state.runtime_cancelled.clone(),
        system::ensure_cuda_install_supported,
    )
    .await?;
    let result = models::install_cuda_engine(app, state.runtime_cancelled.clone()).await;
    Ok(result?)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_accelerator(
    app: AppHandle,
    state: State<'_, AppState>,
    backend: String,
) -> CommandResult<()> {
    let operation = OperationGuard::reserve_runtime_install(&state, backend.clone())?;
    let preflight_backend = backend.clone();
    let _operation = run_preflight(operation, state.runtime_cancelled.clone(), move || {
        accelerators::ensure_install_memory(&preflight_backend)
    })
    .await?;
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
pub async fn transcription_resume_stage(
    app: AppHandle,
    url: String,
    duration: f64,
) -> CommandResult<Option<crate::types::TranscriptionResumeStage>> {
    Ok(
        tokio::task::spawn_blocking(move || transcription::resume_stage(&app, &url, duration))
            .await
            .map_err(|e| format!("Pemeriksaan progres tersimpan gagal: {e}"))??,
    )
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
) -> CommandResult<HistoryDeleteOutcome> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::Ordering, Arc};

    #[tokio::test]
    async fn cancellation_during_preflight_prevents_transfer_for_every_download_kind() {
        for kind in ["model-cuda", "model-vulkan", "cuda", "vulkan"] {
            let state = AppState::default();
            let model = kind.starts_with("model");
            let operation = if model {
                OperationGuard::reserve_model_download(&state, kind.into()).unwrap()
            } else {
                OperationGuard::reserve_runtime_install(&state, kind.into()).unwrap()
            };
            let cancelled = if model {
                state.model_cancelled.clone()
            } else {
                state.runtime_cancelled.clone()
            };
            let (entered, waiting) = tokio::sync::oneshot::channel();
            let (release, resume) = std::sync::mpsc::channel();
            let worker = tokio::spawn(run_preflight(operation, cancelled.clone(), move || {
                entered.send(()).unwrap();
                resume.recv().unwrap();
                Ok(())
            }));
            waiting.await.unwrap();
            assert!(OperationGuard::reserve_model_delete(&state, "base".into()).is_err());
            OperationGuard::request_cancel(&state).unwrap();
            release.send(()).unwrap();
            let result = worker.await.unwrap();
            assert!(result.err().unwrap().contains("dibatalkan"), "{kind}");
            assert!(cancelled.load(Ordering::SeqCst));
            assert_eq!(
                *state.operation.lock().unwrap(),
                crate::state::OperationState::Idle
            );
        }
    }

    #[tokio::test]
    async fn preflight_releases_on_failure_but_holds_reservation_on_success() {
        let state = AppState::default();
        let operation = OperationGuard::reserve_model_download(&state, "base".into()).unwrap();
        let error = run_preflight(operation, state.model_cancelled.clone(), || {
            Err("unsupported".into())
        })
        .await;
        assert_eq!(error.err().unwrap(), "unsupported");
        let operation = OperationGuard::reserve_runtime_install(&state, "cuda".into()).unwrap();
        let guard = run_preflight(operation, state.runtime_cancelled.clone(), || Ok(()))
            .await
            .unwrap();
        assert!(OperationGuard::reserve_history_read(&state).is_err());
        drop(guard);
        assert!(OperationGuard::reserve_history_read(&state).is_ok());
    }

    #[tokio::test]
    async fn cancelled_before_worker_start_never_runs_preflight() {
        let state = AppState::default();
        let operation = OperationGuard::reserve_model_download(&state, "base".into()).unwrap();
        OperationGuard::request_cancel(&state).unwrap();
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = called.clone();
        assert!(
            run_preflight(operation, state.model_cancelled.clone(), move || {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
            .is_err()
        );
        assert!(!called.load(Ordering::SeqCst));
        assert!(OperationGuard::reserve_history_read(&state).is_ok());
    }

    #[tokio::test]
    async fn dropped_ipc_future_does_not_release_a_running_preflight() {
        let state = AppState::default();
        let operation = OperationGuard::reserve_runtime_install(&state, "cuda".into()).unwrap();
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let worker = tokio::spawn(run_preflight(
            operation,
            state.runtime_cancelled.clone(),
            move || {
                entered.send(()).unwrap();
                resume.recv().unwrap();
                Ok(())
            },
        ));
        waiting.await.unwrap();
        worker.abort();
        assert!(worker.await.err().unwrap().is_cancelled());
        assert!(OperationGuard::reserve_history_read(&state).is_err());
        release.send(()).unwrap();
        // Boundedly wait for the detached worker to release its reservation.
        let operation_state = state.operation.clone();
        tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while *operation_state.lock().unwrap() != crate::state::OperationState::Idle {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
        })
        .await
        .unwrap();
        assert!(OperationGuard::reserve_history_read(&state).is_ok());
    }
}
