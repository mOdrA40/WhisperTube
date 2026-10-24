use chrono::Utc;
use rusqlite::{params, Connection};
use std::{
    collections::{BTreeSet, HashSet},
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::{
    paths::app_data_dir,
    types::{
        HistoryDeleteOutcome, HistoryItem, HistoryPageResult, TranscriptRequest, TranscriptResult,
        MAX_TRANSCRIPT_RESULT_BYTES, MAX_TRANSCRIPT_SEGMENTS, MAX_TRANSCRIPT_TEXT_BYTES,
    },
};

const ORPHAN_JOB_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
const HISTORY_PAGE_SIZE: usize = 100;
pub const MAX_JOB_STORAGE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const STORAGE_USAGE_CACHE_TTL: Duration = Duration::from_secs(5);

struct StorageUsageCache {
    bytes: u64,
    measured_at: Instant,
}

static STORAGE_USAGE_CACHE: OnceLock<Mutex<Option<StorageUsageCache>>> = OnceLock::new();

pub fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("whispertube.db"))
}

pub(crate) fn storage_tree_bytes(path: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    for entry in fs::read_dir(path).map_err(|e| format!("Gagal membaca job storage: {e}"))? {
        let entry = entry.map_err(|e| format!("Gagal membaca entry job storage: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Gagal membaca tipe file job storage: {e}"))?;
        if file_type.is_symlink() {
            return Err("Job storage berisi symbolic link yang tidak diizinkan.".into());
        }
        if file_type.is_dir() {
            total = total.saturating_add(storage_tree_bytes(&entry.path())?);
        } else if file_type.is_file() {
            total = total.saturating_add(
                entry
                    .metadata()
                    .map_err(|e| format!("Gagal membaca ukuran job storage: {e}"))?
                    .len(),
            );
        }
        if total > MAX_JOB_STORAGE_BYTES {
            return Ok(total);
        }
    }
    Ok(total)
}

pub fn job_storage_bytes(app: &AppHandle) -> Result<u64, String> {
    let cache = STORAGE_USAGE_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.measured_at.elapsed() < STORAGE_USAGE_CACHE_TTL {
                return Ok(cached.bytes);
            }
        }
    }

    let root = crate::paths::jobs_dir(app)?;
    let canonical_root =
        fs::canonicalize(&root).map_err(|e| format!("Gagal memvalidasi job storage: {e}"))?;
    let bytes = storage_tree_bytes(&canonical_root)?;
    if let Ok(mut guard) = cache.lock() {
        *guard = Some(StorageUsageCache {
            bytes,
            measured_at: Instant::now(),
        });
    }
    Ok(bytes)
}

pub fn invalidate_job_storage_cache() {
    if let Some(cache) = STORAGE_USAGE_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
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
    initialize_schema(&conn)?;
    Ok(db_path)
}

pub(crate) fn initialize_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS pending_job_deletions (job_name TEXT PRIMARY KEY NOT NULL);
        CREATE TABLE IF NOT EXISTS history_file_deletions (
            history_id INTEGER NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('audio','transcript')),
            pending INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(history_id, kind)
        );
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
    Ok(())
}

pub(crate) fn open_connection(app: &AppHandle) -> Result<Connection, String> {
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
    invalidate_job_storage_cache();
    Ok(history_id)
}

