use super::*;

pub(crate) const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "wma", "aiff", "aif", "webm", "mp4",
];

pub(crate) fn can_recover_mp3(path: &Path, stderr: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
        && (stderr.contains("Header missing")
            || stderr.contains("Error submitting packet to decoder"))
}

pub(crate) fn configure_decode(command: &mut Command, recovering: bool) {
    if recovering {
        // FFmpeg exits unsuccessfully when more than 1% of decoded frames fail.
        // Keep failures visible; do not use ignore_err or discard all diagnostics.
        command.args(["-max_error_rate", "0.01"]);
    } else {
        command.arg("-xerror");
    }
}

#[cfg(test)]
pub(crate) fn local_source(path: &Path) -> Result<(PathBuf, String), String> {
    local_source_cancelable(path, &AtomicBool::new(false))
}

// Include file identity in cache/checkpoint keys so replacing a recording at the
// same path cannot silently reuse an older transcript.
fn local_source_cancelable(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<(PathBuf, String), String> {
    crate::io_cancel::check(cancelled)?;
    if !path.is_absolute() {
        return Err("Pilih path audio absolut.".into());
    }
    let input_url = Url::from_file_path(path).map_err(|()| "Path audio tidak valid.")?;
    if input_url
        .host_str()
        .is_some_and(|host| !host.is_empty() && host != "localhost")
    {
        return Err("Pilih file audio pada disk lokal.".into());
    }
    let path =
        std::fs::canonicalize(path).map_err(|e| format!("File audio tidak dapat dibuka: {e}"))?;
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !AUDIO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
        return Err("Format audio tidak didukung.".into());
    }
    let metadata =
        std::fs::metadata(&path).map_err(|e| format!("Gagal membaca file audio: {e}"))?;
    if !metadata.is_file() {
        return Err("Audio harus berupa file biasa.".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A path replaced with a FIFO after the check must not block open.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(&path)
        .map_err(|e| format!("Gagal membuka file audio: {e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Gagal membaca file audio: {e}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 4 * 1024 * 1024 * 1024 {
        return Err("Audio harus berupa file berisi data dengan ukuran maksimal 4 GiB.".into());
    }
    let modified = metadata
        .modified()
        .map_err(|e| format!("Gagal membaca waktu file audio: {e}"))?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| format!("Waktu file audio tidak valid: {e}"))?;
    let mut url = Url::from_file_path(&path).map_err(|()| "Path audio tidak valid.")?;
    let (identity, changed) = crate::runtime_snapshot::file_identity(&file, &metadata);
    let identity = source_identity(&path, identity, changed, cancelled)?;
    url.set_query(Some(&format!(
        "size={}&modified={}&identity={}",
        metadata.len(),
        modified.as_nanos(),
        identity
    )));
    Ok((path, url.into()))
}

fn source_identity(
    path: &Path,
    identity: Option<[u64; 3]>,
    changed: [i64; 2],
    cancelled: &AtomicBool,
) -> Result<String, String> {
    Ok(match identity {
        Some(id) => format!(
            "{:x}-{:x}-{:x}-{}-{}",
            id[0], id[1], id[2], changed[0], changed[1]
        ),
        None => crate::io_cancel::file_sha256(path, cancelled)?,
    })
}

#[cfg(test)]
pub(crate) fn local_path(source: &str) -> Result<Option<PathBuf>, String> {
    local_path_cancelable(source, &AtomicBool::new(false))
}

pub(crate) fn local_path_cancelable(
    source: &str,
    cancelled: &AtomicBool,
) -> Result<Option<PathBuf>, String> {
    crate::io_cancel::check(cancelled)?;
    let Ok(url) = Url::parse(source) else {
        return Ok(None);
    };
    if url.scheme() != "file" {
        return Ok(None);
    }
    if url
        .host_str()
        .is_some_and(|host| !host.is_empty() && host != "localhost")
    {
        return Err("Audio harus berada pada filesystem lokal.".into());
    }
    let path = url.to_file_path().map_err(|()| "Path audio tidak valid.")?;
    let (path, expected) = local_source_cancelable(&path, cancelled)?;
    if expected != source {
        return Err("File audio berubah sejak dipilih. Pilih ulang file audio.".into());
    }
    Ok(Some(path))
}

pub(crate) fn validate_source(source: &str) -> Result<String, String> {
    validate_source_cancelable(source, &AtomicBool::new(false))
}

pub(crate) fn validate_source_cancelable(
    source: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    if local_path_cancelable(source, cancelled)?.is_some() {
        Ok(source.to_string())
    } else {
        validate_media_url(source)
    }
}

pub(crate) fn inspect(
    app: &AppHandle,
    path: &Path,
    active_pid: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<VideoMetadata, String> {
    let (path, source) = local_source_cancelable(path, cancelled)?;
    let (duration, audio_recovered) = super::duration_probe::probe_import_duration(
        &tool_path(app, "ffmpeg")?,
        &path,
        active_pid,
        cancelled,
    )?;
    // Detect replacement during inspection as well as before transcription.
    local_path_cancelable(&source, cancelled)?;
    Ok(VideoMetadata {
        audio_recovered,
        id: source.clone(),
        title: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        channel: "Local audio".into(),
        duration,
        thumbnail: None,
        webpage_url: source,
        availability: None,
        source: "Local audio".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_hash_uses_the_callers_cancellation_token() {
        let path = std::env::temp_dir().join(format!("local-hash-{}.mp3", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"abc").unwrap();
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            source_identity(&path, None, [0, 0], &cancelled).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        cancelled.store(true, Ordering::SeqCst);
        assert_eq!(
            source_identity(&path, None, [0, 0], &cancelled).unwrap_err(),
            "Proses dibatalkan."
        );
        assert_eq!(
            local_source_cancelable(&path, &cancelled).unwrap_err(),
            "Proses dibatalkan."
        );
        let source = local_source(&path).unwrap().1;
        assert_eq!(
            validate_source_cancelable(&source, &cancelled).unwrap_err(),
            "Proses dibatalkan."
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn recovery_only_applies_to_mp3_decode_failures() {
        assert!(can_recover_mp3(
            Path::new("recording.MP3"),
            "[mp3float] Header missing"
        ));
        assert!(can_recover_mp3(
            Path::new("recording.mp3"),
            "Error submitting packet to decoder: Invalid data"
        ));
        for (path, error) in [
            ("audio.wav", "Header missing"),
            ("audio.mp3", "Permission denied"),
            ("audio.mp3", "No such file or directory"),
            ("audio.mp3", "Invalid argument"),
            ("audio.mp3", "Conversion timed out"),
            ("audio.mp3", "Job dibatalkan."),
        ] {
            assert!(!can_recover_mp3(Path::new(path), error));
        }
    }

    #[test]
    fn local_identity_handles_unicode_spaces_and_invalidates_changed_files() {
        let root = std::env::temp_dir().join(format!("whispertube-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("rekaman 日本 #1.MP3");
        std::fs::write(&file, b"test audio").unwrap();
        let (canonical, source) = local_source(&file).unwrap();
        assert_eq!(local_path(&source).unwrap(), Some(canonical));
        assert_eq!(validate_source(&source).unwrap(), source);
        assert!(validate_media_url(&source).is_err());
        std::fs::write(&file, b"replacement recording").unwrap();
        assert!(local_path(&source).unwrap_err().contains("berubah"));
        assert_ne!(local_source(&file).unwrap().1, source);
        std::fs::remove_file(&file).unwrap();
        assert!(local_path(&source).is_err());
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn same_size_and_restored_mtime_do_not_reuse_local_audio() {
        let file =
            std::env::temp_dir().join(format!("local-identity-{}.mp3", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"old audio").unwrap();
        let source = local_source(&file).unwrap().1;
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(&file, b"new audio").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert!(local_path(&source).is_err());
        assert_ne!(local_source(&file).unwrap().1, source);
        std::fs::remove_file(file).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_fifo_without_waiting_for_a_writer() {
        use std::os::unix::{ffi::OsStrExt, fs::OpenOptionsExt};

        let path = std::env::temp_dir().join(format!("local-fifo-{}.wav", uuid::Uuid::new_v4()));
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: name is a valid, NUL-terminated path owned for this call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (sender, receiver) = std::sync::mpsc::channel();
        let source = path.clone();
        let worker = std::thread::spawn(move || {
            sender.send(local_source(&source)).unwrap();
        });
        let result = receiver.recv_timeout(Duration::from_secs(2));
        // Unblock a regressed blocking open before joining, so a failure cannot
        // hang the test suite. No writer exists during the actual assertion.
        let writer = result.is_err().then(|| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .unwrap()
        });
        worker.join().unwrap();
        drop(writer);
        std::fs::remove_file(path).unwrap();
        assert!(result
            .expect("FIFO validation must not wait for a writer")
            .is_err());
    }

    #[test]
    fn rejects_empty_unsupported_directory_relative_and_remote_sources() {
        let root = std::env::temp_dir().join(format!("whispertube-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let empty = root.join("empty.wav");
        let wrong = root.join("playlist.m3u");
        std::fs::write(&empty, []).unwrap();
        std::fs::write(&wrong, b"https://example.test/audio").unwrap();
        assert!(local_source(&empty).is_err());
        assert!(local_source(&wrong).is_err());
        assert!(local_source(&root).is_err());
        assert!(local_source(Path::new("relative.mp3")).is_err());
        assert!(local_path("file://remote.test/audio.mp3").is_err());
        assert!(validate_source("https://youtube.com.evil.test/video").is_err());
        assert!(validate_source("https://www.youtube.com/watch?v=test").is_ok());
        assert_eq!(
            std::fs::read(&wrong).unwrap(),
            b"https://example.test/audio"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
