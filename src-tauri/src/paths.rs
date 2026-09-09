use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Manager};

use crate::models::model_spec;

const RUNTIME_MANIFEST_NAME: &str = "runtime-manifest.json";
const RUNTIME_MANIFEST_VERSION: &str = "whisper.cpp-v1.9.1";
const MAX_RUNTIME_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_RUNTIME_FILES: usize = 256;
const TRANSIENT_STORAGE_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Deserialize, Serialize)]
struct RuntimeManifest {
    version: String,
    executable: String,
    files: Vec<RuntimeFile>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
struct RuntimeFile {
    path: String,
    sha256: String,
}

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Tidak bisa menentukan app data directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat app data directory: {e}"))?;
    Ok(dir)
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("models");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder models: {e}"))?;
    Ok(dir)
}

pub fn jobs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("jobs");
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder jobs: {e}"))?;
    Ok(dir)
}

pub fn runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let platform = runtime_platform();

    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime")
            .join(platform))
    } else {
        Ok(app
            .path()
            .resource_dir()
            .map_err(|e| format!("Gagal menemukan resource directory: {e}"))?
            .join("runtime")
            .join(platform))
    }
}

fn runtime_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    return "windows";
    #[cfg(target_os = "macos")]
    return "macos";
    #[cfg(target_os = "linux")]
    return "linux";
    #[allow(unreachable_code)]
    "unknown"
}

pub fn user_runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("runtime").join(runtime_platform());
    fs::create_dir_all(&dir).map_err(|e| format!("Gagal membuat folder runtime user: {e}"))?;
    Ok(dir)
}

fn exe_name(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub fn tool_path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(runtime_dir(app)?.join(exe_name(name)))
}

pub fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("Gagal membuka runtime: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Gagal membaca runtime: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn write_runtime_manifest(staging: &Path, executable: &str) -> Result<(), String> {
    if !is_regular_file(&staging.join(executable)) {
        return Err("Executable runtime tidak ditemukan.".into());
    }
    let mut files = Vec::new();
    collect_runtime_files(staging, staging, &mut files)?;
    if files.len() > MAX_RUNTIME_FILES {
        return Err("Runtime memiliki terlalu banyak file.".into());
    }
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let manifest = RuntimeManifest {
        version: RUNTIME_MANIFEST_VERSION.into(),
        executable: executable.into(),
        files,
    };
    let bytes = serde_json::to_vec(&manifest)
        .map_err(|e| format!("Gagal membuat manifest runtime: {e}"))?;
    let manifest_path = staging.join(RUNTIME_MANIFEST_NAME);
    let temporary = staging.join(format!("{RUNTIME_MANIFEST_NAME}.tmp"));
    let mut file = File::create(&temporary)
        .map_err(|e| format!("Gagal membuat manifest runtime sementara: {e}"))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Gagal menulis manifest runtime: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Gagal flush manifest runtime: {e}"))?;
    drop(file);
    fs::rename(&temporary, &manifest_path)
        .map_err(|e| format!("Gagal mengaktifkan manifest runtime: {e}"))?;
    Ok(())
}

fn collect_runtime_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<RuntimeFile>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| format!("Gagal membaca runtime: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry runtime: {e}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Gagal membaca tipe file runtime: {e}"))?;
        if file_type.is_symlink() {
            return Err("Runtime berisi symbolic link yang tidak diizinkan.".into());
        }
        if file_type.is_dir() {
            collect_runtime_files(root, &path, files)?;
            continue;
        }
        if !file_type.is_file()
            || path.file_name().and_then(|name| name.to_str()) == Some(RUNTIME_MANIFEST_NAME)
            || path.file_name().and_then(|name| name.to_str()) == Some("runtime-manifest.json.tmp")
        {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "Path relatif runtime tidak valid.".to_string())?
            .to_string_lossy()
            .into_owned();
        files.push(RuntimeFile {
            path: relative,
            sha256: file_sha256(&path)?,
        });
        if files.len() > MAX_RUNTIME_FILES {
            return Err("Runtime memiliki terlalu banyak file.".into());
        }
    }
    Ok(())
}

