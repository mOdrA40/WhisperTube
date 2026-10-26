use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

const VERSION: &str = "qwen-transcribe-0.4.1";
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED: u64 = 1024 * 1024 * 1024;

fn artifact() -> Result<(&'static str, &'static str), String> {
    artifact_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn artifact_for(os: &str, arch: &str) -> Result<(&'static str, &'static str), String> {
    match (os, arch) {
        ("windows", "x86_64") => Ok((
            "windows-x86_64-cuda",
            "ba43fb21a3fe0ab2630e0614fbd742e039c389c68ad10fe23aca903f986adc5b",
        )),
        ("linux", "x86_64") => Ok((
            "linux-x86_64-cuda",
            "c4bdf9c4798bac1868ecdcaa04f27eb7712fb43125268313ce14d508c70d247d",
        )),
        ("linux", "aarch64") => Ok((
            "linux-aarch64-cpu-vulkan",
            "3671d8a07e256642d6bb38cc05ad8bbe88c76f814aeb704d1f8b57c61010db02",
        )),
        ("macos", "aarch64") => Ok((
            "macos-arm64-metal",
            "7369e36dc2a5fc5d52eee5766fa03ad22d3f533c8688bf1deccf3ffdc202ea24",
        )),
        ("macos", "x86_64") => Ok((
            "macos-x86_64-cpu",
            "513a3976a2f0ebc0666c9670551e96c226e39ed14090a51f2efa0874f41258d8",
        )),
        _ => Err("Runtime Qwen belum tersedia untuk platform ini.".into()),
    }
}

fn backends_for(os: &str, arch: &str) -> Vec<&'static str> {
    let Ok((name, _)) = artifact_for(os, arch) else {
        return Vec::new();
    };
    let mut backends = vec!["auto", "cpu"];
    if name.ends_with("-cuda") {
        backends.push("cuda");
    }
    if name.ends_with("-metal") {
        backends.push("metal");
    }
    // Vulkan is intentionally unavailable through the Qwen worker integration.
    backends
}

pub fn supported_backends() -> Vec<&'static str> {
    backends_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn library_name() -> &'static str {
    if cfg!(windows) {
        "transcribe.dll"
    } else if cfg!(target_os = "macos") {
        "libtranscribe.dylib"
    } else {
        "libtranscribe.so"
    }
}

pub fn library_path(app: &AppHandle) -> Result<PathBuf, String> {
    artifact()?;
    Ok(crate::paths::user_runtime_dir(app)?
        .join(VERSION)
        .join(library_name()))
}

pub fn installed(app: &AppHandle) -> bool {
    library_path(app).is_ok_and(|path| crate::paths::is_regular_file(&path))
}

pub fn snapshot(
    app: &AppHandle,
    cancelled: &AtomicBool,
) -> Result<crate::runtime_snapshot::RuntimeSnapshot, String> {
    crate::runtime_snapshot::RuntimeSnapshot::capture(&library_path(app)?, Some(VERSION), cancelled)
}

fn extract(
    archive: &Path,
    staging: &Path,
    tuple: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("Gagal membuka runtime Qwen: {e}"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut total = 0u64;
    let mut count = 0;
    for item in archive
        .entries()
        .map_err(|e| format!("Arsip Qwen invalid: {e}"))?
    {
        crate::io_cancel::check(cancelled)?;
        let mut item = item.map_err(|e| format!("Entry Qwen invalid: {e}"))?;
        let path = item
            .path()
            .map_err(|e| format!("Path arsip Qwen invalid: {e}"))?
            .into_owned();
        let relative = archive_relative(&path, tuple)?;
        if item.header().entry_type().is_dir() {
            continue;
        }
        if !item.header().entry_type().is_file() {
            return Err("Arsip Qwen berisi link atau tipe entry yang tidak aman.".into());
        }
        count += 1;
        let size = item.size();
        total = total.saturating_add(size);
        if count > crate::paths::MAX_RUNTIME_FILES || total > MAX_UNPACKED {
            return Err("Arsip Qwen melebihi batas aman.".into());
        }
        let dest = staging.join(relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Folder Qwen: {e}"))?;
        }
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|e| format!("File runtime Qwen: {e}"))?;
        let mut buffer = [0; 64 * 1024];
        loop {
            crate::io_cancel::check(cancelled)?;
            let n = item
                .read(&mut buffer)
                .map_err(|e| format!("Ekstraksi Qwen: {e}"))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buffer[..n])
                .map_err(|e| format!("Ekstraksi Qwen: {e}"))?;
        }
        out.sync_all().map_err(|e| format!("Flush Qwen: {e}"))?;
    }
    crate::paths::write_runtime_manifest(staging, library_name(), VERSION, cancelled)
}

