use reqwest::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use zip::ZipArchive;

use crate::{
    paths::{engine_path, user_runtime_dir},
    types::{AcceleratorDownloadPayload, AcceleratorInfo},
};

const RELEASE_REPOSITORY: &str = "mOdrA40/WhisperTube";
const RELEASE_TAG: &str = "accelerators-v0.1.0";

// These values are filled by scripts/sync-accelerator-hashes.ps1 after the
// matching public GitHub Release has been built and reviewed. Keeping the
// hashes in the application source prevents a release-side change from being
// silently accepted by a shipped build.
#[allow(dead_code)]
const METAL_MACOS_ARM64_SHA256: Option<&str> =
    Some("a48c3a9b7243f0c9707c2d24aa04c8f7ac17bf7af52dd6847af608014f5070d5");

#[allow(dead_code)]
const METAL_MACOS_X64_SHA256: Option<&str> =
    Some("11c527f865f2077a1579b17362c9dac177f02e62943177eb5c4076a29c3bd08f");

#[allow(dead_code)]
const VULKAN_WINDOWS_X64_SHA256: Option<&str> =
    Some("a8f8e2d0808c48154ab113fe456695218fbb2e2d74e1be5444b340f176f869e8");

#[allow(dead_code)]
const VULKAN_LINUX_X64_SHA256: Option<&str> =
    Some("98f291d42b1e02ea71f9cc6acfa53d5ec5207a9bb67fcd34beb251682f59c07d");

struct PackSpec {
    backend: &'static str,
    label: &'static str,
    asset_name: &'static str,
    trusted_sha256: Option<&'static str>,
    description: &'static str,
}

fn pack_specs() -> Vec<PackSpec> {
    #[cfg(target_os = "macos")]
    {
        let asset_name = if cfg!(target_arch = "aarch64") {
            "whispertube-macos-arm64-metal.zip"
        } else if cfg!(target_arch = "x86_64") {
            "whispertube-macos-x64-metal.zip"
        } else {
            return Vec::new();
        };
        return vec![PackSpec {
            backend: "metal",
            label: "Apple Metal",
            asset_name,
            trusted_sha256: if cfg!(target_arch = "aarch64") {
                METAL_MACOS_ARM64_SHA256
            } else {
                METAL_MACOS_X64_SHA256
            },
            description: "Accelerator GPU Apple Metal untuk macOS",
        }];
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if cfg!(target_arch = "x86_64") {
        let asset_name = if cfg!(target_os = "windows") {
            "whispertube-windows-x64-vulkan.zip"
        } else {
            "whispertube-linux-x64-vulkan.zip"
        };
        return vec![PackSpec {
            backend: "vulkan",
            label: "Vulkan",
            asset_name,
            trusted_sha256: if cfg!(target_os = "windows") {
                VULKAN_WINDOWS_X64_SHA256
            } else {
                VULKAN_LINUX_X64_SHA256
            },
            description: "Accelerator GPU lintas vendor Vulkan",
        }];
    }

    Vec::new()
}

fn executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "whisper-cli.exe"
    } else {
        "whisper-cli"
    }
}

pub fn catalog(app: &AppHandle, gpu_detected: bool) -> Result<Vec<AcceleratorInfo>, String> {
    Ok(pack_specs()
        .into_iter()
        .filter(|_| cfg!(target_os = "macos") || gpu_detected)
        .map(|spec| AcceleratorInfo {
            id: spec.backend.into(),
            label: spec.label.into(),
            backend: spec.backend.into(),
            supported: true,
            installed: engine_path(app, spec.backend)
                .map(|path| path.exists())
                .unwrap_or(false),
            downloadable: spec.trusted_sha256.is_some(),
            description: spec.description.into(),
        })
        .collect())
}

fn emit_progress(
    app: &AppHandle,
    backend: &str,
    percent: f64,
    downloaded_bytes: u64,
    total_bytes: u64,
    bytes_per_second: Option<u64>,
) {
    let _ = app.emit(
        "accelerator-download",
        AcceleratorDownloadPayload {
            backend: backend.into(),
            downloaded_bytes,
            total_bytes,
            percent: percent.clamp(0.0, 100.0),
            bytes_per_second,
        },
    );
}

