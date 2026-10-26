use reqwest::{RequestBuilder, Response};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Bound download UI events independently of HTTP chunk size. Callers still
/// emit the initial snapshot and the final snapshot unconditionally.
pub fn download_progress_due(elapsed: Duration, last_emitted: &mut Duration) -> bool {
    if elapsed.saturating_sub(*last_emitted) < Duration::from_millis(100) {
        return false;
    }
    *last_emitted = elapsed;
    true
}

async fn cancellation_signal(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn send_cancelable(
    request: RequestBuilder,
    cancelled: &AtomicBool,
    timeout: Duration,
    label: &str,
) -> Result<Response, String> {
    tokio::select! {
        result = tokio::time::timeout(timeout, request.send()) => result
            .map_err(|_| format!("Timeout saat menunggu response {label}."))?
            .map_err(|error| format!("Gagal mengunduh {label}: {error}")),
        _ = cancellation_signal(cancelled) => Err(format!("Pengambilan {label} dibatalkan.")),
    }
}

pub async fn next_chunk_cancelable(
    response: &mut Response,
    cancelled: &AtomicBool,
    timeout: Duration,
    label: &str,
) -> Result<Option<bytes::Bytes>, String> {
    tokio::select! {
        result = tokio::time::timeout(timeout, response.chunk()) => result
            .map_err(|_| format!("Timeout saat membaca data {label} selama {} detik.", timeout.as_secs()))?
            .map_err(|error| format!("Download {label} terputus: {error}")),
        _ = cancellation_signal(cancelled) => Err(format!("Pengambilan {label} dibatalkan.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_progress_is_bounded_under_a_chunk_flood() {
        let mut last = Duration::ZERO;
        let emitted = (0..=1000)
            .filter(|millis| download_progress_due(Duration::from_millis(*millis), &mut last))
            .count();
        assert_eq!(emitted, 10);
        assert!(!download_progress_due(
            Duration::from_millis(1050),
            &mut last
        ));
        assert!(download_progress_due(
            Duration::from_millis(1100),
            &mut last
        ));
    }
}
