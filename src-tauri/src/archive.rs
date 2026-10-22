use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use zip::ZipArchive;

pub const MAX_ARCHIVE_ENTRIES: usize = 4096;
pub const MAX_EXTRACTED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub fn extract_zip_safely(
    archive_path: &Path,
    destination: &Path,
    label: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    crate::io_cancel::check(cancelled)?;
    let archive_file =
        File::open(archive_path).map_err(|e| format!("Gagal membuka archive {label}: {e}"))?;
    let mut archive =
        ZipArchive::new(archive_file).map_err(|e| format!("Archive {label} tidak valid: {e}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!("Archive {label} memiliki terlalu banyak file."));
    }
    fs::create_dir_all(destination)
        .map_err(|e| format!("Gagal membuat folder extract {label}: {e}"))?;

    let mut extracted_bytes = 0u64;
    for index in 0..archive.len() {
        crate::io_cancel::check(cancelled)?;
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Gagal membaca entry {label}: {e}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| format!("Archive {label} memiliki path tidak aman."))?
            .to_path_buf();
        extracted_bytes = extracted_bytes.saturating_add(entry.size());
        if extracted_bytes > MAX_EXTRACTED_BYTES {
            return Err(format!("Isi archive {label} melebihi batas ukuran aman."));
        }
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| format!("Gagal membuat folder {label}: {e}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Gagal membuat folder {label}: {e}"))?;
        }
        let mut output =
            File::create(&target).map_err(|e| format!("Gagal menulis file {label}: {e}"))?;
        crate::io_cancel::copy(&mut entry, &mut output, cancelled)
            .map_err(|e| format!("Gagal extract file {label}: {e}"))?;
    }
    Ok(())
}

pub fn find_file(
    root: &Path,
    file_name: &str,
    label: &str,
    cancelled: &AtomicBool,
) -> Result<Option<PathBuf>, String> {
    crate::io_cancel::check(cancelled)?;
    for entry in fs::read_dir(root).map_err(|e| format!("Gagal membaca folder {label}: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry {label}: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, file_name, label, cancelled)? {
                return Ok(Some(found));
            }
        } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub fn copy_tree(
    source: &Path,
    destination: &Path,
    label: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    crate::io_cancel::check(cancelled)?;
    fs::create_dir_all(destination).map_err(|e| format!("Gagal membuat staging {label}: {e}"))?;
    for entry in fs::read_dir(source).map_err(|e| format!("Gagal membaca folder {label}: {e}"))? {
        crate::io_cancel::check(cancelled)?;
        let entry = entry.map_err(|e| format!("Gagal membaca entry {label}: {e}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path, label, cancelled)?;
        } else {
            let mut input =
                File::open(&source_path).map_err(|e| format!("Gagal membuka {label}: {e}"))?;
            let mut output = File::create(&destination_path)
                .map_err(|e| format!("Gagal membuat {label}: {e}"))?;
            crate::io_cancel::copy(&mut input, &mut output, cancelled)
                .map_err(|e| format!("Gagal menyalin {label}: {e}"))?;
            fs::set_permissions(
                &destination_path,
                input
                    .metadata()
                    .map_err(|e| format!("Gagal membaca permission {label}: {e}"))?
                    .permissions(),
            )
            .map_err(|e| format!("Gagal menyalin permission {label}: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn fixture(root: &Path, name: &str, method: CompressionMethod) -> PathBuf {
        let path = root.join("fixture.zip");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        writer
            .add_directory("engine/", SimpleFileOptions::default())
            .unwrap();
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        writer.write_all(b"runtime fixture").unwrap();
        writer.finish().unwrap();
        path
    }

    #[test]
    fn extracts_stored_and_deflated_nested_runtime_files() {
        let root =
            std::env::temp_dir().join(format!("whispertube-zip-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let path = fixture(&root, "engine/whisper-cli", method);
            let target = root.join(format!("output-{method:?}"));
            extract_zip_safely(&path, &target, "test", &AtomicBool::new(false)).unwrap();
            assert_eq!(
                fs::read(target.join("engine/whisper-cli")).unwrap(),
                b"runtime fixture"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_parent_traversal_without_writing_outside_destination() {
        let root =
            std::env::temp_dir().join(format!("whispertube-zip-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = fixture(&root, "../outside.txt", CompressionMethod::Stored);
        assert!(
            extract_zip_safely(&path, &root.join("output"), "test", &AtomicBool::new(false))
                .is_err()
        );
        assert!(!root.join("outside.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelled_extraction_copy_and_search_stop_before_creating_outputs() {
        let root =
            std::env::temp_dir().join(format!("whispertube-zip-cancel-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let archive = fixture(&root, "engine/whisper-cli", CompressionMethod::Deflated);
        let cancelled = AtomicBool::new(true);
        let target = root.join("output");
        assert!(extract_zip_safely(&archive, &target, "test", &cancelled)
            .unwrap_err()
            .contains("dibatalkan"));
        assert!(!target.exists());
        assert!(copy_tree(&root, &target, "test", &cancelled)
            .unwrap_err()
            .contains("dibatalkan"));
        assert!(!target.exists());
        assert!(find_file(&root, "whisper-cli", "test", &cancelled)
            .unwrap_err()
            .contains("dibatalkan"));
        fs::remove_dir_all(root).unwrap();
    }
}