async fn release_asset(
    client: &Client,
    asset_name: &str,
) -> Result<(String, String, Option<String>), String> {
    let api_url =
        format!("https://api.github.com/repos/{RELEASE_REPOSITORY}/releases/tags/{RELEASE_TAG}");
    let response = tokio::time::timeout(Duration::from_secs(30), client.get(api_url).send())
        .await
        .map_err(|_| "Timeout saat mengambil manifest accelerator.".to_string())?
        .map_err(|e| format!("Gagal mengambil manifest accelerator: {e}"))?;
    if response.status().as_u16() == 404 {
        return Err("Release accelerator belum tersedia atau repository masih private. Publikasikan release accelerator terlebih dahulu.".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub mengembalikan HTTP {} saat membaca accelerator release.",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Manifest accelerator tidak bisa dibaca: {e}"))?;
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Manifest accelerator tidak valid: {e}"))?;
    let assets = manifest
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "Manifest accelerator tidak memiliki daftar asset.".to_string())?;
    let checksum_name = format!(
        "{}.sha256",
        asset_name.strip_suffix(".zip").unwrap_or(asset_name)
    );
    let asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(asset_name))
        .ok_or_else(|| format!("Asset accelerator {asset_name} belum dipublish."))?;
    let checksum_asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(checksum_name.as_str()))
        .ok_or_else(|| format!("Checksum asset accelerator {checksum_name} belum dipublish."))?;
    let download_url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| "URL download accelerator tidak tersedia.".to_string())?
        .to_string();
    let asset_digest = match asset.get("digest").and_then(Value::as_str) {
        Some(digest) => {
            let digest = digest
                .strip_prefix("sha256:")
                .ok_or_else(|| "Digest accelerator tidak berformat SHA-256.".to_string())?
                .to_ascii_lowercase();
            if digest.len() != 64 || !digest.chars().all(|value| value.is_ascii_hexdigit()) {
                return Err("Digest accelerator tidak berformat SHA-256.".into());
            }
            Some(digest)
        }
        None => None,
    };
    let checksum_url = checksum_asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| "URL checksum accelerator tidak tersedia.".to_string())?;
    let checksum_response =
        tokio::time::timeout(Duration::from_secs(30), client.get(checksum_url).send())
            .await
            .map_err(|_| "Timeout saat mengambil checksum accelerator.".to_string())?
            .map_err(|e| format!("Gagal mengambil checksum accelerator: {e}"))?;
    if !checksum_response.status().is_success() {
        return Err(format!(
            "Checksum accelerator mengembalikan HTTP {}.",
            checksum_response.status()
        ));
    }
    let checksum = checksum_response
        .text()
        .await
        .map_err(|e| format!("Checksum accelerator tidak bisa dibaca: {e}"))?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if checksum.len() != 64
        || !checksum
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("Checksum accelerator tidak berformat SHA-256.".into());
    }
    Ok((download_url, checksum, asset_digest))
}

struct FinalizePaths<'a> {
    archive: &'a Path,
    extract: &'a Path,
    staging: &'a Path,
    destination: &'a Path,
    trusted_sha256: &'a str,
    sidecar_sha256: &'a str,
    asset_digest: Option<&'a str>,
}

async fn download_archive(
    app: &AppHandle,
    backend: &str,
    client: &Client,
    url: &str,
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut response = tokio::time::timeout(Duration::from_secs(30), client.get(url).send())
        .await
        .map_err(|_| "Timeout saat menunggu response accelerator.".to_string())?
        .map_err(|e| format!("Gagal mengunduh accelerator: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Server accelerator mengembalikan HTTP {}",
            response.status()
        ));
    }
    let total = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(destination)
        .await
        .map_err(|e| format!("Gagal membuat file accelerator: {e}"))?;
    let mut downloaded = 0u64;
    let download_started_at = Instant::now();
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Download accelerator dibatalkan.".into());
        }
        let chunk = tokio::time::timeout(Duration::from_secs(30), response.chunk())
            .await
            .map_err(|_| "Timeout saat membaca data accelerator selama 30 detik.".to_string())?
            .map_err(|e| format!("Download accelerator terputus: {e}"))?;
        let Some(chunk) = chunk else {
            break;
        };
        if cancelled.load(Ordering::SeqCst) {
            return Err("Download accelerator dibatalkan.".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Gagal menyimpan accelerator: {e}"))?;
        downloaded += chunk.len() as u64;
        let elapsed = download_started_at.elapsed().as_secs_f64();
        let bytes_per_second = (elapsed > 0.0).then(|| (downloaded as f64 / elapsed) as u64);
        let percent = if total > 0 {
            downloaded as f64 / total as f64 * 85.0
        } else {
            0.0
        };
        emit_progress(app, backend, percent, downloaded, total, bytes_per_second);
    }
    file.flush()
        .await
        .map_err(|e| format!("Gagal menyelesaikan file accelerator: {e}"))?;
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("Gagal membuka accelerator: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Gagal membaca accelerator: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "Checksum accelerator tidak cocok. Expected {expected}, actual {actual}."
        ));
    }
    Ok(actual)
}

