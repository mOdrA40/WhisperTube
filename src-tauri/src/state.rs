use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JobState {
    #[default]
    Idle,
    Starting,
    Running,
    Cancelling,
}

pub struct JobGuard {
    state: Arc<Mutex<JobState>>,
}

impl JobGuard {
    pub fn is_active(app_state: &AppState) -> Result<bool, String> {
        Ok(*app_state
            .job_state
            .lock()
            .map_err(|_| "State job terkunci")?
            != JobState::Idle)
    }

    pub fn reserve(app_state: &AppState) -> Result<Self, String> {
        let mut state = app_state
            .job_state
            .lock()
            .map_err(|_| "State job terkunci")?;
        if *state != JobState::Idle {
            return Err("Masih ada job transkripsi yang sedang berjalan.".into());
        }
        *state = JobState::Starting;
        app_state.cancelled.store(false, Ordering::SeqCst);
        Ok(Self {
            state: Arc::clone(&app_state.job_state),
        })
    }

    pub fn mark_running(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "State job terkunci")?;
        match *state {
            JobState::Starting => {
                *state = JobState::Running;
                Ok(())
            }
            JobState::Running => Ok(()),
            JobState::Cancelling => Err("Job dibatalkan.".into()),
            JobState::Idle => Err("Lifecycle job tidak valid.".into()),
        }
    }

    pub fn request_cancel(app_state: &AppState) -> Result<bool, String> {
        let mut state = app_state
            .job_state
            .lock()
            .map_err(|_| "State job terkunci")?;
        if *state == JobState::Idle {
            return Ok(false);
        }
        *state = JobState::Cancelling;
        app_state.cancelled.store(true, Ordering::SeqCst);
        Ok(true)
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            *state = JobState::Idle;
        }
    }
}

#[derive(Default)]
pub struct AppState {
    pub job_state: Arc<Mutex<JobState>>,
    pub active_pid: Arc<Mutex<Option<u32>>>,
    pub cancelled: Arc<AtomicBool>,
    pub model_cancelled: Arc<AtomicBool>,
    pub runtime_cancelled: Arc<AtomicBool>,
    pub model_downloading: Arc<Mutex<Option<String>>>,
    pub runtime_installing: Arc<Mutex<Option<String>>>,
}

#[cfg(test)]
mod tests {
    use super::{AppState, JobGuard, JobState};
    use std::sync::{mpsc, Arc, Barrier};
    use std::thread;

    #[test]
    fn reserves_only_one_transcription_job() {
        let state = AppState::default();
        let first = JobGuard::reserve(&state).expect("first reservation should succeed");
        assert_eq!(*state.job_state.lock().unwrap(), JobState::Starting);
        assert!(JobGuard::reserve(&state).is_err());
        drop(first);
        assert!(!JobGuard::is_active(&state).unwrap());
        let second = JobGuard::reserve(&state).expect("reservation should be released");
        drop(second);
    }

    #[test]
    fn cancellation_during_start_cannot_become_running() {
        let state = AppState::default();
        let guard = JobGuard::reserve(&state).unwrap();
        assert!(JobGuard::request_cancel(&state).unwrap());
        assert!(guard.mark_running().is_err());
        assert!(state.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        drop(guard);
        assert!(!JobGuard::is_active(&state).unwrap());
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
        assert!(!JobGuard::is_active(&state).unwrap());
    }
}
