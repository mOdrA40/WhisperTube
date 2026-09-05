use serde_json::Value;
use std::process::{Command, Stdio};
use std::{sync::OnceLock, time::Duration};
use tauri::AppHandle;
use url::Url;

use crate::{browsers::cookie_args, paths::tool_path, types::VideoMetadata};

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
const TIKTOK_REHYDRATION_ERROR: &str = "unable to extract universal data for rehydration";
const TIKTOK_TRANSIENT_ERROR_PREFIX: &str = "media_tiktok_transient:";
const SOURCE_TRANSIENT_ERROR_PREFIX: &str = "media_source_transient:";
const SOURCE_RATE_LIMITED_ERROR_PREFIX: &str = "media_source_rate_limited:";
const SOURCE_MEMBERSHIP_ERROR_PREFIX: &str = "media_source_membership_required:";
const SOURCE_ACCESS_ERROR_PREFIX: &str = "media_source_access_required:";
const SOURCE_UNAVAILABLE_ERROR_PREFIX: &str = "media_source_unavailable:";
const SOURCE_INPUT_ERROR_PREFIX: &str = "media_source_input:";
const SOURCE_BROWSER_ERROR_PREFIX: &str = "media_source_browser:";
const SOURCE_BROWSER_DECRYPTION_ERROR_PREFIX: &str = "media_source_browser_decryption:";
const SOURCE_COOKIE_FILE_ERROR_PREFIX: &str = "media_source_cookie_file:";
const SOURCE_JS_RUNTIME_ERROR_PREFIX: &str = "media_source_js_runtime:";
const SOURCE_RUNTIME_ERROR_PREFIX: &str = "media_source_runtime:";
const SOURCE_METADATA_ERROR_PREFIX: &str = "media_source_metadata:";
static NODE_AVAILABLE: OnceLock<bool> = OnceLock::new();

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
    let parsed = Url::parse(raw).map_err(|_| "URL tidak valid.".to_string())?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("URL harus menggunakan http/https.".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL dengan kredensial tidak diperbolehkan.".into());
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if source_for_host(&host).is_none() {
        return Err("Domain video ini belum termasuk platform yang didukung.".into());
    }
    Ok(parsed.to_string())
}

fn source_label_from_metadata(value: &Value, safe_url: &str) -> String {
    value
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
        .unwrap_or_else(|| "Video".into())
}

fn run_output(mut command: Command) -> Result<std::process::Output, String> {
    crate::process::hide_console(&mut command);
    command
        .output()
        .map_err(|e| format!("Gagal menjalankan process: {e}"))
}

