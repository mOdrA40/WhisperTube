use serde_json::Value;
use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;
use url::Url;

use crate::{
    cookies::args as cookie_args,
    paths::{is_regular_file, tool_path},
    transcription::MAX_MEDIA_DURATION_SECONDS,
    types::VideoMetadata,
};

#[derive(Clone, Copy)]
struct SourceDefinition {
    label: &'static str,
    hosts: &'static [&'static str],
}

const SOURCES: &[SourceDefinition] = &[
    SourceDefinition {
        label: "YouTube",
        hosts: &["youtube.com", "youtu.be"],
    },
    SourceDefinition {
        label: "TikTok",
        hosts: &["tiktok.com"],
    },
    SourceDefinition {
        label: "X / Twitter",
        hosts: &["x.com", "twitter.com"],
    },
    SourceDefinition {
        label: "Facebook",
        hosts: &["facebook.com", "fb.watch"],
    },
    SourceDefinition {
        label: "Instagram",
        hosts: &["instagram.com", "instagr.am"],
    },
    SourceDefinition {
        label: "Reddit",
        hosts: &["reddit.com", "redd.it"],
    },
    SourceDefinition {
        label: "Twitch",
        hosts: &["twitch.tv"],
    },
    SourceDefinition {
        label: "Vimeo",
        hosts: &["vimeo.com"],
    },
    SourceDefinition {
        label: "Dailymotion",
        hosts: &["dailymotion.com", "dai.ly"],
    },
    SourceDefinition {
        label: "Pinterest",
        hosts: &["pinterest.com", "pin.it"],
    },
    SourceDefinition {
        label: "LinkedIn",
        hosts: &["linkedin.com"],
    },
    SourceDefinition {
        label: "Tumblr",
        hosts: &["tumblr.com"],
    },
    SourceDefinition {
        label: "Bilibili",
        hosts: &["bilibili.com", "b23.tv"],
    },
    SourceDefinition {
        label: "VK",
        hosts: &["vk.com"],
    },
];

