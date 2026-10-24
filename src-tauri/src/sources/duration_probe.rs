use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};
use tauri::AppHandle;

use super::{run_output_limited, validate_media_duration};

const MAX_PROBE_BYTES: usize = 32 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);
const LOCAL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

struct ProbeDirectory(PathBuf);

impl ProbeDirectory {
    fn new() -> Result<Self, String> {
        let path =
            std::env::temp_dir().join(format!("whispertube-media-probe-{}", uuid::Uuid::new_v4()));
        let mut builder = fs::DirBuilder::new();
        builder.recursive(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .map_err(|error| format!("Gagal membuat folder probe media: {error}"))?;
        Ok(Self(path))
    }
}

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("WhisperTube media probe cleanup warning: {error}");
        }
    }
}

fn parsed_duration(progress: &[u8]) -> Result<f64, String> {
    // Machine progress is separate from untrusted container metadata in stderr.
    // Require the completed stream, never a partial header/last timeout sample.
    let text = String::from_utf8_lossy(progress);
    if text.lines().last() != Some("progress=end") {
        return Err("Probe audio belum selesai.".into());
    }
    let micros = text
        .lines()
        .filter_map(|line| line.strip_prefix("out_time_us="))
        .next_back()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or("Durasi file media tidak dapat diverifikasi.")?;
    let duration = micros as f64 / 1_000_000.0;
    validate_media_duration(duration, false)
        .map_err(|error| format!("{}{error}", super::SOURCE_DURATION_ERROR_PREFIX))?;
    Ok(duration)
}