fn runtime_manifest_matches(runtime_dir: &Path, executable: &Path) -> bool {
    let Ok(runtime_metadata) = fs::symlink_metadata(runtime_dir) else {
        return false;
    };
    if !runtime_metadata.is_dir() {
        return false;
    }
    let manifest_path = runtime_dir.join(RUNTIME_MANIFEST_NAME);
    let Ok(metadata) = fs::symlink_metadata(&manifest_path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > MAX_RUNTIME_MANIFEST_BYTES {
        return false;
    }
    let Ok(file) = File::open(&manifest_path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_RUNTIME_MANIFEST_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_RUNTIME_MANIFEST_BYTES
    {
        return false;
    }
    let Ok(manifest) = serde_json::from_slice::<RuntimeManifest>(&bytes) else {
        return false;
    };
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if manifest.version != RUNTIME_MANIFEST_VERSION
        || manifest.executable != name
        || manifest.files.len() > MAX_RUNTIME_FILES
    {
        return false;
    }
    let executable_relative = executable
        .strip_prefix(runtime_dir)
        .ok()
        .and_then(|path| path.to_str());
    if executable_relative.is_none_or(|path| !manifest.files.iter().any(|file| file.path == path)) {
        return false;
    }
    if manifest
        .files
        .iter()
        .any(|file| !is_safe_manifest_path(&file.path))
    {
        return false;
    }
    let mut expected = manifest.files;
    expected.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let mut actual = Vec::new();
    if collect_runtime_files(runtime_dir, runtime_dir, &mut actual).is_err() {
        return false;
    }
    actual.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    expected == actual
}

fn is_safe_manifest_path(value: &str) -> bool {
    let path = Path::new(value);
    path.is_relative()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn cleanup_stale_entries(directory: &Path, matches: impl Fn(&str) -> bool) -> Vec<String> {
    let mut errors = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return errors,
        Err(error) => {
            errors.push(format!(
                "Gagal membaca temporary storage {}: {error}",
                directory.display()
            ));
            return errors;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!("Gagal membaca temporary storage: {error}"));
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if !matches(&name) {
            continue;
        }
        let path = entry.path();
        let stale = fs::symlink_metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= TRANSIENT_STORAGE_GRACE);
        if !stale {
            continue;
        }
        let result = fs::symlink_metadata(&path).and_then(|metadata| {
            if metadata.file_type().is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            }
        });
        if let Err(error) = result {
            errors.push(format!("Gagal membersihkan {}: {error}", path.display()));
        }
    }
    errors
}

