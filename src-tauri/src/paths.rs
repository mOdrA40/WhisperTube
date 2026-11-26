use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Manager};

use crate::models::model_spec;

const RUNTIME_MANIFEST_NAME: &str = "runtime-manifest.json";
const CORE_RUNTIME_MANIFEST_VERSION: &str = "whisper.cpp-v1.9.5";
const MAX_RUNTIME_MANIFEST_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_RUNTIME_FILES: usize = 256;
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
    crate::durable::create_private_dir_all(&dir)
        .map_err(|e| format!("Gagal membuat app data directory: {e}"))?;
    Ok(dir)
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    models_dir_in(&app_data_dir(app)?).map_err(|e| format!("Gagal membuka folder models: {e}"))
}

fn models_dir_in(app_data: &Path) -> std::io::Result<PathBuf> {
    let path = app_data.join("models");
    create_plain_dir(&path)?;
    Ok(path)
}

pub fn jobs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("jobs");
    crate::durable::create_private_dir_all(&dir)
        .map_err(|e| format!("Gagal membuat folder jobs: {e}"))?;
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
    user_runtime_dir_in(&app_data_dir(app)?)
        .map_err(|e| format!("Gagal membuka folder runtime user: {e}"))
}

fn user_runtime_dir_in(app_data: &Path) -> std::io::Result<PathBuf> {
    let runtime = app_data.join("runtime");
    let platform = runtime.join(runtime_platform());
    create_plain_dir(&runtime)?;
    create_plain_dir(&platform)?;
    Ok(platform)
}

fn create_plain_dir(path: &Path) -> std::io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => crate::durable::sync_parent(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    crate::durable::require_plain_directory(path)
}

pub(crate) fn exe_name(base: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub fn tool_path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(runtime_dir(app)?.join(exe_name(name)))
}

pub(crate) fn whisper_cli_command(engine: &Path) -> Command {
    let mut command = Command::new(engine);
    // The pinned Windows archives dynamically load ggml backends. Scope the
    // default working-directory search to the packaged runtime and clear the
    // upstream option for loading an out-of-tree backend.
    if let Some(directory) = engine.parent().filter(|path| !path.as_os_str().is_empty()) {
        command.current_dir(directory);
    }
    command.env_remove("GGML_BACKEND_PATH");
    crate::process::hide_console(&mut command);
    command
}

pub fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file() && metadata.len() > 0 && !crate::durable::is_link(&metadata)
    })
}

pub fn core_runtime_manifest_version() -> &'static str {
    CORE_RUNTIME_MANIFEST_VERSION
}

pub fn accelerator_runtime_manifest_version() -> &'static str {
    crate::accelerators::PACK_GENERATION
}

pub fn write_runtime_manifest(
    staging: &Path,
    executable: &str,
    version: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    crate::io_cancel::check(cancelled)?;
    if !is_regular_file(&staging.join(executable)) {
        return Err("Executable runtime tidak ditemukan.".into());
    }
    let mut files = Vec::new();
    collect_runtime_files(staging, staging, &mut files, cancelled)?;
    if files.len() > MAX_RUNTIME_FILES {
        return Err("Runtime memiliki terlalu banyak file.".into());
    }
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let manifest = RuntimeManifest {
        version: version.into(),
        executable: executable.into(),
        files,
    };
    let bytes = serde_json::to_vec(&manifest)
        .map_err(|e| format!("Gagal membuat manifest runtime: {e}"))?;
    let manifest_path = staging.join(RUNTIME_MANIFEST_NAME);
    let temporary = staging.join(format!("{RUNTIME_MANIFEST_NAME}.tmp"));
    crate::io_cancel::check(cancelled)?;
    let mut file = File::create(&temporary)
        .map_err(|e| format!("Gagal membuat manifest runtime sementara: {e}"))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Gagal menulis manifest runtime: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Gagal flush manifest runtime: {e}"))?;
    drop(file);
    crate::io_cancel::check(cancelled)?;
    fs::rename(&temporary, &manifest_path)
        .map_err(|e| format!("Gagal mengaktifkan manifest runtime: {e}"))?;
    Ok(())
}

