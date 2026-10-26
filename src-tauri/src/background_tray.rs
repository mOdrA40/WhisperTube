//! Close-to-tray applies only to the accepted audio/transcription job.
//! Keep the WebView alive so its result, progress listeners and job stay intact.
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime, Window,
};

use crate::state::{AppState, OperationState};

const TRAY_ID: &str = "transcription-background";
const OPEN_ID: &str = "transcription-background-open";

fn ensure_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        return tray.set_visible(true).map_err(|error| error.to_string());
    }
    let icon = app
        .default_window_icon()
        .ok_or("Ikon tray WhisperTube tidak tersedia.")?;
    // The brand is language-neutral. Linux tray hosts require a menu to open it.
    let open = MenuItem::with_id(app, OPEN_ID, "WhisperTube", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let menu = Menu::with_items(app, &[&open]).map_err(|error| error.to_string())?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon.clone())
        .tooltip("WhisperTube")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id.as_ref() == OPEN_ID {
                restore_or_log(app);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                restore_or_log(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn enter_background(
    prepare_tray: impl FnOnce() -> Result<(), String>,
    hide_window: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    // Never hide before a recovery path exists. On failure close stays prevented.
    prepare_tray()?;
    hide_window()
}

fn intercept_close(
    operation: Result<OperationState, String>,
    prevent_close: impl FnOnce(),
    background: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match operation {
        Ok(OperationState::Transcribing) => {
            prevent_close();
            background()
        }
        Ok(_) => Ok(()),
        Err(error) => {
            // Unknown state must not terminate a potentially active worker.
            prevent_close();
            Err(error)
        }
    }
}

pub fn on_close<R: Runtime>(window: &Window<R>, api: &tauri::CloseRequestApi) {
    if window.label() != "main" {
        return;
    }
    let state = window.state::<AppState>();
    let operation = state
        .operation
        .lock()
        .map(|operation| operation.clone())
        .map_err(|_| "State operasi tray terkunci.".to_owned());
    if let Err(error) = intercept_close(
        operation,
        || api.prevent_close(),
        || {
            enter_background(
                || ensure_tray(window.app_handle()),
                || window.hide().map_err(|error| error.to_string()),
            )
        },
    ) {
        eprintln!("WhisperTube close-to-tray gagal; jendela tidak ditutup: {error}");
    }
}

/// Also used by single-instance activation to restore a hidden/minimized window.
pub fn restore<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Jendela WhisperTube tidak tersedia.")?;
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    // Keep the tray available if showing/focusing the window failed.
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_visible(false).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn restore_or_log<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = restore(app) {
        eprintln!("WhisperTube gagal membuka jendela dari latar belakang: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::JobGuard;
    use std::{cell::Cell, sync::atomic::Ordering};

    #[test]
    fn active_job_close_hides_without_releasing_or_cancelling_the_worker() {
        let state = AppState::default();
        let guard = JobGuard::reserve(&state).unwrap();
        let prevented = Cell::new(false);
        let tray = Cell::new(false);
        let visible = Cell::new(true);
        intercept_close(
            Ok(state.operation.lock().unwrap().clone()),
            || prevented.set(true),
            || {
                enter_background(
                    || {
                        tray.set(true);
                        Ok(())
                    },
                    || {
                        assert!(tray.get());
                        assert!(prevented.get());
                        visible.set(false);
                        Ok(())
                    },
                )
            },
        )
        .unwrap();
        assert!(!visible.get());
        assert!(tray.get());
        assert!(!state.cancelled.load(Ordering::SeqCst));
        assert!(JobGuard::reserve(&state).is_err());
        guard.mark_running().unwrap();
        drop(guard);
        // Finishing must not close/reopen the hidden window or lose the tray.
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
        assert!(!visible.get());
        assert!(tray.get());
    }

    #[test]
    fn all_other_operations_keep_normal_close_behavior() {
        for operation in [
            OperationState::Idle,
            OperationState::Inspecting,
            OperationState::ModelDownloading("model".into()),
            OperationState::RuntimeInstalling("runtime".into()),
            OperationState::ModelDeleting("model".into()),
            OperationState::HistoryReading,
            OperationState::HistoryDeleting,
            OperationState::HistoryExporting,
            OperationState::HistoryRevealing,
            OperationState::AppUpdating,
            OperationState::ResettingData,
        ] {
            intercept_close(
                Ok(operation),
                || panic!("must allow normal close"),
                || panic!("must not create a tray"),
            )
            .unwrap();
        }
    }

    #[test]
    fn tray_creation_failure_keeps_window_and_job_alive() {
        let prevented = Cell::new(false);
        let result = intercept_close(
            Ok(OperationState::Transcribing),
            || prevented.set(true),
            || {
                enter_background(
                    || Err("tray unavailable".into()),
                    || panic!("must not hide"),
                )
            },
        );
        assert!(prevented.get());
        assert_eq!(result, Err("tray unavailable".into()));
    }

    #[test]
    fn hide_failure_keeps_recovery_tray_and_prevents_destructive_close() {
        let prevented = Cell::new(false);
        let tray = Cell::new(false);
        let result = intercept_close(
            Ok(OperationState::Transcribing),
            || prevented.set(true),
            || {
                enter_background(
                    || {
                        tray.set(true);
                        Ok(())
                    },
                    || Err("hide failed".into()),
                )
            },
        );
        assert!(prevented.get());
        assert!(tray.get());
        assert_eq!(result, Err("hide failed".into()));
    }

    #[test]
    fn unreadable_job_state_keeps_window_open_and_does_not_hide() {
        let prevented = Cell::new(false);
        assert_eq!(
            intercept_close(
                Err("state poisoned".into()),
                || prevented.set(true),
                || panic!("must not hide unknown state")
            ),
            Err("state poisoned".into())
        );
        assert!(prevented.get());
    }
}
