mod accelerators;
mod browsers;
mod commands;
mod history;
mod models;
mod paths;
mod process;
mod resources;
mod sources;
mod state;
mod system;
mod transcription;
mod types;

use std::sync::atomic::Ordering;
use tauri::Manager;

fn stop_active_work(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<state::AppState>() else {
        return;
    };
    state.cancelled.store(true, Ordering::SeqCst);
    state.model_cancelled.store(true, Ordering::SeqCst);
    state.runtime_cancelled.store(true, Ordering::SeqCst);
    let pid = state.active_pid.lock().ok().and_then(|guard| *guard);
    if let Some(pid) = pid {
        process::terminate_process_tree(pid);
    }
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
            if let Err(error) = history::cleanup_job_storage(&handle) {
                eprintln!("WhisperTube startup cleanup warning: {error}");
            }
            paths::models_dir(&handle).map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_status,
            commands::list_browsers,
            commands::list_models,
            commands::download_model,
            commands::install_cuda_engine,
            commands::install_accelerator,
            commands::delete_model,
            commands::inspect_media,
            commands::start_transcription,
            commands::cancel_job,
            commands::list_history,
            commands::load_history,
            commands::delete_history,
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
