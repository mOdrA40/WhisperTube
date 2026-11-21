use std::{
    fs::File,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use serde::Serialize;
use tauri::{ipc::Channel, Manager, ResourceId, State, Webview};
use tauri_plugin_updater::Update;
use tokio::io::AsyncWriteExt;

use crate::{
    state::{AppState, OperationGuard},
    types::ErrorPayload,
};

const MAX_UPDATE_BYTES: u64 = 256 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    percent: f64,
}

fn checked_size(bytes: u64, next: u64, limit: u64) -> Result<u64, String> {
    bytes
        .checked_add(next)
        .filter(|&total| total <= limit)
        .ok_or_else(|| "Ukuran paket pembaruan melebihi batas aman.".into())
}

fn decode_minisign(value: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| "Encoding tanda tangan pembaruan tidak valid.")?;
    String::from_utf8(bytes).map_err(|_| "Tanda tangan pembaruan bukan UTF-8.".into())
}

fn verify_signature(bytes: &[u8], public_key: &str, signature: &str) -> Result<(), String> {
    let key = PublicKey::decode(&decode_minisign(public_key)?)
        .map_err(|e| format!("Kunci publik pembaruan tidak valid: {e}"))?;
    let signature = Signature::decode(&decode_minisign(signature)?)
        .map_err(|e| format!("Tanda tangan pembaruan tidak valid: {e}"))?;
    key.verify(bytes, &signature, true)
        .map_err(|e| format!("Verifikasi tanda tangan pembaruan gagal: {e}"))
}

async fn download(update: &Update, on_progress: &Channel<UpdateProgress>) -> Result<File, String> {
    let mut client = reqwest::Client::builder()
        .user_agent(concat!("WhisperTube/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .timeout(DOWNLOAD_TIMEOUT);
    if update.no_proxy {
        client = client.no_proxy();
    } else if let Some(proxy) = &update.proxy {
        client = client
            .proxy(reqwest::Proxy::all(proxy.as_str()).map_err(|e| e.without_url().to_string())?);
    }
    let mut headers = update.headers.clone();
    headers
        .entry(reqwest::header::ACCEPT)
        .or_insert(reqwest::header::HeaderValue::from_static(
            "application/octet-stream",
        ));
    let response = client
        .build()
        .map_err(|e| format!("Klien pembaruan gagal dibuat: {}", e.without_url()))?
        .get(update.download_url.clone())
        .headers(headers)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("Paket pembaruan gagal diunduh: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "Paket pembaruan gagal diunduh: HTTP {}",
            response.status()
        ));
    }
    stream_package(response, MAX_UPDATE_BYTES, |size, total, finished| {
        let percent = if finished {
            100.0
        } else {
            total.filter(|&length| length != 0).map_or(0.0, |length| {
                (size as f64 / length as f64 * 100.0).min(100.0)
            })
        };
        let _ = on_progress.send(UpdateProgress {
            downloaded_bytes: size,
            total_bytes: total,
            percent,
        });
    })
    .await
}

async fn stream_package(
    mut response: reqwest::Response,
    max_bytes: u64,
    mut emit: impl FnMut(u64, Option<u64>, bool),
) -> Result<File, String> {
    let total = response.content_length();
    if let Some(length) = total {
        checked_size(0, length, max_bytes)?;
    }
    let file = tempfile::tempfile().map_err(|e| format!("File sementara pembaruan: {e}"))?;
    let mut file = tokio::fs::File::from_std(file);
    let mut size = 0;
    let started = Instant::now();
    let mut last = Duration::ZERO;
    emit(0, total, false);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Unduhan pembaruan terputus: {}", e.without_url()))?
    {
        size = checked_size(size, chunk.len() as u64, max_bytes)?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Gagal menyimpan pembaruan: {e}"))?;
        if crate::network::download_progress_due(started.elapsed(), &mut last) {
            emit(size, total, false);
        }
    }
    if size == 0 || total.is_some_and(|length| length != size) {
        return Err("Paket pembaruan kosong atau belum selesai diunduh.".into());
    }
    file.flush()
        .await
        .map_err(|e| format!("Gagal menyelesaikan file pembaruan: {e}"))?;
    emit(size, Some(size), true);
    Ok(file.into_std().await)
}