fn collect_runtime_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<RuntimeFile>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    crate::io_cancel::check(cancelled)?;
    for entry in fs::read_dir(directory).map_err(|e| format!("Gagal membaca runtime: {e}"))? {
        crate::io_cancel::check(cancelled)?;
        let entry = entry.map_err(|e| format!("Gagal membaca entry runtime: {e}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Gagal membaca tipe file runtime: {e}"))?;
        if file_type.is_symlink() {
            return Err("Runtime berisi symbolic link yang tidak diizinkan.".into());
        }
        if file_type.is_dir() {
            collect_runtime_files(root, &path, files, cancelled)?;
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
            sha256: crate::io_cancel::file_sha256(&path, cancelled)?,
        });
        if files.len() > MAX_RUNTIME_FILES {
            return Err("Runtime memiliki terlalu banyak file.".into());
        }
    }
    Ok(())
}

fn runtime_manifest_header(
    runtime_dir: &Path,
    executable: &Path,
    expected_version: &str,
) -> Option<RuntimeManifest> {
    let Ok(runtime_metadata) = fs::symlink_metadata(runtime_dir) else {
        return None;
    };
    if !runtime_metadata.is_dir() {
        return None;
    }
    let manifest_path = runtime_dir.join(RUNTIME_MANIFEST_NAME);
    let Ok(file) = crate::durable::open_regular_read(&manifest_path) else {
        return None;
    };
    if file.metadata().ok()?.len() > MAX_RUNTIME_MANIFEST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_RUNTIME_MANIFEST_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_RUNTIME_MANIFEST_BYTES
    {
        return None;
    }
    let manifest = serde_json::from_slice::<RuntimeManifest>(&bytes).ok()?;
    let name = executable.file_name().and_then(|name| name.to_str())?;
    if manifest.version != expected_version
        || manifest.executable != name
        || manifest.files.len() > MAX_RUNTIME_FILES
    {
        return None;
    }
    let executable_relative = executable
        .strip_prefix(runtime_dir)
        .ok()
        .and_then(|path| path.to_str());
    if executable_relative.is_none_or(|path| !manifest.files.iter().any(|file| file.path == path)) {
        return None;
    }
    if manifest
        .files
        .iter()
        .any(|file| !is_safe_manifest_path(&file.path))
    {
        return None;
    }
    Some(manifest)
}

#[cfg(test)]
fn runtime_manifest_matches(runtime_dir: &Path, executable: &Path, expected_version: &str) -> bool {
    runtime_manifest_matches_cancelable(
        runtime_dir,
        executable,
        expected_version,
        &AtomicBool::new(false),
    )
    .unwrap_or(false)
}

pub(crate) fn runtime_manifest_matches_cancelable(
    runtime_dir: &Path,
    executable: &Path,
    expected_version: &str,
    cancelled: &AtomicBool,
) -> Result<bool, String> {
    crate::io_cancel::check(cancelled)?;
    let Some(manifest) = runtime_manifest_header(runtime_dir, executable, expected_version) else {
        return Ok(false);
    };
    let mut actual = Vec::new();
    collect_runtime_files(runtime_dir, runtime_dir, &mut actual, cancelled)?;
    Ok(runtime_files_match(manifest.files, actual))
}

#[cfg(unix)]
pub(crate) fn runtime_manifest_matches_hashes<'a>(
    runtime_dir: &Path,
    executable: &Path,
    expected_version: &str,
    hashes: impl Iterator<Item = (&'a Path, &'a str)>,
) -> bool {
    let Some(manifest) = runtime_manifest_header(runtime_dir, executable, expected_version) else {
        return false;
    };
    let actual = hashes
        .filter(|(path, _)| {
            !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(RUNTIME_MANIFEST_NAME | "runtime-manifest.json.tmp")
            )
        })
        .map(|(path, sha256)| RuntimeFile {
            path: path.to_string_lossy().into_owned(),
            sha256: sha256.to_owned(),
        })
        .collect();
    runtime_files_match(manifest.files, actual)
}

fn runtime_files_match(mut expected: Vec<RuntimeFile>, mut actual: Vec<RuntimeFile>) -> bool {
    expected.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    actual.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    expected == actual
}

