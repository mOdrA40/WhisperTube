use chrono::Utc;
use rusqlite::{params, Connection};
use std::{
    collections::{BTreeSet, HashSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use crate::{
    paths::app_data_dir,
    types::{HistoryItem, TranscriptRequest, TranscriptResult},
};

const ORPHAN_JOB_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

pub fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("whispertube.db"))
}

fn configure_connection(conn: &Connection) -> Result<(), String> {
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| format!("Gagal mengatur timeout database: {e}"))?;
    Ok(())
}

pub fn init_db(app: &AppHandle) -> Result<PathBuf, String> {
    let db_path = database_path(app)?;
    let conn = Connection::open(&db_path).map_err(|e| format!("Gagal membuka database: {e}"))?;
    configure_connection(&conn)?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            channel TEXT NOT NULL,
            source_url TEXT NOT NULL,
            created_at TEXT NOT NULL,
            duration REAL NOT NULL,
            language TEXT NOT NULL,
            model TEXT NOT NULL,
            backend TEXT NOT NULL,
            result_path TEXT NOT NULL
        );
        ",
    )
    .map_err(|e| format!("Gagal inisialisasi database: {e}"))?;
    Ok(db_path)
}

fn open_connection(app: &AppHandle) -> Result<Connection, String> {
    let conn = Connection::open(database_path(app)?)
        .map_err(|e| format!("Gagal membuka history database: {e}"))?;
    configure_connection(&conn)?;
    Ok(conn)
}

pub fn save_history_record_with<F>(
    app: &AppHandle,
    request: &TranscriptRequest,
    language: &str,
    backend: &str,
    result_path: &Path,
    write_result: F,
) -> Result<i64, String>
where
    F: FnOnce(i64) -> Result<(), String>,
{
    let mut conn = open_connection(app)?;
    let transaction = conn
        .transaction()
        .map_err(|e| format!("Gagal memulai penyimpanan history: {e}"))?;
    transaction
        .execute(
        "INSERT INTO history (title, channel, source_url, created_at, duration, language, model, backend, result_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            request.title,
            request.channel,
            request.url,
            Utc::now().to_rfc3339(),
            request.duration,
            language,
            request.model_id,
            backend,
            result_path.to_string_lossy().to_string(),
        ],
    )
    .map_err(|e| format!("Gagal menyimpan history: {e}"))?;
    let history_id = transaction.last_insert_rowid();
    write_result(history_id)?;
    transaction
        .commit()
        .map_err(|e| format!("Gagal menyelesaikan penyimpanan history: {e}"))?;
    Ok(history_id)
}

pub fn list_history(app: &AppHandle) -> Result<Vec<HistoryItem>, String> {
    let conn = open_connection(app)?;
    let mut statement = conn
        .prepare(
            "SELECT id, title, channel, source_url, created_at, duration, language, model, backend
             FROM history ORDER BY id DESC LIMIT 100",
        )
        .map_err(|e| format!("Gagal membaca history: {e}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                title: row.get(1)?,
                channel: row.get(2)?,
                source_url: row.get(3)?,
                created_at: row.get(4)?,
                duration: row.get(5)?,
                language: row.get(6)?,
                model: row.get(7)?,
                backend: row.get(8)?,
            })
        })
        .map_err(|e| format!("Gagal query history: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Gagal decode history: {e}"))
}

