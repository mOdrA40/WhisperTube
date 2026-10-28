use crate::{
    history,
    types::{
        HistoryFileInfo, HistoryFileKind, HistoryFileScope, HistoryFilesDeleteOutcome,
        TranscriptResult,
    },
};
use rusqlite::{params, Connection};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

fn scope_name(scope: HistoryFileScope) -> &'static str {
    match scope {
        HistoryFileScope::Audio => "audio",
        HistoryFileScope::Transcript => "transcript",
    }
}
fn file_name(kind: HistoryFileKind) -> &'static str {
    match kind {
        HistoryFileKind::Audio => "audio.wav",
        HistoryFileKind::Txt => "transcript.txt",
        HistoryFileKind::Srt => "transcript.srt",
        HistoryFileKind::Vtt => "transcript.vtt",
    }
}
fn file_scope(kind: HistoryFileKind) -> &'static str {
    match kind {
        HistoryFileKind::Audio => "audio",
        _ => "transcript",
    }
}
pub(crate) fn is_deleted(conn: &Connection, id: i64, kind: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM history_file_deletions WHERE history_id=?1 AND kind=?2)",
        params![id, kind],
        |row| row.get(0),
    )
    .map_err(|e| format!("Gagal membaca status file history: {e}"))
}
fn job_directory(conn: &Connection, root: &Path, id: i64) -> Result<Option<PathBuf>, String> {
    if id <= 0 {
        return Err("Identitas history tidak valid.".into());
    }
    let path: String = conn
        .query_row("SELECT result_path FROM history WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .map_err(|e| format!("History tidak ditemukan: {e}"))?;
    history::job_dir_for_result_in(root, &path)
}
pub(crate) fn metadata_only_result(
    conn: &Connection,
    id: i64,
    job: &Path,
) -> Result<TranscriptResult, String> {
    let mut result = conn
        .query_row(
            "SELECT title,channel,language,duration,model,backend FROM history WHERE id=?1",
            [id],
            |row| {
                Ok(TranscriptResult {
                    audio_recovered: false,
                    history_id: id,
                    title: row.get(0)?,
                    channel: row.get(1)?,
                    language: row.get(2)?,
                    duration: row.get(3)?,
                    source_duration: None,
                    model: row.get(4)?,
                    backend: row.get(5)?,
                    segments: vec![],
                    text: String::new(),
                    txt_path: String::new(),
                    srt_path: String::new(),
                    vtt_path: String::new(),
                    audio_path: None,
                    transcript_available: false,
                })
            },
        )
        .map_err(|e| format!("Gagal membaca metadata history: {e}"))?;
    let audio = job.join("audio.wav");
    if !is_deleted(conn, id, "audio")? && crate::paths::is_regular_file(&audio) {
        result.audio_path = Some(audio.to_string_lossy().into_owned());
    }
    Ok(result)
}
pub(crate) fn removed_transcript_directories(
    conn: &Connection,
    root: &Path,
) -> Result<BTreeSet<PathBuf>, String> {
    let paths = conn.prepare("SELECT h.result_path FROM history h JOIN history_file_deletions d ON h.id=d.history_id WHERE d.kind='transcript'")
        .and_then(|mut q| q.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>())
        .map_err(|e| format!("Gagal membaca status transkrip: {e}"))?;
    let mut directories = BTreeSet::new();
    for path in paths {
        if let Some(dir) = history::job_dir_for_result_in(root, &path)? {
            directories.insert(dir);
        }
    }
    Ok(directories)
}
fn queue_deletion(
    conn: &mut Connection,
    root: &Path,
    ids: &[i64],
    scope: HistoryFileScope,
) -> Result<Vec<i64>, String> {
    if ids.is_empty() || ids.len() > 100 || ids.iter().any(|id| *id <= 0) {
        return Err("Pilih 1 sampai 100 history yang valid.".into());
    }
    let ids = ids.iter().copied().collect::<BTreeSet<_>>();
    let transaction = conn
        .transaction()
        .map_err(|e| format!("Gagal memulai penghapusan file: {e}"))?;
    for id in &ids {
        job_directory(&transaction, root, *id)?;
        transaction.execute("INSERT INTO history_file_deletions(history_id,kind,pending) VALUES (?1,?2,1) ON CONFLICT(history_id,kind) DO UPDATE SET pending=1", params![id, scope_name(scope)])
            .map_err(|e| format!("Gagal menyimpan intent penghapusan file: {e}"))?;
    }
    transaction
        .commit()
        .map_err(|e| format!("Gagal menyelesaikan penghapusan file: {e}"))?;
    Ok(ids.into_iter().collect())
}
pub(crate) fn cleanup_pending(conn: &Connection, root: &Path) -> Result<Vec<String>, String> {
    // Replay completed transcript intents too: older versions omitted transcript.json.
    let rows = conn
        .prepare("SELECT history_id,kind FROM history_file_deletions WHERE pending=1 OR kind='transcript'")
        .and_then(|mut q| {
            q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| format!("Gagal membaca penghapusan file tertunda: {e}"))?;
    let mut warnings = vec![];
    for (id, kind) in rows {
        let cleanup = || -> Result<(), String> {
            if let Some(job) = job_directory(conn, root, id)? {
                let names: &[&str] = match kind.as_str() {
                    "audio" => &["audio.wav"],
                    "transcript" => &[
                        "transcript.json",
                        "transcript.txt",
                        "transcript.srt",
                        "transcript.vtt",
                        "result.json",
                        "result.json.tmp",
                        ".cleanup-complete",
                    ],
                    _ => return Err("Jenis penghapusan file tidak valid.".into()),
                };
                let mut errors = vec![];
                for name in names.iter().copied().chain(std::iter::once(".in-progress")) {
                    match fs::remove_file(job.join(name)) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => errors.push(format!("{name}: {e}")),
                    }
                }
                if let Err(e) = history::cleanup_source_audio(&job) {
                    errors.push(e);
                }
                if let Err(e) = crate::checkpoint::cleanup(&job) {
                    errors.push(e);
                }
                if kind == "transcript" {
                    // Final export writes use transcript.<UUID>.tmp siblings.
                    // A crash can leave an older sibling even after a retry
                    // commits successfully. Unlink only that owned pattern.
                    for entry in fs::read_dir(&job)
                        .map_err(|e| format!("Gagal membaca staging transkrip: {e}"))?
                    {
                        let entry =
                            entry.map_err(|e| format!("Gagal membaca staging transkrip: {e}"))?;
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        let owned = name
                            .strip_prefix("transcript.")
                            .and_then(|name| name.strip_suffix(".tmp"))
                            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
                        if owned {
                            match fs::remove_file(entry.path()) {
                                Ok(()) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                                Err(e) => errors.push(format!("Staging {name}: {e}")),
                            }
                        }
                    }
                }
                if !errors.is_empty() {
                    return Err(errors.join("; "));
                }
            }
            conn.execute(
                "UPDATE history_file_deletions SET pending=0 WHERE history_id=?1 AND kind=?2 AND pending<>0",
                params![id, kind],
            )
            .map_err(|e| format!("Gagal menyelesaikan cleanup file: {e}"))?;
            Ok(())
        };
        if let Err(e) = cleanup() {
            conn.execute(
                "UPDATE history_file_deletions SET pending=1 WHERE history_id=?1 AND kind=?2 AND pending<>1",
                params![id, kind],
            )
            .map_err(|error| format!("Gagal menyimpan status cleanup file: {error}"))?;
            warnings.push(format!("Penghapusan file {kind} history {id} tertunda; file mungkin masih ada di disk: {e}"));
        }
    }
    Ok(warnings)
}
pub fn delete(
    app: &AppHandle,
    ids: &[i64],
    scope: HistoryFileScope,
) -> Result<HistoryFilesDeleteOutcome, String> {
    let mut conn = history::open_connection(app)?;
    let root = crate::paths::jobs_dir(app)?;
    let updated_ids = queue_deletion(&mut conn, &root, ids, scope)?;
    let cleanup_warnings = cleanup_pending(&conn, &root)
        .unwrap_or_else(|e| vec![format!("Pembersihan file tertunda: {e}")]);
    history::invalidate_job_storage_cache();
    Ok(HistoryFilesDeleteOutcome {
        updated_ids,
        cleanup_warnings,
    })
}
fn list_in(conn: &Connection, root: &Path, id: i64) -> Result<Vec<HistoryFileInfo>, String> {
    let job = job_directory(conn, root, id)?.ok_or("Folder file history tidak tersedia.")?;
    [HistoryFileKind::Audio, HistoryFileKind::Txt, HistoryFileKind::Srt, HistoryFileKind::Vtt].into_iter().map(|kind| {
        let path = job.join(file_name(kind));
        let deleted = is_deleted(conn, id, file_scope(kind))?;
        let pending: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM history_file_deletions WHERE history_id=?1 AND kind=?2 AND pending=1)", params![id, file_scope(kind)], |r| r.get(0))
            .map_err(|e| format!("Gagal membaca status cleanup: {e}"))?;
        let bytes = match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() => Some(m.len()),
            Ok(_) => None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Gagal membaca file history: {e}")),
        };
        Ok(HistoryFileInfo { kind, path: path.to_string_lossy().into_owned(), bytes, available: bytes.is_some() && !deleted, pending_deletion: pending })
    }).collect()
}
pub fn list(app: &AppHandle, id: i64) -> Result<Vec<HistoryFileInfo>, String> {
    list_in(
        &history::open_connection(app)?,
        &crate::paths::jobs_dir(app)?,
        id,
    )
}
pub fn reveal(app: &AppHandle, id: i64, kind: HistoryFileKind) -> Result<(), String> {
    let file = list(app, id)?
        .into_iter()
        .find(|f| file_name(f.kind) == file_name(kind))
        .ok_or("Jenis file tidak tersedia.")?;
    if !file.available {
        return Err("File sudah dihapus atau tidak disimpan.".into());
    }
    let path =
        fs::canonicalize(&file.path).map_err(|e| format!("Gagal memvalidasi lokasi file: {e}"))?;
    let expected = Path::new(&file.path)
        .parent()
        .ok_or("Folder file tidak valid.")?;
    if path.parent() != Some(expected) {
        return Err("File berada di luar folder history.".into());
    }
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| format!("Gagal membuka lokasi file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        job: PathBuf,
        db: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("history-files-{}", uuid::Uuid::new_v4()));
            let job = root.join("saved-job");
            fs::create_dir_all(job.join(".checkpoints")).unwrap();
            for name in [
                "audio.wav",
                "transcript.json",
                "transcript.txt",
                "transcript.srt",
                "transcript.vtt",
                "result.json",
                "source.webm",
                ".in-progress",
            ] {
                fs::write(job.join(name), format!("original {name}")).unwrap();
            }
            fs::write(job.join(".checkpoints/current.json"), b"checkpoint text").unwrap();
            fs::write(
                job.join(format!("transcript.{}.tmp", uuid::Uuid::new_v4())),
                b"interrupted export",
            )
            .unwrap();
            fs::write(job.join("result.json.tmp"), b"interrupted result").unwrap();
            fs::write(job.join("transcript.notes.tmp"), b"unrecognized file").unwrap();
            let db = root.join("history.db");
            let conn = Connection::open(&db).unwrap();
            history::initialize_schema(&conn).unwrap();
            conn.execute("INSERT INTO history(title,channel,source_url,created_at,duration,language,model,backend,result_path) VALUES ('Video','Channel','url','date',60,'id','base','cpu',?1)", [job.join("result.json").to_str().unwrap()]).unwrap();
            Self { root, job, db }
        }
        fn open(&self) -> Connection {
            Connection::open(&self.db).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn completed_legacy_tombstone_is_reconciled_after_reopen() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Transcript).unwrap();
        conn.execute("UPDATE history_file_deletions SET pending=0", [])
            .unwrap();
        drop(conn);
        let conn = fixture.open();
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert!(!fixture.job.join("transcript.json").exists());
        assert!(fixture.job.join("audio.wav").exists());
        assert!(fixture.job.join("transcript.notes.tmp").exists());
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn legacy_completed_tombstone_reports_locked_json_as_pending() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Transcript).unwrap();
        conn.execute("UPDATE history_file_deletions SET pending=0", [])
            .unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(fixture.job.join("transcript.json"))
            .unwrap();
        assert!(!cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert_eq!(
            conn.query_row("SELECT pending FROM history_file_deletions", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            1
        );
        assert!(fixture.job.join("audio.wav").exists());
        drop(lock);
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert!(!fixture.job.join("transcript.json").exists());
    }

    #[test]
    fn audio_deletion_is_durable_and_preserves_transcripts_and_history() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        let original = fs::read(fixture.job.join("result.json")).unwrap();
        queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Audio).unwrap();
        assert!(fixture.job.join("audio.wav").exists()); // Intent committed before unlink.
        drop(conn);
        let conn = fixture.open();
        history::initialize_schema(&conn).unwrap(); // Non-destructive schema initialization.
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert!(!fixture.job.join("audio.wav").exists());
        assert!(!fixture.job.join("source.webm").exists());
        assert!(!fixture.job.join(".checkpoints").exists());
        assert_eq!(fs::read(fixture.job.join("result.json")).unwrap(), original);
        for extension in ["txt", "srt", "vtt"] {
            assert!(fixture.job.join(format!("transcript.{extension}")).exists());
        }
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let files = list_in(&conn, &fixture.root, 1).unwrap();
        assert!(!files[0].available);
        assert!(files[1..].iter().all(|file| file.available));
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
    }

    #[test]
    fn transcript_deletion_keeps_audio_and_loadable_metadata_across_startup() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Transcript).unwrap();
        drop(conn);
        let conn = fixture.open();
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        for name in [
            "result.json",
            "transcript.json",
            "transcript.txt",
            "transcript.srt",
            "transcript.vtt",
            ".checkpoints",
            "result.json.tmp",
        ] {
            assert!(!fixture.job.join(name).exists());
        }
        let leftovers = fs::read_dir(&fixture.job)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!leftovers
            .iter()
            .any(|name| name.starts_with("transcript.") && name != "transcript.notes.tmp"));
        assert_eq!(
            fs::read(fixture.job.join("transcript.notes.tmp")).unwrap(),
            b"unrecognized file"
        );
        assert_eq!(
            fs::read(fixture.job.join("audio.wav")).unwrap(),
            b"original audio.wav"
        );
        history::cleanup_job_storage_in(&conn, &fixture.root).unwrap();
        let result = metadata_only_result(&conn, 1, &fixture.job).unwrap();
        assert!(!result.transcript_available);
        assert!(result.text.is_empty() && result.segments.is_empty());
        assert!(result.audio_path.is_some());
        assert_eq!(result.title, "Video");
        let files = list_in(&conn, &fixture.root, 1).unwrap();
        assert!(files[0].available);
        assert!(files[1..].iter().all(|file| !file.available));
    }

    #[test]
    fn failed_cleanup_stays_pending_and_retries_without_touching_other_files() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.job.join("audio.wav")).unwrap();
        fs::create_dir(fixture.job.join("audio.wav")).unwrap(); // Cannot unlink a directory as an audio file.
        let mut conn = fixture.open();
        queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Audio).unwrap();
        assert!(!cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert!(list_in(&conn, &fixture.root, 1).unwrap()[0].pending_deletion);
        assert!(fixture.job.join("transcript.txt").exists());
        fs::remove_dir(fixture.job.join("audio.wav")).unwrap();
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert!(!list_in(&conn, &fixture.root, 1).unwrap()[0].pending_deletion);
    }

    #[test]
    fn invalid_batch_rolls_back_and_external_paths_are_rejected() {
        let fixture = Fixture::new();
        let mut conn = fixture.open();
        assert!(
            queue_deletion(&mut conn, &fixture.root, &[1, 999], HistoryFileScope::Audio).is_err()
        );
        assert!(!is_deleted(&conn, 1, "audio").unwrap());
        assert!(
            queue_deletion(&mut conn, &fixture.root, &[1; 101], HistoryFileScope::Audio).is_err()
        );
        let external =
            std::env::temp_dir().join(format!("history-external-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&external).unwrap();
        fs::write(external.join("audio.wav"), b"external audio").unwrap();
        conn.execute(
            "UPDATE history SET result_path=?1 WHERE id=1",
            [external.join("result.json").to_str().unwrap()],
        )
        .unwrap();
        assert!(queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Audio).is_err());
        assert_eq!(
            fs::read(external.join("audio.wav")).unwrap(),
            b"external audio"
        );
        fs::remove_dir_all(external).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_audio_is_reported_pending_then_deleted_after_unlock() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(fixture.job.join("audio.wav"))
            .unwrap();
        let mut conn = fixture.open();
        queue_deletion(&mut conn, &fixture.root, &[1], HistoryFileScope::Audio).unwrap();
        assert!(!cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        drop(locked);
        assert_eq!(
            fs::read(fixture.job.join("audio.wav")).unwrap(),
            b"original audio.wav"
        );
        assert!(list_in(&conn, &fixture.root, 1).unwrap()[0].pending_deletion);
        assert!(cleanup_pending(&conn, &fixture.root).unwrap().is_empty());
        assert!(!fixture.job.join("audio.wav").exists());
        assert!(fixture.job.join("transcript.txt").exists());
    }
}
