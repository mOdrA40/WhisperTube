use std::sync::{atomic::AtomicBool, Arc, Mutex};

#[derive(Default)]
pub struct AppState {
    pub active_pid: Arc<Mutex<Option<u32>>>,
    pub cancelled: Arc<AtomicBool>,
}
