use reqwest::Client as AsyncClient;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{
    fs,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use zip::ZipArchive;

use crate::{
    paths::{model_path, models_dir, user_runtime_dir},
    types::{CudaDownloadPayload, ModelDownloadPayload, ModelInfo},
};

const CUDA_ENGINE_VERSION: &str = "v1.9.1";
const CUDA_ENGINE_BUILD: &str = "12.4.0";
const CUDA_ENGINE_SHA256: &str = "106a2030eff8998e4ef320fe72e263a78449e9040386ee27c41ea80b001b601b";

struct TemporaryFileGuard {
    path: PathBuf,
    committed: bool,
}

impl TemporaryFileGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for TemporaryFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn cuda_engine_url() -> String {
    format!(
        "https://github.com/ggml-org/whisper.cpp/releases/download/{CUDA_ENGINE_VERSION}/whisper-cublas-{CUDA_ENGINE_BUILD}-bin-x64.zip"
    )
}

#[derive(Clone)]
pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub size_mb: u64,
    pub vram_required_mb: u64,
    pub sha1: &'static str,
}

const MODELS: [ModelSpec; 3] = [
    ModelSpec {
        id: "base",
        label: "Fast",
        description: "Ringan untuk CPU/laptop sederhana",
        size_mb: 142,
        vram_required_mb: 2048,
        sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
    },
    ModelSpec {
        id: "large-v3-turbo-q5_0",
        label: "Balanced",
        description: "Default: cepat dan akurat untuk penggunaan umum",
        size_mb: 547,
        vram_required_mb: 4096,
        sha1: "e050f7970618a659205450ad97eb95a18d69c9ee",
    },
    ModelSpec {
        id: "large-v3-q5_0",
        label: "Accurate",
        description: "Akurasi tinggi, lebih berat dan lambat",
        size_mb: 1100,
        vram_required_mb: 7168,
        sha1: "e6e2ed78495d403bef4b7cff42ef4aaadcfea8de",
    },
];

pub fn model_spec(id: &str) -> Result<ModelSpec, String> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .cloned()
        .ok_or_else(|| format!("Model tidak dikenal: {id}"))
}

pub fn list_models(app: &AppHandle) -> Result<Vec<ModelInfo>, String> {
    let dir = models_dir(app)?;
    Ok(MODELS
        .iter()
        .map(|model| ModelInfo {
            id: model.id.into(),
            label: model.label.into(),
            description: model.description.into(),
            size_mb: model.size_mb,
            vram_required_mb: model.vram_required_mb,
            installed: dir.join(format!("ggml-{}.bin", model.id)).exists(),
        })
        .collect())
}

pub fn recommended_model_id(available_vram_mb: Option<u64>) -> String {
    let available = available_vram_mb.unwrap_or_default();
    MODELS
        .iter()
        .rev()
        .find(|model| available >= model.vram_required_mb)
        .map(|model| model.id)
        .unwrap_or(MODELS[0].id)
        .to_string()
}

pub fn ensure_vram_available(model_id: &str, available_vram_mb: Option<u64>) -> Result<(), String> {
    let spec = model_spec(model_id)?;
    let available = available_vram_mb.ok_or_else(|| {
        "VRAM NVIDIA tidak bisa dibaca. Tutup aplikasi GPU lain atau gunakan CPU secara manual."
            .to_string()
    })?;
    if available < spec.vram_required_mb {
        return Err(format!(
            "Model {} membutuhkan sekitar {:.1} GB VRAM bebas, tetapi yang tersedia hanya {:.1} GB. Pilih model yang lebih ringan atau tutup aplikasi GPU lain.",
            spec.label,
            spec.vram_required_mb as f64 / 1024.0,
            available as f64 / 1024.0,
        ));
    }
    Ok(())
}

fn verify_sha1(path: &Path, expected: &str) -> Result<(), String> {
    let mut file =
        File::open(path).map_err(|e| format!("Gagal membuka file untuk verifikasi: {e}"))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Gagal membaca file: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "Checksum model tidak cocok. Expected {expected}, actual {actual}. File dihapus demi keamanan."
        ));
    }
    Ok(())
}