#[cfg(test)]
fn validated_user_runtime(runtime_dir: &Path, executable: &Path, expected_version: &str) -> bool {
    is_regular_file(executable)
        && runtime_manifest_matches(runtime_dir, executable, expected_version)
}

fn is_safe_manifest_path(value: &str) -> bool {
    let path = Path::new(value);
    path.is_relative()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn cleanup_stale_entries(
    directory: &Path,
    directories: bool,
    matches: impl Fn(&str) -> bool,
) -> Vec<String> {
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
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                errors.push(format!("Gagal memeriksa {}: {error}", path.display()));
                continue;
            }
        };
        if metadata.file_type().is_symlink()
            || (directories && !metadata.is_dir())
            || (!directories && !metadata.is_file())
        {
            continue;
        }
        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(error) => {
                errors.push(format!("Gagal membaca waktu {}: {error}", path.display()));
                continue;
            }
        };
        let stale = SystemTime::now()
            .duration_since(modified)
            .ok()
            .is_some_and(|age| age >= TRANSIENT_STORAGE_GRACE);
        if !stale {
            continue;
        }
        let result = fs::symlink_metadata(&path).and_then(|metadata| {
            if metadata.file_type().is_symlink()
                || (directories && !metadata.is_dir())
                || (!directories && !metadata.is_file())
            {
                return Ok(());
            }
            if directories {
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

fn owned_uuid_name(name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|suffix| {
            uuid::Uuid::parse_str(suffix).is_ok() && (suffix.len() == 36 || suffix.len() == 32)
        })
    })
}

fn owned_temp_directory(name: &str) -> bool {
    owned_uuid_name(
        name,
        &[
            "whispertube-cuda-",
            "whispertube-accelerator-",
            "whispertube-cuda-probe-",
            "whispertube-metal-probe-",
            "whispertube-vulkan-probe-",
            "whispertube-media-probe-",
        ],
    )
}

pub fn cleanup_stale_transient_storage(app: &AppHandle) -> Result<(), String> {
    let mut errors = match user_runtime_dir(app) {
        Ok(runtime_root) => cleanup_stale_entries(&runtime_root, true, |name| {
            owned_uuid_name(
                name,
                &[
                    ".cuda-staging-",
                    ".vulkan-staging-",
                    ".metal-staging-",
                    ".qwen-staging-",
                ],
            )
        }),
        Err(error) => vec![error],
    };
    let temp_root = std::env::temp_dir();
    errors.extend(cleanup_stale_entries(
        &temp_root,
        true,
        owned_temp_directory,
    ));
    match models_dir(app) {
        Ok(models_root) => errors.extend(cleanup_stale_entries(&models_root, false, |name| {
            name.ends_with(".bin.download")
        })),
        Err(error) => errors.push(error),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn clear_invalid_runtime_destination(
    destination: &Path,
    executable: &str,
    expected_version: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    crate::io_cancel::check(cancelled)?;
    if !destination.exists() {
        return Ok(());
    }
    if is_regular_file(&destination.join(executable))
        && runtime_manifest_matches_cancelable(
            destination,
            &destination.join(executable),
            expected_version,
            cancelled,
        )?
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
        crate::io_cancel::check(cancelled)?;
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

pub fn activate_runtime(
    staging: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    // Commit boundary: once rename starts, the installed runtime wins over a
    // concurrent late cancellation. Before this point cancellation must abort.
    crate::io_cancel::check(cancelled)?;
    fs::rename(staging, destination).map_err(|e| format!("Gagal mengaktifkan runtime: {e}"))
}

fn backend_manifest_version(backend: &str) -> &'static str {
    if matches!(backend, "metal" | "vulkan") {
        accelerator_runtime_manifest_version()
    } else {
        CORE_RUNTIME_MANIFEST_VERSION
    }
}

pub fn engine_path(app: &AppHandle, backend: &str) -> Result<PathBuf, String> {
    engine_path_cancelable(app, backend, &AtomicBool::new(false))
}

pub fn engine_path_cancelable(
    app: &AppHandle,
    backend: &str,
    cancelled: &AtomicBool,
) -> Result<PathBuf, String> {
    crate::io_cancel::check(cancelled)?;
    if let Some(path) = preferred_bundled_metal(&runtime_dir(app)?, backend) {
        return Ok(path);
    }
    let Some(user_backend_dir) = user_engine_dir(app, backend) else {
        return fallback_engine_path(app, backend);
    };
    let user_path = user_backend_dir.join(exe_name("whisper-cli"));
    let expected_version = backend_manifest_version(backend);
    let valid = if is_regular_file(&user_path) {
        match runtime_manifest_matches_cancelable(
            &user_backend_dir,
            &user_path,
            expected_version,
            cancelled,
        ) {
            Ok(valid) => valid,
            Err(error) => {
                crate::io_cancel::check(cancelled)?;
                eprintln!("WhisperTube user runtime validation warning: {error}");
                false
            }
        }
    } else {
        false
    };
    if valid {
        return Ok(user_path);
    }
    fallback_engine_path(app, backend)
}

fn user_engine_dir(app: &AppHandle, backend: &str) -> Option<PathBuf> {
    match user_runtime_dir(app) {
        Ok(path) => Some(path.join(backend)),
        Err(error) => {
            eprintln!("WhisperTube user runtime directory warning: {error}");
            None
        }
    }
}

fn preferred_bundled_metal(runtime: &Path, backend: &str) -> Option<PathBuf> {
    // The macOS bundle already ships the updated engine. A legacy separately
    // downloaded Metal pack must not take precedence over it.
    if backend != "metal" {
        return None;
    }
    let path = runtime.join(backend).join(exe_name("whisper-cli"));
    is_regular_file(&path).then_some(path)
}

fn fallback_engine_path(app: &AppHandle, backend: &str) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        let development_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime-dev")
            .join(runtime_platform())
            .join(backend)
            .join(exe_name("whisper-cli"));
        if is_regular_file(&development_path) {
            return Ok(development_path);
        }
    }
    Ok(runtime_dir(app)?
        .join(backend)
        .join(exe_name("whisper-cli")))
}

