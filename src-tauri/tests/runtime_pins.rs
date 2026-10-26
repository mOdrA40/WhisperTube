#[path = "../build_support.rs"]
mod build_support;

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
