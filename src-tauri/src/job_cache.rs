use crate::{history, paths, types::JobCacheEntry};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

fn ordinary_job(root: &Path, id: &str) -> Result<PathBuf, String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| "Identitas cache job tidak valid.")?;
    if uuid.to_string() != id {
        return Err("Identitas cache job tidak valid.".into());
    }
    let path = root.join(id);
    let metadata =
        fs::symlink_metadata(&path).map_err(|e| format!("Cache job tidak tersedia: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Cache job bukan direktori biasa.".into());
    }
    let path = fs::canonicalize(path).map_err(|e| format!("Gagal memvalidasi cache job: {e}"))?;
    if path.parent() != Some(root) {
        return Err("Cache job berada di luar storage WhisperTube.".into());
    }
    Ok(path)
}

fn list_in(root: &Path, committed: &HashSet<PathBuf>) -> Result<Vec<JobCacheEntry>, String> {
    let root =
        fs::canonicalize(root).map_err(|e| format!("Gagal memvalidasi cache storage: {e}"))?;
    let mut items = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("Gagal membaca cache storage: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca cache job: {e}"))?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let directory = ordinary_job(&root, &id)?;
        if committed.contains(&directory) {
            continue;
        }
        let (title, source_url) =
            crate::transcription::cached_job_source(&directory).unwrap_or_default();
        let bytes = match history::storage_tree_bytes(&directory) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                eprintln!("WhisperTube cache size warning ({id}): {error}");
                None
            }
        };
        let modified = fs::metadata(&directory)
            .and_then(|metadata| metadata.modified())
            .map_err(|e| format!("Gagal membaca waktu cache job: {e}"))?;
        items.push(JobCacheEntry {
            id,
            title,
            source_url,
            bytes,
            modified_at: chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339(),
        });
    }
    items.sort_unstable_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then(left.id.cmp(&right.id))
    });
    Ok(items)
}

fn discard_in(root: &Path, committed: &HashSet<PathBuf>, id: &str) -> Result<(), String> {
    let root =
        fs::canonicalize(root).map_err(|e| format!("Gagal memvalidasi cache storage: {e}"))?;
    let directory = ordinary_job(&root, id)?;
    if committed.contains(&directory) {
        return Err("Job masih dirujuk history dan tidak boleh dihapus sebagai cache.".into());
    }
    fs::remove_dir_all(directory).map_err(|e| format!("Gagal menghapus cache job: {e}"))
}

pub fn list(app: &AppHandle) -> Result<Vec<JobCacheEntry>, String> {
    list_in(
        &paths::jobs_dir(app)?,
        &history::referenced_job_directories(app)?,
    )
}

pub fn discard(app: &AppHandle, id: &str) -> Result<(), String> {
    let result = discard_in(
        &paths::jobs_dir(app)?,
        &history::referenced_job_directories(app)?,
        id,
    );
    history::invalidate_job_storage_cache();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selective_discard_preserves_committed_jobs_and_other_resumable_work() {
        let fixture =
            std::env::temp_dir().join(format!("whispertube-cache-test-{}", uuid::Uuid::new_v4()));
        let root = fixture.join("jobs");
        fs::create_dir_all(&root).unwrap();
        fs::write(fixture.join("model.bin"), b"installed model").unwrap();
        let ids: Vec<String> = (0..3).map(|_| uuid::Uuid::new_v4().to_string()).collect();
        for id in &ids {
            fs::create_dir(root.join(id)).unwrap();
            fs::write(root.join(id).join("audio.wav"), b"retained audio").unwrap();
        }
        let committed = HashSet::from([fs::canonicalize(root.join(&ids[0])).unwrap()]);
        let listed = list_in(&root, &committed).unwrap();
        let before = history::storage_tree_bytes(&root).unwrap();
        assert!(crate::transcription::ensure_job_storage_budget(before, before).is_err());
        assert_eq!(listed.len(), 2);
        assert!(listed
            .iter()
            .all(|item| item.id != ids[0] && item.bytes == Some(14)));
        assert!(discard_in(&root, &committed, &ids[0]).is_err());
        assert!(discard_in(&root, &committed, "../outside").is_err());
        discard_in(&root, &committed, &ids[1]).unwrap();
        crate::transcription::ensure_job_storage_budget(
            history::storage_tree_bytes(&root).unwrap(),
            before,
        )
        .unwrap();
        assert!(!root.join(&ids[1]).exists());
        assert_eq!(
            fs::read(root.join(&ids[0]).join("audio.wav")).unwrap(),
            b"retained audio"
        );
        assert_eq!(
            fs::read(root.join(&ids[2]).join("audio.wav")).unwrap(),
            b"retained audio"
        );
        assert_eq!(list_in(&root, &committed).unwrap().len(), 1);
        assert_eq!(
            fs::read(fixture.join("model.bin")).unwrap(),
            b"installed model"
        );
        fs::remove_dir_all(fixture).unwrap();
    }
}
