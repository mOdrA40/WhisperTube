use reqwest::blocking::Client;
use sha1::{Digest, Sha1};
use std::{
    fs,
    fs::File,
    io::{Read, Write},
    path::Path,
};
use tauri::{AppHandle, Emitter};

use crate::{
    paths::{model_path, models_dir},
    types::{ModelDownloadPayload, ModelInfo},
};

#[derive(Clone)]
pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub size_mb: u64,
    pub sha1: &'static str,
}

const MODELS: [ModelSpec; 3] = [
    ModelSpec {
        id: "base",
        label: "Fast",
        description: "Ringan untuk CPU/laptop sederhana",
        size_mb: 142,
        sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
    },
    ModelSpec {
        id: "large-v3-turbo-q5_0",
        label: "Balanced",
        description: "Default: cepat dan akurat untuk penggunaan umum",
        size_mb: 547,
        sha1: "e050f7970618a659205450ad97eb95a18d69c9ee",
    },
    ModelSpec {
        id: "large-v3-q5_0",
        label: "Accurate",
        description: "Akurasi tinggi, lebih berat dan lambat",
        size_mb: 1100,
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
            installed: dir.join(format!("ggml-{}.bin", model.id)).exists(),
        })
        .collect())
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

pub async fn download_model(app: AppHandle, model_id: String) -> Result<(), String> {
    let spec = model_spec(&model_id)?;
    let dest = model_path(&app, &model_id)?;
    let app_for_task = app.clone();
    tokio::task::spawn_blocking(move || {
        if dest.exists() {
            return Ok(());
        }
        let temp = dest.with_extension("bin.download");
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin?download=true",
            spec.id
        );
        let client = Client::builder()
            .user_agent("WhisperTube/0.1")
            .build()
            .map_err(|e| format!("Gagal membuat HTTP client: {e}"))?;
        let mut response = client
            .get(url)
            .send()
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
        let mut file = File::create(&temp).map_err(|e| format!("Gagal membuat file model: {e}"))?;
        let mut downloaded = 0u64;
        let mut buffer = [0u8; 1024 * 256];
        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|e| format!("Download model terputus: {e}"))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|e| format!("Gagal menulis model: {e}"))?;
            downloaded += read as u64;
            let percent = if total > 0 {
                downloaded as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let _ = app_for_task.emit(
                "model-download",
                ModelDownloadPayload {
                    id: spec.id.into(),
                    percent: percent.clamp(0.0, 100.0),
                },
            );
        }
        drop(file);
        if let Err(error) = verify_sha1(&temp, spec.sha1) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        fs::rename(&temp, &dest).map_err(|e| format!("Gagal memasang model: {e}"))?;
        let _ = app_for_task.emit(
            "model-download",
            ModelDownloadPayload {
                id: spec.id.into(),
                percent: 100.0,
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| format!("Download task gagal: {e}"))?
}

pub fn delete_model(app: &AppHandle, model_id: &str) -> Result<(), String> {
    let path = model_path(app, model_id)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("Gagal menghapus model: {e}"))?;
    }
    Ok(())
}