pub fn list_history(app: &AppHandle, before_id: Option<i64>) -> Result<HistoryPageResult, String> {
    if before_id.is_some_and(|id| id <= 0) {
        return Err("Cursor history tidak valid.".into());
    }
    let conn = open_connection(app)?;
    let total_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
        .map_err(|e| format!("Gagal menghitung history: {e}"))?;
    let mut statement = conn
        .prepare(
            "SELECT id, title, channel, source_url, created_at, duration, language, model, backend
             FROM history WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2",
        )
        .map_err(|e| format!("Gagal membaca history: {e}"))?;
    let rows = statement
        .query_map(params![before_id, (HISTORY_PAGE_SIZE + 1) as i64], |row| {
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
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Gagal decode history: {e}"))?;
    let has_more = items.len() > HISTORY_PAGE_SIZE;
    items.truncate(HISTORY_PAGE_SIZE);
    Ok(HistoryPageResult {
        items,
        has_more,
        total_count,
    })
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
    if crate::history_files::is_deleted(&conn, id, "transcript")? {
        return crate::history_files::metadata_only_result(&conn, id, &job_dir);
    }
    let safe_result_path = job_dir.join("result.json");
    let canonical_result = fs::canonicalize(&safe_result_path)
        .map_err(|e| format!("Gagal memvalidasi file history: {e}"))?;
    if canonical_result.parent() != Some(job_dir.as_path())
        || canonical_result.file_name() != Some(OsStr::new("result.json"))
    {
        return Err("File history berada di luar folder job WhisperTube.".into());
    }
    let file = fs::File::open(canonical_result)
        .map_err(|e| format!("File transcript history tidak ditemukan: {e}"))?;
    let mut bytes = Vec::with_capacity(MAX_TRANSCRIPT_RESULT_BYTES.min(64 * 1024) as usize);
    file.take(MAX_TRANSCRIPT_RESULT_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Gagal membaca file history: {e}"))?;
    let mut result = decode_history_result(&bytes)?;
    result.transcript_available = true;
    if crate::history_files::is_deleted(&conn, id, "audio")?
        || !crate::paths::is_regular_file(&job_dir.join("audio.wav"))
    {
        result.audio_path = None;
    }
    Ok(result)
}

fn decode_history_result(bytes: &[u8]) -> Result<TranscriptResult, String> {
    if bytes.len() as u64 > MAX_TRANSCRIPT_RESULT_BYTES {
        return Err("File history melebihi batas ukuran aman.".into());
    }
    let result: TranscriptResult =
        serde_json::from_slice(bytes).map_err(|e| format!("File history rusak: {e}"))?;
    if result.segments.len() > MAX_TRANSCRIPT_SEGMENTS {
        return Err("File history memiliki terlalu banyak segment.".into());
    }
    if result.text.len() > MAX_TRANSCRIPT_TEXT_BYTES {
        return Err("Teks history melebihi batas ukuran aman.".into());
    }
    Ok(result)
}

fn job_dir_for_result(app: &AppHandle, result_path: &str) -> Result<Option<PathBuf>, String> {
    let jobs_root = crate::paths::jobs_dir(app)?;
    job_dir_for_result_in(&jobs_root, result_path)
}

// A result file is written inside the SQLite transaction. After a crash it
// can exist without a committed row, so file existence cannot decide whether
// a checkpoint job is finished.
pub fn referenced_job_directories(app: &AppHandle) -> Result<HashSet<PathBuf>, String> {
    let conn = open_connection(app)?;
    let mut statement = conn
        .prepare("SELECT result_path FROM history")
        .map_err(|e| format!("Gagal membaca referensi history untuk resume: {e}"))?;
    let paths = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Gagal query referensi history untuk resume: {e}"))?;
    let mut directories = HashSet::new();
    for path in paths {
        let path = path.map_err(|e| format!("Path history untuk resume tidak valid: {e}"))?;
        if let Some(directory) = job_dir_for_result(app, &path)? {
            directories.insert(directory);
        }
    }
    Ok(directories)
}

pub(crate) fn job_dir_for_result_in(
    jobs_root: &Path,
    result_path: &str,
) -> Result<Option<PathBuf>, String> {
    let canonical_jobs_root = fs::canonicalize(jobs_root)
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
    let conn = open_connection(app)?;
    let result = cleanup_job_storage_in(&conn, &root);
    invalidate_job_storage_cache();
    result
}

pub(crate) fn cleanup_source_audio(job_dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(job_dir)
        .map_err(|e| format!("Gagal membaca audio sumber untuk cleanup: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Gagal membaca entry audio sumber: {e}"))?;
        if entry.file_name().to_string_lossy().starts_with("source.") {
            // Unlink only direct source files, never recurse into a matching directory.
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("Gagal menghapus audio sumber: {error}")),
            }
        }
    }
    Ok(())
}