pub fn cleanup_stale_transient_storage(app: &AppHandle) -> Result<(), String> {
    let runtime_root = user_runtime_dir(app)?;
    let mut errors = cleanup_stale_entries(&runtime_root, |name| {
        name.starts_with(".cuda-staging-")
            || name.starts_with(".vulkan-staging-")
            || name.starts_with(".metal-staging-")
    });
    let temp_root = std::env::temp_dir();
    errors.extend(cleanup_stale_entries(&temp_root, |name| {
        name.starts_with("whispertube-cuda-")
            || name.starts_with("whispertube-accelerator-")
            || name.starts_with("whispertube-cuda-probe-")
            || name.starts_with("whispertube-metal-probe-")
            || name.starts_with("whispertube-vulkan-probe-")
    }));
    let models_root = models_dir(app)?;
    errors.extend(cleanup_stale_entries(&models_root, |name| {
        name.ends_with(".bin.download")
    }));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn clear_invalid_runtime_destination(
    destination: &Path,
    executable: &str,
) -> Result<(), String> {
    if !destination.exists() {
        return Ok(());
    }
    if is_regular_file(&destination.join(executable))
        && runtime_manifest_matches(destination, &destination.join(executable))
    {
        return Err(
            "Runtime baru saja dipasang oleh proses lain. Klik Re-check components.".into(),
        );
    }

    let metadata = fs::symlink_metadata(destination)
        .map_err(|error| format!("Gagal memeriksa runtime lama yang tidak valid: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(
            "Runtime lama tidak valid berupa symbolic link dan tidak dapat diganti otomatis."
                .into(),
        );
    }
    if metadata.is_dir() {
        fs::remove_dir_all(destination).map_err(|error| {
            format!("Gagal membersihkan runtime lama yang tidak valid: {error}")
        })?;
    } else {
        fs::remove_file(destination).map_err(|error| {
            format!("Gagal membersihkan runtime lama yang tidak valid: {error}")
        })?;
    }
    Ok(())
}

pub fn engine_path(app: &AppHandle, backend: &str) -> Result<PathBuf, String> {
    let user_backend_dir = user_runtime_dir(app)?.join(backend);
    let user_path = user_backend_dir.join(exe_name("whisper-cli"));
    if is_regular_file(&user_path) && runtime_manifest_matches(&user_backend_dir, &user_path) {
        return Ok(user_path);
    }
    Ok(runtime_dir(app)?
        .join(backend)
        .join(exe_name("whisper-cli")))
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let _ = model_spec(id)?;
    Ok(models_dir(app)?.join(format!("ggml-{id}.bin")))
}

#[cfg(test)]
mod tests {
    use super::{
        clear_invalid_runtime_destination, is_regular_file, runtime_manifest_matches,
        write_runtime_manifest,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "whispertube-paths-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn invalid_runtime_directory_is_not_treated_as_an_engine_and_is_replaced() {
        let root = temporary_path("invalid-runtime");
        let destination = root.join("vulkan");
        fs::create_dir_all(destination.join("whisper-cli.exe"))
            .expect("test runtime directory should be created");

        assert!(!is_regular_file(&destination.join("whisper-cli.exe")));
        clear_invalid_runtime_destination(&destination, "whisper-cli.exe")
            .expect("invalid runtime directory should be removable");
        assert!(!destination.exists());

        fs::create_dir_all(&destination).expect("test runtime directory should be created");
        fs::write(destination.join("whisper-cli.exe"), [])
            .expect("empty test runtime should be created");
        assert!(!is_regular_file(&destination.join("whisper-cli.exe")));
        clear_invalid_runtime_destination(&destination, "whisper-cli.exe")
            .expect("empty runtime should be removable");
        assert!(!destination.exists());

        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn runtime_manifest_rejects_modified_user_engine() {
        let root = temporary_path("runtime-manifest");
        fs::create_dir_all(&root).expect("runtime directory should be created");
        let executable = root.join("whisper-cli.exe");
        fs::write(&executable, b"trusted runtime").expect("runtime should be written");
        write_runtime_manifest(&root, "whisper-cli.exe").expect("manifest should be written");
        assert!(runtime_manifest_matches(&root, &executable));

        fs::write(&executable, b"modified runtime").expect("runtime should be modified");
        assert!(!runtime_manifest_matches(&root, &executable));
        clear_invalid_runtime_destination(&root, "whisper-cli.exe")
            .expect("modified runtime should be replaceable");
        assert!(!root.exists());
    }

    #[test]
    fn runtime_manifest_rejects_unlisted_runtime_file() {
        let root = temporary_path("runtime-manifest-extra-file");
        fs::create_dir_all(&root).expect("runtime directory should be created");
        let executable = root.join("whisper-cli.exe");
        fs::write(&executable, b"trusted runtime").expect("runtime should be written");
        write_runtime_manifest(&root, "whisper-cli.exe").expect("manifest should be written");
        fs::write(root.join("unexpected.dll"), b"unlisted file")
            .expect("extra runtime file should be written");
        assert!(!runtime_manifest_matches(&root, &executable));
        fs::remove_dir_all(root).expect("runtime directory should be removed");
    }
}
