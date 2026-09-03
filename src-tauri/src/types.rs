use serde::{Deserialize, Serialize};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub yt_dlp: bool,
    pub ffmpeg: bool,
    pub cpu_engine: bool,
    pub cuda_engine: bool,
    pub nvidia: bool,
    pub gpu_name: Option<String>,
    pub cpu_threads: usize,
    pub recommendation: String,
    pub recommended_model_id: String,
    pub recommended_backend: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub size_mb: u64,
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
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadPayload {
    pub id: String,
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
    pub backend: String,
    pub language: String,
    pub model_id: String,
    pub keep_audio: bool,
}