fn cleanup_result_signature(path: &Path) -> Result<String, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Gagal memeriksa hasil history: {e}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("File history bukan file biasa.".into());
    }
    let modified = metadata
        .modified()
        .and_then(|time| {
            time.duration_since(SystemTime::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map_err(|e| format!("Gagal membaca waktu hasil history: {e}"))?;
    Ok(format!(
        "{}:{}:{}",
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    ))
}

// Only the pipeline that has just committed a retained-audio result may call
// this without decoding the result again. Any leftover marker/source/checkpoint
// still forces full validation during startup, regardless of the receipt.
pub(crate) fn record_retained_audio_cleanup(job_dir: &Path) -> Result<(), String> {
    let signature = cleanup_result_signature(&job_dir.join("result.json"))?;
    write_export_atomically(
        &mut signature.as_bytes(),
        &job_dir.join(".cleanup-complete"),
    )
}

fn cleanup_committed_job(job_dir: &Path) -> Result<(), String> {
    let mut has_temporary = false;
    let mut has_wav = false;
    for entry in
        fs::read_dir(job_dir).map_err(|e| format!("Gagal memeriksa cleanup history: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Gagal memeriksa artifact history: {e}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        has_temporary |=
            name == ".in-progress" || name == ".checkpoints" || name.starts_with("source.");
        has_wav |= name == "audio.wav";
    }
    // No payload needs decoding when there is nothing to delete. A receipt
    // also avoids decoding retained-audio histories on every launch, but never
    // authorizes deletion: new temporary artifacts always require validation.
    let signature = cleanup_result_signature(&job_dir.join("result.json"))?;
    let receipt = job_dir.join(".cleanup-complete");
    if !has_temporary {
        if !has_wav {
            return Ok(());
        }
        let mut bytes = Vec::new();
        if crate::paths::is_regular_file(&receipt) {
            fs::File::open(&receipt)
                .and_then(|file| file.take(256).read_to_end(&mut bytes))
                .map_err(|e| format!("Gagal membaca receipt cleanup: {e}"))?;
            if bytes == signature.as_bytes() {
                return Ok(());
            }
        }
    }
    cleanup_committed_job_artifacts(job_dir)?;
    // An interrupted receipt write is harmless: the next launch validates the
    // result again. Do not rewrite the receipt on the fast path.
    write_export_atomically(&mut signature.as_bytes(), &receipt)
}

fn cleanup_committed_job_artifacts(job_dir: &Path) -> Result<(), String> {
    let marker = job_dir.join(".in-progress");
    let wav = job_dir.join("audio.wav");
    // Only a committed history row authorizes this cleanup. A result file on
    // its own may belong to a rolled-back transaction and must remain resumable.
    let result_path = fs::canonicalize(job_dir.join("result.json"))
        .map_err(|e| format!("Gagal memvalidasi hasil untuk cleanup history: {e}"))?;
    if result_path.parent() != Some(job_dir) {
        return Err("File history berada di luar folder job WhisperTube.".into());
    }
    let file = fs::File::open(result_path)
        .map_err(|e| format!("Gagal membuka hasil untuk cleanup history: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_TRANSCRIPT_RESULT_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Gagal membaca hasil untuk cleanup history: {e}"))?;
    let result = decode_history_result(&bytes)?;
    // A missing/unreadable result must not authorize deleting recoverable data,
    // including the original download. Validate retention metadata first.
    cleanup_source_audio(job_dir)?;
    crate::checkpoint::cleanup(job_dir)?;
    if result.audio_path.is_none() {
        match fs::remove_file(&wav) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Gagal membersihkan audio history: {error}")),
        }
    }
    // Keep the marker until all cleanup succeeds; retries also inspect the
    // artifacts themselves to recover jobs whose marker was already removed.
    match fs::remove_file(marker) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Gagal membersihkan marker history: {error}")),
    }
}

pub(crate) fn cleanup_job_storage_in(conn: &Connection, root: &Path) -> Result<(), String> {
    let canonical_root =
        fs::canonicalize(root).map_err(|e| format!("Gagal memvalidasi job storage: {e}"))?;
    let mut warnings = cleanup_pending_deletions(conn, root)?;
    warnings.extend(crate::history_files::cleanup_pending(conn, root)?);
    let removed_transcripts = crate::history_files::removed_transcript_directories(conn, root)?;
    let mut statement = conn
        .prepare("SELECT result_path FROM history")
        .map_err(|e| format!("Gagal membaca referensi history: {e}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Gagal membaca referensi file history: {e}"))?;
    let history_paths = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Gagal membaca path history: {e}"))?;
    // The committed row owns its directory even when result.json is missing,
    // corrupt or unreadable. Resolve the parent independently of that file.
    let mut committed_jobs = HashSet::new();
    for path in history_paths {
        if let Some(directory) = job_dir_for_result_in(&canonical_root, &path)? {
            committed_jobs.insert(directory);
        }
    }
    let entries = fs::read_dir(root).map_err(|e| format!("Gagal membaca job storage: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Gagal membaca entry job storage: {e}"))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| format!("Gagal membaca tipe job storage: {e}"))?;
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let canonical_job_dir =
            fs::canonicalize(&path).map_err(|e| format!("Gagal memvalidasi folder job: {e}"))?;
        if canonical_job_dir.parent() != Some(canonical_root.as_path()) {
            continue;
        }
        if committed_jobs.contains(&canonical_job_dir) {
            if removed_transcripts.contains(&canonical_job_dir) {
                continue;
            }
            if let Err(error) = cleanup_committed_job(&canonical_job_dir) {
                warnings.push(format!(
                    "Pembersihan history {} tertunda: {error}",
                    path.display()
                ));
            }
            continue;
        }
        if is_older_than(&path, ORPHAN_JOB_GRACE) {
            if let Err(error) = fs::remove_dir_all(&path) {
                warnings.push(format!(
                    "Gagal membersihkan job storage orphan {}: {error}",
                    path.display()
                ));
            }
        }
    }
    if warnings.is_empty() {
        Ok(())
    } else {
        Err(warnings.join("; "))
    }
}

// Delete intent and rows commit together. Files are removed only after commit;
// interrupted cleanup is replayed at startup, including partially removed folders.
fn queue_history_deletion(
    conn: &mut Connection,
    root: &Path,
    ids: &[i64],
) -> Result<Vec<i64>, String> {
    if ids.len() > 100 || ids.iter().any(|id| *id <= 0) {
        return Err("Daftar history yang akan dihapus tidak valid.".into());
    }
    let ids = ids.iter().copied().collect::<BTreeSet<_>>();
    let transaction = conn
        .transaction()
        .map_err(|e| format!("Gagal memulai penghapusan: {e}"))?;
    for id in &ids {
        let path: String = transaction
            .query_row(
                "SELECT result_path FROM history WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| format!("History {id} tidak ditemukan: {e}"))?;
        if let Some(dir) = job_dir_for_result_in(root, &path)? {
            let name = dir
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "Nama folder history tidak valid.".to_string())?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO pending_job_deletions(job_name) VALUES (?1)",
                    [name],
                )
                .map_err(|e| format!("Gagal menyimpan intent penghapusan: {e}"))?;
        }
        transaction
            .execute(
                "DELETE FROM history_file_deletions WHERE history_id = ?1",
                [id],
            )
            .map_err(|e| format!("Gagal membersihkan status file history: {e}"))?;
        transaction
            .execute("DELETE FROM history WHERE id = ?1", [id])
            .map_err(|e| format!("Gagal menghapus history {id}: {e}"))?;
    }
    transaction
        .commit()
        .map_err(|e| format!("Gagal menyelesaikan penghapusan: {e}"))?;
    Ok(ids.into_iter().collect())
}