fn archive_relative(path: &Path, tuple: &str) -> Result<PathBuf, String> {
    if path
        .components()
        .any(|p| !matches!(p, Component::Normal(_)))
        || path.to_string_lossy().contains(['\\', ':'])
    {
        return Err("Path arsip Qwen tidak aman.".into());
    }
    let root = format!("transcribe-native-{tuple}");
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Root arsip Qwen tidak cocok.")?;
    if relative.as_os_str().is_empty() {
        return Err("Entry arsip Qwen kosong.".into());
    }
    Ok(relative.to_owned())
}

pub async fn ensure_runtime(app: &AppHandle, cancelled: Arc<AtomicBool>) -> Result<(), String> {
    let (tuple, sha) = artifact()?;
    let library = library_path(app)?;
    if crate::paths::is_regular_file(&library) {
        let app = app.clone();
        let token = cancelled.clone();
        return tokio::task::spawn_blocking(move || snapshot(&app, &token).map(|_| ()))
            .await
            .map_err(|e| format!("Verifikasi runtime Qwen: {e}"))?;
    }
    let parent = crate::paths::user_runtime_dir(app)?;
    let staging = parent.join(format!(".qwen-staging-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|e| format!("Folder runtime Qwen: {e}"))?;
    let result = install(app, &staging, &library, tuple, sha, cancelled).await;
    if let Err(error) = fs::remove_dir_all(&staging) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("Qwen staging cleanup: {error}");
        }
    }
    result
}

