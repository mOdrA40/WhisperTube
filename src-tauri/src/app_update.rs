use std::{
    fs::File,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use reqwest_updater::{header, Client, Proxy, Response};
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
    // Tauri initializes the shared rustls provider before returning an Update.
    let mut client = Client::builder()
        .user_agent(concat!("WhisperTube/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .timeout(DOWNLOAD_TIMEOUT);
    if update.no_proxy {
        client = client.no_proxy();
    } else if let Some(proxy) = &update.proxy {
        client = client.proxy(Proxy::all(proxy.as_str()).map_err(|e| e.without_url().to_string())?);
    }
    let mut headers = update.headers.clone();
    headers
        .entry(header::ACCEPT)
        .or_insert(header::HeaderValue::from_static("application/octet-stream"));
    let response = client
        .build()
        .map_err(|e| format!("Klien pembaruan gagal dibuat: {}", e.without_url()))?
        .get(update.download_url.clone())
        .headers(headers)
        .send()
        .await
        .and_then(Response::error_for_status)
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
    mut response: Response,
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

#[cfg(target_os = "linux")]
fn install_appimage(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::{self, Read, Write};

    fn is_appimage(bytes: &[u8]) -> bool {
        bytes.starts_with(b"\x7fELF") && bytes.get(8..11) == Some(b"AI\x02")
    }

    if !is_appimage(bytes) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Paket pembaruan bukan AppImage yang valid.",
        ));
    }

    let target = path.canonicalize()?;
    let mut installed = crate::durable::open_regular_read(&target)?;
    let mut header = [0; 11];
    installed.read_exact(&mut header)?;
    if !is_appimage(&header) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "File AppImage yang sedang berjalan tidak valid.",
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("Folder AppImage tidak dapat ditentukan."))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged
        .as_file()
        .set_permissions(installed.metadata()?.permissions())?;
    staged.as_file().sync_all()?;
    staged.persist(&target).map_err(|failure| failure.error)?;
    crate::durable::sync_parent(&target)
}

#[cfg(windows)]
pub(crate) fn cleanup_stale_installers(app: &tauri::AppHandle) -> std::io::Result<()> {
    cleanup_stale_installers_in(
        &std::env::temp_dir(),
        &app.package_info().name,
        std::time::SystemTime::now(),
    )
}