fn cleanup_pending_deletions(conn: &Connection, root: &Path) -> Result<Vec<String>, String> {
    let root = fs::canonicalize(root).map_err(|e| format!("Gagal memvalidasi job storage: {e}"))?;
    let names = conn
        .prepare("SELECT job_name FROM pending_job_deletions")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| format!("Gagal membaca intent penghapusan: {e}"))?;
    let references = conn
        .prepare("SELECT result_path FROM history")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| format!("Gagal membaca referensi history: {e}"))?;
    let mut live_dirs = HashSet::new();
    for path in references {
        if let Some(dir) = job_dir_for_result_in(&root, &path)? {
            live_dirs.insert(dir);
        }
    }
    let mut warnings = Vec::new();
    for name in names {
        let cleanup = || -> Result<(), String> {
            let mut components = Path::new(&name).components();
            if !matches!(components.next(), Some(std::path::Component::Normal(_)))
                || components.next().is_some()
            {
                return Err("Nama folder pada intent penghapusan tidak aman.".into());
            }
            let target = root.join(&name);
            match fs::symlink_metadata(&target) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Gagal membaca folder tertunda: {e}")),
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err("Folder tertunda bukan direktori biasa.".into());
                    }
                    let canonical = fs::canonicalize(&target)
                        .map_err(|e| format!("Gagal memvalidasi folder tertunda: {e}"))?;
                    if canonical.parent() != Some(root.as_path()) || live_dirs.contains(&canonical)
                    {
                        return Err(
                            "Folder tertunda berada di luar storage atau masih dirujuk history."
                                .into(),
                        );
                    }
                    fs::remove_dir_all(&target)
                        .map_err(|e| format!("Gagal membersihkan file history: {e}"))?;
                }
            }
            conn.execute(
                "DELETE FROM pending_job_deletions WHERE job_name = ?1",
                [&name],
            )
            .map_err(|e| format!("Gagal menyelesaikan intent penghapusan: {e}"))?;
            Ok(())
        };
        if let Err(error) = cleanup() {
            warnings.push(format!(
                "History telah dihapus; pembersihan {name} tertunda: {error}"
            ));
        }
    }
    Ok(warnings)
}

pub fn delete_history(app: &AppHandle, ids: &[i64]) -> Result<HistoryDeleteOutcome, String> {
    let mut conn = open_connection(app)?;
    let root = crate::paths::jobs_dir(app)?;
    let deleted_ids = queue_history_deletion(&mut conn, &root, ids)?;
    let cleanup_warnings = cleanup_pending_deletions(&conn, &root).unwrap_or_else(|error| {
        vec![format!(
            "History telah dihapus; pembersihan tertunda: {error}"
        )]
    });
    invalidate_job_storage_cache();
    Ok(HistoryDeleteOutcome {
        deleted_ids,
        cleanup_warnings,
    })
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
    if let Ok(metadata) = fs::symlink_metadata(&target_path) {
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(
                "Target export harus berupa file biasa, bukan symbolic link atau device.".into(),
            );
        }
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
    let mut input = fs::File::open(canonical_source)
        .map_err(|e| format!("Gagal membuka source export: {e}"))?;
    write_export_atomically(&mut input, &target_path)
}

