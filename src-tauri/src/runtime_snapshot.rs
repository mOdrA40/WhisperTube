#[cfg(unix)]
use std::io::Read;
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    fs::{self, File},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Mutex, OnceLock},
    time::SystemTime,
};

#[derive(Clone, Hash, PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    identity: Option<[u64; 3]>,
    changed: [i64; 2],
}

// Windows status probes can reuse metadata while their open handles deny writes.
// Inference always verifies from scratch.
struct StatusStamp {
    version: String,
    files: Vec<FileStamp>,
}

static STATUS_CACHE: OnceLock<Mutex<HashMap<PathBuf, StatusStamp>>> = OnceLock::new();
const MAX_STATUS_CACHE_ENTRIES: usize = 8;

// Owned by one transcription reservation or one short hardware status probe.
// Windows handles deny writes/deletion until the job releases its runtime.
pub struct RuntimeSnapshot {
    root: PathBuf,
    executable: PathBuf,
    version: Option<String>,
    stamps: Vec<FileStamp>,
    #[cfg(unix)]
    hashes: Vec<String>,
    _files: Vec<File>,
}

impl RuntimeSnapshot {
    pub fn probe_fingerprint(&self) -> Option<u64> {
        // A bundled or dev engine may load dependencies outside the
        // executable snapshot. Only reuse a probe across jobs when its
        // complete, verified inventory can be identified.
        self.version.as_ref()?;
        if self
            .stamps
            .iter()
            .any(|file| file.identity.is_none() || file.modified.is_none())
        {
            return None;
        }
        let mut hash = DefaultHasher::new();
        (&self.root, &self.version, &self.stamps).hash(&mut hash);
        #[cfg(unix)]
        self.hashes.hash(&mut hash);
        Some(hash.finish())
    }

    pub fn capture(
        executable: &Path,
        version: Option<&str>,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        Self::capture_inner(executable, version, cancelled, false)
    }

    pub(crate) fn capture_for_status(
        executable: &Path,
        version: Option<&str>,
    ) -> Result<Self, String> {
        Self::capture_inner(executable, version, &AtomicBool::new(false), true)
    }

    fn capture_inner(
        executable: &Path,
        version: Option<&str>,
        cancelled: &AtomicBool,
        for_status: bool,
    ) -> Result<Self, String> {
        let parent = executable.parent().ok_or("Folder runtime tidak valid.")?;
        if version.is_some() {
            crate::durable::require_plain_directory(parent)
                .map_err(|e| format!("Gagal memvalidasi folder runtime: {e}"))?;
        }
        let root = fs::canonicalize(parent)
            .map_err(|e| format!("Gagal memvalidasi folder runtime: {e}"))?;
        let executable = root.join(executable.file_name().ok_or("Nama runtime tidak valid.")?);
        // Acquire before hashing so concurrent Qwen model and system
        // refreshes share the first verified result for the current runtime.
        let mut cache = if cfg!(windows) && for_status && version.is_some() {
            Some(
                STATUS_CACHE
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                    .map_err(|_| "Cache status runtime terkunci.")?,
            )
        } else {
            None
        };
        let (stamps, files) = inventory(&root, &executable, version.is_some(), cancelled)?;
        #[cfg(unix)]
        let hashes = inventory_hashes(&stamps, &files, cancelled)?;
        let stable = stamps
            .iter()
            .all(|stamp| stamp.identity.is_some() && stamp.modified.is_some());
        let reuse = stable
            && version.zip(cache.as_ref()).is_some_and(|(version, cache)| {
                cache
                    .get(&executable)
                    .is_some_and(|previous| previous.version == version && previous.files == stamps)
            });
        if !reuse {
            if let Some(version) = version {
                #[cfg(unix)]
                let matches = crate::paths::runtime_manifest_matches_hashes(
                    &root,
                    &executable,
                    version,
                    stamps
                        .iter()
                        .zip(&hashes)
                        .map(|(stamp, hash)| (stamp.path.as_path(), hash.as_str())),
                );
                #[cfg(windows)]
                let matches = crate::paths::runtime_manifest_matches_cancelable(
                    &root,
                    &executable,
                    version,
                    cancelled,
                )?;
                if !matches {
                    return Err("Runtime gagal verifikasi sebelum transkripsi.".into());
                }
            }
            let (after, _) = inventory(&root, &executable, version.is_some(), cancelled)?;
            if after != stamps {
                return Err("Runtime berubah selama verifikasi.".into());
            }
            if let (Some(version), Some(cache)) = (version.filter(|_| stable), cache.as_mut()) {
                if cache.len() >= MAX_STATUS_CACHE_ENTRIES {
                    cache.clear();
                }
                cache.insert(
                    executable.clone(),
                    StatusStamp {
                        version: version.into(),
                        files: stamps.clone(),
                    },
                );
            }
        }
        Ok(Self {
            root,
            executable,
            version: version.map(str::to_owned),
            stamps,
            #[cfg(unix)]
            hashes,
            _files: files,
        })
    }