const METADATA_MAX_ATTEMPTS: usize = 3;
const METADATA_RETRY_BASE_DELAY: Duration = Duration::from_millis(900);
const METADATA_PROCESS_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_METADATA_STDOUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_METADATA_STDERR_BYTES: usize = 256 * 1024;
const METADATA_PRINT_TEMPLATE: &str = concat!(
    r#"{"id":%(id|"unknown")j,"title":%(title|"Untitled video")j,"#,
    r#""channel":%(channel|null)j,"uploader":%(uploader|null)j,"#,
    r#""duration":%(duration|0)j,"is_live":%(is_live|false)j,"#,
    r#""live_status":%(live_status|null)j,"thumbnail":%(thumbnail|null)j,"#,
    r#""webpage_url":%(webpage_url|null)j,"availability":%(availability|null)j,"#,
    r#""extractor_key":%(extractor_key|null)j,"extractor":%(extractor|null)j}"#,
);
const MAX_MEDIA_URL_BYTES: usize = 8 * 1024;
const MAX_METADATA_ID_BYTES: usize = 512;
const MAX_METADATA_TITLE_BYTES: usize = 4 * 1024;
const MAX_METADATA_CHANNEL_BYTES: usize = 2 * 1024;
const MAX_METADATA_SOURCE_BYTES: usize = 512;
const MAX_METADATA_AVAILABILITY_BYTES: usize = 512;
const MAX_METADATA_THUMBNAIL_URL_BYTES: usize = 4 * 1024;
const THUMBNAIL_HOSTS: &[&str] = &[
    "ytimg.com",
    "youtube.com",
    "googleusercontent.com",
    "tiktokcdn.com",
    "tiktok.com",
    "fbcdn.net",
    "facebook.com",
    "cdninstagram.com",
    "instagram.com",
    "twimg.com",
    "twitter.com",
    "x.com",
    "redditmedia.com",
    "redd.it",
    "reddit.com",
    "vimeocdn.com",
    "vimeo.com",
    "dmcdn.net",
    "dailymotion.com",
    "pinimg.com",
    "pinterest.com",
    "licdn.com",
    "linkedin.com",
    "tumblr.com",
    "bilibili.com",
    "b23.tv",
    "vk.com",
];
const TIKTOK_REHYDRATION_ERROR: &str = "unable to extract universal data for rehydration";
const TIKTOK_TRANSIENT_ERROR_PREFIX: &str = "media_tiktok_transient:";
const SOURCE_TRANSIENT_ERROR_PREFIX: &str = "media_source_transient:";
const SOURCE_RATE_LIMITED_ERROR_PREFIX: &str = "media_source_rate_limited:";
const SOURCE_MEMBERSHIP_ERROR_PREFIX: &str = "media_source_membership_required:";
const SOURCE_ACCESS_ERROR_PREFIX: &str = "media_source_access_required:";
const SOURCE_UNAVAILABLE_ERROR_PREFIX: &str = "media_source_unavailable:";
const SOURCE_INPUT_ERROR_PREFIX: &str = "media_source_input:";
const SOURCE_DURATION_ERROR_PREFIX: &str = "media_source_duration:";
const SOURCE_COOKIE_FILE_ERROR_PREFIX: &str = "media_source_cookie_file:";
const SOURCE_JS_RUNTIME_ERROR_PREFIX: &str = "media_source_js_runtime:";
const SOURCE_RUNTIME_ERROR_PREFIX: &str = "media_source_runtime:";
const SOURCE_METADATA_ERROR_PREFIX: &str = "media_source_metadata:";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MetadataRetryReason {
    TikTokRehydration,
    RateLimited,
    TransientNetwork,
}

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn source_for_host(host: &str) -> Option<SourceDefinition> {
    SOURCES
        .iter()
        .find(|source| source.hosts.iter().any(|domain| host_matches(host, domain)))
        .copied()
}

pub fn validate_media_url(raw: &str) -> Result<String, String> {
    if raw.len() > MAX_MEDIA_URL_BYTES {
        return Err("URL video terlalu panjang.".into());
    }
    let parsed = Url::parse(raw).map_err(|_| "URL tidak valid.".to_string())?;
    if parsed.scheme() != "https" {
        return Err("URL harus menggunakan HTTPS.".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL dengan kredensial tidak diperbolehkan.".into());
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if source_for_host(&host).is_none() {
        return Err("Domain video ini belum termasuk platform yang didukung.".into());
    }
    let safe_url = parsed.to_string();
    if safe_url.len() > MAX_MEDIA_URL_BYTES {
        return Err("URL video terlalu panjang.".into());
    }
    Ok(safe_url)
}

pub(crate) fn validate_media_duration(duration: f64, is_live: bool) -> Result<(), String> {
    if is_live || !duration.is_finite() || duration <= 0.0 {
        return Err("Live stream atau media tanpa durasi pasti belum didukung.".into());
    }
    if duration > MAX_MEDIA_DURATION_SECONDS {
        return Err(format!(
            "Durasi media melebihi batas {} jam.",
            (MAX_MEDIA_DURATION_SECONDS / 3600.0) as u64
        ));
    }
    Ok(())
}

fn source_label_from_metadata(value: &Value, safe_url: &str) -> Result<String, String> {
    let source = value
        .get("extractor_key")
        .or_else(|| value.get("extractor"))
        .and_then(Value::as_str)
        .map(|value| value.replace("IE", ""))
        .filter(|value| !value.is_empty())
        .or_else(|| {
            Url::parse(safe_url)
                .ok()
                .and_then(|url| url.host_str().and_then(source_for_host))
                .map(|source| source.label.to_string())
        })
        .unwrap_or_else(|| "Video".into());
    if source.len() > MAX_METADATA_SOURCE_BYTES {
        return Err(format!(
            "{SOURCE_METADATA_ERROR_PREFIX}Nama source metadata terlalu panjang."
        ));
    }
    Ok(source)
}

fn required_metadata_text(
    value: &Value,
    key: &str,
    fallback: &str,
    maximum: usize,
) -> Result<String, String> {
    let text = value.get(key).and_then(Value::as_str).unwrap_or(fallback);
    if text.len() > maximum {
        return Err(format!(
            "{SOURCE_METADATA_ERROR_PREFIX}Field metadata {key} terlalu panjang."
        ));
    }
    Ok(text.to_string())
}

fn optional_metadata_text(value: &Value, key: &str, maximum: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| text.len() <= maximum)
        .map(str::to_string)
}

fn safe_thumbnail(value: &Value) -> Option<String> {
    let thumbnail = value
        .get("thumbnail")
        .and_then(Value::as_str)
        .filter(|thumbnail| thumbnail.len() <= MAX_METADATA_THUMBNAIL_URL_BYTES)?;
    let parsed = Url::parse(thumbnail).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !THUMBNAIL_HOSTS
            .iter()
            .any(|domain| host_matches(&host, domain))
    {
        return None;
    }
    Some(parsed.to_string())
}

fn safe_webpage_url(value: &Value, safe_url: &str) -> String {
    let candidate = value.get("webpage_url").and_then(Value::as_str);
    candidate
        .filter(|candidate| candidate.len() <= MAX_MEDIA_URL_BYTES)
        .and_then(|candidate| validate_media_url(candidate).ok())
        .unwrap_or_else(|| safe_url.to_string())
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn drain_bounded(mut reader: impl Read, limit: usize) -> Result<(Vec<u8>, bool), String> {
    let mut captured = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("Gagal membaca output metadata: {error}")),
        };
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    Ok((captured, truncated))
}

