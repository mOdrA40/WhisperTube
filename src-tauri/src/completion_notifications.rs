use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;
#[cfg(windows)]
mod windows;

#[derive(Clone, Deserialize)]
pub struct NotificationCopy {
    title: String,
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionNotifications {
    success: NotificationCopy,
    completed_with_note: NotificationCopy,
    failed: NotificationCopy,
    cancelled: NotificationCopy,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success,
    CompletedWithNote,
    Failed,
    Cancelled,
}

impl TranscriptionNotifications {
    fn for_outcome(&self, status: &Outcome) -> &NotificationCopy {
        match status {
            Outcome::Success => &self.success,
            Outcome::CompletedWithNote => &self.completed_with_note,
            Outcome::Failed => &self.failed,
            Outcome::Cancelled => &self.cancelled,
        }
    }
}

fn outcome<T>(result: &Result<T, String>, cancellation_requested: bool, has_note: bool) -> Outcome {
    match result {
        Ok(_) if has_note => Outcome::CompletedWithNote,
        Ok(_) => Outcome::Success,
        // A late cancel must not disguise an unrelated error as cancellation.
        Err(error) if cancellation_requested && crate::io_cancel::is_cancellation_error(error) => {
            Outcome::Cancelled
        }
        Err(_) => Outcome::Failed,
    }
}

/// Notification delivery is best effort and must never change the job outcome.
pub fn finish<R: Runtime>(
    app: &AppHandle<R>,
    result: Result<crate::types::TranscriptResult, String>,
    cancellation_requested: bool,
    notifications: Option<&TranscriptionNotifications>,
    legacy_message: Option<&str>,
) -> Result<crate::types::TranscriptResult, String> {
    let status = outcome(
        &result,
        cancellation_requested,
        result
            .as_ref()
            .is_ok_and(|result| result.source_duration.is_some()),
    );
    let legacy = legacy_message.map(|body| NotificationCopy {
        title: "WhisperTube".into(),
        body: body.into(),
    });
    let copy = match notifications {
        Some(copy) => Some(copy.for_outcome(&status)),
        None if matches!(status, Outcome::Success | Outcome::CompletedWithNote) => legacy.as_ref(),
        None => None,
    };
    complete(
        result,
        copy,
        || {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "main window is no longer available".to_owned())?;
            focused_when_visible(
                window.is_visible().map_err(|error| error.to_string()),
                || window.is_focused().map_err(|error| error.to_string()),
            )
        },
        |title, body| {
            #[cfg(windows)]
            {
                windows::send(app, title, body, status == Outcome::Cancelled)
            }
            #[cfg(not(windows))]
            {
                app.notification()
                    .builder()
                    .title(title)
                    .body(body)
                    .show()
                    .map_err(|error| error.to_string())
            }
        },
    )
}

fn focused_when_visible(
    visible: Result<bool, String>,
    focused: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    // A tray-hidden window is background even if a focus event is pending.
    if visible? {
        focused()
    } else {
        Ok(false)
    }
}

fn complete<T, E>(
    result: Result<T, E>,
    copy: Option<&NotificationCopy>,
    focused: impl FnOnce() -> Result<bool, String>,
    send: impl FnOnce(&str, &str) -> Result<(), String>,
) -> Result<T, E> {
    let Some(copy) = copy else { return result };
    let title = copy.title.trim();
    let body = copy.body.trim();
    if [(title, 80), (body, 240)].iter().any(|(text, limit)| {
        text.is_empty() || text.chars().count() > *limit || text.chars().any(char::is_control)
    }) {
        eprintln!("WhisperTube notification skipped: invalid copy");
        return result;
    }
    match focused() {
        Ok(false) => {
            if let Err(error) = send(title, body) {
                eprintln!("WhisperTube notification failed: {error}");
            }
        }
        Ok(true) => {}
        Err(error) => eprintln!("WhisperTube notification focus check failed: {error}"),
    }
    result
}

