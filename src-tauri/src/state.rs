use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum OperationState {
    #[default]
    Idle,
    Transcribing,
    Finalizing,
    ModelDownloading(String),
    RuntimeInstalling(String),
    ModelDeleting(String),
    Inspecting,
    HistoryReading,
    HistoryDeleting,
    HistoryExporting,
    HistoryRevealing,
    AppUpdating,
    ResettingData,
}

pub struct OperationGuard {
    state: Arc<Mutex<OperationState>>,
    operation: OperationState,
}

impl OperationGuard {
    fn reserve(
        state: &AppState,
        operation: OperationState,
        cancellation: Option<&AtomicBool>,
    ) -> Result<Self, String> {
        let mut active = state
            .operation
            .lock()
            .map_err(|_| "State operasi terkunci")?;
        if *active != OperationState::Idle {
            return Err(operation_conflict_message(&active));
        }
        *active = operation.clone();
        if let Some(cancellation) = cancellation {
            cancellation.store(false, Ordering::SeqCst);
        }
        Ok(Self {
            state: Arc::clone(&state.operation),
            operation,
        })
    }

    pub fn reserve_model_download(state: &AppState, model_id: String) -> Result<Self, String> {
        Self::reserve(
            state,
            OperationState::ModelDownloading(model_id),
            Some(&state.model_cancelled),
        )
    }

    pub fn reserve_runtime_install(state: &AppState, operation: String) -> Result<Self, String> {
        Self::reserve(
            state,
            OperationState::RuntimeInstalling(operation),
            Some(&state.runtime_cancelled),
        )
    }

    pub fn reserve_model_delete(state: &AppState, model_id: String) -> Result<Self, String> {
        Self::reserve(state, OperationState::ModelDeleting(model_id), None)
    }

    pub fn reserve_inspecting(state: &AppState) -> Result<Self, String> {
        Self::reserve(
            state,
            OperationState::Inspecting,
            Some(&state.inspect_cancelled),
        )
    }

    pub fn reserve_history_read(state: &AppState) -> Result<Self, String> {
        Self::reserve(state, OperationState::HistoryReading, None)
    }

    pub fn reserve_history_delete(state: &AppState) -> Result<Self, String> {
        Self::reserve(state, OperationState::HistoryDeleting, None)
    }

    pub fn reserve_history_export(state: &AppState) -> Result<Self, String> {
        Self::reserve(state, OperationState::HistoryExporting, None)
    }

    pub fn reserve_history_reveal(state: &AppState) -> Result<Self, String> {
        Self::reserve(state, OperationState::HistoryRevealing, None)
    }

    pub fn reserve_app_update(state: &AppState) -> Result<(), String> {
        let mut active = state
            .operation
            .lock()
            .map_err(|_| "State operasi terkunci")?;
        if *active != OperationState::Idle {
            return Err(operation_conflict_message(&active));
        }
        *active = OperationState::AppUpdating;
        Ok(())
    }

    pub fn reserve_data_reset(state: &AppState) -> Result<Self, String> {
        Self::reserve(state, OperationState::ResettingData, None)
            .map_err(|error| format!("operation_conflict:{error}"))
    }

    pub fn request_cancel(state: &AppState) -> Result<(), String> {
        let active = state
            .operation
            .lock()
            .map_err(|_| "State operasi terkunci")?;
        let should_cancel = match &*active {
            OperationState::Finalizing => {
                return Err(
                    "job_finalizing:Hasil sedang disimpan dan tidak dapat dibatalkan.".into(),
                );
            }
            OperationState::Transcribing => {
                state.cancelled.store(true, Ordering::SeqCst);
                true
            }
            OperationState::ModelDownloading(_) => {
                state.model_cancelled.store(true, Ordering::SeqCst);
                true
            }
            OperationState::RuntimeInstalling(_) => {
                state.runtime_cancelled.store(true, Ordering::SeqCst);
                true
            }
            OperationState::Inspecting => {
                state.inspect_cancelled.store(true, Ordering::SeqCst);
                true
            }
            OperationState::Idle
            | OperationState::ModelDeleting(_)
            | OperationState::HistoryReading
            | OperationState::HistoryDeleting
            | OperationState::HistoryExporting
            | OperationState::HistoryRevealing
            | OperationState::AppUpdating
            | OperationState::ResettingData => false,
        };
        if should_cancel {
            let pid = state
                .active_pid
                .lock()
                .map_err(|_| "State process terkunci")?
                .as_ref()
                .copied();
            if let Some(pid) = pid {
                crate::process::terminate_registered(pid);
            }
        }
        Ok(())
    }

    pub fn release_app_update(state: &AppState) -> Result<(), String> {
        let mut active = state
            .operation
            .lock()
            .map_err(|_| "State operasi terkunci")?;
        if *active == OperationState::AppUpdating {
            *active = OperationState::Idle;
        }
        Ok(())
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.lock() {
            if *active == self.operation {
                *active = OperationState::Idle;
            }
        }
    }
}

