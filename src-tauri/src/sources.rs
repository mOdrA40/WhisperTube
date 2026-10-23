use serde_json::Value;
use std::process::{Command, ExitStatus};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, SystemTime},
};
use tauri::AppHandle;
use url::Url;

use crate::{
    cookies::CookieJar,
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

fn metadata_channel(value: &Value) -> Result<&str, String> {
    let channel = ["channel", "uploader"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .find(|text| !text.trim().is_empty())
        .unwrap_or("Unknown channel");
    if channel.len() > MAX_METADATA_CHANNEL_BYTES {
        return Err(format!(
            "{SOURCE_METADATA_ERROR_PREFIX}Field metadata channel terlalu panjang."
        ));
    }
    Ok(channel)
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

fn run_output(
    mut command: Command,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<CapturedOutput, String> {
    let mut stdout = Vec::with_capacity(64 * 1024);
    let mut stderr = Vec::with_capacity(64 * 1024);
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;
    let mut active = None;
    let status = crate::process::run_piped(
        &mut command,
        Some(METADATA_PROCESS_TIMEOUT),
        Some(cancelled),
        |pid| {
            active = Some(crate::process::ActiveProcessGuard::new(active_pid, pid)?);
            Ok(())
        },
        |event| {
            use crate::process::{OutputEvent, OutputStream};
            if let OutputEvent::Data(stream, bytes) = event {
                let (captured, limit, truncated) = match stream {
                    OutputStream::Stdout => (
                        &mut stdout,
                        MAX_METADATA_STDOUT_BYTES,
                        &mut stdout_truncated,
                    ),
                    OutputStream::Stderr => (
                        &mut stderr,
                        MAX_METADATA_STDERR_BYTES,
                        &mut stderr_truncated,
                    ),
                };
                let retained = bytes.len().min(limit.saturating_sub(captured.len()));
                captured.extend_from_slice(&bytes[..retained]);
                *truncated |= retained < bytes.len();
            }
            Ok(())
        },
    )?;
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

#[derive(Clone, PartialEq, Eq)]
struct NodeIdentity {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    permissions: std::fs::Permissions,
}

static NODE_PROBE: OnceLock<Mutex<Option<NodeIdentity>>> = OnceLock::new();

fn probe_node_cached(
    cache: &Mutex<Option<NodeIdentity>>,
    program: &Path,
    cancelled: &AtomicBool,
    probe: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    crate::io_cancel::check(cancelled)?;
    let metadata = std::fs::metadata(program)
        .map_err(|e| format!("Runtime Node.js tidak dapat diperiksa: {e}"))?;
    let identity = NodeIdentity {
        path: program.into(),
        size: metadata.len(),
        modified: metadata.modified().ok(),
        permissions: metadata.permissions(),
    };
    if identity.modified.is_some()
        && cache
            .lock()
            .map_err(|_| "Cache runtime Node.js terkunci.")?
            .as_ref()
            == Some(&identity)
    {
        return Ok(true);
    }
    let available = probe()?;
    crate::io_cancel::check(cancelled)?;
    // Never cache failures: installing/fixing Node must work on the next try.
    *cache
        .lock()
        .map_err(|_| "Cache runtime Node.js terkunci.")? = available.then_some(identity);
    Ok(available)
}

fn node_program() -> Option<PathBuf> {
    let mut directories = Vec::new();
    #[cfg(windows)]
    if let Some(parent) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        directories.push(parent);
    }
    if let Some(path) = std::env::var_os("PATH") {
        directories.extend(std::env::split_paths(&path));
    }
    node_program_in(directories)
}

fn node_program_in(directories: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let name = crate::paths::exe_name("node");
    directories
        .into_iter()
        // PATH entries may be symlinks (Homebrew and version managers). Resolve
        // each candidate before validating it, preserving executable precedence.
        .filter_map(|directory| std::fs::canonicalize(directory.join(&name)).ok())
        .find(|candidate| is_regular_file(candidate))
}

pub fn js_runtime_args(
    active_pid: &Mutex<Option<u32>>,
    cancelled: &AtomicBool,
) -> Result<Vec<String>, String> {
    crate::io_cancel::check(cancelled)?;
    let Some(program) = node_program() else {
        // Preserve the platform's existing executable lookup when it resolves
        // a runtime outside PATH. Without an absolute identity, do not cache it.
        let mut command = Command::new("node");
        configure_node_version_probe(&mut command);
        return Ok(
            if crate::process::probe_optional_runtime(
                &mut command,
                Duration::from_secs(3),
                active_pid,
                cancelled,
            )? {
                vec!["--js-runtimes".into(), "node".into()]
            } else {
                Vec::new()
            },
        );
    };
    let node_available = probe_node_cached(
        NODE_PROBE.get_or_init(|| Mutex::new(None)),
        &program,
        cancelled,
        || {
            let mut command = Command::new(&program);
            configure_node_version_probe(&mut command);
            crate::process::probe_optional_runtime(
                &mut command,
                Duration::from_secs(3),
                active_pid,
                cancelled,
            )
        },
    )?;
    Ok(if node_available {
        // Pin yt-dlp to the binary we probed. For a non-Unicode PATH entry,
        // preserve its original own PATH lookup because the argument is UTF-8.
        vec![
            "--js-runtimes".into(),
            program
                .to_str()
                .map(|path| format!("node:{path}"))
                .unwrap_or_else(|| "node".into()),
        ]
    } else {
        Vec::new()
    })
}

const NODE_VERSION_CHECK: &str =
    "process.exit(Number(process.versions.node.split('.')[0]) >= 22 ? 0 : 1)";
const DENO_VERSION_CHECK: &str = "const [major,minor]=Deno.version.deno.split('.').map(Number); Deno.exit(major>2||(major===2&&minor>=3)?0:1)";

fn configure_node_version_probe(command: &mut Command) {
    command.args(["-e", NODE_VERSION_CHECK]);
}

/// EJS scripts ship in yt-dlp, but YouTube still needs a supported JS runtime.
/// Other platforms remain usable without this external prerequisite.
pub fn source_js_runtime_args(
    url: &str,
    active_pid: &Mutex<Option<u32>>,
    cancelled: &AtomicBool,
) -> Result<Vec<String>, String> {
    let args = js_runtime_args(active_pid, cancelled)?;
    if !args.is_empty() || !is_youtube_source(url) {
        return Ok(args);
    }
    let mut deno = Command::new("deno");
    deno.args(["eval", DENO_VERSION_CHECK]);
    if crate::process::probe_optional_runtime(
        &mut deno,
        Duration::from_secs(3),
        active_pid,
        cancelled,
    )? {
        return Ok(vec!["--js-runtimes".into(), "deno".into()]);
    }
    require_source_js_runtime(url, args)
}

fn is_youtube_source(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| host_matches(&host, "youtube.com") || host_matches(&host, "youtu.be"))
}

fn require_source_js_runtime(url: &str, args: Vec<String>) -> Result<Vec<String>, String> {
    if is_youtube_source(url) && args.is_empty() {
        Err(format!("{SOURCE_JS_RUNTIME_ERROR_PREFIX}YouTube memerlukan Node.js 22+ atau Deno 2.3+ pada PATH. Runtime JavaScript tidak dibundel; pasang runtime lalu mulai ulang WhisperTube."))
    } else {
        Ok(args)
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
        let cookie_jar = CookieJar::prepare(&app, cookies_path.as_deref())
            .map_err(|error| format!("{SOURCE_COOKIE_FILE_ERROR_PREFIX}{error}"))?;
        let runtime_args = source_js_runtime_args(&safe_url, &active_pid, &cancelled)?;
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
                command.args(&runtime_args);
                command.args(cookie_jar.args());
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
        let channel = metadata_channel(&value)?;
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
    #[test]
    fn youtube_requires_js_but_other_sources_do_not() {
        for url in [
            "https://youtube.com/watch?v=x",
            "https://www.youtube.com/watch?v=x",
            "https://youtu.be/x",
        ] {
            let error = super::require_source_js_runtime(url, vec![]).unwrap_err();
            assert!(error.starts_with("media_source_js_runtime:"));
            assert!(error.contains("Node.js 22+"));
            assert!(error.contains("Deno 2.3+"));
            let args = vec!["--js-runtimes".into(), "node:/runtime/node".into()];
            assert_eq!(
                super::require_source_js_runtime(url, args.clone()).unwrap(),
                args
            );
        }
        for url in [
            "https://vimeo.com/123",
            "https://www.tiktok.com/video/123",
            "https://notyoutube.com/123",
        ] {
            assert!(super::require_source_js_runtime(url, vec![])
                .unwrap()
                .is_empty());
        }
    }
    #[cfg(unix)]
    #[test]
    fn node_lookup_preserves_path_precedence_for_symlinks_and_skips_broken_links() {
        use std::{fs, os::unix::fs::symlink};
        let root =
            std::env::temp_dir().join(format!("whispertube-node-path-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let managed = root.join("managed-node");
        fs::write(&managed, b"managed runtime").unwrap();
        fs::write(second.join("node"), b"system runtime").unwrap();
        symlink(&managed, first.join("node")).unwrap();
        assert_eq!(
            super::node_program_in([first.clone(), second.clone()]),
            Some(fs::canonicalize(&managed).unwrap())
        );
        fs::remove_file(&managed).unwrap();
        assert_eq!(
            super::node_program_in([first, second.clone()]),
            Some(fs::canonicalize(second.join("node")).unwrap())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn node_probe_reuses_success_but_retries_failures_and_changed_binaries() {
        use std::{
            cell::Cell,
            fs,
            sync::{atomic::AtomicBool, Mutex},
        };
        let program =
            std::env::temp_dir().join(format!("whispertube-node-cache-{}", uuid::Uuid::new_v4()));
        fs::write(&program, b"node fixture").unwrap();
        let cache = Mutex::new(None);
        let cancelled = AtomicBool::new(false);
        let calls = Cell::new(0);
        let probe = || {
            calls.set(calls.get() + 1);
            Ok(true)
        };
        assert!(super::probe_node_cached(&cache, &program, &cancelled, probe).unwrap());
        assert!(super::probe_node_cached(&cache, &program, &cancelled, probe).unwrap());
        assert_eq!(calls.get(), 1);
        fs::write(&program, b"new node fixture version").unwrap();
        assert!(super::probe_node_cached(&cache, &program, &cancelled, probe).unwrap());
        assert_eq!(calls.get(), 2);
        let empty = Mutex::new(None);
        assert!(!super::probe_node_cached(&empty, &program, &cancelled, || Ok(false)).unwrap());
        assert!(super::probe_node_cached(&empty, &program, &cancelled, probe).unwrap());
        assert_eq!(calls.get(), 3);
        let cancelled = AtomicBool::new(true);
        assert!(
            super::probe_node_cached(&cache, &program, &cancelled, || panic!(
                "must not probe after cancellation"
            ))
            .unwrap_err()
            .contains("dibatalkan")
        );
        fs::remove_file(program).unwrap();
    }
    use super::{
        metadata_error_prefix, metadata_retry_reason, safe_thumbnail, source_for_host,
        validate_media_duration, validate_media_url, MetadataRetryReason, METADATA_PRINT_TEMPLATE,
    };
    use serde_json::{json, Value};

    #[test]
    fn metadata_channel_preserves_precedence_and_skips_empty_or_invalid_values() {
        for channel in [
            json!(null),
            json!(""),
            json!(" \t\n"),
            json!(42),
            json!({}),
            json!([]),
        ] {
            assert_eq!(
                super::metadata_channel(&json!({"channel": channel, "uploader": "Uploader"}))
                    .unwrap(),
                "Uploader"
            );
        }
        assert_eq!(
            super::metadata_channel(&json!({"uploader": "Uploader"})).unwrap(),
            "Uploader"
        );
        assert_eq!(
            super::metadata_channel(&json!({"channel": " Channel ", "uploader": "Uploader"}))
                .unwrap(),
            " Channel "
        );
        for value in [
            json!({}),
            json!(null),
            json!([]),
            json!({"channel": null, "uploader": "  "}),
        ] {
            assert_eq!(super::metadata_channel(&value).unwrap(), "Unknown channel");
        }
        let oversized = "x".repeat(super::MAX_METADATA_CHANNEL_BYTES + 1);
        assert!(
            super::metadata_channel(&json!({"channel": oversized, "uploader": "Fallback"}))
                .is_err()
        );
        assert!(super::metadata_channel(&json!({"uploader": oversized})).is_err());
    }

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
    fn bounded_metadata_runner_rejects_oversized_stdout() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::tests::captured_probe_child",
                "--nocapture",
            ])
            .env("WHISPERTUBE_CAPTURE_CHILD", "large-stdout");
        let tracker = std::sync::Arc::new(std::sync::Mutex::new(None));
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        assert!(super::run_output(command, &tracker, &cancelled)
            .err()
            .unwrap()
            .contains("terlalu besar"));
        assert_eq!(*tracker.lock().unwrap(), None);
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
