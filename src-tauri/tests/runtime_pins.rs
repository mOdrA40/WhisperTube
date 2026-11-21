#[path = "../build_support.rs"]
mod build_support;

#[test]
fn unix_ffmpeg_build_receipt_requires_current_version_and_matching_binary() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("ffmpeg");
    let receipt = root.path().join(".ffmpeg-pin");
    let bytes = b"verified FFmpeg fixture";
    let digest = format!("{:x}", Sha256::digest(bytes));
    std::fs::write(&executable, bytes).unwrap();
    std::fs::write(&receipt, format!("9.0.1 {digest}")).unwrap();
    assert!(build_support::verify_ffmpeg_receipt(&executable, &receipt, "9.0.2").is_err());
    std::fs::write(&receipt, format!("9.0.2 {digest}")).unwrap();
    build_support::verify_ffmpeg_receipt(&executable, &receipt, "9.0.2").unwrap();
    std::fs::write(&executable, b"changed FFmpeg fixture").unwrap();
    assert!(build_support::verify_ffmpeg_receipt(&executable, &receipt, "9.0.2").is_err());
}

#[test]
fn native_build_rejects_old_or_modified_downloader_without_running_it() {
    use sha2::{Digest, Sha256};
    let root = std::env::temp_dir().join(format!("native-runtime-pin-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("yt-dlp");
    let fixture = b"approved executable fixture; never execute";
    let expected = format!("{:x}", Sha256::digest(fixture));
    std::fs::write(&path, fixture).unwrap();
    build_support::verify_digest(&path, &expected).unwrap();
    for bytes in [
        &b"outdated executable"[..],
        &b"changed executable"[..],
        &b""[..],
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(build_support::verify_digest(&path, &expected).is_err());
    }
    assert!(build_support::verify_digest(&root, &expected).is_err());
    assert!(build_support::verify_digest(&root.join("absent"), &expected).is_err());
    assert_eq!(
        std::fs::canonicalize(&root).unwrap().parent(),
        Some(
            std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .as_path()
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}
