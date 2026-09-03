use chrono::Utc;
use rusqlite::{params, Connection};
use std::{
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