#[cfg(windows)]
fn cleanup_stale_installers_in(
    root: &std::path::Path,
    app_name: &str,
    now: std::time::SystemTime,
) -> std::io::Result<()> {
    use std::{fs, io, os::windows::fs::OpenOptionsExt, path::Path};

    const GRACE: Duration = Duration::from_secs(24 * 60 * 60);
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const DELETE: u32 = 0x10000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;

    fn expired(metadata: &fs::Metadata, now: std::time::SystemTime) -> bool {
        metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= GRACE)
    }

    fn open_directory(path: &Path, access: u32) -> io::Result<(File, fs::Metadata)> {
        let directory = fs::OpenOptions::new()
            .access_mode(access)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir() || crate::durable::is_link(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expected an ordinary updater directory",
            ));
        }
        Ok((directory, metadata))
    }

    // Keep the root and each candidate directory open without delete sharing
    // while resolving their children. Only aged entries with the updater's
    // exact naming pattern are eligible.
    let (_root_handle, _) = open_directory(root, FILE_READ_ATTRIBUTES)?;
    let prefix = format!("{app_name}-");
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some((version, suffix)) = name
            .to_str()
            .and_then(|name| name.strip_prefix(&prefix))
            .and_then(|name| name.rsplit_once("-updater-"))
        else {
            continue;
        };
        if semver::Version::parse(version).is_err()
            || suffix.len() != 6
            || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            continue;
        }
        let path = entry.path();
        let Ok((directory, metadata)) = open_directory(&path, FILE_READ_ATTRIBUTES | DELETE) else {
            continue;
        };
        if !expired(&metadata, now) {
            continue;
        }
        let installer = path.join(format!("{app_name}-{version}-installer.exe"));
        match fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | DELETE)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&installer)
        {
            Ok(handle) => {
                let metadata = handle.metadata()?;
                if !metadata.is_file()
                    || crate::durable::is_link(&metadata)
                    || !expired(&metadata, now)
                {
                    continue;
                }
                if let Err(error) = mark_for_deletion(&handle) {
                    eprintln!("WhisperTube stale updater package skipped: {error}");
                    continue;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => continue,
        }
        // Windows can only mark this directory for removal if it is empty.
        match mark_for_deletion(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => eprintln!("WhisperTube stale updater directory skipped: {error}"),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn mark_for_deletion(file: &File) -> std::io::Result<()> {
    use std::{ffi::c_void, os::windows::io::AsRawHandle};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetFileInformationByHandle(
            file: *mut c_void,
            class: i32,
            information: *const c_void,
            size: u32,
        ) -> i32;
    }

    const FILE_DISPOSITION_INFO: i32 = 4;
    let delete_on_close = 1u8; // FILE_DISPOSITION_INFO contains one BOOLEAN.
                               // SAFETY: the caller owns a live handle opened with DELETE access.
    let status = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FILE_DISPOSITION_INFO,
            (&delete_on_close as *const u8).cast(),
            std::mem::size_of_val(&delete_on_close) as u32,
        )
    };
    if status == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[tauri::command]
pub async fn install_app_update(
    webview: Webview,
    state: State<'_, AppState>,
    rid: ResourceId,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), ErrorPayload> {
    crate::wait_for_startup(webview.app_handle()).await;
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
    #[cfg(target_os = "linux")]
    let appimage = match tauri::utils::platform::bundle_type() {
        Some(tauri::utils::config::BundleType::AppImage) => Some(
            webview
                .app_handle()
                .env()
                .appimage
                .clone()
                .ok_or("Lokasi AppImage yang sedang berjalan tidak dapat ditentukan.")?,
        ),
        _ => None,
    };
    let file = download(&update, &on_progress).await?;
    tokio::task::spawn_blocking(move || {
        let _reservation = reservation;
        // SAFETY: the anonymous tempfile is owned exclusively by this task.
        // All writes finished before the read-only mapping was created.
        let bytes = unsafe { memmap2::MmapOptions::new().map(&file) }
            .map_err(|e| format!("Paket pembaruan tidak dapat dibuka: {e}"))?;
        verify_signature(&bytes, &public_key, &update.signature)?;
        #[cfg(target_os = "linux")]
        if let Some(path) = appimage {
            update
                .verify(&bytes)
                .map_err(|e| format!("Verifikasi paket AppImage gagal: {e}"))?;
            return install_appimage(std::path::Path::new(&path), &bytes)
                .map_err(|e| format!("Pemasangan pembaruan AppImage gagal: {e}"));
        }
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
    fn packaged_updater_requires_an_authenticated_version() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater: tauri_plugin_updater::Config =
            serde_json::from_value(config["plugins"]["updater"].clone()).unwrap();
        assert!(updater.require_signed_version);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_replacement_preserves_an_open_executable_and_its_mode() {
        use std::{fs, io::Read, os::unix::fs::PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let image = root.path().join("WhisperTube.AppImage");
        let original = b"\x7fELF\x02\x01\x01\x00AI\x02 original executable";
        fs::write(&image, original).unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o751)).unwrap();
        let mut running = File::open(&image).unwrap();

        let error = install_appimage(&image, b"wrong package format").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&image).unwrap(), original);

        let replacement = b"\x7fELF\x02\x01\x01\x00AI\x02 replacement fixture";
        install_appimage(&image, replacement).unwrap();
        assert_eq!(fs::read(&image).unwrap(), replacement);
        assert_eq!(
            fs::metadata(&image).unwrap().permissions().mode() & 0o777,
            0o751
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        let mut old_bytes = Vec::new();
        running.read_to_end(&mut old_bytes).unwrap();
        assert_eq!(old_bytes, original);

        let other = root.path().join("another executable");
        let other_bytes = b"\x7fELF ordinary program";
        fs::write(&other, other_bytes).unwrap();
        let error = install_appimage(&other, replacement).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(fs::read(other).unwrap(), other_bytes);
    }

    #[cfg(windows)]
    #[test]
    fn stale_updater_cleanup_preserves_recent_and_unrelated_content() {
        use std::{fs, os::windows::fs::OpenOptionsExt};

        let storage = tempfile::tempdir().unwrap();
        let expired = storage.path().join("WhisperTube-1.2.2-updater-abcd12");
        fs::create_dir(&expired).unwrap();
        let owned = storage.path().join("WhisperTube-1.2.3-updater-abcd12");
        let neighbor = storage.path().join("WhisperTube-other-updater-abcd12");
        fs::create_dir(&owned).unwrap();
        fs::create_dir(&neighbor).unwrap();
        let installer = owned.join("WhisperTube-1.2.3-installer.exe");
        fs::write(&installer, b"inert installer fixture").unwrap();
        let canary = owned.join("user.txt");
        fs::write(&canary, b"unrelated").unwrap();
        let now = std::time::SystemTime::now();
        let after_grace = now + Duration::from_secs(48 * 60 * 60);
        cleanup_stale_installers_in(storage.path(), "WhisperTube", now).unwrap();
        assert!(installer.is_file());
        let reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&installer)
            .unwrap();
        cleanup_stale_installers_in(storage.path(), "WhisperTube", after_grace).unwrap();
        assert!(!expired.exists());
        assert!(installer.is_file());
        drop(reader);
        cleanup_stale_installers_in(storage.path(), "WhisperTube", after_grace).unwrap();
        assert!(!installer.exists());
        assert!(neighbor.is_dir());
        assert_eq!(fs::read(&canary).unwrap(), b"unrelated");
    }

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

        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
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
