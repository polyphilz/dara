use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{
    database::{
        self,
        commands::{run_writer, CommandResult},
        snapshot::FinalizedSnapshotSummary,
        Database, DatabaseDiagnosticsSnapshot, DatabaseError, DatabasePaths,
        MediaMaintenanceReport,
    },
    media::OcrCoordinator,
    search::{SearchService, SemanticSearchPhase},
};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub application_version: String,
    pub database: DatabaseDiagnosticsSnapshot,
    pub semantic_model: SemanticModelDiagnostics,
    pub storage: DiagnosticsStorage,
    pub latest_snapshot: Option<FinalizedSnapshotSummary>,
    pub last_media_maintenance: Option<MediaMaintenanceReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelDiagnostics {
    pub model_name: String,
    pub phase: SemanticSearchPhase,
    pub downloaded_bytes: u64,
    pub expected_bytes: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsStorage {
    pub relational_database_bytes: u64,
    pub media_database_bytes: u64,
    pub model_bytes: u64,
    pub snapshots_bytes: u64,
    pub logs_bytes: u64,
}

#[tauri::command]
pub async fn load_diagnostics(
    app: AppHandle,
    database: State<'_, Database>,
    search: State<'_, SearchService>,
    ocr: State<'_, OcrCoordinator>,
) -> CommandResult<DiagnosticsSnapshot> {
    let client = database.client();
    let paths = database.paths().clone();
    let app_log_dir = app.path().app_log_dir().ok();
    let search = search.inner().clone();
    let semantic_status = search.status();
    let last_error = match semantic_status.phase {
        SemanticSearchPhase::Failed | SemanticSearchPhase::Unavailable => semantic_status.message,
        _ => None,
    };
    let semantic_model = SemanticModelDiagnostics {
        model_name: search.model_name().into(),
        phase: semantic_status.phase,
        downloaded_bytes: semantic_status.downloaded_bytes,
        expected_bytes: semantic_status.model_bytes,
        last_error,
    };
    let last_media_maintenance = ocr.last_media_maintenance();

    let database = run_writer(move || client.load_database_diagnostics()).await?;
    let (storage, latest_snapshot) =
        run_writer(move || load_filesystem_diagnostics(&paths, app_log_dir.as_deref(), &search))
            .await?;

    Ok(DiagnosticsSnapshot {
        application_version: env!("CARGO_PKG_VERSION").into(),
        database,
        semantic_model,
        storage,
        latest_snapshot,
        last_media_maintenance,
    })
}

fn load_filesystem_diagnostics(
    paths: &DatabasePaths,
    app_log_dir: Option<&Path>,
    search: &SearchService,
) -> database::Result<(DiagnosticsStorage, Option<FinalizedSnapshotSummary>)> {
    let data_log_dir = paths.root.join("logs");
    let mut logs_bytes = directory_size(&data_log_dir)?;
    if app_log_dir.is_some_and(|path| path != data_log_dir) {
        logs_bytes =
            logs_bytes.saturating_add(directory_size(app_log_dir.expect("checked above"))?);
    }

    Ok((
        DiagnosticsStorage {
            relational_database_bytes: sqlite_size(&paths.main)?,
            media_database_bytes: sqlite_size(&paths.media)?,
            model_bytes: search.model_disk_usage_bytes()?,
            snapshots_bytes: directory_size(&paths.backups)?,
            logs_bytes,
        },
        database::snapshot::latest_finalized_snapshot(&paths.backups)?,
    ))
}

fn sqlite_size(path: &Path) -> Result<u64, DatabaseError> {
    let mut total = file_size(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        total = total.saturating_add(file_size(&PathBuf::from(companion))?);
    }
    Ok(total)
}

fn file_size(path: &Path) -> Result<u64, DatabaseError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(metadata.len()),
        Ok(_) => Ok(0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn directory_size(path: &Path) -> Result<u64, DatabaseError> {
    let mut total = 0_u64;
    let mut pending = vec![path.to_owned()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            let metadata = entry.file_type()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(entry.metadata()?.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{directory_size, sqlite_size};

    #[test]
    fn sqlite_size_includes_wal_and_shared_memory_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("dara.sqlite3");
        fs::write(&database, [0_u8; 3]).expect("database fixture");
        fs::write(directory.path().join("dara.sqlite3-wal"), [0_u8; 5]).expect("WAL fixture");
        fs::write(directory.path().join("dara.sqlite3-shm"), [0_u8; 7])
            .expect("shared-memory fixture");

        assert_eq!(sqlite_size(&database).expect("SQLite size"), 15);
    }

    #[test]
    fn directory_size_counts_nested_regular_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).expect("nested directory");
        fs::write(directory.path().join("first.log"), [0_u8; 11]).expect("first fixture");
        fs::write(nested.join("second.log"), [0_u8; 13]).expect("second fixture");

        assert_eq!(
            directory_size(directory.path()).expect("directory size"),
            24
        );
    }
}