#[tauri::command]
pub async fn install_app_update(
    webview: Webview,
    state: State<'_, AppState>,
    rid: ResourceId,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), ErrorPayload> {
    let reservation = OperationGuard::reserve_app_update(&state)?;
    let update = webview
        .resources_table()
        .get::<Update>(rid)
        .map_err(|e| format!("Resource pembaruan tidak tersedia: {e}"))?;
    let version = semver::Version::parse(&update.version)
        .map_err(|e| format!("Versi pembaruan tidak valid: {e}"))?;
    if version <= webview.package_info().version {
        return Err("Pembaruan harus lebih baru dari versi terpasang.".into());
    }
    let public_key = webview
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .ok_or("Kunci publik pembaruan belum dikonfigurasi.")?
        .to_owned();
    let file = download(&update, &on_progress).await?;
    tokio::task::spawn_blocking(move || {
        let _reservation = reservation;
        // SAFETY: the anonymous tempfile is owned exclusively by this task.
        // All writes finished before the read-only mapping was created.
        let bytes = unsafe { memmap2::MmapOptions::new().map(&file) }
            .map_err(|e| format!("Paket pembaruan tidak dapat dibuka: {e}"))?;
        verify_signature(&bytes, &public_key, &update.signature)?;
        update
            .install(&bytes)
            .map_err(|e| format!("Pemasangan pembaruan gagal: {e}"))
    })
    .await
    .map_err(|e| format!("Pekerja pembaruan gagal: {e}"))??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_total_and_streamed_bytes_against_the_same_limit() {
        assert_eq!(checked_size(0, 16, 16), Ok(16));
        assert_eq!(checked_size(10, 6, 16), Ok(16));
        assert!(checked_size(10, 7, 16).is_err());
        assert!(checked_size(u64::MAX, 1, u64::MAX).is_err());
    }

    #[tokio::test]
    async fn streamed_package_enforces_the_budget_without_content_length() {
        use std::io::{BufRead, BufReader, Read, Seek, Write};
        use std::net::TcpListener;

        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        for (limit, accepted) in [(6, true), (5, false)] {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = BufReader::new(&stream);
                loop {
                    let mut line = String::new();
                    if request.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n").unwrap();
            });
            let response = client
                .get(format!("http://{address}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.content_length(), None);
            let mut last = None;
            let result = stream_package(response, limit, |size, length, finished| {
                last = Some((size, length, finished));
            })
            .await;
            server.join().unwrap();
            if accepted {
                let mut file = result.unwrap();
                file.rewind().unwrap();
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"abcdef");
                assert_eq!(last, Some((6, Some(6), true)));
            } else {
                assert!(result.unwrap_err().contains("melebihi batas"));
                assert!(!last.unwrap().2);
            }
        }
    }

    #[test]
    fn refuses_invalid_signature_encoding() {
        assert!(verify_signature(b"payload", "wrong key", "wrong signature").is_err());
    }

    #[test]
    fn updater_accepts_signed_bytes_and_rejects_tampering() {
        // Public key and signature from the minisign-verify interoperability test.
        let key = STANDARD.encode(concat!(
            "untrusted comment: fixture public key\n",
            "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3"
        ));
        let signature = STANDARD.encode(concat!(
            "untrusted comment: signature from minisign secret key\n",
            "RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/",
            "z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\n",
            "trusted comment: timestamp:1556193335\tfile:test\n",
            "y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg=="
        ));
        verify_signature(b"test", &key, &signature).unwrap();
        assert!(verify_signature(b"test!", &key, &signature).is_err());
    }
}