fn operation_conflict_message(operation: &OperationState) -> String {
    match operation {
        OperationState::Idle => "Tidak ada operasi aktif.".into(),
        OperationState::Transcribing => {
            "Transkripsi sedang berjalan. Tunggu sampai selesai atau batalkan terlebih dahulu."
                .into()
        }
        OperationState::Finalizing => {
            "Hasil sedang disimpan. Tunggu sampai selesai terlebih dahulu.".into()
        }
        OperationState::ModelDownloading(model_id) => {
            format!("Download model {model_id} sedang berjalan.")
        }
        OperationState::RuntimeInstalling(operation) => {
            format!("Installer runtime {operation} sedang berjalan.")
        }
        OperationState::ModelDeleting(model_id) => {
            format!("Model {model_id} sedang dihapus.")
        }
        OperationState::Inspecting => "Pemeriksaan metadata sedang berjalan.".into(),
        OperationState::HistoryReading => {
            "History sedang dibaca. Tunggu sampai selesai terlebih dahulu.".into()
        }
        OperationState::HistoryDeleting => {
            "History sedang dihapus. Tunggu sampai selesai terlebih dahulu.".into()
        }
        OperationState::HistoryExporting => {
            "Export transcript sedang berjalan. Tunggu sampai selesai terlebih dahulu.".into()
        }
        OperationState::HistoryRevealing => {
            "Lokasi audio sedang dibuka. Tunggu sampai selesai terlebih dahulu.".into()
        }
        OperationState::AppUpdating => {
            "Pembaruan aplikasi sedang berjalan. Tunggu sampai selesai terlebih dahulu.".into()
        }
        OperationState::ResettingData => {
            "Penghapusan data aplikasi sedang berjalan. Tunggu sampai selesai terlebih dahulu."
                .into()
        }
    }
}

pub struct JobGuard {
    operation: OperationGuard,
    cancelled: Arc<AtomicBool>,
}

impl JobGuard {
    pub fn reserve(app_state: &AppState) -> Result<Self, String> {
        let operation = OperationGuard::reserve(
            app_state,
            OperationState::Transcribing,
            Some(&app_state.cancelled),
        )?;
        Ok(Self {
            operation,
            cancelled: app_state.cancelled.clone(),
        })
    }

    pub fn mark_running(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err("Job dibatalkan.".into())
        } else {
            Ok(())
        }
    }

    /// Serialize the last cancellation check with request_cancel. Once this
    /// boundary wins, persistence must finish without accepting a late cancel.
    pub fn begin_finalization(&mut self) -> Result<(), String> {
        let mut active = self
            .operation
            .state
            .lock()
            .map_err(|_| "State operasi terkunci")?;
        if self.cancelled.load(Ordering::SeqCst) {
            return Err("Job dibatalkan.".into());
        }
        if *active != OperationState::Transcribing {
            return Err("Job tidak berada dalam tahap transkripsi.".into());
        }
        *active = OperationState::Finalizing;
        self.operation.operation = OperationState::Finalizing;
        Ok(())
    }
}

#[derive(Default)]
pub struct AppState {
    pub operation: Arc<Mutex<OperationState>>,
    pub active_pid: Arc<Mutex<Option<u32>>>,
    pub cancelled: Arc<AtomicBool>,
    pub model_cancelled: Arc<AtomicBool>,
    pub runtime_cancelled: Arc<AtomicBool>,
    pub inspect_cancelled: Arc<AtomicBool>,
    pub capability_probe: Arc<Mutex<HashSet<String>>>,
}

#[cfg(test)]
mod tests {
    use super::{AppState, JobGuard, OperationGuard, OperationState};
    use std::sync::{mpsc, Arc, Barrier};
    use std::thread;

    #[test]
    fn reserves_only_one_transcription_job() {
        let state = AppState::default();
        let first = JobGuard::reserve(&state).expect("first reservation should succeed");
        assert!(JobGuard::reserve(&state).is_err());
        drop(first);
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
        let second = JobGuard::reserve(&state).expect("reservation should be released");
        drop(second);
    }

    #[test]
    fn cancellation_during_start_cannot_become_running() {
        let state = AppState::default();
        let guard = JobGuard::reserve(&state).unwrap();
        OperationGuard::request_cancel(&state).unwrap();
        assert!(guard.mark_running().is_err());
        assert!(state.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        drop(guard);
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
    }

    #[test]
    fn cancellation_before_finalization_prevents_persistence() {
        let state = AppState::default();
        let mut guard = JobGuard::reserve(&state).unwrap();
        OperationGuard::request_cancel(&state).unwrap();
        assert_eq!(guard.begin_finalization(), Err("Job dibatalkan.".into()));
        assert_eq!(
            *state.operation.lock().unwrap(),
            OperationState::Transcribing
        );
        drop(guard);
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
    }

    #[test]
    fn finalization_rejects_cancel_and_keeps_exclusive_reservation() {
        let state = AppState::default();
        let mut guard = JobGuard::reserve(&state).unwrap();
        guard.begin_finalization().unwrap();
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Finalizing);
        assert!(OperationGuard::request_cancel(&state)
            .unwrap_err()
            .starts_with("job_finalizing:"));
        assert!(!state.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_history_read(&state).is_err());
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
        assert!(OperationGuard::reserve_app_update(&state).is_err());
        drop(guard);
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
        assert!(JobGuard::reserve(&state).is_ok());
    }