pub(crate) fn engine_candidate_exists(app: &AppHandle, backend: &str) -> bool {
    // Auto uses this only to skip absent engines. A candidate still needs a
    // verified snapshot before enumeration or inference.
    user_engine_dir(app, backend)
        .is_some_and(|directory| is_regular_file(&directory.join(exe_name("whisper-cli"))))
        || fallback_engine_path(app, backend).is_ok_and(|path| is_regular_file(&path))
}

/// Resolve and retain the same verified files throughout probe and inference.
pub fn engine_with_snapshot(
    app: &AppHandle,
    backend: &str,
    cancelled: &AtomicBool,
) -> Result<(PathBuf, crate::runtime_snapshot::RuntimeSnapshot), String> {
    crate::io_cancel::check(cancelled)?;
    if let Some(path) = preferred_bundled_metal(&runtime_dir(app)?, backend) {
        let snapshot = crate::runtime_snapshot::RuntimeSnapshot::capture(&path, None, cancelled)?;
        return Ok((path, snapshot));
    }
    let version = backend_manifest_version(backend);
    if let Some(user_path) = user_engine_dir(app, backend)
        .map(|path| path.join(exe_name("whisper-cli")))
        .filter(|path| is_regular_file(path))
    {
        match crate::runtime_snapshot::RuntimeSnapshot::capture(
            &user_path,
            Some(version),
            cancelled,
        ) {
            Ok(snapshot) => return Ok((user_path, snapshot)),
            Err(error) => {
                crate::io_cancel::check(cancelled)?;
                eprintln!("WhisperTube user runtime validation warning: {error}");
            }
        }
    }
    let path = fallback_engine_path(app, backend)?;
    let snapshot = crate::runtime_snapshot::RuntimeSnapshot::capture(&path, None, cancelled)?;
    Ok((path, snapshot))
}

