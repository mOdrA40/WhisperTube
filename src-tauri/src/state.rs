use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum OperationState {
    #[default]
    Idle,
    Transcribing,
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
                crate::process::terminate_process_tree(pid);
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
    _operation: OperationGuard,
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
            _operation: operation,
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
}

#[derive(Default)]
pub struct AppState {
    pub operation: Arc<Mutex<OperationState>>,
    pub active_pid: Arc<Mutex<Option<u32>>>,
    pub cancelled: Arc<AtomicBool>,
    pub model_cancelled: Arc<AtomicBool>,
    pub runtime_cancelled: Arc<AtomicBool>,
    pub inspect_cancelled: Arc<AtomicBool>,
    pub vulkan_probe: Arc<Mutex<Option<VulkanProbeResult>>>,
}

#[derive(Clone, Debug)]
pub struct VulkanProbeResult {
    pub signature: String,
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
