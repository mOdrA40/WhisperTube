use chrono::Utc;
use rusqlite::{params, Connection};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

use crate::{
    paths::app_data_dir,
    types::{HistoryItem, TranscriptRequest, TranscriptResult},
};

pub fn init_db(app: &AppHandle) -> Result<PathBuf, String> {
    let db_path = app_data_dir(app)?.join("whispertube.db");
    let conn = Connection::open(&db_path).map_err(|e| format!("Gagal membuka database: {e}"))?;
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

pub fn save_history_record(
    app: &AppHandle,
    request: &TranscriptRequest,
    language: &str,
    backend: &str,
    result_path: &Path,
) -> Result<i64, String> {
    let db_path = init_db(app)?;
    let conn =
        Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
    conn.execute(
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
    Ok(conn.last_insert_rowid())
}

pub fn list_history(app: &AppHandle) -> Result<Vec<HistoryItem>, String> {
    let db_path = init_db(app)?;
    let conn =
        Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
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
    let db_path = init_db(app)?;
    let conn =
        Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
    let result_path: String = conn
        .query_row(
            "SELECT result_path FROM history WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| format!("History tidak ditemukan: {e}"))?;
    let bytes = fs::read(&result_path)
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

pub fn delete_history(app: &AppHandle, ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    if ids.len() > 100 || ids.iter().any(|id| *id <= 0) {
        return Err("Daftar history yang akan dihapus tidak valid.".into());
    }

    let db_path = init_db(app)?;
    let conn =
        Connection::open(db_path).map_err(|e| format!("Gagal membuka history database: {e}"))?;
    let mut job_dirs = Vec::new();
    for id in ids {
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

    for job_dir in &job_dirs {
        fs::remove_dir_all(job_dir)
            .map_err(|e| format!("Gagal menghapus file history secara permanen: {e}"))?;
    }

    let transaction = conn
        .unchecked_transaction()
        .map_err(|e| format!("Gagal memulai penghapusan history: {e}"))?;
    for id in ids {
        transaction
            .execute("DELETE FROM history WHERE id = ?1", [id])
            .map_err(|e| format!("Gagal menghapus history {id}: {e}"))?;
    }
    transaction
        .commit()
        .map_err(|e| format!("Gagal menyelesaikan penghapusan history: {e}"))?;
    Ok(())
}

pub fn copy_export(app: &AppHandle, source: &str, target: &str) -> Result<(), String> {
    let source_path = PathBuf::from(source);
    if !source_path.exists() {
        return Err("File export sumber sudah tidak tersedia.".into());
    }
    let canonical_source = fs::canonicalize(&source_path)
        .map_err(|e| format!("Gagal memvalidasi source export: {e}"))?;
    let canonical_data = fs::canonicalize(app_data_dir(app)?)
        .map_err(|e| format!("Gagal memvalidasi app data: {e}"))?;
    if !canonical_source.starts_with(&canonical_data) {
        return Err("Source export berada di luar storage WhisperTube.".into());
    }
    fs::copy(canonical_source, target).map_err(|e| format!("Gagal menyimpan export: {e}"))?;
    Ok(())
}