// Probe only a local, finite file; never hand a platform/CDN URL or cookies
// to FFmpeg. Decode audio to the null sink under a watchdog; do not trust a
// container's claimed duration or text tags. No uncompressed media is stored.
pub(crate) fn probe_local_duration(
    ffmpeg: &Path,
    media: &Path,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<f64, String> {
    crate::io_cancel::check(cancelled)?;
    if !crate::paths::is_regular_file(media) {
        return Err("File probe media kosong atau bukan file biasa.".into());
    }
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-xerror",
            "-nostats",
            "-progress",
            "pipe:1",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(media)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-t"])
        .arg((crate::transcription::MAX_MEDIA_DURATION_SECONDS + 1.0).to_string())
        .args(["-f", "null", "-"]);
    let output = run_output_limited(
        command,
        active_pid,
        cancelled,
        64 * 1024,
        LOCAL_PROBE_TIMEOUT,
    )?;
    if !output.status.success() {
        // Raw FFmpeg/yt-dlp stderr can contain signed URLs or credentials.
        return Err(
            "File media tidak dapat dibaca sebagai audio untuk memverifikasi durasinya.".into(),
        );
    }
    parsed_duration(&output.stdout)
}

pub(super) fn recover_public_duration(
    app: &AppHandle,
    yt_dlp: &Path,
    url: &str,
    runtime_args: &[String],
    cookies: &crate::cookies::CookieJar,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<f64, String> {
    let ffmpeg = crate::paths::tool_path(app, "ffmpeg")?;
    recover_with_tools(
        &ffmpeg,
        yt_dlp,
        url,
        runtime_args,
        &cookies.args(),
        active_pid,
        cancelled,
    )
}

fn recover_with_tools(
    ffmpeg: &Path,
    yt_dlp: &Path,
    url: &str,
    runtime_args: &[String],
    cookies: &[String],
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<f64, String> {
    crate::io_cancel::check(cancelled)?;
    if !super::supports_duration_probe(url) {
        return Err("Source tidak memenuhi syarat probe media publik.".into());
    }
    let directory = ProbeDirectory::new()?;
    crate::resources::require_disk(
        &directory.0,
        MAX_PROBE_BYTES as u64 + 16 * 1024 * 1024,
        "pemeriksaan durasi media",
    )?;
    let mut command = Command::new(yt_dlp);
    command
        .args([
            "--ignore-config",
            "--no-playlist",
            "--no-warnings",
            "--no-progress",
            "--quiet",
            "--socket-timeout",
            "10",
            "--retries",
            "0",
            "--fragment-retries",
            "0",
            "--max-filesize",
            "32M",
            "--match-filter",
        ])
        .arg(super::download_media_filter(url))
        .args(["-f", "bestaudio/best", "-o", "-"])
        .args(runtime_args)
        .args(cookies)
        .arg(url);
    // stdout is streamed by the existing process owner/watchdog into a bounded
    // buffer. It aborts immediately at the cap even if Content-Length is absent.
    let output = run_output_limited(
        command,
        active_pid,
        cancelled,
        MAX_PROBE_BYTES,
        DOWNLOAD_TIMEOUT,
    )?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err("Probe audio publik gagal atau melebihi batas 32 MiB/45 detik. Coba cookies platform jika akses dibatasi.".into());
    }
    crate::io_cancel::check(cancelled)?;
    let media = directory.0.join("media");
    fs::write(&media, output.stdout)
        .map_err(|error| format!("Gagal menulis media probe: {error}"))?;
    probe_local_duration(ffmpeg, &media, active_pid, cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parser_requires_completed_finite_bounded_audio() {
        assert_eq!(
            parsed_duration(
                b"out_time_us=1000000\nprogress=continue\nout_time_us=61970000\nprogress=end\n"
            ),
            Ok(61.97)
        );
        assert_eq!(
            parsed_duration(b"out_time_us=28800000000\nprogress=end\n"),
            Ok(28800.0)
        );
        for input in [
            "Duration: 00:01:01.97",
            "out_time_us=0\nprogress=end",
            "out_time_us=-1\nprogress=end",
            "out_time_us=NaN\nprogress=end",
            "out_time_us=28800000001\nprogress=end",
            "out_time_us=61970000\nprogress=continue",
        ] {
            assert!(parsed_duration(input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn probe_directory_is_removed_on_failure_and_cancellation() {
        let path;
        {
            let directory = ProbeDirectory::new().unwrap();
            path = directory.0.clone();
            fs::write(path.join("media"), b"fixture").unwrap();
        }
        assert!(!path.exists());
        let cancelled = Arc::new(AtomicBool::new(true));
        let tracker = Arc::new(Mutex::new(None));
        assert!(probe_local_duration(
            Path::new("must-not-start"),
            Path::new("unused"),
            &tracker,
            &cancelled
        )
        .unwrap_err()
        .contains("dibatalkan"));
        assert_eq!(*tracker.lock().unwrap(), None);
    }

    #[test]
    #[ignore = "explicit network QA; run with WHISPERTUBE_PUBLIC_VOD_URL and native runtime tools"]
    fn public_vod_without_cookies() {
        let url = std::env::var("WHISPERTUBE_PUBLIC_VOD_URL").expect("explicit URL required");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(if cfg!(windows) {
                "windows"
            } else if cfg!(target_os = "macos") {
                "macos"
            } else {
                "linux"
            });
        let tracker = Arc::new(Mutex::new(None));
        let token = Arc::new(AtomicBool::new(false));
        // Exercise the installed CLI's actual missing-field semantics, not a
        // handwritten interpretation of its filter language. No network here.
        let fixture = ProbeDirectory::new().unwrap();
        for (duration, live, status, eligible, accepted) in [
            (None, false, None, true, true),
            (None, false, None, false, false),
            (Some(60), false, None, false, true),
            (Some(28800), false, None, true, true),
            (Some(28801), false, None, true, false),
            (Some(60), true, Some("is_live"), true, false),
            (Some(60), false, Some("is_upcoming"), true, false),
            (Some(60), false, Some("post_live"), true, false),
        ] {
            let path = fixture.0.join("info.json");
            fs::write(
                &path,
                serde_json::to_vec(&serde_json::json!({
                    "id":"fixture", "title":"fixture", "url":"https://example.invalid/audio.mp3",
                    "ext":"mp3", "extractor":"generic", "extractor_key":"Generic",
                    "duration":duration, "is_live":live, "live_status":status,
                }))
                .unwrap(),
            )
            .unwrap();
            let mut command = Command::new(root.join(crate::paths::exe_name("yt-dlp")));
            command
                .args([
                    "--ignore-config",
                    "--skip-download",
                    "--no-warnings",
                    "--print",
                    "%(id)s",
                    "--match-filter",
                ])
                .arg(super::super::download_media_filter(if eligible {
                    &url
                } else {
                    "https://youtube.com/watch?v=fixture"
                }))
                .arg("--load-info-json")
                .arg(&path);
            let result =
                run_output_limited(command, &tracker, &token, 4096, Duration::from_secs(5))
                    .unwrap();
            assert!(
                result.status.success(),
                "fixture CLI rejected filter syntax"
            );
            assert_eq!(
                !result.stdout.is_empty(),
                accepted,
                "duration={duration:?} live={live} status={status:?} eligible={eligible}"
            );
        }
        let duration = recover_with_tools(
            &root.join(crate::paths::exe_name("ffmpeg")),
            &root.join(crate::paths::exe_name("yt-dlp")),
            &url,
            &[],
            &[],
            &tracker,
            &token,
        )
        .unwrap();
        assert!(duration > 0.0);
        assert_eq!(*tracker.lock().unwrap(), None);
        println!("public VOD duration without cookies: {duration}");
    }
}