pub fn load_history(app: &AppHandle, id: i64) -> Result<TranscriptResult, String> {
    let conn = open_connection(app)?;
    let result_path: String = conn
        .query_row(
            "SELECT result_path FROM history WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| format!("History tidak ditemukan: {e}"))?;
    let job_dir = job_dir_for_result(app, &result_path)?
        .ok_or_else(|| "File transcript history tidak ditemukan.".to_string())?;
    let safe_result_path = job_dir.join("result.json");
    let canonical_result = fs::canonicalize(&safe_result_path)
        .map_err(|e| format!("Gagal memvalidasi file history: {e}"))?;
    if canonical_result.parent() != Some(job_dir.as_path())
        || canonical_result.file_name() != Some(OsStr::new("result.json"))
    {
        return Err("File history berada di luar folder job WhisperTube.".into());
    }
    let bytes = fs::read(canonical_result)
        .map_err(|e| format!("File transcript history tidak ditemukan: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("File history rusak: {e}"))
}

fn job_dir_for_result(app: &AppHandle, result_path: &str) -> Result<Option<PathBuf>, String> {
    let jobs_root = crate::paths::jobs_dir(app)?;
    let canonical_jobs_root = fs::canonicalize(&jobs_root)
        .map_err(|e| format!("Gagal memvalidasi history storage: {e}"))?;
    let result_path = PathBuf::from(result_path);
    if result_path.file_name() != Some(OsStr::new("result.json")) {
        return Err("Path history tidak valid.".into());
    }
    let job_dir = result_path
        .parent()
        .ok_or_else(|| "Folder job history tidak valid.".to_string())?;
    let canonical_job_dir = match fs::canonicalize(job_dir) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Gagal memvalidasi folder job history: {error}")),
    };
    if canonical_job_dir.parent() != Some(canonical_jobs_root.as_path()) {
        return Err("Folder history berada di luar storage WhisperTube.".into());
    }
    Ok(Some(canonical_job_dir))
}

fn is_older_than(path: &Path, age: Duration) -> bool {
    let modified = match fs::metadata(path).and_then(|metadata| metadata.modified()) {
        Ok(modified) => modified,
        Err(_) => return false,
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|elapsed| elapsed >= age)
        .unwrap_or(false)
}

pub fn cleanup_job_storage(app: &AppHandle) -> Result<(), String> {
    let root = crate::paths::jobs_dir(app)?;
    let canonical_root =
        fs::canonicalize(&root).map_err(|e| format!("Gagal memvalidasi job storage: {e}"))?;
    let conn = open_connection(app)?;
    let mut statement = conn
        .prepare("SELECT result_path FROM history")
        .map_err(|e| format!("Gagal membaca referensi history: {e}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Gagal membaca referensi file history: {e}"))?;
    let history_paths = rows
        .map(|row| row.map(PathBuf::from))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Gagal membaca path history: {e}"))?;
    let valid_results = history_paths
        .into_iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .collect::<HashSet<_>>();
    let entries = fs::read_dir(&root).map_err(|e| format!("Gagal membaca job storage: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Gagal membaca entry job storage: {e}"))?;
        let path = entry.path();
        if !path.is_dir() || !is_older_than(&path, ORPHAN_JOB_GRACE) {
            continue;
        }
        let canonical_job_dir =
            fs::canonicalize(&path).map_err(|e| format!("Gagal memvalidasi folder job: {e}"))?;
        if canonical_job_dir.parent() != Some(canonical_root.as_path()) {
            continue;
        }
        let result_path = path.join("result.json");
        let has_valid_history = fs::canonicalize(&result_path)
            .map(|result| valid_results.contains(&result))
            .unwrap_or(false);
        if has_valid_history {
            let marker = path.join(".in-progress");
            if marker.exists() {
                fs::remove_file(marker)
                    .map_err(|e| format!("Gagal membersihkan marker history: {e}"))?;
            }
            continue;
        }
        fs::remove_dir_all(&path)
            .map_err(|e| format!("Gagal membersihkan job storage orphan: {e}"))?;
    }
    Ok(())
}

struct StagedDeletion {
    original: PathBuf,
    trash: PathBuf,
}

fn restore_staged_deletions(staged: &mut [StagedDeletion]) -> Result<(), String> {
    let mut errors = Vec::new();
    for item in staged.iter().rev() {
        if !item.trash.exists() {
            continue;
        }
        if let Err(error) = fs::rename(&item.trash, &item.original) {
            errors.push(format!("{}: {error}", item.original.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Sebagian file history gagal dipulihkan: {}",
            errors.join("; ")
        ))
    }
}

pub fn delete_history(app: &AppHandle, ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    if ids.len() > 100 || ids.iter().any(|id| *id <= 0) {
        return Err("Daftar history yang akan dihapus tidak valid.".into());
    }

    let unique_ids = ids.iter().copied().collect::<BTreeSet<_>>();
    let mut conn = open_connection(app)?;
    let mut job_dirs = Vec::new();
    for id in &unique_ids {
        let result_path: String = conn
            .query_row(
                "SELECT result_path FROM history WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| format!("History {id} tidak ditemukan: {e}"))?;
        if let Some(job_dir) = job_dir_for_result(app, &result_path)? {
            if !job_dirs.contains(&job_dir) {
                job_dirs.push(job_dir);
            }
        }
    }

    let jobs_root = crate::paths::jobs_dir(app)?;
    let mut staged = Vec::new();
    for original in job_dirs {
        let trash = jobs_root.join(format!(".deleted-{}", Uuid::new_v4()));
        if let Err(error) = fs::rename(&original, &trash) {
            let restore_error = restore_staged_deletions(&mut staged).err();
            return Err(match restore_error {
                Some(restore_error) => {
                    format!("Gagal menyiapkan penghapusan history: {error}. {restore_error}")
                }
                None => format!("Gagal menyiapkan penghapusan history: {error}"),
            });
        }
        staged.push(StagedDeletion { original, trash });
    }

    let transaction = match conn.transaction() {
        Ok(transaction) => transaction,
        Err(error) => {
            let restore_error = restore_staged_deletions(&mut staged).err();
            return Err(match restore_error {
                Some(restore_error) => {
                    format!("Gagal memulai penghapusan history: {error}. {restore_error}")
                }
                None => format!("Gagal memulai penghapusan history: {error}"),
            });
        }
    };
    for id in &unique_ids {
        if let Err(error) = transaction.execute("DELETE FROM history WHERE id = ?1", [id]) {
            drop(transaction);
            let restore_error = restore_staged_deletions(&mut staged).err();
            return Err(match restore_error {
                Some(restore_error) => {
                    format!("Gagal menghapus history {id}: {error}. {restore_error}")
                }
                None => format!("Gagal menghapus history {id}: {error}"),
            });
        }
    }
    if let Err(error) = transaction.commit() {
        let restore_error = restore_staged_deletions(&mut staged).err();
        return Err(match restore_error {
            Some(restore_error) => {
                format!("Gagal menyelesaikan penghapusan history: {error}. {restore_error}")
            }
            None => format!("Gagal menyelesaikan penghapusan history: {error}"),
        });
    }

    let mut cleanup_errors = Vec::new();
    for item in &staged {
        if let Err(error) = fs::remove_dir_all(&item.trash) {
            cleanup_errors.push(format!("{}: {error}", item.trash.display()));
        }
    }
    if !cleanup_errors.is_empty() {
        return Err(format!(
            "History sudah dihapus dari database, tetapi file sementara gagal dibersihkan: {}",
            cleanup_errors.join("; ")
        ));
    }
    Ok(())
}

pub fn copy_export(app: &AppHandle, source: &str, target: &str) -> Result<(), String> {
    let source_path = PathBuf::from(source);
    if !source_path.exists() {
        return Err("File export sumber sudah tidak tersedia.".into());
    }
    let source_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Nama file export tidak valid.".to_string())?;
    if !matches!(
        source_name,
        "transcript.txt" | "transcript.srt" | "transcript.vtt"
    ) {
        return Err("Source export tidak termasuk file transcript yang diizinkan.".into());
    }
    let canonical_source = fs::canonicalize(&source_path)
        .map_err(|e| format!("Gagal memvalidasi source export: {e}"))?;
    let job_dir = source_path
        .parent()
        .ok_or_else(|| "Folder source export tidak valid.".to_string())?;
    let canonical_job_dir = fs::canonicalize(job_dir)
        .map_err(|e| format!("Gagal memvalidasi folder source export: {e}"))?;
    let canonical_jobs_root = fs::canonicalize(crate::paths::jobs_dir(app)?)
        .map_err(|e| format!("Gagal memvalidasi jobs storage: {e}"))?;
    if canonical_job_dir.parent() != Some(canonical_jobs_root.as_path())
        || canonical_source.parent() != Some(canonical_job_dir.as_path())
    {
        return Err("Source export berada di luar storage job WhisperTube.".into());
    }
    let target_path = PathBuf::from(target);
    if target_path.is_dir() || canonical_source == target_path {
        return Err("Target export tidak valid.".into());
    }
    if target_path.exists() {
        let canonical_target = fs::canonicalize(&target_path)
            .map_err(|e| format!("Gagal memvalidasi target export: {e}"))?;
        if canonical_target == canonical_source {
            return Err("Target export tidak boleh sama dengan source.".into());
        }
    }
    let target_extension = target_path.extension().and_then(|value| value.to_str());
    let source_extension = Path::new(source_name)
        .extension()
        .and_then(|value| value.to_str());
    if !target_extension
        .zip(source_extension)
        .is_some_and(|(target, source)| target.eq_ignore_ascii_case(source))
    {
        return Err("Ekstensi target export harus sama dengan source.".into());
    }
    fs::copy(canonical_source, target_path).map_err(|e| format!("Gagal menyimpan export: {e}"))?;
    Ok(())
}

pub fn reveal_audio(app: &AppHandle, audio_path: &str) -> Result<(), String> {
    let path = PathBuf::from(audio_path);
    if path.file_name() != Some(OsStr::new("audio.wav")) {
        return Err("Path audio tidak valid.".into());
    }
    let canonical_path =
        fs::canonicalize(&path).map_err(|e| format!("File audio tidak ditemukan: {e}"))?;
    let canonical_job_dir = canonical_path
        .parent()
        .ok_or_else(|| "Folder audio tidak valid.".to_string())?;
    let canonical_jobs_root = fs::canonicalize(crate::paths::jobs_dir(app)?)
        .map_err(|e| format!("Gagal memvalidasi jobs storage: {e}"))?;
    if canonical_job_dir.parent() != Some(canonical_jobs_root.as_path()) {
        return Err("File audio berada di luar storage job WhisperTube.".into());
    }
    app.opener()
        .reveal_item_in_dir(canonical_path)
        .map_err(|e| format!("Gagal membuka lokasi audio: {e}"))?;
    Ok(())
}