/// Preserve the job reservation during unwinding and report a genuine failure.
/// The normal panic hook still records diagnostics for debugging.
pub fn run_worker<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err("Transcription task gagal: worker panic.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn tray_hidden_window_notifies_even_without_a_focus_update() {
        let sent = Cell::new(false);
        let result = complete(
            Ok::<_, String>(42),
            Some(&copy()),
            || focused_when_visible(Ok(false), || panic!("hidden focus may be stale")),
            |_, _| {
                sent.set(true);
                Ok(())
            },
        );
        assert_eq!(result, Ok(42));
        assert!(sent.get());
        for focused in [false, true] {
            assert_eq!(focused_when_visible(Ok(true), || Ok(focused)), Ok(focused));
        }
        assert_eq!(
            focused_when_visible(Err("window unavailable".into()), || panic!(
                "must not query failed window"
            )),
            Err("window unavailable".into())
        );
    }

    fn copy() -> NotificationCopy {
        NotificationCopy {
            title: " Status ".into(),
            body: " Open app. ".into(),
        }
    }

    #[test]
    fn ipc_packet_selects_the_matching_terminal_copy() {
        let packet: TranscriptionNotifications = serde_json::from_value(serde_json::json!({
            "success": { "title": "Done", "body": "Ready" },
            "completedWithNote": { "title": "Note", "body": "Check coverage" },
            "failed": { "title": "Failed", "body": "Open app" },
            "cancelled": { "title": "Cancelled", "body": "Stopped" }
        }))
        .unwrap();
        for (status, title, body) in [
            (Outcome::Success, "Done", "Ready"),
            (Outcome::CompletedWithNote, "Note", "Check coverage"),
            (Outcome::Failed, "Failed", "Open app"),
            (Outcome::Cancelled, "Cancelled", "Stopped"),
        ] {
            let copy = packet.for_outcome(&status);
            assert_eq!((copy.title.as_str(), copy.body.as_str()), (title, body));
        }
    }

    #[test]
    fn cancelled_preflight_and_worker_panic_keep_reservation_until_finalization() {
        use crate::state::{AppState, JobGuard, OperationGuard, OperationState};
        let state = AppState::default();
        for cancelled in [false, true] {
            let guard = JobGuard::reserve(&state).unwrap();
            if cancelled {
                OperationGuard::request_cancel(&state).unwrap();
            }
            let result = run_worker::<()>(|| {
                guard.mark_running()?;
                panic!("worker failure");
            });
            assert_eq!(
                outcome(&result, cancelled, false),
                if cancelled {
                    Outcome::Cancelled
                } else {
                    Outcome::Failed
                }
            );
            assert!(JobGuard::reserve(&state).is_err());
            assert_eq!(
                complete(
                    result.clone(),
                    Some(&copy()),
                    || Ok(false),
                    |_, _| {
                        assert!(JobGuard::reserve(&state).is_err());
                        Ok(())
                    }
                ),
                result
            );
            drop(guard);
            assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
        }
    }

    #[tokio::test]
    async fn dropped_awaiting_caller_keeps_background_worker_reserved() {
        use crate::state::{AppState, JobGuard, OperationState};
        use std::sync::{mpsc, Arc};
        let state = Arc::new(AppState::default());
        let guard = JobGuard::reserve(&state).unwrap();
        let (release, wait) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let worker_state = state.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let result = run_worker(|| {
                guard.mark_running()?;
                wait.recv().unwrap();
                Ok(())
            });
            complete(
                result,
                Some(&copy()),
                || Ok(false),
                |_, _| {
                    assert!(JobGuard::reserve(&worker_state).is_err());
                    Ok(())
                },
            )
            .unwrap();
            drop(guard);
            finished.send(()).unwrap();
        });
        drop(worker);
        assert!(JobGuard::reserve(&state).is_err());
        release.send(()).unwrap();
        done.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
    }

    #[test]
    fn outcomes_distinguish_success_notes_failures_and_late_cancellation() {
        assert_eq!(outcome(&Ok::<_, String>(()), true, false), Outcome::Success);
        assert_eq!(
            outcome(&Ok::<_, String>(()), false, true),
            Outcome::CompletedWithNote
        );
        for error in [
            "Job dibatalkan.",
            "Transkripsi dibatalkan.",
            "Proses dibatalkan.",
        ] {
            assert_eq!(
                outcome(&Err::<(), _>(error.into()), true, false),
                Outcome::Cancelled
            );
            assert_eq!(
                outcome(&Err::<(), _>(error.into()), false, false),
                Outcome::Failed
            );
        }
        assert_eq!(
            outcome(&Err::<(), _>("disk full".into()), true, false),
            Outcome::Failed
        );
        assert_eq!(
            outcome(&run_worker::<()>(|| panic!("test panic")), true, false),
            Outcome::Failed
        );
    }

    #[test]
    fn every_terminal_result_notifies_once_in_background_and_is_preserved() {
        for original in [Ok(42), Err("failed"), Err("Job dibatalkan.")] {
            let count = Cell::new(0);
            let result = complete(
                original,
                Some(&copy()),
                || Ok(false),
                |title, body| {
                    assert_eq!((title, body), ("Status", "Open app."));
                    count.set(count.get() + 1);
                    Ok(())
                },
            );
            assert_eq!(result, original);
            assert_eq!(count.get(), 1);
        }
    }

    #[test]
    fn foreground_and_unknown_focus_never_send() {
        for focus in [Ok(true), Err("window gone".into())] {
            assert_eq!(
                complete(
                    Err::<(), _>("failed"),
                    Some(&copy()),
                    || focus,
                    |_, _| panic!("must not send")
                ),
                Err("failed")
            );
        }
    }

    #[test]
    fn missing_or_invalid_copy_does_not_query_focus() {
        assert_eq!(
            complete(
                Ok::<_, ()>(42),
                None,
                || panic!("missing copy"),
                |_, _| panic!("must not send")
            ),
            Ok(42)
        );
        for (title, body) in [
            (" ".into(), "ok".into()),
            ("ok".into(), "bad\0copy".into()),
            ("x".repeat(81), "ok".into()),
            ("ok".into(), "x".repeat(241)),
        ] {
            assert_eq!(
                complete(
                    Ok::<_, ()>(42),
                    Some(&NotificationCopy { title, body }),
                    || panic!("invalid copy"),
                    |_, _| panic!("must not send")
                ),
                Ok(42)
            );
        }
    }

    #[test]
    fn notification_failure_preserves_both_success_and_original_error() {
        for original in [Ok(42), Err("original error")] {
            assert_eq!(
                complete(
                    original,
                    Some(&copy()),
                    || Ok(false),
                    |_, _| Err("OS unavailable".into())
                ),
                original
            );
        }
        assert_eq!(run_worker(|| Ok(42)), Ok(42));
        assert_eq!(
            run_worker::<()>(|| Err("original error".into())),
            Err("original error".into())
        );
    }
}