fn emit_model_progress(
    app: &AppHandle,
    id: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    bytes_per_second: Option<u64>,
) {
    let percent = if total_bytes > 0 {
        downloaded_bytes as f64 / total_bytes as f64 * 100.0
    } else {
        0.0
    };
    let _ = app.emit(
        "model-download",
        ModelDownloadPayload {
            id: id.into(),
            downloaded_bytes,
            total_bytes,
            percent: percent.clamp(0.0, 100.0),
            bytes_per_second,
        },
    );
}

pub async fn download_model(
    app: AppHandle,
    model_id: String,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let spec = model_spec(&model_id)?;
    let dest = model_path(&app, &model_id)?;
    let temp = dest.with_extension("bin.download");
    let temp_guard = TemporaryFileGuard::new(temp.clone());
    if temp.exists() {
        tokio::fs::remove_file(&temp)
            .await
            .map_err(|e| format!("Gagal membersihkan download model lama: {e}"))?;
    }
    if dest.exists() {
        return Ok(());
    }
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin?download=true",
        spec.id
    );
    let client = AsyncClient::builder()
        .user_agent("WhisperTube/0.1")
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Gagal membuat HTTP client: {e}"))?;
    let mut response = tokio::time::timeout(Duration::from_secs(30), client.get(url).send())
        .await
        .map_err(|_| "Timeout saat menunggu response model.".to_string())?
        .map_err(|e| format!("Gagal mengunduh model: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Server model mengembalikan HTTP {}",
            response.status()
        ));
    }
    let total = response
        .content_length()
        .unwrap_or(spec.size_mb * 1024 * 1024);
    let mut file = tokio::fs::File::create(&temp)
        .await
        .map_err(|e| format!("Gagal membuat file model: {e}"))?;
    let mut downloaded = 0u64;
    let download_started_at = Instant::now();
    emit_model_progress(&app, spec.id, 0, total, None);
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Download model dibatalkan.".into());
        }
        let chunk = tokio::time::timeout(Duration::from_secs(30), response.chunk())
            .await
            .map_err(|_| "Timeout saat membaca data model selama 30 detik.".to_string())?
            .map_err(|e| format!("Download model terputus: {e}"))?;
        let Some(chunk) = chunk else {
            break;
        };
        if cancelled.load(Ordering::SeqCst) {
            return Err("Download model dibatalkan.".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Gagal menulis model: {e}"))?;
        downloaded += chunk.len() as u64;
        let elapsed = download_started_at.elapsed().as_secs_f64();
        let bytes_per_second = (elapsed > 0.0).then(|| (downloaded as f64 / elapsed) as u64);
        emit_model_progress(&app, spec.id, downloaded, total, bytes_per_second);
    }
    file.flush()
        .await
        .map_err(|e| format!("Gagal menyelesaikan file model: {e}"))?;
    drop(file);
    let temp_for_verify = temp.clone();
    let expected_sha1 = spec.sha1;
    tokio::task::spawn_blocking(move || verify_sha1(&temp_for_verify, expected_sha1))
        .await
        .map_err(|e| format!("Verifikasi model gagal: {e}"))??;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Download model dibatalkan.".into());
    }
    tokio::fs::rename(&temp, &dest)
        .await
        .map_err(|e| format!("Gagal memasang model: {e}"))?;
    temp_guard.commit();
    emit_model_progress(&app, spec.id, total, total, None);
    Ok(())
}

fn emit_cuda_progress(
    app: &AppHandle,
    percent: f64,
    downloaded_bytes: u64,
    total_bytes: u64,
    bytes_per_second: Option<u64>,
) {
    let _ = app.emit(
        "cuda-download",
        CudaDownloadPayload {
            downloaded_bytes,
            total_bytes,
            percent: percent.clamp(0.0, 100.0),
            bytes_per_second,
        },
    );
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| format!("Gagal membuka CUDA package: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Gagal membaca CUDA package: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(format!(
            "Checksum CUDA package tidak cocok. Expected {expected}, actual {actual}. File tidak dipasang."
        ));
    }
    Ok(())
}