    #[test]
    fn concurrent_cancel_and_finalization_have_exactly_one_winner() {
        let state = Arc::new(AppState::default());
        let barrier = Arc::new(Barrier::new(2));
        let mut guard = JobGuard::reserve(&state).unwrap();
        let worker_barrier = barrier.clone();
        let worker = thread::spawn(move || {
            worker_barrier.wait();
            let result = guard.begin_finalization();
            (guard, result)
        });
        barrier.wait();
        let cancelled = OperationGuard::request_cancel(&state);
        // Retain the guard until both results are observed so Idle cannot
        // accept a no-op cancellation after the worker has finished.
        let (guard, finalized) = worker.join().unwrap();
        assert_ne!(cancelled.is_ok(), finalized.is_ok());
        assert_eq!(
            state.cancelled.load(std::sync::atomic::Ordering::SeqCst),
            cancelled.is_ok()
        );
        drop(guard);
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
    }

    #[test]
    fn panic_during_finalization_releases_reservation() {
        let state = AppState::default();
        let result = std::panic::catch_unwind(|| {
            let mut guard = JobGuard::reserve(&state).unwrap();
            guard.begin_finalization().unwrap();
            panic!("simulated persistence panic");
        });
        assert!(result.is_err());
        assert!(JobGuard::reserve(&state).is_ok());
    }

    #[test]
    fn concurrent_start_attempts_have_one_winner() {
        let state = Arc::new(AppState::default());
        let barrier = Arc::new(Barrier::new(2));
        let (sender, receiver) = mpsc::channel();
        let mut handles = Vec::new();

        for _ in 0..2 {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            let sender = sender.clone();
            handles.push(thread::spawn(move || {
                let guard = JobGuard::reserve(&state).ok();
                sender.send(guard.is_some()).unwrap();
                barrier.wait();
                drop(guard);
            }));
        }
        drop(sender);

        let winners = receiver.into_iter().filter(|won| *won).count();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(winners, 1);
        assert_eq!(*state.operation.lock().unwrap(), OperationState::Idle);
    }

    #[test]
    fn all_operations_share_one_reservation() {
        let state = AppState::default();
        let _transcription = JobGuard::reserve(&state).unwrap();
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert_eq!(
            *state.operation.lock().unwrap(),
            OperationState::Transcribing
        );
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
    }

    #[test]
    fn cancellation_after_reservation_is_not_overwritten_by_a_reset() {
        let state = AppState::default();
        state
            .model_cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let guard = OperationGuard::reserve_model_download(&state, "base".into()).unwrap();
        assert!(!state
            .model_cancelled
            .load(std::sync::atomic::Ordering::SeqCst));

        OperationGuard::request_cancel(&state).unwrap();
        assert!(state
            .model_cancelled
            .load(std::sync::atomic::Ordering::SeqCst));
        drop(guard);
    }

    #[test]
    fn every_operation_rejects_all_competing_reservations() {
        let state = AppState::default();
        let model = OperationGuard::reserve_model_download(&state, "base".into()).unwrap();
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_runtime_install(&state, "cuda".into()).is_err());
        assert!(OperationGuard::reserve_model_delete(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_inspecting(&state).is_err());
        assert!(OperationGuard::reserve_app_update(&state).is_err());
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
        drop(model);

        let runtime = OperationGuard::reserve_runtime_install(&state, "vulkan".into()).unwrap();
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_model_delete(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_inspecting(&state).is_err());
        assert!(OperationGuard::reserve_app_update(&state).is_err());
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
        drop(runtime);

        let model_delete = OperationGuard::reserve_model_delete(&state, "base".into()).unwrap();
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_runtime_install(&state, "cuda".into()).is_err());
        assert!(OperationGuard::reserve_inspecting(&state).is_err());
        assert!(OperationGuard::reserve_app_update(&state).is_err());
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
        drop(model_delete);

        OperationGuard::reserve_app_update(&state).unwrap();
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_runtime_install(&state, "cuda".into()).is_err());
        assert!(OperationGuard::reserve_model_delete(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_inspecting(&state).is_err());
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
        OperationGuard::release_app_update(&state).unwrap();

        let inspect = OperationGuard::reserve_inspecting(&state).unwrap();
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_runtime_install(&state, "cuda".into()).is_err());
        assert!(OperationGuard::reserve_model_delete(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_app_update(&state).is_err());
        assert!(OperationGuard::reserve_data_reset(&state).is_err());
        drop(inspect);

        let reset = OperationGuard::reserve_data_reset(&state).unwrap();
        assert!(JobGuard::reserve(&state).is_err());
        assert!(OperationGuard::reserve_model_download(&state, "base".into()).is_err());
        assert!(OperationGuard::reserve_history_read(&state).is_err());
        assert!(OperationGuard::reserve_history_delete(&state).is_err());
        assert!(OperationGuard::reserve_history_export(&state).is_err());
        drop(reset);
    }
}