    pub fn validate(&self, cancelled: &AtomicBool) -> Result<(), String> {
        let (current, files) = inventory(
            &self.root,
            &self.executable,
            self.version.is_some(),
            cancelled,
        )?;
        if current != self.stamps {
            return Err(
                "Runtime berubah selama transkripsi. Coba lagi untuk memilih backend ulang.".into(),
            );
        }
        #[cfg(unix)]
        if inventory_hashes(&current, &files, cancelled)? != self.hashes {
            return Err("Isi runtime berubah selama transkripsi. Pilih ulang backend.".into());
        }
        #[cfg(windows)]
        let _files = files;
        // Filesystems that cannot provide a stable identity keep the original
        // full hash check. A weak size/mtime fingerprint must not skip integrity.
        #[cfg(windows)]
        if self
            .stamps
            .iter()
            .any(|stamp| stamp.identity.is_none() || stamp.modified.is_none())
        {
            if let Some(version) = &self.version {
                if !crate::paths::runtime_manifest_matches_cancelable(
                    &self.root,
                    &self.executable,
                    version,
                    cancelled,
                )? {
                    return Err("Runtime gagal verifikasi selama transkripsi.".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn inventory_hashes(
    stamps: &[FileStamp],
    files: &[File],
    cancelled: &AtomicBool,
) -> Result<Vec<String>, String> {
    // POSIX read handles do not prevent writes, and ctime can be coarse.
    files
        .iter()
        .zip(stamps)
        .map(|(file, stamp)| {
            let mut contents = file.take(stamp.size.saturating_add(1));
            let hash = crate::io_cancel::hash_reader(&mut contents, cancelled)?;
            if contents.limit() != 1 {
                return Err("Ukuran file runtime berubah selama verifikasi.".into());
            }
            Ok(hash)
        })
        .collect()
}

fn inventory(
    root: &Path,
    executable: &Path,
    all_files: bool,
    cancelled: &AtomicBool,
) -> Result<(Vec<FileStamp>, Vec<File>), String> {
    fn visit(
        root: &Path,
        path: &Path,
        entries: &mut Vec<(FileStamp, File)>,
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        crate::io_cancel::check(cancelled)?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| format!("Gagal membaca identitas runtime: {e}"))?;
        if crate::durable::is_link(&metadata) {
            return Err("Runtime berisi symbolic link.".into());
        }
        if metadata.is_dir() {
            for entry in
                fs::read_dir(path).map_err(|e| format!("Gagal membaca folder runtime: {e}"))?
            {
                visit(
                    root,
                    &entry
                        .map_err(|e| format!("Gagal membaca entry runtime: {e}"))?
                        .path(),
                    entries,
                    cancelled,
                )?;
            }
        } else if metadata.is_file() {
            if entries.len() > crate::paths::MAX_RUNTIME_FILES {
                return Err("Runtime memiliki terlalu banyak file.".into());
            }
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.custom_flags(0x00200000).share_mode(1); // FILE_SHARE_READ; no writer/delete may race the verified job.
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            let file = options
                .open(path)
                .map_err(|e| format!("Runtime tidak dapat dikunci untuk transkripsi: {e}"))?;
            let metadata = file
                .metadata()
                .map_err(|e| format!("Gagal membaca metadata runtime: {e}"))?;
            if !metadata.is_file() || crate::durable::is_link(&metadata) {
                return Err("Runtime bukan file biasa.".into());
            }
            let (identity, changed) = file_identity(&file, &metadata);
            let stamp = FileStamp {
                path: path
                    .strip_prefix(root)
                    .map_err(|_| "Path runtime tidak valid.")?
                    .into(),
                size: metadata.len(),
                modified: metadata.modified().ok(),
                identity,
                changed,
            };
            entries.push((stamp, file));
        } else {
            return Err("Runtime bukan file biasa.".into());
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(
        root,
        if all_files { root } else { executable },
        &mut entries,
        cancelled,
    )?;
    entries.sort_unstable_by(|(left, _), (right, _)| left.path.cmp(&right.path));
    Ok(entries.into_iter().unzip())
}

#[cfg(unix)]
pub(crate) fn file_identity(_file: &File, metadata: &fs::Metadata) -> (Option<[u64; 3]>, [i64; 2]) {
    use std::os::unix::fs::MetadataExt;
    (
        Some([metadata.dev(), metadata.ino(), 0]),
        [metadata.ctime(), metadata.ctime_nsec()],
    )
}

#[cfg(windows)]
pub(crate) fn file_identity(file: &File, _metadata: &fs::Metadata) -> (Option<[u64; 3]>, [i64; 2]) {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct FileId {
        volume: u64,
        id: [u8; 16],
    }
    #[repr(C)]
    struct BasicInfo {
        creation: i64,
        access: i64,
        write: i64,
        change: i64,
        attributes: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut std::ffi::c_void,
            class: i32,
            information: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let mut id = FileId {
        volume: 0,
        id: [0; 16],
    };
    // SAFETY: a live owned File handle, FileIdInfo=18, correct repr(C) buffer.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            18,
            (&mut id as *mut FileId).cast(),
            std::mem::size_of::<FileId>() as u32,
        )
    };
    let identity = (success != 0).then(|| {
        [
            id.volume,
            u64::from_ne_bytes(id.id[..8].try_into().unwrap()),
            u64::from_ne_bytes(id.id[8..].try_into().unwrap()),
        ]
    });
    let mut basic = BasicInfo {
        creation: 0,
        access: 0,
        write: 0,
        change: 0,
        attributes: 0,
    };
    // SAFETY: live File handle and correctly sized FILE_BASIC_INFO (class 0).
    let basic_ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            0,
            (&mut basic as *mut BasicInfo).cast(),
            std::mem::size_of::<BasicInfo>() as u32,
        )
    };
    if basic_ok == 0 || basic.change == 0 {
        return (None, [0, 0]);
    }
    (identity, [basic.change, 0])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_read_locked_runtime_can_still_launch_a_native_process() {
        let executable = std::env::current_exe().unwrap();
        let cancelled = AtomicBool::new(false);
        let snapshot = RuntimeSnapshot::capture(&executable, None, &cancelled).unwrap();
        snapshot.validate(&cancelled).unwrap();
        let status = std::process::Command::new(&executable)
            .args(["--exact", "process::tests::runtime_probe_child"])
            .env_remove("WHISPERTUBE_RUNTIME_PROBE_CHILD")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        snapshot.validate(&cancelled).unwrap();
    }
    #[test]
    fn unchanged_runtime_is_reusable_and_new_dependencies_invalidate_it() {
        let root = std::env::temp_dir().join(format!("runtime-snapshot-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let engine = root.join("whisper-cli.exe");
        fs::write(&engine, b"trusted executable").unwrap();
        fs::write(root.join("dependency.dll"), b"trusted dependency").unwrap();
        let cancelled = AtomicBool::new(false);
        crate::paths::write_runtime_manifest(&root, "whisper-cli.exe", "test", &cancelled).unwrap();
        let snapshot = RuntimeSnapshot::capture(&engine, Some("test"), &cancelled).unwrap();
        snapshot.validate(&cancelled).unwrap();
        snapshot.validate(&cancelled).unwrap();
        fs::write(root.join("unexpected.dll"), b"new dependency").unwrap();
        assert!(snapshot
            .validate(&cancelled)
            .unwrap_err()
            .contains("berubah"));
        drop(snapshot);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dependency_mutation_is_blocked_or_detected_even_with_restored_mtime() {
        let root = std::env::temp_dir().join(format!("runtime-snapshot-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let engine = root.join("whisper-cli.exe");
        let dependency = root.join("dependency.dll");
        fs::write(&engine, b"trusted executable").unwrap();
        fs::write(&dependency, b"trusted").unwrap();
        let modified = fs::metadata(&dependency).unwrap().modified().unwrap();
        let cancelled = AtomicBool::new(false);
        crate::paths::write_runtime_manifest(&root, "whisper-cli.exe", "test", &cancelled).unwrap();
        let snapshot = RuntimeSnapshot::capture_for_status(&engine, Some("test")).unwrap();
        drop(RuntimeSnapshot::capture_for_status(&engine, Some("test")).unwrap());
        #[cfg(windows)]
        {
            assert!(fs::write(&dependency, b"changed").is_err());
            assert!(fs::remove_file(&dependency).is_err());
            snapshot.validate(&cancelled).unwrap();
        }
        #[cfg(unix)]
        {
            fs::write(&dependency, b"changed").unwrap();
            File::options()
                .write(true)
                .open(&dependency)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(modified))
                .unwrap();
            assert!(snapshot.validate(&cancelled).is_err());
            assert!(RuntimeSnapshot::capture_for_status(&engine, Some("test")).is_err());
        }
        let _ = modified;
        drop(snapshot);
        fs::write(&dependency, b"changed").unwrap();
        assert!(RuntimeSnapshot::capture_for_status(&engine, Some("test")).is_err());
        assert!(RuntimeSnapshot::capture(&engine, Some("test"), &cancelled).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
