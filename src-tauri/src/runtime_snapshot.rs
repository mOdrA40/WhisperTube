use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::SystemTime,
};

#[derive(PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    identity: Option<[u64; 3]>,
    changed: [i64; 2],
}

// Owned by one transcription reservation, never a process-global mtime cache.
// Windows handles deny writes/deletion until the job releases its runtime.
pub struct RuntimeSnapshot {
    root: PathBuf,
    executable: PathBuf,
    version: Option<String>,
    stamps: Vec<FileStamp>,
    _files: Vec<File>,
}

impl RuntimeSnapshot {
    pub fn capture(
        executable: &Path,
        version: Option<&str>,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        let root = fs::canonicalize(executable.parent().ok_or("Folder runtime tidak valid.")?)
            .map_err(|e| format!("Gagal memvalidasi folder runtime: {e}"))?;
        let executable = root.join(executable.file_name().ok_or("Nama runtime tidak valid.")?);
        let (stamps, files) = inventory(&root, &executable, version.is_some(), cancelled)?;
        if let Some(version) = version {
            if !crate::paths::runtime_manifest_matches_cancelable(
                &root,
                &executable,
                version,
                cancelled,
            )? {
                return Err("Runtime gagal verifikasi sebelum transkripsi.".into());
            }
        }
        let (after, _) = inventory(&root, &executable, version.is_some(), cancelled)?;
        if after != stamps {
            return Err("Runtime berubah selama verifikasi.".into());
        }
        Ok(Self {
            root,
            executable,
            version: version.map(str::to_owned),
            stamps,
            _files: files,
        })
    }

    pub fn validate(&self, cancelled: &AtomicBool) -> Result<(), String> {
        let (current, _) = inventory(
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
        // Filesystems that cannot provide a stable identity keep the original
        // full hash check. A weak size/mtime fingerprint must not skip integrity.
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

fn inventory(
    root: &Path,
    executable: &Path,
    all_files: bool,
    cancelled: &AtomicBool,
) -> Result<(Vec<FileStamp>, Vec<File>), String> {
    fn visit(
        root: &Path,
        path: &Path,
        stamps: &mut Vec<FileStamp>,
        files: &mut Vec<File>,
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        crate::io_cancel::check(cancelled)?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| format!("Gagal membaca identitas runtime: {e}"))?;
        if metadata.file_type().is_symlink() {
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
                    stamps,
                    files,
                    cancelled,
                )?;
            }
        } else if metadata.is_file() {
            if files.len() > crate::paths::MAX_RUNTIME_FILES {
                return Err("Runtime memiliki terlalu banyak file.".into());
            }
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1); // FILE_SHARE_READ; no writer/delete may race the verified job.
            }
            let file = options
                .open(path)
                .map_err(|e| format!("Runtime tidak dapat dikunci untuk transkripsi: {e}"))?;
            let metadata = file
                .metadata()
                .map_err(|e| format!("Gagal membaca metadata runtime: {e}"))?;
            let (identity, changed) = file_identity(&file, &metadata);
            stamps.push(FileStamp {
                path: path
                    .strip_prefix(root)
                    .map_err(|_| "Path runtime tidak valid.")?
                    .into(),
                size: metadata.len(),
                modified: metadata.modified().ok(),
                identity,
                changed,
            });
            files.push(file);
        } else {
            return Err("Runtime bukan file biasa.".into());
        }
        Ok(())
    }
    let (mut stamps, mut files) = (Vec::new(), Vec::new());
    visit(
        root,
        if all_files { root } else { executable },
        &mut stamps,
        &mut files,
        cancelled,
    )?;
    stamps.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    Ok((stamps, files))
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
        let snapshot = RuntimeSnapshot::capture(&engine, Some("test"), &cancelled).unwrap();
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
        }
        let _ = modified;
        drop(snapshot);
        fs::write(&dependency, b"changed").unwrap();
        assert!(RuntimeSnapshot::capture(&engine, Some("test"), &cancelled).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
