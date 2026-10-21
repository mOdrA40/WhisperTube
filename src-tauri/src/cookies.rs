use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};
use tauri::AppHandle;
use uuid::Uuid;

const MAX_COOKIE_BYTES: u64 = 256 * 1024 * 1024;

// yt-dlp writes its cookie jar on exit. The original export is read only.
pub struct CookieJar {
    directory: Option<PathBuf>,
}

impl CookieJar {
    pub fn prepare(app: &AppHandle, source: Option<&str>) -> Result<Self, String> {
        Self::copy_in(
            &crate::paths::app_data_dir(app)?.join("cookie-jars"),
            source,
        )
    }

    fn copy_in(root: &Path, source: Option<&str>) -> Result<Self, String> {
        let Some(source) = source.filter(|value| !value.is_empty()) else {
            return Ok(Self { directory: None });
        };
        let input =
            File::open(source).map_err(|e| format!("File cookies.txt tidak bisa dibaca: {e}"))?;
        let metadata = input
            .metadata()
            .map_err(|e| format!("Gagal membaca metadata cookies: {e}"))?;
        if !metadata.is_file() || metadata.len() > MAX_COOKIE_BYTES {
            return Err("File cookies.txt bukan file biasa atau terlalu besar.".into());
        }
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .recursive(true)
            .create(root)
            .map_err(|e| format!("Gagal membuat storage cookies: {e}"))?;
        if fs::symlink_metadata(root)
            .map_err(|e| format!("Gagal memvalidasi storage cookies: {e}"))?
            .file_type()
            .is_symlink()
        {
            return Err("Storage cookies tidak boleh berupa symbolic link.".into());
        }
        let directory = root.join(Uuid::new_v4().to_string());
        builder
            .recursive(false)
            .create(&directory)
            .map_err(|e| format!("Gagal membuat cookie jar: {e}"))?;
        let jar = Self {
            directory: Some(directory.clone()),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options
            .open(directory.join("cookies.txt"))
            .map_err(|e| format!("Gagal membuat salinan cookies: {e}"))?;
        let copied = io::copy(&mut input.take(MAX_COOKIE_BYTES + 1), &mut output)
            .map_err(|e| format!("Gagal menyalin cookies: {e}"))?;
        if copied > MAX_COOKIE_BYTES {
            return Err("File cookies.txt terlalu besar.".into());
        }
        Ok(jar)
    }

    pub fn args(&self) -> Vec<String> {
        self.directory
            .as_ref()
            .map(|dir| {
                vec![
                    "--cookies".into(),
                    dir.join("cookies.txt").to_string_lossy().into_owned(),
                ]
            })
            .unwrap_or_default()
    }
}

impl Drop for CookieJar {
    fn drop(&mut self) {
        if let Some(directory) = &self.directory {
            if let Err(error) = fs::remove_dir_all(directory) {
                eprintln!("WhisperTube cookie cleanup warning: {error}");
            }
        }
    }
}

// Startup only: no live operation owns these directories yet.
pub fn cleanup_stale(app: &AppHandle) -> Result<(), String> {
    cleanup_stale_in(&crate::paths::app_data_dir(app)?.join("cookie-jars"))
}

fn cleanup_stale_in(root: &Path) -> Result<(), String> {
    match fs::symlink_metadata(root) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Gagal membaca storage cookies: {e}")),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err("Storage cookies bukan direktori biasa.".into());
        }
        Ok(_) => {}
    }
    let canonical_root =
        fs::canonicalize(root).map_err(|e| format!("Gagal memvalidasi cookies: {e}"))?;
    for entry in fs::read_dir(root).map_err(|e| format!("Gagal membaca cookies: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca cookie jar: {e}"))?;
        let kind = entry
            .file_type()
            .map_err(|e| format!("Gagal membaca tipe cookie jar: {e}"))?;
        if !kind.is_dir()
            || kind.is_symlink()
            || Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err()
        {
            continue;
        }
        let path = fs::canonicalize(entry.path())
            .map_err(|e| format!("Gagal memvalidasi cookie jar: {e}"))?;
        if path.parent() != Some(canonical_root.as_path()) {
            return Err("Cookie jar berada di luar storage.".into());
        }
        fs::remove_dir_all(path).map_err(|e| format!("Gagal membersihkan cookie jar lama: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_copy_preserves_original_and_cleans_up() {
        let root = std::env::temp_dir().join(format!("cookie-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let original = root.join("original.txt");
        fs::write(&original, b"original secret").unwrap();
        let original_permissions = fs::metadata(&original).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&original, readonly).unwrap();
        let jar = CookieJar::copy_in(&root.join("jars"), Some(original.to_str().unwrap())).unwrap();
        let copy = PathBuf::from(&jar.args()[1]);
        assert_ne!(copy, original);
        assert_eq!(fs::read(&copy).unwrap(), b"original secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(copy.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        fs::write(&copy, b"yt-dlp updated jar").unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"original secret");
        drop(jar);
        assert!(!copy.exists());
        let mut stale =
            CookieJar::copy_in(&root.join("jars"), Some(original.to_str().unwrap())).unwrap();
        cleanup_stale_in(&root.join("jars")).unwrap();
        assert!(!Path::new(&stale.args()[1]).exists());
        stale.directory = None;
        assert!(CookieJar::copy_in(&root.join("jars"), None)
            .unwrap()
            .args()
            .is_empty());
        assert!(CookieJar::copy_in(&root.join("jars"), Some("missing-cookie-file")).is_err());
        fs::set_permissions(&original, original_permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_cookie_file_is_rejected_without_copying() {
        let root = std::env::temp_dir().join(format!("cookie-size-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("oversized.txt");
        File::create(&path)
            .unwrap()
            .set_len(MAX_COOKIE_BYTES + 1)
            .unwrap();
        assert!(CookieJar::copy_in(&root.join("jars"), Some(path.to_str().unwrap())).is_err());
        assert!(!root.join("jars").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
