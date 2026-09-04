use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    pub id: String,
    pub label: String,
    pub profiles: Vec<BrowserProfile>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorInfo {
    pub id: String,
    pub label: String,
    pub backend: String,
    pub supported: bool,
    pub installed: bool,
    pub downloadable: bool,
    pub description: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub yt_dlp: bool,
    pub ffmpeg: bool,
    pub cpu_engine: bool,
    pub cuda_engine: bool,
    pub cuda_supported: bool,
    pub nvidia: bool,
    pub gpu_name: Option<String>,
    pub gpu_memory_mb: Option<u64>,
    pub gpu_free_memory_mb: Option<u64>,
    pub cpu_threads: usize,
    pub recommendation: String,
    pub recommended_model_id: String,
    pub recommended_backend: String,
    pub accelerators: Vec<AcceleratorInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub size_mb: u64,
    pub vram_required_mb: u64,
    pub installed: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: f64,
    pub thumbnail: Option<String>,
    pub webpage_url: String,
    pub availability: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub stage: String,
    pub percent: f64,
    pub message: String,
    pub backend: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadPayload {
    pub id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CudaDownloadPayload {
    pub percent: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorDownloadPayload {
    pub backend: String,
    pub percent: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub from: String,
    pub to: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptResult {
    pub history_id: i64,
    pub title: String,
    pub channel: String,
    pub language: String,
    pub duration: f64,
    pub model: String,
    pub backend: String,
    pub segments: Vec<Segment>,
    pub text: String,
    pub txt_path: String,
    pub srt_path: String,
    pub vtt_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: i64,
    pub title: String,
    pub channel: String,
    pub source_url: String,
    pub created_at: String,
    pub duration: f64,
    pub language: String,
    pub model: String,
    pub backend: String,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRequest {
    pub url: String,
    pub title: String,
    pub channel: String,
    pub duration: f64,
    pub browser: String,
    #[serde(default)]
    pub browser_profile: Option<String>,
    pub backend: String,
    pub language: String,
    pub model_id: String,
    pub keep_audio: bool,
}