fn extract_zip_safely(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file =
        File::open(archive_path).map_err(|e| format!("Gagal membuka archive CUDA: {e}"))?;
    let mut archive =
        ZipArchive::new(archive_file).map_err(|e| format!("Archive CUDA tidak valid: {e}"))?;
    fs::create_dir_all(destination)
        .map_err(|e| format!("Gagal membuat folder extract CUDA: {e}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Gagal membaca entry archive CUDA: {e}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "Archive CUDA memiliki path yang tidak aman.".to_string())?
            .to_path_buf();
        let target = destination.join(relative);
        if entry.name().ends_with('/') {
            fs::create_dir_all(&target).map_err(|e| format!("Gagal membuat folder CUDA: {e}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Gagal membuat folder CUDA: {e}"))?;
        }
        let mut output =
            File::create(&target).map_err(|e| format!("Gagal menulis file CUDA: {e}"))?;
        io::copy(&mut entry, &mut output).map_err(|e| format!("Gagal extract file CUDA: {e}"))?;
    }
    Ok(())
}

fn find_file(root: &Path, file_name: &str) -> Result<Option<PathBuf>, String> {
    for entry in fs::read_dir(root).map_err(|e| format!("Gagal membaca folder CUDA: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry CUDA: {e}"))?;
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
    fs::create_dir_all(destination).map_err(|e| format!("Gagal membuat staging CUDA: {e}"))?;
    for entry in fs::read_dir(source).map_err(|e| format!("Gagal membaca Release CUDA: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry Release CUDA: {e}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)
                .map_err(|e| format!("Gagal menyalin file CUDA: {e}"))?;
        }
    }
    Ok(())
}

async fn download_cuda_package(
    app: &AppHandle,
    zip_path: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Download CUDA dibatalkan.".into());
    }
    emit_cuda_progress(app, 0.0, 0, 0, None);
    let client = AsyncClient::builder()
        .user_agent("WhisperTube/0.1")
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Gagal membuat HTTP client CUDA: {e}"))?;
    let mut response = tokio::time::timeout(
        Duration::from_secs(30),
        client.get(cuda_engine_url()).send(),
    )
    .await
    .map_err(|_| "Timeout saat menunggu response CUDA.".to_string())?
    .map_err(|e| format!("Gagal mengunduh CUDA engine: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Server CUDA mengembalikan HTTP {}",
            response.status()
        ));
    }

    let total = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(zip_path)
        .await
        .map_err(|e| format!("Gagal membuat file download CUDA: {e}"))?;
    let mut downloaded = 0u64;
    let download_started_at = Instant::now();
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Download CUDA dibatalkan.".into());
        }
        let chunk = tokio::time::timeout(Duration::from_secs(30), response.chunk())
            .await
            .map_err(|_| "Timeout saat membaca data CUDA selama 30 detik.".to_string())?
            .map_err(|e| format!("Download CUDA terputus: {e}"))?;
        let Some(chunk) = chunk else {
            break;
        };
        if cancelled.load(Ordering::SeqCst) {
            return Err("Download CUDA dibatalkan.".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Gagal menyimpan CUDA package: {e}"))?;
        downloaded += chunk.len() as u64;
        let elapsed = download_started_at.elapsed().as_secs_f64();
        let bytes_per_second = (elapsed > 0.0).then(|| (downloaded as f64 / elapsed) as u64);
        let percent = if total > 0 {
            downloaded as f64 / total as f64 * 85.0
        } else {
            0.0
        };
        emit_cuda_progress(app, percent, downloaded, total, bytes_per_second);
    }
    file.flush()
        .await
        .map_err(|e| format!("Gagal menyelesaikan file CUDA: {e}"))?;
    Ok(())
}