pub(crate) fn engine_for_status(
    app: &AppHandle,
    backend: &str,
) -> Result<Option<(PathBuf, crate::runtime_snapshot::RuntimeSnapshot)>, String> {
    use crate::runtime_snapshot::RuntimeSnapshot;

    if let Some(path) = preferred_bundled_metal(&runtime_dir(app)?, backend) {
        let snapshot = RuntimeSnapshot::capture_for_status(&path, None)?;
        return Ok(Some((path, snapshot)));
    }
    if let Some(user_path) = user_engine_dir(app, backend)
        .map(|path| path.join(exe_name("whisper-cli")))
        .filter(|path| is_regular_file(path))
    {
        let version = Some(backend_manifest_version(backend));
        let snapshot = if backend == "vulkan" {
            // Status also executes this CLI to discover GPUs. A metadata
            // cache hit may describe availability, but may not authorize spawn.
            RuntimeSnapshot::capture(&user_path, version, &AtomicBool::new(false))
        } else {
            RuntimeSnapshot::capture_for_status(&user_path, version)
        };
        match snapshot {
            Ok(snapshot) => return Ok(Some((user_path, snapshot))),
            Err(error) => eprintln!("WhisperTube runtime status validation warning: {error}"),
        }
    }

    let path = fallback_engine_path(app, backend)?;
    if !is_regular_file(&path) {
        return Ok(None);
    }
    let snapshot = if backend == "vulkan" {
        RuntimeSnapshot::capture(&path, None, &AtomicBool::new(false))?
    } else {
        RuntimeSnapshot::capture_for_status(&path, None)?
    };
    Ok(Some((path, snapshot)))
}

/// Reports whether the same validated engine selected for inference is
/// available. User-installed runtimes are fully checked against their manifest.
pub fn engine_available(app: &AppHandle, backend: &str) -> Result<bool, String> {
    Ok(is_regular_file(&engine_path(app, backend)?))
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let _ = model_spec(id)?;
    Ok(models_dir(app)?.join(crate::models::model_filename(id)))
}

#[cfg(test)]
mod tests {
    use super::{
        clear_invalid_runtime_destination, is_regular_file, runtime_manifest_matches,
        validated_user_runtime, write_runtime_manifest,
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
    #[cfg(any(unix, windows))]
    fn managed_storage_rejects_redirected_models_and_runtime_directories() {
        let storage = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = storage.path().join("runtime");
        #[cfg(unix)]
        let link = std::os::unix::fs::symlink;
        #[cfg(windows)]
        let link = |target: &std::path::Path, path: &std::path::Path| {
            let status = std::process::Command::new("pwsh")
                .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:WHISPERTUBE_TEST_LINK -Target $env:WHISPERTUBE_TEST_TARGET | Out-Null"])
                .env("WHISPERTUBE_TEST_LINK", path)
                .env("WHISPERTUBE_TEST_TARGET", target)
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other("Could not create test junction"))
            }
        };
        let models = storage.path().join("models");
        link(outside.path(), &models).unwrap();
        assert!(super::models_dir_in(storage.path()).is_err());
        #[cfg(unix)]
        fs::remove_file(&models).unwrap();
        #[cfg(windows)]
        fs::remove_dir(&models).unwrap();
        assert_eq!(super::models_dir_in(storage.path()).unwrap(), models);

        link(outside.path(), &root).unwrap();
        assert!(super::user_runtime_dir_in(storage.path()).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        #[cfg(unix)]
        fs::remove_file(&root).unwrap();
        #[cfg(windows)]
        fs::remove_dir(&root).unwrap();

        fs::create_dir(&root).unwrap();
        let path = root.join(super::runtime_platform());
        link(outside.path(), &path).unwrap();
        assert!(super::user_runtime_dir_in(storage.path()).is_err());
        #[cfg(unix)]
        fs::remove_file(&path).unwrap();
        #[cfg(windows)]
        fs::remove_dir(&path).unwrap();

        assert_eq!(super::user_runtime_dir_in(storage.path()).unwrap(), path);
    }

