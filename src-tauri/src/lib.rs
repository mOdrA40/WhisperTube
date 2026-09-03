mod browsers;
mod commands;
mod history;
mod models;
mod paths;
mod state;
mod system;
mod transcription;
mod types;
mod youtube;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            history::init_db(&handle).map_err(std::io::Error::other)?;
            paths::models_dir(&handle).map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_status,
            commands::list_browsers,
            commands::list_models,
            commands::download_model,
            commands::install_cuda_engine,
            commands::delete_model,
            commands::inspect_youtube,
            commands::start_transcription,
            commands::cancel_job,
            commands::list_history,
            commands::load_history,
            commands::copy_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running WhisperTube");
}