fn extract_zip_safely(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file =
        File::open(archive_path).map_err(|e| format!("Gagal membuka archive accelerator: {e}"))?;
    let mut archive = ZipArchive::new(archive_file)
        .map_err(|e| format!("Archive accelerator tidak valid: {e}"))?;
    fs::create_dir_all(destination)
        .map_err(|e| format!("Gagal membuat folder extract accelerator: {e}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Gagal membaca entry accelerator: {e}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "Archive accelerator memiliki path tidak aman.".to_string())?
            .to_path_buf();
        let target = destination.join(relative);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&target)
                .map_err(|e| format!("Gagal membuat folder accelerator: {e}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Gagal membuat folder accelerator: {e}"))?;
        }
        let mut output =
            File::create(&target).map_err(|e| format!("Gagal menulis accelerator: {e}"))?;
        io::copy(&mut entry, &mut output).map_err(|e| format!("Gagal extract accelerator: {e}"))?;
    }
    Ok(())
}

fn find_file(root: &Path, file_name: &str) -> Result<Option<PathBuf>, String> {
    for entry in fs::read_dir(root).map_err(|e| format!("Gagal membaca folder accelerator: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry accelerator: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, file_name)? {
                return Ok(Some(found));
            }
        } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|e| format!("Gagal membuat staging accelerator: {e}"))?;
    for entry in
        fs::read_dir(source).map_err(|e| format!("Gagal membaca folder accelerator: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Gagal membaca entry accelerator: {e}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)
                .map_err(|e| format!("Gagal menyalin accelerator: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn mark_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Gagal mengatur permission accelerator: {e}"))?;
    Ok(())
}

fn finalize_blocking(
    app: &AppHandle,
    spec: &PackSpec,
    cancelled: &AtomicBool,
    paths: FinalizePaths<'_>,
) -> Result<(), String> {
    let actual_sha256 = verify_sha256(paths.archive, paths.trusted_sha256)?;
    if actual_sha256 != paths.sidecar_sha256 {
        return Err(format!(
            "Checksum sidecar accelerator tidak cocok dengan hash tepercaya. Expected {}, actual {}.",
            paths.trusted_sha256, paths.sidecar_sha256
        ));
    }
    if let Some(asset_digest) = paths.asset_digest {
        if actual_sha256 != asset_digest {
            return Err(format!(
                "Digest GitHub accelerator tidak cocok. Expected {asset_digest}, actual {actual_sha256}."
            ));
        }
    }
    if cancelled.load(Ordering::SeqCst) {
        return Err("Download accelerator dibatalkan.".into());
    }
    emit_progress(app, spec.backend, 88.0, 0, 0, None);
    extract_zip_safely(paths.archive, paths.extract)?;
    let cli = find_file(paths.extract, executable_name())?.ok_or_else(|| {
        format!(
            "{} tidak ditemukan dalam accelerator package.",
            executable_name()
        )
    })?;
    let release_dir = cli
        .parent()
        .ok_or_else(|| "Folder accelerator tidak valid.".to_string())?;
    copy_tree(release_dir, paths.staging)?;
    let staging_cli = paths.staging.join(executable_name());
    #[cfg(unix)]
    mark_executable(&staging_cli)?;
    if !staging_cli.exists() {
        return Err("Accelerator gagal dipasang ke staging.".into());
    }
    emit_progress(app, spec.backend, 94.0, 0, 0, None);
    let mut command = std::process::Command::new(&staging_cli);
    crate::process::hide_console(&mut command);
    let test = command
        .arg("--version")
        .output()
        .map_err(|e| format!("Accelerator tidak bisa dijalankan: {e}"))?;
    if !test.status.success() {
        let error = String::from_utf8_lossy(&test.stderr).trim().to_string();
        return Err(if error.is_empty() {
            "Accelerator gagal melakukan self-check.".into()
        } else {
            format!("Accelerator gagal melakukan self-check: {error}")
        });
    }
    if cancelled.load(Ordering::SeqCst) {
        return Err("Download accelerator dibatalkan.".into());
    }
    if paths.destination.exists() {
        return Err(
            "Accelerator baru saja dipasang oleh proses lain. Klik Re-check components.".into(),
        );
    }
    fs::rename(paths.staging, paths.destination)
        .map_err(|e| format!("Gagal mengaktifkan accelerator: {e}"))?;
    emit_progress(app, spec.backend, 100.0, 0, 0, None);
    Ok(())
}

pub async fn install(
    app: AppHandle,
    backend: String,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let spec = pack_specs()
        .into_iter()
        .find(|spec| spec.backend == backend)
        .ok_or_else(|| format!("Accelerator {backend} tidak didukung pada platform ini."))?;
    let trusted_sha256 = spec.trusted_sha256.ok_or_else(|| {
        format!(
            "Accelerator {} belum memiliki checksum tepercaya dalam build aplikasi ini.",
            spec.label
        )
    })?;
    let runtime_root = user_runtime_dir(&app)?;
    let destination = runtime_root.join(spec.backend);
    if destination.join(executable_name()).exists() {
        return Ok(());
    }
    let temp_root =
        std::env::temp_dir().join(format!("whispertube-accelerator-{}", uuid::Uuid::new_v4()));
    let archive_path = temp_root.join(spec.asset_name);
    let extract_path = temp_root.join("extract");
    let staging_path = runtime_root.join(format!(
        ".{}-staging-{}",
        spec.backend,
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_root)
        .map_err(|e| format!("Gagal membuat temporary accelerator folder: {e}"))?;
    let result = async {
        let client = Client::builder()
            .user_agent("WhisperTube/0.1")
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Gagal membuat HTTP client accelerator: {e}"))?;
        let (url, expected_sha256, asset_digest) = release_asset(&client, spec.asset_name).await?;
        download_archive(&app, spec.backend, &client, &url, &archive_path, &cancelled).await?;
        let app_for_finalize = app.clone();
        let cancelled_for_finalize = cancelled.clone();
        let archive_for_finalize = archive_path.clone();
        let extract_for_finalize = extract_path.clone();
        let staging_for_finalize = staging_path.clone();
        let destination_for_finalize = destination.clone();
        tokio::task::spawn_blocking(move || {
            finalize_blocking(
                &app_for_finalize,
                &spec,
                &cancelled_for_finalize,
                FinalizePaths {
                    archive: &archive_for_finalize,
                    extract: &extract_for_finalize,
                    staging: &staging_for_finalize,
                    destination: &destination_for_finalize,
                    trusted_sha256,
                    sidecar_sha256: &expected_sha256,
                    asset_digest: asset_digest.as_deref(),
                },
            )
        })
        .await
        .map_err(|e| format!("Accelerator finalize task gagal: {e}"))?
    }
    .await;
    let _ = fs::remove_dir_all(&temp_root);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::pack_specs;

    #[test]
    fn pack_asset_matches_target_platform() {
        let specs = pack_specs();
        if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            assert_eq!(specs[0].asset_name, "whispertube-windows-x64-vulkan.zip");
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            assert_eq!(specs[0].asset_name, "whispertube-linux-x64-vulkan.zip");
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            assert_eq!(specs[0].asset_name, "whispertube-macos-arm64-metal.zip");
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
            assert_eq!(specs[0].asset_name, "whispertube-macos-x64-metal.zip");
        } else {
            assert!(specs.is_empty());
        }
    }

    #[test]
    fn checksum_asset_drops_zip_suffix() {
        let asset_name = "whispertube-windows-x64-vulkan.zip";
        let checksum_name = format!(
            "{}.sha256",
            asset_name.strip_suffix(".zip").unwrap_or(asset_name)
        );
        assert_eq!(checksum_name, "whispertube-windows-x64-vulkan.sha256");
    }
}