    #[test]
    fn only_available_bundled_metal_takes_precedence_over_user_runtime() {
        let runtime = tempfile::tempdir().unwrap();
        let filename = super::exe_name("whisper-cli");
        for backend in ["metal", "vulkan", "cpu"] {
            let directory = runtime.path().join(backend);
            let engine = directory.join(&filename);
            assert_eq!(
                super::preferred_bundled_metal(runtime.path(), backend),
                None
            );
            fs::create_dir(directory).unwrap();
            fs::write(&engine, b"bundled engine").unwrap();
            let expected = (backend == "metal").then_some(engine);
            assert_eq!(
                super::preferred_bundled_metal(runtime.path(), backend),
                expected
            );
        }
    }

    #[test]
    fn whisper_cli_scopes_dynamic_backend_loading_to_its_runtime_directory() {
        let runtime = tempfile::tempdir().unwrap();
        let engine = runtime.path().join(super::exe_name("whisper-cli"));
        let command = super::whisper_cli_command(&engine);
        assert_eq!(command.get_program(), engine.as_os_str());
        assert_eq!(command.get_current_dir(), Some(runtime.path()));
        assert!(command
            .get_envs()
            .any(|(key, value)| { key == "GGML_BACKEND_PATH" && value.is_none() }));
    }

    #[test]
    fn accelerator_pack_generation_rejects_and_replaces_legacy_runtime() {
        let root = temporary_path("pack-generation");
        let destination = root.join("vulkan");
        let staging = root.join("staging");
        fs::create_dir_all(&destination).unwrap();
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        fs::write(destination.join("whisper-cli.exe"), b"legacy runtime").unwrap();
        write_runtime_manifest(
            &destination,
            "whisper-cli.exe",
            "whisper.cpp-v1.9.2",
            &cancelled,
        )
        .unwrap();
        let version = super::accelerator_runtime_manifest_version();
        assert!(!runtime_manifest_matches(
            &destination,
            &destination.join("whisper-cli.exe"),
            version
        ));
        clear_invalid_runtime_destination(&destination, "whisper-cli.exe", version, &cancelled)
            .unwrap();
        assert!(!destination.exists());
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("whisper-cli.exe"), b"corrected runtime").unwrap();
        write_runtime_manifest(&staging, "whisper-cli.exe", version, &cancelled).unwrap();
        super::activate_runtime(&staging, &destination, &cancelled).unwrap();
        assert!(runtime_manifest_matches(
            &destination,
            &destination.join("whisper-cli.exe"),
            version
        ));
        assert!(clear_invalid_runtime_destination(
            &destination,
            "whisper-cli.exe",
            version,
            &cancelled
        )
        .is_err());
        assert_eq!(
            fs::read(destination.join("whisper-cli.exe")).unwrap(),
            b"corrected runtime"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transient_cleanup_only_removes_expired_owned_directories() {
        let root = temporary_path("cleanup");
        fs::create_dir_all(&root).unwrap();
        let old = format!("whispertube-cuda-{}", uuid::Uuid::new_v4());
        let old_media = format!("whispertube-media-probe-{}", uuid::Uuid::new_v4());
        let new = format!("whispertube-metal-probe-{}", uuid::Uuid::new_v4());
        let matching_file = format!("whispertube-accelerator-{}", uuid::Uuid::new_v4());
        for name in [&old, &old_media, &new, "whispertube-cuda-personal-backup"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        fs::write(root.join(&matching_file), b"preserve").unwrap();
        fs::write(
            root.join("whispertube-cuda-personal-backup.zip"),
            b"preserve",
        )
        .unwrap();
        for name in [
            &old,
            &old_media,
            &matching_file,
            "whispertube-cuda-personal-backup",
            "whispertube-cuda-personal-backup.zip",
        ] {
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            options.write(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.custom_flags(0x02000000);
            }
            options
                .open(root.join(name))
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(
                        SystemTime::now() - std::time::Duration::from_secs(48 * 3600),
                    ),
                )
                .unwrap();
        }
        assert!(super::cleanup_stale_entries(&root, true, super::owned_temp_directory).is_empty());
        assert!(!root.join(old).exists());
        assert!(!root.join(old_media).exists());
        assert!(root.join(new).is_dir());
        assert!(root.join(matching_file).is_file());
        assert!(root.join("whispertube-cuda-personal-backup").is_dir());
        assert!(root.join("whispertube-cuda-personal-backup.zip").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_runtime_directory_is_not_treated_as_an_engine_and_is_replaced() {
        let root = temporary_path("invalid-runtime");
        let destination = root.join("vulkan");
        fs::create_dir_all(destination.join("whisper-cli.exe"))
            .expect("test runtime directory should be created");

        assert!(!is_regular_file(&destination.join("whisper-cli.exe")));
        clear_invalid_runtime_destination(
            &destination,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("invalid runtime directory should be removable");
        assert!(!destination.exists());

        fs::create_dir_all(&destination).expect("test runtime directory should be created");
        fs::write(destination.join("whisper-cli.exe"), [])
            .expect("empty test runtime should be created");
        assert!(!is_regular_file(&destination.join("whisper-cli.exe")));
        clear_invalid_runtime_destination(
            &destination,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &std::sync::atomic::AtomicBool::new(false),
        )
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
        write_runtime_manifest(
            &root,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("manifest should be written");
        assert!(runtime_manifest_matches(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));
        assert!(validated_user_runtime(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));

        fs::write(&executable, b"modified runtime").expect("runtime should be modified");
        assert!(!runtime_manifest_matches(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));
        assert!(!validated_user_runtime(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));
        clear_invalid_runtime_destination(
            &root,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("modified runtime should be replaceable");
        assert!(!root.exists());
    }

    #[test]
    fn runtime_availability_rejects_missing_manifested_dependency() {
        let root = temporary_path("runtime-manifest-missing-dependency");
        fs::create_dir_all(&root).expect("runtime directory should be created");
        let executable = root.join("whisper-cli.exe");
        let dependency = root.join("whisper.dll");
        fs::write(&executable, b"trusted runtime").expect("runtime should be written");
        fs::write(&dependency, b"trusted dependency").expect("dependency should be written");
        write_runtime_manifest(
            &root,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("manifest should be written");
        assert!(validated_user_runtime(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));

        fs::remove_file(&dependency).expect("dependency should be removed");
        assert!(!validated_user_runtime(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));
        fs::remove_dir_all(root).expect("runtime directory should be removed");
    }

    #[test]
    fn runtime_manifest_rejects_unlisted_runtime_file() {
        let root = temporary_path("runtime-manifest-extra-file");
        fs::create_dir_all(&root).expect("runtime directory should be created");
        let executable = root.join("whisper-cli.exe");
        fs::write(&executable, b"trusted runtime").expect("runtime should be written");
        write_runtime_manifest(
            &root,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("manifest should be written");
        fs::write(root.join("unexpected.dll"), b"unlisted file")
            .expect("extra runtime file should be written");
        assert!(!runtime_manifest_matches(
            &root,
            &executable,
            super::core_runtime_manifest_version()
        ));
        fs::remove_dir_all(root).expect("runtime directory should be removed");
    }

    #[test]
    fn cancelled_runtime_is_not_activated_even_after_manifest_was_written() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let root = temporary_path("runtime-activation-cancel");
        let staging = root.join("staging");
        let destination = root.join("installed");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("whisper-cli.exe"), b"runtime").unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(write_runtime_manifest(
            &staging,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &cancelled
        )
        .unwrap_err()
        .contains("dibatalkan"));
        assert!(!staging.join(super::RUNTIME_MANIFEST_NAME).exists());
        cancelled.store(false, Ordering::SeqCst);
        write_runtime_manifest(
            &staging,
            "whisper-cli.exe",
            super::core_runtime_manifest_version(),
            &cancelled,
        )
        .unwrap();
        cancelled.store(true, Ordering::SeqCst);
        assert!(super::activate_runtime(&staging, &destination, &cancelled)
            .unwrap_err()
            .contains("dibatalkan"));
        assert!(!destination.exists());
        assert!(staging.join(super::RUNTIME_MANIFEST_NAME).is_file());
        cancelled.store(false, Ordering::SeqCst);
        super::activate_runtime(&staging, &destination, &cancelled).unwrap();
        assert!(!staging.exists());
        assert_eq!(
            fs::read(destination.join("whisper-cli.exe")).unwrap(),
            b"runtime"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