struct ActiveProcessGuard {
    active_pid: Arc<Mutex<Option<u32>>>,
}

impl ActiveProcessGuard {
    fn new(active_pid: Arc<Mutex<Option<u32>>>, pid: u32) -> Self {
        if let Ok(mut active) = active_pid.lock() {
            *active = Some(pid);
        }
        Self { active_pid }
    }
}

impl Drop for ActiveProcessGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active_pid.lock() {
            *active = None;
        }
    }
}

fn run_output(
    mut command: Command,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<CapturedOutput, String> {
    crate::process::hide_console(&mut command);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Gagal menjalankan process metadata: {e}"))?;
    let _active_process = ActiveProcessGuard::new(Arc::clone(active_pid), child.id());
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            crate::process::terminate_child(&mut child);
            let _ = child.wait();
            return Err("Stdout metadata tidak tersedia.".into());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            crate::process::terminate_child(&mut child);
            let _ = child.wait();
            return Err("Stderr metadata tidak tersedia.".into());
        }
    };
    let stdout_reader =
        std::thread::spawn(move || drain_bounded(stdout, MAX_METADATA_STDOUT_BYTES));
    let stderr_reader =
        std::thread::spawn(move || drain_bounded(stderr, MAX_METADATA_STDERR_BYTES));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if cancelled.load(Ordering::SeqCst) => {
                crate::process::terminate_child(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err("Pemeriksaan metadata dibatalkan.".into());
            }
            Ok(None) if started.elapsed() >= METADATA_PROCESS_TIMEOUT => {
                crate::process::terminate_child(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err("Pemeriksaan metadata melewati batas waktu 45 detik.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                crate::process::terminate_child(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("Gagal memantau process metadata: {error}"));
            }
        }
    };
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| "Reader stdout metadata gagal.".to_string())??;
    let (mut stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| "Reader stderr metadata gagal.".to_string())??;
    if stdout_truncated {
        return Err("Output metadata terlalu besar untuk diproses dengan aman.".into());
    }
    if stderr_truncated {
        stderr.extend_from_slice(b"\n[stderr dipotong karena terlalu panjang]");
    }
    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
    })
}

