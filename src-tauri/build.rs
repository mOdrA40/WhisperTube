mod build_support;

fn main() {
    // Direct Cargo/Tauri release builds must not ship a stale cached downloader.
    println!("cargo:rerun-if-changed=runtime-pins.json");
    if std::env::var("PROFILE").as_deref() != Ok("debug") {
        verify_runtime_pins();
    }
    tauri_build::build()
}

fn verify_runtime_pins() {
    use std::path::PathBuf;
    let platform = match std::env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        other => panic!("Unsupported runtime platform: {other}"),
    };
    let pins: serde_json::Value = serde_json::from_str(include_str!("runtime-pins.json")).unwrap();
    let pin = &pins["ytDlp"]["platforms"][platform];
    let root = PathBuf::from("runtime").join(platform);
    let path = root.join(pin["file"].as_str().unwrap());
    println!("cargo:rerun-if-changed={}", path.display());
    let result = build_support::verify_digest(&path, pin["sha256"].as_str().unwrap());
    assert!(
        result.is_ok(),
        "Pinned yt-dlp checksum failed for {}. Rerun the platform setup before building.",
        path.display()
    );
    let ffmpeg = &pins["ffmpeg"];
    let path = root.join(if platform == "windows" {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    });
    println!("cargo:rerun-if-changed={}", path.display());
    let version = ffmpeg["version"].as_str().unwrap();
    let result = if platform == "windows" {
        build_support::verify_digest(&path, ffmpeg["windowsBinarySha256"].as_str().unwrap())
    } else {
        let receipt = root.join(".ffmpeg-pin");
        println!("cargo:rerun-if-changed={}", receipt.display());
        build_support::verify_ffmpeg_receipt(&path, &receipt, version)
    };
    assert!(
        result.is_ok(),
        "Pinned FFmpeg verification failed at {}. Rerun the platform setup: {result:?}",
        path.display()
    );
}
