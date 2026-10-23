mod accelerators;
mod archive;
mod checkpoint;
mod commands;
mod cookies;
#[cfg(unix)]
mod export_permissions;
mod history;
mod history_files;
mod io_cancel;
mod job_cache;
mod models;
mod network;
mod paths;
mod process;
mod resources;
mod runtime_snapshot;
mod sources;
mod state;
mod system;
mod transcription;
mod types;
mod user_data;

use std::sync::atomic::Ordering;
use tauri::Manager;

fn stop_active_work(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<state::AppState>() else {
        return;
    };
    state.cancelled.store(true, Ordering::SeqCst);
    state.model_cancelled.store(true, Ordering::SeqCst);
    state.runtime_cancelled.store(true, Ordering::SeqCst);
    state.inspect_cancelled.store(true, Ordering::SeqCst);
    process::shutdown();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state::AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            history::init_db(&handle).map_err(std::io::Error::other)?;
            if let Err(error) = cookies::cleanup_stale(&handle) {
                eprintln!("WhisperTube cookie storage cleanup warning: {error}");
            }
            if let Err(error) = history::cleanup_job_storage(&handle) {
                eprintln!("WhisperTube startup cleanup warning: {error}");
            }
            if let Err(error) = paths::cleanup_stale_transient_storage(&handle) {
                eprintln!("WhisperTube transient storage cleanup warning: {error}");
            }
            paths::models_dir(&handle).map_err(std::io::Error::other)?;
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Some(window) = app.get_webview_window("main") {
                window
                    .set_decorations(false)
                    .map_err(std::io::Error::other)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_status,
            commands::list_models,
            commands::download_model,
            commands::install_cuda_engine,
            commands::install_accelerator,
            commands::delete_model,
            commands::reset_user_data,
            commands::begin_app_update,
            commands::end_app_update,
            commands::inspect_media,
            commands::start_transcription,
            commands::transcription_resume_stage,
            commands::cancel_job,
            commands::list_history,
            commands::load_history,
            commands::delete_history,
            commands::history_files,
            commands::delete_history_files,
            commands::reveal_history_file,
            commands::list_job_cache,
            commands::discard_job_cache,
            commands::copy_export,
            commands::reveal_audio
        ])
        .build(tauri::generate_context!())
        .expect("error while building WhisperTube")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                stop_active_work(app);
            }
        });
}