fn write_export_atomically(input: &mut impl Read, target: &Path) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "Folder target export tidak valid.".to_string())?;
    let permissions = match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if metadata.permissions().readonly() {
                return Err("Target export bersifat read-only.".into());
            }
            Some(metadata.permissions())
        }
        Ok(_) => return Err("Target export harus berupa file biasa.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Gagal memeriksa target export: {error}")),
    };
    // A sibling is on the same filesystem, so rename publishes a complete file
    // without truncating the existing destination during a failed copy.
    let temporary = parent.join(format!(".whispertube-export-{}.tmp", uuid::Uuid::new_v4()));
    let output = create_export_staging(&temporary, permissions.as_ref().map(|_| target))
        .map_err(|e| format!("Gagal membuat staging export: {e}"))?;
    let result = (|| -> std::io::Result<()> {
        let mut output = output;
        #[cfg(unix)]
        if permissions.is_some() {
            crate::export_permissions::preserve(target, &output).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("Gagal mempertahankan izin target export: {error}"),
                )
            })?;
        }
        std::io::copy(input, &mut output)?;
        #[cfg(windows)]
        if let Some(permissions) = permissions {
            output.set_permissions(permissions)?;
        }
        output.sync_all()?;
        drop(output);
        crate::durable::publish(&temporary, target)
    })();
    if let Err(error) = result {
        let cleanup = match fs::remove_file(&temporary) {
            Ok(()) => String::new(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => format!(
                " Staging {} belum dibersihkan: {error}",
                temporary.display()
            ),
        };
        return Err(format!("Gagal menyimpan export: {error}.{cleanup}"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_export_staging(
    path: &Path,
    _existing_target: Option<&Path>,
) -> std::io::Result<fs::File> {
    crate::export_permissions::create_private(path)
}

#[cfg(windows)]
fn create_export_staging(path: &Path, existing_target: Option<&Path>) -> std::io::Result<fs::File> {
    use std::{
        ffi::c_void,
        io,
        os::windows::{
            ffi::OsStrExt,
            fs::MetadataExt,
            io::{FromRawHandle, OwnedHandle},
        },
    };
    let Some(target) = existing_target else {
        return fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path);
    };
    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit: i32,
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetFileSecurityW(
            path: *const u16,
            information: u32,
            descriptor: *mut c_void,
            size: u32,
            needed: *mut u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            path: *const u16,
            access: u32,
            share: u32,
            security: *const SecurityAttributes,
            disposition: u32,
            attributes: u32,
            template: *mut c_void,
        ) -> *mut c_void;
    }
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path export mengandung NUL.",
            ));
        }
        units.push(0);
        Ok(units)
    }
    // Canonical paths also supply the verbatim prefix for long Windows paths.
    let target = fs::canonicalize(target)?;
    let attributes = fs::metadata(&target)?.file_attributes();
    let target = wide(&target)?;
    let parent = fs::canonicalize(path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Folder staging tidak valid.")
    })?)?;
    let temporary = wide(&parent.join(path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Nama staging tidak valid.")
    })?))?;
    let mut needed = 0;
    // SAFETY: the first call requests only a buffer size; target is NUL-terminated.
    let success =
        unsafe { GetFileSecurityW(target.as_ptr(), 4, std::ptr::null_mut(), 0, &mut needed) };
    if success == 0 && io::Error::last_os_error().raw_os_error() != Some(122) {
        return Err(io::Error::last_os_error());
    }
    if needed == 0 || needed > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ukuran ACL export tidak valid.",
        ));
    }
    // DWORD alignment is required for the self-relative security descriptor.
    let mut descriptor = vec![0u32; (needed as usize).div_ceil(4)];
    // SAFETY: descriptor is aligned and has at least needed writable bytes.
    if unsafe {
        GetFileSecurityW(
            target.as_ptr(),
            4,
            descriptor.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let security = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor: descriptor.as_mut_ptr().cast(),
        inherit: 0,
    };
    // Apply the target's DACL at creation, before any transcript bytes exist.
    // Preserve EFS encryption when the destination file was individually encrypted.
    // SAFETY: all pointers are live for the call; CREATE_NEW cannot clobber a
    // pre-existing staging file, and the returned handle is uniquely owned.
    let handle = unsafe {
        CreateFileW(
            temporary.as_ptr(),
            0x40000000,
            1,
            &security,
            1,
            0x80 | (attributes & 0x4000),
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW succeeded and transfers an owned, non-inherited handle.
    Ok(fs::File::from(unsafe {
        OwnedHandle::from_raw_handle(handle)
    }))
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

#[cfg(test)]
mod tests {
    use super::decode_history_result;
    use super::{
        cleanup_job_storage_in, cleanup_pending_deletions, cleanup_source_audio, initialize_schema,
        queue_history_deletion,
    };
    use crate::types::MAX_TRANSCRIPT_RESULT_BYTES;
    use rusqlite::Connection;
    use std::{fs, path::PathBuf};

    struct Fixture {
        root: PathBuf,
        database: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("history-delete-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self {
                database: root.join("history.db"),
                root,
            }
        }
        fn open(&self) -> Connection {
            let conn = Connection::open(&self.database).unwrap();
            initialize_schema(&conn).unwrap();
            conn
        }
        fn insert(&self, conn: &Connection, name: &str) -> i64 {
            let dir = self.root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("result.json"), b"result").unwrap();
            conn.execute("INSERT INTO history(title,channel,source_url,created_at,duration,language,model,backend,result_path)
                VALUES ('title','channel','url','date',1,'id','base','cpu',?1)",
                [dir.join("result.json").to_str().unwrap()]).unwrap();
            conn.last_insert_rowid()
        }

        fn completed_job(&self, conn: &Connection, name: &str, keep_audio: bool) -> PathBuf {
            let id = self.insert(conn, name);
            let job = self.root.join(name);
            let result = crate::types::TranscriptResult {
                history_id: id,
                title: "title".into(),
                channel: "channel".into(),
                language: "id".into(),
                duration: 1.0,
                source_duration: None,
                model: "base".into(),
                backend: "cpu".into(),
                segments: vec![],
                text: "saved transcript".into(),
                txt_path: job.join("transcript.txt").to_string_lossy().into_owned(),
                srt_path: job.join("transcript.srt").to_string_lossy().into_owned(),
                vtt_path: job.join("transcript.vtt").to_string_lossy().into_owned(),
                audio_path: keep_audio
                    .then(|| job.join("audio.wav").to_string_lossy().into_owned()),
                transcript_available: true,
            };
            fs::write(
                job.join("result.json"),
                serde_json::to_vec(&result).unwrap(),
            )
            .unwrap();
            for extension in ["txt", "srt", "vtt"] {
                fs::write(job.join(format!("transcript.{extension}")), b"saved export").unwrap();
            }
            fs::write(job.join("audio.wav"), b"saved audio").unwrap();
            fs::write(job.join(".in-progress"), b"checkpoint").unwrap();
            fs::create_dir(job.join(".checkpoints")).unwrap();
            fs::write(job.join(".checkpoints/0000.json"), b"saved chunk").unwrap();
            job
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn whole_history_deletion_removes_partial_file_intents_in_the_same_transaction() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        let job = fixture.completed_job(&conn, "partial", true);
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(job.join("result.json")).unwrap()).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("transcriptAvailable");
        assert!(
            decode_history_result(&serde_json::to_vec(&legacy).unwrap())
                .unwrap()
                .transcript_available
        );
        assert!(decode_history_result(&serde_json::to_vec(&legacy).unwrap())
            .unwrap()
            .source_duration
            .is_none());
        legacy["sourceDuration"] = serde_json::json!(60.0);
        let decoded = decode_history_result(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(decoded.duration, 1.0);
        assert_eq!(decoded.source_duration, Some(60.0));
        conn.execute(
            "INSERT INTO history_file_deletions(history_id,kind) VALUES (1,'transcript')",
            [],
        )
        .unwrap();
        queue_history_deletion(&mut conn, &fixture.root, &[1]).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history_file_deletions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(crate::history_files::cleanup_pending(&conn, &fixture.root)
            .unwrap()
            .is_empty());
        assert!(cleanup_pending_deletions(&conn, &fixture.root)
            .unwrap()
            .is_empty());
        assert!(!job.exists());
    }

    fn expire_job(path: &std::path::Path) {
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x02000000); // FILE_FLAG_BACKUP_SEMANTICS opens a directory.
        }
        #[cfg(not(windows))]
        options.write(false);
        options
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 3600),
            ))
            .unwrap();
    }

    #[test]
    fn startup_recovers_committed_cleanup_and_preserves_retained_audio_and_exports() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        let discard = fixture.completed_job(&conn, "discard", false);
        let retain = fixture.completed_job(&conn, "retain", true);
        let no_marker = fixture.completed_job(&conn, "no-marker", false);
        // A cleanup failure may have been logged before commit() removed the marker.
        fs::remove_file(no_marker.join(".in-progress")).unwrap();
        // Recover both fresh and expired committed jobs without the orphan grace.
        expire_job(&discard);
        drop(conn);
        let conn = fixture.open();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        assert!(!discard.join("audio.wav").exists());
        assert!(!no_marker.join("audio.wav").exists());
        assert_eq!(fs::read(retain.join("audio.wav")).unwrap(), b"saved audio");
        for job in [&discard, &retain, &no_marker] {
            assert!(!job.join(".checkpoints").exists());
            assert!(!job.join(".in-progress").exists());
            assert!(job.join("result.json").exists());
            for extension in ["txt", "srt", "vtt"] {
                assert_eq!(
                    fs::read(job.join(format!("transcript.{extension}"))).unwrap(),
                    b"saved export"
                );
            }
        }
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
    }

    #[test]
    fn changed_retention_metadata_invalidates_cleanup_receipt() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        let job = fixture.completed_job(&conn, "retained", true);
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        let original = fs::read(job.join("result.json")).unwrap();
        fs::write(job.join("result.json"), b"{corrupt").unwrap();
        assert!(cleanup_job_storage_in(&conn, &fixture.root).is_err());
        assert!(job.join("audio.wav").exists());
        let mut result: crate::types::TranscriptResult = serde_json::from_slice(&original).unwrap();
        result.audio_path = None;
        fs::write(
            job.join("result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        assert!(!job.join("audio.wav").exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_receipt_cannot_overwrite_a_symlink_target() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        let job = fixture.completed_job(&conn, "retained", true);
        let outside = fixture.root.join("unrelated.txt");
        fs::write(&outside, b"preserve").unwrap();
        std::os::unix::fs::symlink(&outside, job.join(".cleanup-complete")).unwrap();
        assert!(cleanup_job_storage_in(&conn, &fixture.root).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"preserve");
        assert!(job.join("audio.wav").exists());
    }

    #[cfg(windows)]
    #[test]
    fn freshly_committed_retained_job_needs_no_payload_read_on_first_startup() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let conn = fixture.open();
        let job = fixture.completed_job(&conn, "new-retained", true);
        crate::checkpoint::cleanup(&job).unwrap();
        super::record_retained_audio_cleanup(&job).unwrap();
        fs::remove_file(job.join(".in-progress")).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(job.join("result.json"))
            .unwrap();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        assert!(job.join("audio.wav").exists());
        drop(lock);
    }

    #[cfg(windows)]
    #[test]
    fn clean_history_skips_payload_reads_but_new_artifacts_force_validation() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let conn = fixture.open();
        let job = fixture.completed_job(&conn, "retained", true);
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(job.join("result.json"))
            .unwrap();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        fs::write(job.join("source.webm"), b"recoverable").unwrap();
        assert!(cleanup_job_storage_in(&conn, &fixture.root).is_err());
        assert!(job.join("source.webm").exists());
        drop(lock);
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        assert!(!job.join("source.webm").exists());
        assert!(job.join("audio.wav").exists());
    }

    #[test]
    fn startup_removes_leftover_sources_only_from_committed_jobs() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        let discard = fixture.completed_job(&conn, "discard", false);
        let retain = fixture.completed_job(&conn, "retain", true);
        // Simulate a completed retry that already removed every other temporary artifact.
        for job in [&discard, &retain] {
            fs::remove_file(job.join(".in-progress")).unwrap();
            fs::remove_dir_all(job.join(".checkpoints")).unwrap();
            fs::write(job.join("source.webm"), b"leftover download").unwrap();
            fs::write(job.join("source.webm.part"), b"leftover fragment").unwrap();
        }
        fs::remove_file(discard.join("audio.wav")).unwrap();
        let pending = fixture.root.join("pending");
        fs::create_dir(&pending).unwrap();
        fs::write(pending.join("source.webm"), b"resumable download").unwrap();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        for job in [&discard, &retain] {
            assert!(!job.join("source.webm").exists());
            assert!(!job.join("source.webm.part").exists());
            assert_eq!(
                fs::read(job.join("transcript.txt")).unwrap(),
                b"saved export"
            );
            assert!(job.join("result.json").exists());
        }
        assert_eq!(fs::read(retain.join("audio.wav")).unwrap(), b"saved audio");
        assert_eq!(
            fs::read(pending.join("source.webm")).unwrap(),
            b"resumable download"
        );
    }

    #[test]
    fn source_cleanup_reports_failure_without_recursing_and_can_be_retried() {
        let fixture = Fixture::new();
        let blocked = fixture.root.join("source.webm");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("preserve.txt"), b"preserve").unwrap();
        assert!(cleanup_source_audio(&fixture.root).is_err());
        assert_eq!(fs::read(blocked.join("preserve.txt")).unwrap(), b"preserve");
        fs::remove_dir_all(&blocked).unwrap();
        fs::write(&blocked, b"download").unwrap();
        cleanup_source_audio(&fixture.root).unwrap();
        cleanup_source_audio(&fixture.root).unwrap();
        assert!(!blocked.exists());
    }

    #[cfg(windows)]
    #[test]
    fn source_cleanup_retries_after_a_lock_with_ready_audio_preserved() {
        use std::os::windows::fs::OpenOptionsExt;

        let fixture = Fixture::new();
        let source = fixture.root.join("source.webm");
        let wav = fixture.root.join("audio.wav");
        let metadata = fixture.root.join(".audio-cache.json");
        fs::write(&source, b"download").unwrap();
        fs::write(&wav, b"completed WAV").unwrap();
        fs::write(&metadata, br#"{"audio_ready":true}"#).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&source)
            .unwrap();
        assert!(cleanup_source_audio(&fixture.root).is_err());
        drop(lock);
        cleanup_source_audio(&fixture.root).unwrap();
        cleanup_source_audio(&fixture.root).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&wav).unwrap(), b"completed WAV");
        assert_eq!(fs::read(&metadata).unwrap(), br#"{"audio_ready":true}"#);
    }

    #[test]
    fn pending_deletion_warning_does_not_block_orphan_expiry_or_committed_cleanup() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        conn.execute("INSERT INTO pending_job_deletions VALUES ('../unsafe')", [])
            .unwrap();
        let committed = fixture.completed_job(&conn, "committed", false);
        let expired = fixture.root.join("expired");
        fs::create_dir(&expired).unwrap();
        fs::write(expired.join("audio.wav"), b"expired audio").unwrap();
        expire_job(&expired);
        // Even a result file cannot authorize cleanup without a committed row.
        let recent = fixture.root.join("recent");
        fs::create_dir(&recent).unwrap();
        fs::copy(committed.join("result.json"), recent.join("result.json")).unwrap();
        fs::write(recent.join("audio.wav"), b"resumable audio").unwrap();
        let error = cleanup_job_storage_in(&conn, &fixture.root).unwrap_err();
        assert!(error.contains("Nama folder pada intent penghapusan tidak aman"));
        assert!(!expired.exists());
        assert!(!committed.join("audio.wav").exists());
        assert_eq!(
            fs::read(recent.join("audio.wav")).unwrap(),
            b"resumable audio"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pending_job_deletions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn failed_committed_cleanup_is_retried_after_reopen() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        let job = fixture.completed_job(&conn, "job", false);
        fs::remove_dir_all(job.join(".checkpoints")).unwrap();
        fs::write(job.join(".checkpoints"), b"blocked").unwrap();
        assert!(cleanup_job_storage_in(&conn, &fixture.root).is_err());
        assert!(job.join(".in-progress").exists());
        assert!(job.join("audio.wav").exists());
        drop(conn);
        fs::remove_file(job.join(".checkpoints")).unwrap();
        let conn = fixture.open();
        cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        assert!(!job.join("audio.wav").exists());
        assert!(!job.join(".in-progress").exists());
        assert!(job.join("result.json").exists());
    }

    #[test]
    fn unreadable_retention_metadata_preserves_audio_and_does_not_block_other_jobs() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        let broken = fixture.completed_job(&conn, "broken", true);
        let healthy = fixture.completed_job(&conn, "healthy", false);
        fs::write(broken.join("result.json"), b"{truncated").unwrap();
        assert!(cleanup_job_storage_in(&conn, &fixture.root).is_err());
        assert!(broken.join("audio.wav").exists());
        assert!(broken.join(".checkpoints").exists());
        assert!(broken.join(".in-progress").exists());
        assert!(!healthy.join("audio.wav").exists());
        assert!(!healthy.join(".checkpoints").exists());
    }

    #[test]
    fn expired_committed_jobs_with_missing_or_invalid_results_keep_recoverable_files() {
        let fixture = Fixture::new();
        let conn = fixture.open();
        for name in ["missing", "corrupt", "directory"] {
            let job = fixture.completed_job(&conn, name, true);
            fs::write(job.join("source.webm"), b"recoverable source").unwrap();
            fs::remove_file(job.join("result.json")).unwrap();
            match name {
                "corrupt" => fs::write(job.join("result.json"), b"{broken").unwrap(),
                "directory" => fs::create_dir(job.join("result.json")).unwrap(),
                _ => {}
            }
            expire_job(&job);
        }
        let orphan = fixture.root.join("orphan");
        fs::create_dir(&orphan).unwrap();
        expire_job(&orphan);
        let error = cleanup_job_storage_in(&conn, &fixture.root).unwrap_err();
        for name in ["missing", "corrupt", "directory"] {
            assert!(error.contains(name));
            let job = fixture.root.join(name);
            assert_eq!(fs::read(job.join("audio.wav")).unwrap(), b"saved audio");
            assert_eq!(
                fs::read(job.join("source.webm")).unwrap(),
                b"recoverable source"
            );
            assert!(job.join(".in-progress").exists());
            assert!(job.join(".checkpoints/0000.json").exists());
            for extension in ["txt", "srt", "vtt"] {
                assert_eq!(
                    fs::read(job.join(format!("transcript.{extension}"))).unwrap(),
                    b"saved export"
                );
            }
        }
        assert!(!orphan.exists());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
    }

    #[test]
    fn failed_export_keeps_the_old_destination_and_removes_partial_staging() {
        use std::io::{self, Read};
        struct BrokenReader(bool);
        impl Read for BrokenReader {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    return Err(io::Error::other("simulated disk/source failure"));
                }
                self.0 = true;
                bytes[0] = b'x';
                Ok(1)
            }
        }
        let fixture = Fixture::new();
        let target = fixture.root.join("saved.txt");
        fs::write(&target, b"old complete export").unwrap();
        assert!(super::write_export_atomically(&mut BrokenReader(false), &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old complete export");
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
        super::write_export_atomically(&mut &b"new complete export"[..], &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new complete export");
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
        let fresh = fixture.root.join("new.txt");
        super::write_export_atomically(&mut &b"fresh"[..], &fresh).unwrap();
        assert_eq!(fs::read(fresh).unwrap(), b"fresh");
    }

    #[cfg(windows)]
    #[test]
    fn export_staging_and_replacement_preserve_a_private_windows_dacl() {
        fn acl(path: &std::path::Path, make_private: bool) -> String {
            let script = if make_private {
                "$p=$env:WHISPERTUBE_EXPORT_ACL_PATH; $a=Get-Acl -LiteralPath $p; $a.SetAccessRuleProtection($true,$false); $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; $rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','Allow'); $a.SetAccessRule($rule); Set-Acl -LiteralPath $p -AclObject $a; (Get-Acl -LiteralPath $p).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)"
            } else {
                "(Get-Acl -LiteralPath $env:WHISPERTUBE_EXPORT_ACL_PATH).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)"
            };
            let mut command = std::process::Command::new("pwsh");
            crate::process::hide_console(&mut command);
            let output = command
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    &format!("$ErrorActionPreference='Stop'; {script}"),
                ])
                .env_remove("PSModulePath")
                .env("WHISPERTUBE_EXPORT_ACL_PATH", path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let sddl = String::from_utf8(output.stdout).unwrap();
            let (control, entries) = sddl.trim().split_once('(').unwrap();
            // Windows recomputes the historical AUTO_INHERITED marker when a
            // file is created. Compare every ACE and protection/inheritance
            // policy; that historical marker does not change access rights.
            format!("{}({entries}", control.replace("AI", ""))
        }
        let fixture = Fixture::new();
        let target = fixture.root.join("private.txt");
        fs::write(&target, b"private old export").unwrap();
        let expected = acl(&target, true);
        let staging = fixture.root.join("private-stage.tmp");
        let file = super::create_export_staging(&staging, Some(&target)).unwrap();
        // The empty staging file must already have the private ACL; applying
        // permissions only after copying would expose bytes through the parent ACL.
        assert_eq!(acl(&staging, false), expected);
        drop(file);
        fs::remove_file(staging).unwrap();
        super::write_export_atomically(&mut &b"private new export"[..], &target).unwrap();
        assert_eq!(acl(&target, false), expected);
        assert_eq!(fs::read(target).unwrap(), b"private new export");
    }

    #[cfg(windows)]
    #[test]
    fn locked_export_destination_is_preserved_on_publish_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let target = fixture.root.join("locked.txt");
        fs::write(&target, b"old export").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        assert!(super::write_export_atomically(&mut &b"replacement"[..], &target).is_err());
        drop(lock);
        assert_eq!(fs::read(target).unwrap(), b"old export");
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
    }

    #[test]
    fn committed_delete_is_replayed_after_database_reopen() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        let id = fixture.insert(&conn, "job");
        assert_eq!(
            queue_history_deletion(&mut conn, &fixture.root, &[id, id]).unwrap(),
            [id]
        );
        assert!(fixture.root.join("job/result.json").exists());
        drop(conn); // Simulate termination after commit, before file cleanup.
        let conn = fixture.open();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(cleanup_pending_deletions(&conn, &fixture.root)
            .unwrap()
            .is_empty());
        assert!(!fixture.root.join("job").exists());
        assert!(cleanup_pending_deletions(&conn, &fixture.root)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn failed_transaction_preserves_rows_and_files() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        let id = fixture.insert(&conn, "job");
        assert!(queue_history_deletion(&mut conn, &fixture.root, &[id, id + 1]).is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pending_job_deletions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(fixture.root.join("job/result.json").exists());
    }

    #[test]
    fn cleanup_failures_remain_durable_and_do_not_delete_live_or_external_data() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        let id = fixture.insert(&conn, "job");
        queue_history_deletion(&mut conn, &fixture.root, &[id]).unwrap();
        // Deterministic cleanup error, independent of OS permissions.
        fs::remove_dir_all(fixture.root.join("job")).unwrap();
        fs::write(fixture.root.join("job"), b"blocked").unwrap();
        conn.execute(
            "INSERT INTO pending_job_deletions VALUES ('../outside')",
            [],
        )
        .unwrap();
        fixture.insert(&conn, "live");
        conn.execute("INSERT INTO pending_job_deletions VALUES ('live')", [])
            .unwrap();
        assert_eq!(
            cleanup_pending_deletions(&conn, &fixture.root)
                .unwrap()
                .len(),
            3
        );
        assert!(fixture.root.join("live/result.json").exists());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pending_job_deletions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
        fs::remove_file(fixture.root.join("job")).unwrap();
        assert_eq!(
            cleanup_pending_deletions(&conn, &fixture.root)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn rejects_history_result_before_deserializing_an_oversized_file() {
        let bytes = vec![b'x'; MAX_TRANSCRIPT_RESULT_BYTES as usize + 1];
        let error = match decode_history_result(&bytes) {
            Ok(_) => panic!("oversized history result should be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("batas ukuran aman"));
    }
}