pub fn js_runtime_args() -> Vec<String> {
    let node_available = NODE_AVAILABLE.get_or_init(|| {
        let mut command = Command::new("node");
        crate::process::hide_console(&mut command);
        command
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    if *node_available {
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

fn is_browser_cookie_error(normalized: &str) -> bool {
    (normalized.contains("cookie database")
        && (normalized.contains("could not copy") || normalized.contains("permission denied")))
        || normalized.contains("could not extract cookies from")
}

fn is_browser_cookie_decryption_error(normalized: &str) -> bool {
    normalized.contains("failed to decrypt with dpapi")
        || (normalized.contains("dpapi") && normalized.contains("decrypt"))
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
        || is_browser_cookie_error(&normalized)
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
    if is_browser_cookie_decryption_error(&normalized) {
        return SOURCE_BROWSER_DECRYPTION_ERROR_PREFIX;
    }
    if is_browser_cookie_error(&normalized) {
        return SOURCE_BROWSER_ERROR_PREFIX;
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
    browser: String,
    profile: Option<String>,
    cookies_path: Option<String>,
) -> Result<VideoMetadata, String> {
    let safe_url =
        validate_media_url(&url).map_err(|error| format!("{SOURCE_INPUT_ERROR_PREFIX}{error}"))?;
    let yt_dlp = tool_path(&app, "yt-dlp")?;
    if !yt_dlp.exists() {
        return Err(format!(
            "{SOURCE_RUNTIME_ERROR_PREFIX}yt-dlp belum terpasang. Jalankan scripts/setup-windows.ps1."
        ));
    }
    tokio::task::spawn_blocking(move || {
        let cookie_args = cookie_args(&browser, profile.as_deref(), cookies_path.as_deref())
            .map_err(|error| {
                if cookies_path.is_some() {
                    format!("{SOURCE_COOKIE_FILE_ERROR_PREFIX}{error}")
                } else {
                    format!("{SOURCE_BROWSER_ERROR_PREFIX}{error}")
                }
            })?;
        let output = {
            let mut attempt = 0;
            loop {
                attempt += 1;
                let mut command = Command::new(&yt_dlp);
                command.args([
                    "--ignore-config",
                    "--dump-single-json",
                    "--skip-download",
                    "--no-playlist",
                    "--no-warnings",
                ]);
                command.args(js_runtime_args());
                command.args(&cookie_args);
                command.arg(&safe_url);
                let output = run_output(command)?;

                if output.status.success() {
                    break output;
                }

                let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if metadata_retry_reason(&safe_url, &error).is_none()
                    || attempt >= METADATA_MAX_ATTEMPTS
                {
                    break output;
                }

                std::thread::sleep(METADATA_RETRY_BASE_DELAY * attempt as u32);
            }
        };

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let error_prefix = metadata_error_prefix(&safe_url, &error);
            return Err(if error.is_empty() {
                "Metadata video gagal dibaca. Untuk video yang memerlukan login, pilih browser yang sudah login.".into()
            } else {
                format!("{error_prefix}yt-dlp: {error}")
            });
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("{SOURCE_METADATA_ERROR_PREFIX}Output metadata yt-dlp tidak valid: {e}"))?;
        Ok(VideoMetadata {
            id: value.get("id").and_then(Value::as_str).unwrap_or("unknown").into(),
            title: value.get("title").and_then(Value::as_str).unwrap_or("Untitled video").into(),
            channel: value
                .get("channel")
                .or_else(|| value.get("uploader"))
                .and_then(Value::as_str)
                .unwrap_or("Unknown channel")
                .into(),
            duration: value.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
            thumbnail: value.get("thumbnail").and_then(Value::as_str).map(str::to_string),
            webpage_url: value
                .get("webpage_url")
                .and_then(Value::as_str)
                .unwrap_or(&safe_url)
                .to_string(),
            availability: value.get("availability").and_then(Value::as_str).map(str::to_string),
            source: source_label_from_metadata(&value, &safe_url),
        })
    })
    .await
    .map_err(|e| format!("Metadata task gagal: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::{
        metadata_error_prefix, metadata_retry_reason, source_for_host, validate_media_url,
        MetadataRetryReason,
    };

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
    fn rejects_lookalike_or_unsupported_hosts() {
        assert!(validate_media_url("https://notyoutube.com/video").is_err());
        assert!(validate_media_url("https://youtube.com.evil.example/video").is_err());
        assert!(validate_media_url("https://example.com/video").is_err());
        assert!(source_for_host("cdn.tiktok.com").is_some());
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
        assert_eq!(
            metadata_error_prefix(
                "https://www.youtube.com/watch?v=123",
                "ERROR: Could not copy Chrome cookie database. Permission denied"
            ),
            super::SOURCE_BROWSER_ERROR_PREFIX
        );
        assert_eq!(
            metadata_error_prefix(
                "https://www.youtube.com/watch?v=123",
                "ERROR: Failed to decrypt with DPAPI"
            ),
            super::SOURCE_BROWSER_DECRYPTION_ERROR_PREFIX
        );
        assert_eq!(
            metadata_error_prefix(
                "https://www.youtube.com/watch?v=123",
                "ERROR: Failed to decrypt with DPAPI"
            ),
            super::SOURCE_BROWSER_DECRYPTION_ERROR_PREFIX
        );
    }
}
