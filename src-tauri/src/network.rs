use reqwest::{RequestBuilder, Response};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

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
