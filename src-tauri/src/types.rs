use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub detail: Option<String>,
}

impl From<String> for ErrorPayload {
    fn from(raw: String) -> Self {
        const KNOWN_CODES: &[&str] = &[
            "media_tiktok_transient",
            "media_source_transient",
            "media_source_rate_limited",
            "media_source_membership_required",
            "media_source_access_required",
            "media_source_unavailable",
            "media_source_input",
            "media_source_duration",
            "media_source_duration_unknown",
            "media_source_blocked",
            "media_source_extractor",
            "media_source_cookie_file",
            "media_source_js_runtime",
            "media_source_runtime",
            "media_source_metadata",
            "operation_conflict",
            "storage_quota_exceeded",
        ];

        for code in KNOWN_CODES {
            if let Some(detail) = raw
                .strip_prefix(code)
                .and_then(|value| value.strip_prefix(':'))
            {
                return Self {
                    code: (*code).into(),
                    detail: non_empty_detail(detail),
                };
            }
        }

        Self {
            code: "backend_error".into(),
            detail: non_empty_detail(&raw),
        }
    }
}

impl From<&str> for ErrorPayload {
    fn from(raw: &str) -> Self {
        raw.to_string().into()
    }
}

fn non_empty_detail(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod error_tests {
    #[test]
    fn source_diagnostics_survive_the_ipc_boundary() {
        for code in [
            "media_source_duration_unknown",
            "media_source_blocked",
            "media_source_extractor",
        ] {
            let error = super::ErrorPayload::from(format!("{code}:safe detail"));
            assert_eq!(error.code, code);
            assert_eq!(error.detail.as_deref(), Some("safe detail"));
        }
    }
}

pub(crate) const MAX_TRANSCRIPT_RESULT_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_TRANSCRIPT_SEGMENTS: usize = 100_000;
pub(crate) const MAX_TRANSCRIPT_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorInfo {
    pub id: String,
    pub label: String,
    pub backend: String,
    pub supported: bool,
    pub installed: bool,
    pub downloadable: bool,
    pub memory_available_mb: Option<u64>,
    pub memory_required_mb: Option<u64>,
    pub memory_sufficient: bool,
    pub preflight_checked: bool,
    pub preflight_compatible: bool,
    pub description: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ComputeDeviceInfo {
    pub id: String,
    pub backend: String,
    pub name: String,
    pub vendor: String,
    pub device_index: Option<usize>,
    pub integrated: bool,
    pub total_memory_mb: Option<u64>,
    pub free_memory_mb: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub qwen_backends: Vec<&'static str>,
    pub yt_dlp: bool,
    pub ffmpeg: bool,
    pub cpu_engine: bool,
    pub cuda_engine: bool,
    pub cuda_supported: bool,
    pub nvidia: bool,
    pub gpu_name: Option<String>,
    pub gpu_memory_mb: Option<u64>,
    pub gpu_free_memory_mb: Option<u64>,
    pub available_memory_mb: u64,
    pub nvidia_driver_version: Option<String>,
    pub cuda_installable: bool,
    pub cpu_threads: usize,
    pub recommendation: String,
    pub recommended_model_id: String,
    pub recommended_backend: String,
    pub accelerators: Vec<AcceleratorInfo>,
    pub compute_devices: Vec<ComputeDeviceInfo>,
    pub job_storage_bytes: u64,
    pub job_storage_limit_bytes: u64,
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
    #[serde(default)]
    pub audio_recovered: bool,
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: f64,
    pub thumbnail: Option<String>,
    pub webpage_url: String,
    pub availability: Option<String>,
    pub source: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub stage: String,
    pub message_code: String,
    pub percent: f64,
    pub message: String,
    pub backend: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub network_bytes_per_second: Option<u64>,
    pub cpu_usage_percent: Option<f64>,
    pub gpu_usage_percent: Option<f64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadPayload {
    pub id: String,
    pub phase: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub bytes_per_second: Option<u64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CudaDownloadPayload {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub bytes_per_second: Option<u64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorDownloadPayload {
    pub backend: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub bytes_per_second: Option<u64>,
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
    #[serde(default)]
    pub audio_recovered: bool,
    pub history_id: i64,
    pub title: String,
    pub channel: String,
    pub language: String,
    pub duration: f64,
    // Present when source metadata materially differs from decoded audio.
    // Older saved results do not have this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_duration: Option<f64>,
    pub model: String,
    pub backend: String,
    pub segments: Vec<Segment>,
    pub text: String,
    pub txt_path: String,
    pub srt_path: String,
    pub vtt_path: String,
    #[serde(default)]
    pub audio_path: Option<String>,
    #[serde(default = "transcript_available_default")]
    pub transcript_available: bool,
}

fn transcript_available_default() -> bool {
    true
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryFileKind {
    Audio,
    Txt,
    Srt,
    Vtt,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryFileScope {
    Audio,
    Transcript,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFileInfo {
    pub kind: HistoryFileKind,
    pub path: String,
    pub bytes: Option<u64>,
    pub available: bool,
    pub pending_deletion: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFilesDeleteOutcome {
    pub updated_ids: Vec<i64>,
    pub cleanup_warnings: Vec<String>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPageResult {
    pub items: Vec<HistoryItem>,
    pub has_more: bool,
    pub total_count: i64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDeleteOutcome {
    pub deleted_ids: Vec<i64>,
    pub cleanup_warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCacheEntry {
    pub id: String,
    pub title: String,
    pub source_url: String,
    pub bytes: Option<u64>,
    pub modified_at: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionResumeStage {
    Download,
    Audio,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRequest {
    pub url: String,
    pub title: String,
    pub channel: String,
    pub duration: f64,
    #[serde(default)]
    pub cookies_path: Option<String>,
    pub backend: String,
    #[serde(default)]
    pub compute_device_id: Option<String>,
    pub language: String,
    pub model_id: String,
    pub keep_audio: bool,
}