pub fn js_runtime_args() -> Vec<String> {
    let mut command = Command::new("node");
    crate::process::hide_console(&mut command);
    let node_available = command
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if node_available {
        vec!["--js-runtimes".into(), "node".into()]
    } else {
        Vec::new()
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn is_membership_error(normalized: &str) -> bool {
    contains_any(
        normalized,
        &[
            "available to this channel's members",
            "members-only",
            "members only",
            "join this channel",
        ],
    )
}

fn is_access_error(normalized: &str) -> bool {
    contains_any(
        normalized,
        &[
            "sign in to confirm",
            "login required",
            "private video",
            "age-restricted",
            "age restricted",
            "http error 401",
            "http error 403",
        ],
    )
}

fn is_unavailable_error(normalized: &str) -> bool {
    contains_any(
        normalized,
        &[
            "video unavailable",
            "requested content is not available",
            "http error 404",
            "geo-restricted",
            "geoblocked",
            "drm",
            "no video formats",
        ],
    )
}

fn is_tiktok_url(safe_url: &str) -> bool {
    let is_tiktok = Url::parse(safe_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .and_then(|host| source_for_host(&host))
        .is_some_and(|source| source.label == "TikTok");

    is_tiktok
}

fn metadata_retry_reason(safe_url: &str, error: &str) -> Option<MetadataRetryReason> {
    let normalized = error.to_ascii_lowercase();
    if is_membership_error(&normalized)
        || is_access_error(&normalized)
        || is_unavailable_error(&normalized)
    {
        return None;
    }

    if is_tiktok_url(safe_url) && normalized.contains(TIKTOK_REHYDRATION_ERROR) {
        return Some(MetadataRetryReason::TikTokRehydration);
    }

    if normalized.contains("http error 429") {
        return Some(MetadataRetryReason::RateLimited);
    }

    let transient_http_error = [
        "http error 408",
        "http error 425",
        "http error 500",
        "http error 502",
        "http error 503",
        "http error 504",
        "http error 521",
        "http error 522",
        "http error 523",
        "http error 524",
    ];
    let transient_network_error = [
        "timed out",
        "timeout",
        "connection reset",
        "connection aborted",
        "connection refused",
        "temporary failure in name resolution",
        "network is unreachable",
        "incomplete read",
        "remote end closed connection",
    ];

    if transient_http_error
        .iter()
        .chain(transient_network_error.iter())
        .any(|needle| normalized.contains(needle))
    {
        Some(MetadataRetryReason::TransientNetwork)
    } else {
        None
    }
}

fn metadata_error_prefix(safe_url: &str, error: &str) -> &'static str {
    let normalized = error.to_ascii_lowercase();
    if is_membership_error(&normalized) {
        return SOURCE_MEMBERSHIP_ERROR_PREFIX;
    }
    if normalized.contains("no supported javascript runtime")
        || normalized.contains("the page needs to be reloaded")
    {
        return SOURCE_JS_RUNTIME_ERROR_PREFIX;
    }
    if is_access_error(&normalized) {
        return SOURCE_ACCESS_ERROR_PREFIX;
    }
    if is_unavailable_error(&normalized) {
        return SOURCE_UNAVAILABLE_ERROR_PREFIX;
    }
    match metadata_retry_reason(safe_url, error) {
        Some(MetadataRetryReason::TikTokRehydration) => TIKTOK_TRANSIENT_ERROR_PREFIX,
        Some(MetadataRetryReason::RateLimited) => SOURCE_RATE_LIMITED_ERROR_PREFIX,
        Some(MetadataRetryReason::TransientNetwork) => SOURCE_TRANSIENT_ERROR_PREFIX,
        None => "",
    }
}

pub async fn inspect_media(
    app: AppHandle,
    url: String,
    cookies_path: Option<String>,
    active_pid: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
) -> Result<VideoMetadata, String> {
    let safe_url =
        validate_media_url(&url).map_err(|error| format!("{SOURCE_INPUT_ERROR_PREFIX}{error}"))?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    if !is_regular_file(&yt_dlp) {
        return Err(format!(
            "{SOURCE_RUNTIME_ERROR_PREFIX}yt-dlp belum terpasang. Jalankan scripts/setup-windows.ps1."
        ));
    }
    tokio::task::spawn_blocking(move || {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Pemeriksaan metadata dibatalkan.".into());
        }
        let cookie_args = cookie_args(cookies_path.as_deref())
            .map_err(|error| format!("{SOURCE_COOKIE_FILE_ERROR_PREFIX}{error}"))?;
        let output = {
            let mut attempt = 0;
            loop {
                attempt += 1;
                let mut command = Command::new(&yt_dlp);
                command.args([
                    "--ignore-config",
                    "--print",
                    METADATA_PRINT_TEMPLATE,
                    "--skip-download",
                    "--no-playlist",
                    "--no-warnings",
                ]);
                command.args(js_runtime_args());
                command.args(&cookie_args);
                command.arg(&safe_url);
                let output = run_output(command, &active_pid, &cancelled)?;

                if output.status.success() {
                    break output;
                }

                let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if metadata_retry_reason(&safe_url, &error).is_none()
                    || attempt >= METADATA_MAX_ATTEMPTS
                {
                    break output;
                }

                if cancelled.load(Ordering::SeqCst) {
                    return Err("Pemeriksaan metadata dibatalkan.".into());
                }
                std::thread::sleep(METADATA_RETRY_BASE_DELAY * attempt as u32);
            }
        };

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let error_prefix = metadata_error_prefix(&safe_url, &error);
            return Err(if error.is_empty() {
                "Metadata video gagal dibaca. Untuk video yang memerlukan login, import cookies.txt yang masih baru.".into()
            } else {
                format!("{error_prefix}yt-dlp: {error}")
            });
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("{SOURCE_METADATA_ERROR_PREFIX}Output metadata yt-dlp tidak valid: {e}"))?;
        let duration = value.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
        let is_live = value.get("is_live").and_then(Value::as_bool).unwrap_or(false)
            || matches!(value.get("live_status").and_then(Value::as_str), Some("is_live" | "is_upcoming"));
        validate_media_duration(duration, is_live)
            .map_err(|error| format!("{SOURCE_DURATION_ERROR_PREFIX}{error}"))?;
        let title = required_metadata_text(
            &value,
            "title",
            "Untitled video",
            MAX_METADATA_TITLE_BYTES,
        )?;
        let channel = value
            .get("channel")
            .or_else(|| value.get("uploader"))
            .and_then(Value::as_str)
            .unwrap_or("Unknown channel");
        if channel.len() > MAX_METADATA_CHANNEL_BYTES {
            return Err(format!(
                "{SOURCE_METADATA_ERROR_PREFIX}Field metadata channel terlalu panjang."
            ));
        }
        Ok(VideoMetadata {
            id: required_metadata_text(&value, "id", "unknown", MAX_METADATA_ID_BYTES)?,
            title,
            channel: channel.to_string(),
            duration,
            thumbnail: safe_thumbnail(&value),
            webpage_url: safe_webpage_url(&value, &safe_url),
            availability: optional_metadata_text(
                &value,
                "availability",
                MAX_METADATA_AVAILABILITY_BYTES,
            ),
            source: source_label_from_metadata(&value, &safe_url)?,
        })
    })
    .await
    .map_err(|e| format!("Metadata task gagal: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::{
        drain_bounded, metadata_error_prefix, metadata_retry_reason, safe_thumbnail,
        source_for_host, validate_media_duration, validate_media_url, MetadataRetryReason,
        METADATA_PRINT_TEMPLATE,
    };
    use serde_json::{json, Value};
    use std::io::Cursor;

    #[test]
    fn accepts_supported_social_video_hosts() {
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://www.tiktok.com/@user/video/123",
            "https://x.com/user/status/123",
            "https://www.facebook.com/reel/123",
            "https://www.instagram.com/reel/123/",
            "https://www.reddit.com/r/test/comments/abc/video/",
        ] {
            assert!(
                validate_media_url(url).is_ok(),
                "expected {url} to be accepted"
            );
        }
    }

    #[test]
    fn rejects_plaintext_http_urls() {
        assert!(validate_media_url("http://www.youtube.com/watch?v=abc").is_err());
    }

    #[test]
    fn rejects_overlong_video_urls() {
        let url = format!("https://youtube.com/watch?v={}", "x".repeat(9 * 1024));
        assert!(validate_media_url(&url).is_err());
    }

    #[test]
    fn rejects_live_unknown_and_excessively_long_media() {
        assert!(validate_media_duration(60.0, false).is_ok());
        assert!(validate_media_duration(0.0, false).is_err());
        assert!(validate_media_duration(f64::NAN, false).is_err());
        assert!(validate_media_duration(60.0, true).is_err());
        assert!(validate_media_duration(8.0 * 60.0 * 60.0, false).is_ok());
        assert!(validate_media_duration(8.0 * 60.0 * 60.0 + 1.0, false).is_err());
    }

    #[test]
    fn bounded_metadata_reader_drains_but_caps_retained_bytes() {
        let input = vec![b'x'; 1024];
        let (captured, truncated) = drain_bounded(Cursor::new(input), 128).unwrap();
        assert_eq!(captured.len(), 128);
        assert!(truncated);
    }

    #[test]
    fn metadata_print_template_requests_only_bounded_fields() {
        for field in [
            "id",
            "title",
            "channel",
            "uploader",
            "duration",
            "is_live",
            "live_status",
            "thumbnail",
            "webpage_url",
            "availability",
            "extractor_key",
            "extractor",
        ] {
            assert!(
                METADATA_PRINT_TEMPLATE.contains(&format!("\"{field}\"")),
                "metadata template should contain field {field}"
            );
        }
        for placeholder in [
            r#"%(id|"unknown")j"#,
            r#"%(title|"Untitled video")j"#,
            "%(channel|null)j",
            "%(uploader|null)j",
            "%(duration|0)j",
            "%(is_live|false)j",
            "%(live_status|null)j",
            "%(thumbnail|null)j",
            "%(webpage_url|null)j",
            "%(availability|null)j",
            "%(extractor_key|null)j",
            "%(extractor|null)j",
        ] {
            assert!(
                METADATA_PRINT_TEMPLATE.contains(placeholder),
                "metadata template should contain placeholder {placeholder}"
            );
        }
        assert!(METADATA_PRINT_TEMPLATE.len() < 1024);
        assert!(!METADATA_PRINT_TEMPLATE.contains("formats"));
        assert!(!METADATA_PRINT_TEMPLATE.contains("automatic_captions"));
    }

    #[test]
    fn metadata_print_template_renders_valid_json_with_optional_defaults() {
        let mut rendered = METADATA_PRINT_TEMPLATE.to_string();
        for (placeholder, value) in [
            (r#"%(id|"unknown")j"#, r#""video-id""#),
            (r#"%(title|"Untitled video")j"#, r#""Video title""#),
            ("%(channel|null)j", "null"),
            ("%(uploader|null)j", r#""Uploader""#),
            ("%(duration|0)j", "3600"),
            ("%(is_live|false)j", "false"),
            ("%(live_status|null)j", "null"),
            ("%(thumbnail|null)j", "null"),
            ("%(webpage_url|null)j", r#""https://example.test/video""#),
            ("%(availability|null)j", "null"),
            ("%(extractor_key|null)j", r#""Test""#),
            ("%(extractor|null)j", r#""test""#),
        ] {
            rendered = rendered.replace(placeholder, value);
        }

        let parsed: Value = serde_json::from_str(&rendered)
            .expect("metadata print template should render valid JSON");
        assert_eq!(parsed["id"], "video-id");
        assert_eq!(parsed["channel"], Value::Null);
        assert_eq!(parsed["uploader"], "Uploader");
        assert_eq!(parsed["duration"], 3600);
        assert_eq!(parsed["is_live"], false);
    }

    #[test]
    fn rejects_lookalike_or_unsupported_hosts() {
        assert!(validate_media_url("https://notyoutube.com/video").is_err());
        assert!(validate_media_url("https://youtube.com.evil.example/video").is_err());
        assert!(validate_media_url("https://example.com/video").is_err());
        assert!(source_for_host("cdn.tiktok.com").is_some());
    }

    #[test]
    fn thumbnail_policy_allows_platform_cdn_and_rejects_arbitrary_https_hosts() {
        assert!(safe_thumbnail(&json!({
            "thumbnail": "https://i.ytimg.com/vi/video-id/maxresdefault.jpg"
        }))
        .is_some());
        assert!(safe_thumbnail(&json!({
            "thumbnail": "https://tracking.example.test/thumb.jpg"
        }))
        .is_none());
    }

    #[test]
    fn retries_tiktok_rehydration_errors() {
        let error = "ERROR: [TikTok] 123: Unable to extract universal data for rehydration";
        assert_eq!(
            metadata_retry_reason("https://www.tiktok.com/@user/video/123", error),
            Some(MetadataRetryReason::TikTokRehydration)
        );
        assert_ne!(
            metadata_retry_reason("https://www.youtube.com/watch?v=123", error),
            Some(MetadataRetryReason::TikTokRehydration)
        );
        assert_eq!(
            metadata_retry_reason(
                "https://www.tiktok.com/@user/video/123",
                "ERROR: unable to download video"
            ),
            None
        );
    }

    #[test]
    fn retries_transient_network_errors_across_supported_sources() {
        for url in [
            "https://www.youtube.com/watch?v=123",
            "https://www.instagram.com/reel/123/",
            "https://x.com/user/status/123",
        ] {
            assert_eq!(
                metadata_retry_reason(url, "HTTP Error 429: Too Many Requests"),
                Some(MetadataRetryReason::RateLimited),
                "expected {url} to retry rate limits"
            );
        }
        assert_eq!(
            metadata_retry_reason(
                "https://www.youtube.com/watch?v=123",
                "ERROR: Sign in to confirm you're not a bot"
            ),
            None
        );
        assert_eq!(
            metadata_retry_reason(
                "https://www.instagram.com/reel/123/",
                "HTTP Error 429: Requested content is not available, rate-limit reached or login required"
            ),
            None
        );
        assert_eq!(
            metadata_retry_reason(
                "https://www.youtube.com/watch?v=123",
                "ERROR: HTTP Error 404: Not Found"
            ),
            None
        );
        assert_eq!(
            metadata_error_prefix(
                "https://www.youtube.com/watch?v=123",
                "HTTP Error 429: Too Many Requests"
            ),
            super::SOURCE_RATE_LIMITED_ERROR_PREFIX
        );
        assert_eq!(
            metadata_error_prefix(
                "https://www.instagram.com/reel/123/",
                "ERROR: login required"
            ),
            super::SOURCE_ACCESS_ERROR_PREFIX
        );
        assert_eq!(
            metadata_error_prefix(
                "https://x.com/user/status/123",
                "ERROR: HTTP Error 404: Not Found"
            ),
            super::SOURCE_UNAVAILABLE_ERROR_PREFIX
        );
        assert_eq!(
            metadata_error_prefix(
                "https://www.youtube.com/watch?v=123",
                "This video is available to this channel's members on level: VIP"
            ),
            super::SOURCE_MEMBERSHIP_ERROR_PREFIX
        );
    }
}