fn finalize_cuda_engine_blocking(
    app: &AppHandle,
    cancelled: &AtomicBool,
    zip_path: &Path,
    extract_path: &Path,
    staging_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    verify_sha256(zip_path, CUDA_ENGINE_SHA256)?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Download CUDA dibatalkan.".into());
    }
    emit_cuda_progress(app, 88.0, 0, 0, None);
    extract_zip_safely(zip_path, extract_path)?;
    let cli = find_file(extract_path, "whisper-cli.exe")?
        .ok_or_else(|| "whisper-cli.exe tidak ditemukan dalam CUDA package.".to_string())?;
    let release_dir = cli
        .parent()
        .ok_or_else(|| "Folder CUDA tidak valid.".to_string())?;
    copy_tree(release_dir, staging_path)?;
    let staging_cli = staging_path.join("whisper-cli.exe");
    if !staging_cli.exists() {
        return Err("CUDA engine gagal dipasang ke staging.".into());
    }
    emit_cuda_progress(app, 94.0, 0, 0, None);
    let mut command = Command::new(&staging_cli);
    crate::process::hide_console(&mut command);
    let test = command
        .arg("--version")
        .output()
        .map_err(|e| format!("CUDA engine tidak bisa dijalankan: {e}"))?;
    if !test.status.success() {
        let error = String::from_utf8_lossy(&test.stderr).trim().to_string();
        return Err(if error.is_empty() {
            "CUDA engine gagal melakukan self-check.".into()
        } else {
            format!("CUDA engine gagal melakukan self-check: {error}")
        });
    }
    if cancelled.load(Ordering::SeqCst) {
        return Err("Download CUDA dibatalkan.".into());
    }
    if destination.exists() {
        return Err(
            "CUDA engine baru saja dipasang oleh proses lain. Klik Re-check components.".into(),
        );
    }
    fs::rename(staging_path, destination)
        .map_err(|e| format!("Gagal mengaktifkan CUDA engine: {e}"))?;
    emit_cuda_progress(app, 100.0, 0, 0, None);
    Ok(())
}

pub async fn install_cuda_engine(app: AppHandle, cancelled: Arc<AtomicBool>) -> Result<(), String> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err("Download CUDA otomatis saat ini hanya didukung di Windows.".into());
    }
    let runtime_root = user_runtime_dir(&app)?;
    let destination = runtime_root.join("cuda");
    if destination.join("whisper-cli.exe").exists() {
        return Ok(());
    }

    let temp_root = std::env::temp_dir().join(format!("whispertube-cuda-{}", Uuid::new_v4()));
    let zip_path = temp_root.join("whisper-cuda.zip");
    let extract_path = temp_root.join("extract");
    let staging_path = runtime_root.join(format!(".cuda-staging-{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_root)
        .map_err(|e| format!("Gagal membuat temporary CUDA folder: {e}"))?;

    let result = async {
        download_cuda_package(&app, &zip_path, &cancelled).await?;
        let app_for_finalize = app.clone();
        let cancelled_for_finalize = cancelled.clone();
        let zip_for_finalize = zip_path.clone();
        let extract_for_finalize = extract_path.clone();
        let staging_for_finalize = staging_path.clone();
        let destination_for_finalize = destination.clone();
        tokio::task::spawn_blocking(move || {
            finalize_cuda_engine_blocking(
                &app_for_finalize,
                &cancelled_for_finalize,
                &zip_for_finalize,
                &extract_for_finalize,
                &staging_for_finalize,
                &destination_for_finalize,
            )
        })
        .await
        .map_err(|e| format!("CUDA finalize task gagal: {e}"))?
    }
    .await;

    let _ = fs::remove_dir_all(&temp_root);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging_path);
    }
    result
}

pub fn delete_model(app: &AppHandle, model_id: &str) -> Result<(), String> {
    let path = model_path(app, model_id)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("Gagal menghapus model: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_vram_available, recommended_model_id, TemporaryFileGuard};
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    #[test]
    fn recommends_model_from_available_vram() {
        assert_eq!(recommended_model_id(None), "base");
        assert_eq!(recommended_model_id(Some(4096)), "large-v3-turbo-q5_0");
        assert_eq!(recommended_model_id(Some(7168)), "large-v3-q5_0");
    }

    #[test]
    fn rejects_model_when_free_vram_is_below_guardrail() {
        let error = ensure_vram_available("large-v3-q5_0", Some(6144)).unwrap_err();
        assert!(error.contains("Accurate"));
        assert!(ensure_vram_available("large-v3-q5_0", Some(7168)).is_ok());
    }

    #[test]
    fn temporary_model_file_is_removed_when_not_committed() {
        let path =
            std::env::temp_dir().join(format!("whispertube-model-test-{}.bin", Uuid::new_v4()));
        fs::write(&path, b"partial").unwrap();
        {
            let _guard = TemporaryFileGuard::new(PathBuf::from(&path));
        }
        assert!(!path.exists());
    }
}