async fn install(
    app: &AppHandle,
    staging: &Path,
    library: &Path,
    tuple: &str,
    sha: &str,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    crate::resources::require_disk(staging, MAX_ARCHIVE + MAX_UNPACKED, "runtime Qwen")?;
    let archive = staging.join("archive.tar.gz");
    let url=format!("https://github.com/handy-computer/transcribe.cpp/releases/download/v0.4.1/transcribe-native-0.4.1-{tuple}.tar.gz");
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP Qwen: {e}"))?;
    let mut response = crate::network::send_cancelable(
        client.get(url),
        &cancelled,
        Duration::from_secs(120),
        "runtime Qwen",
    )
    .await?;
    if !response.status().is_success() {
        return Err(format!("Runtime Qwen HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|n| n > MAX_ARCHIVE) {
        return Err("Runtime Qwen terlalu besar.".into());
    }
    let total = response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut file = tokio::fs::File::create(&archive)
        .await
        .map_err(|e| format!("Arsip Qwen: {e}"))?;
    crate::models::emit_model_progress(app, super::MODEL_ID, 0, total, None, "runtime");
    let started = Instant::now();
    let mut last_progress = Duration::ZERO;
    while let Some(chunk) = crate::network::next_chunk_cancelable(
        &mut response,
        &cancelled,
        Duration::from_secs(120),
        "runtime Qwen",
    )
    .await?
    {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > MAX_ARCHIVE {
            return Err("Runtime Qwen terlalu besar.".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Download Qwen: {e}"))?;
        if crate::network::download_progress_due(started.elapsed(), &mut last_progress) {
            let speed = (started.elapsed().as_secs_f64() > 0.0)
                .then(|| (downloaded as f64 / started.elapsed().as_secs_f64()) as u64);
            crate::models::emit_model_progress(
                app,
                super::MODEL_ID,
                downloaded,
                total,
                speed,
                "runtime",
            );
        }
    }
    crate::models::emit_model_progress(
        app,
        super::MODEL_ID,
        downloaded,
        downloaded,
        None,
        "runtime",
    );
    file.flush()
        .await
        .map_err(|e| format!("Flush runtime Qwen: {e}"))?;
    drop(file);
    let archive = archive.to_owned();
    let staging = staging.to_owned();
    let destination = library.parent().ok_or("Folder Qwen invalid.")?.to_owned();
    let tuple = tuple.to_owned();
    let sha = sha.to_owned();
    tokio::task::spawn_blocking(move || {
        if crate::io_cancel::file_sha256(&archive,&cancelled)?!=sha { return Err("SHA-256 runtime Qwen tidak cocok.".into()); }
        let unpacked=staging.join("unpacked"); fs::create_dir(&unpacked).map_err(|e|format!("Folder Qwen: {e}"))?;
        extract(&archive,&unpacked,&tuple,&cancelled)?;
        crate::io_cancel::check(&cancelled)?;
        if destination.exists() { return Err("Folder runtime Qwen sudah ada tetapi tidak lengkap. Hapus folder runtime Qwen yang rusak lalu ulangi unduhan.".into()); }
        fs::rename(unpacked,destination).map_err(|e|format!("Aktivasi runtime Qwen: {e}"))
    }).await.map_err(|e|format!("Instalasi runtime Qwen: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::{extract, library_name, AtomicBool, VERSION};
    use std::fs;
    #[test]
    fn backend_capabilities_match_the_shipping_runtime_architecture() {
        for (os, arch, expected) in [
            ("macos", "x86_64", vec!["auto", "cpu"]),
            ("macos", "aarch64", vec!["auto", "cpu", "metal"]),
            ("windows", "x86_64", vec!["auto", "cpu", "cuda"]),
            ("linux", "x86_64", vec!["auto", "cpu", "cuda"]),
            ("linux", "aarch64", vec!["auto", "cpu"]),
            ("windows", "aarch64", vec![]),
        ] {
            assert_eq!(super::backends_for(os, arch), expected, "{os}/{arch}");
        }
    }
    #[test]
    fn extraction_creates_a_verifiable_inventory_and_rejects_links() {
        let root = std::env::temp_dir().join(format!("qwen-extract-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        for link in [false, true] {
            let archive_path = root.join(format!("{link}.tar.gz"));
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&archive_path).unwrap(),
                flate2::Compression::default(),
            );
            let mut builder = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            if link {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_link_name("outside").unwrap();
                header.set_size(0);
            } else {
                header.set_size(7);
            }
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    format!("transcribe-native-test/{}", library_name()),
                    if link { &b""[..] } else { &b"fixture"[..] },
                )
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
            let staging = root.join(format!("unpacked-{link}"));
            fs::create_dir(&staging).unwrap();
            let cancelled = AtomicBool::new(false);
            let result = extract(&archive_path, &staging, "test", &cancelled);
            if link {
                assert!(result.is_err());
            } else {
                result.unwrap();
                assert!(crate::paths::runtime_manifest_matches_cancelable(
                    &staging,
                    &staging.join(library_name()),
                    VERSION,
                    &cancelled
                )
                .unwrap());
                fs::write(staging.join(library_name()), b"changed").unwrap();
                assert!(!crate::paths::runtime_manifest_matches_cancelable(
                    &staging,
                    &staging.join(library_name()),
                    VERSION,
                    &cancelled
                )
                .unwrap());
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_archive_traversal_and_wrong_root() {
        for name in [
            "../evil",
            "/absolute",
            "transcribe-native-test/../evil",
            "transcribe-native-test/C:/evil",
            "other/file",
        ] {
            assert!(
                super::archive_relative(std::path::Path::new(name), "test").is_err(),
                "{name}"
            );
        }
        assert_eq!(
            super::archive_relative(
                std::path::Path::new("transcribe-native-test/licenses/LICENSE"),
                "test"
            )
            .unwrap(),
            std::path::Path::new("licenses/LICENSE")
        );
    }
}
