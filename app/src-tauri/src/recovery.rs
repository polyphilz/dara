use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app_lock::{AppDataLock, AppDataLockError},
    database::{
        self,
        migrations::MigrationHeads,
        snapshot::{self, ValidatedSnapshot},
        DatabasePaths,
    },
};

const RECOVERY_ARGUMENT: &str = "recovery";
const LIST_ARGUMENT: &str = "list";
const VERIFY_ARGUMENT: &str = "verify";
const RESTORE_ARGUMENT: &str = "restore";
const RESTORE_INTENT_FILE_NAME: &str = ".dara-restore-intent.json";
const RESTORE_INTENT_TEMP_FILE_NAME: &str = ".dara-restore-intent.json.tmp";
const RESTORE_INTENT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum RestorePhase {
    Staged,
    Installing,
    InstalledValidated,
    RollingBack,
    Completed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum RestoreFile {
    Main,
    Media,
    MainWal,
    MainShm,
    MediaWal,
    MediaShm,
}

impl RestoreFile {
    const ALL: [Self; 6] = [
        Self::Main,
        Self::Media,
        Self::MainWal,
        Self::MainShm,
        Self::MediaWal,
        Self::MediaShm,
    ];

    const fn file_name(self) -> &'static str {
        match self {
            Self::Main => "dara.sqlite3",
            Self::Media => "media.sqlite3",
            Self::MainWal => "dara.sqlite3-wal",
            Self::MainShm => "dara.sqlite3-shm",
            Self::MediaWal => "media.sqlite3-wal",
            Self::MediaShm => "media.sqlite3-shm",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RestoreIntent {
    format_version: u32,
    phase: RestorePhase,
    source_manifest: PathBuf,
    expected: snapshot::SnapshotManifest,
    stage_directory: String,
    rollback_directory: String,
    rollback_files: Vec<RestoreFile>,
    safety_snapshot: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RestoreFailpoint {
    IntentWritten,
    InstallingRecorded,
    RollbackMain,
    RollbackMedia,
    RollbackCompanions,
    MediaInstalled,
    MainInstalled,
    InstalledValidated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RecoveryCommand {
    List {
        data_directory: PathBuf,
    },
    Verify {
        manifest: PathBuf,
    },
    Restore {
        manifest: PathBuf,
        data_directory: PathBuf,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Entrypoint {
    Application,
    Recovery(RecoveryCommand),
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("{0}")]
    Usage(&'static str),

    #[error(transparent)]
    Lock(#[from] AppDataLockError),

    #[error(transparent)]
    Database(#[from] database::DatabaseError),

    #[error("recovery I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("recovery output serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("snapshot manifest must be inside a Dara backups directory: {0}")]
    ManifestOutsideBackups(PathBuf),

    #[error("Dara data directory does not exist or is not a directory: {0}")]
    InvalidDataDirectory(PathBuf),

    #[error("restore target has an incomplete or non-regular database pair")]
    InvalidRestoreTarget,

    #[error("restore journal is invalid: {0}")]
    InvalidRestoreJournal(String),

    #[error("a previous restore must be validated by launching Dara before another restore")]
    RestoreAwaitingLaunch,

    #[cfg(test)]
    #[error("injected restore interruption")]
    InjectedInterruption,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotReport {
    manifest: PathBuf,
    created_at: i64,
    application_version: String,
    migration_heads: MigrationHeads,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedSnapshotReport {
    valid: bool,
    snapshot: SnapshotReport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RestoreReport {
    restored_from: PathBuf,
    data_directory: PathBuf,
    snapshot_created_at: i64,
    safety_snapshot: Option<PathBuf>,
    awaiting_application_validation: bool,
}

pub fn run_from_args(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<String>, RecoveryError> {
    let command = match parse_entrypoint(arguments)? {
        Entrypoint::Application => return Ok(None),
        Entrypoint::Recovery(command) => command,
    };
    let output = match command {
        RecoveryCommand::List { data_directory } => {
            require_existing_data_directory(&data_directory)?;
            let _lock = AppDataLock::acquire(&data_directory)?;
            let reports = list_snapshots(&DatabasePaths::new(data_directory))?;
            serde_json::to_string_pretty(&reports)?
        }
        RecoveryCommand::Verify { manifest } => {
            let (manifest, data_root) = canonical_manifest_and_data_root(&manifest)?;
            let _lock = AppDataLock::acquire(&data_root)?;
            let snapshot = snapshot::load_and_validate_snapshot(&manifest)?;
            serde_json::to_string_pretty(&VerifiedSnapshotReport {
                valid: true,
                snapshot: snapshot_report(snapshot),
            })?
        }
        RecoveryCommand::Restore {
            manifest,
            data_directory,
        } => serde_json::to_string_pretty(&restore_snapshot(&manifest, &data_directory)?)?,
    };
    Ok(Some(output))
}

fn require_existing_data_directory(path: &Path) -> Result<(), RecoveryError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(RecoveryError::InvalidDataDirectory(path.to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(RecoveryError::InvalidDataDirectory(path.to_owned()))
        }
        Err(error) => Err(error.into()),
    }
}

fn parse_entrypoint(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Entrypoint, RecoveryError> {
    let mut arguments = arguments.into_iter();
    let _executable = arguments.next();
    let Some(mode) = arguments.next() else {
        return Ok(Entrypoint::Application);
    };
    if mode != OsStr::new(RECOVERY_ARGUMENT) {
        return Ok(Entrypoint::Application);
    }
    let operation = arguments
        .next()
        .ok_or(RecoveryError::Usage(recovery_usage()))?;
    let command = if operation == OsStr::new(LIST_ARGUMENT) {
        let path = required_argument(&mut arguments)?;
        reject_extra_arguments(&mut arguments)?;
        RecoveryCommand::List {
            data_directory: path.into(),
        }
    } else if operation == OsStr::new(VERIFY_ARGUMENT) {
        let path = required_argument(&mut arguments)?;
        reject_extra_arguments(&mut arguments)?;
        RecoveryCommand::Verify {
            manifest: path.into(),
        }
    } else if operation == OsStr::new(RESTORE_ARGUMENT) {
        let manifest = required_argument(&mut arguments)?;
        let data_directory = required_argument(&mut arguments)?;
        reject_extra_arguments(&mut arguments)?;
        RecoveryCommand::Restore {
            manifest: manifest.into(),
            data_directory: data_directory.into(),
        }
    } else {
        return Err(RecoveryError::Usage(recovery_usage()));
    };
    Ok(Entrypoint::Recovery(command))
}

fn required_argument(
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<OsString, RecoveryError> {
    arguments
        .next()
        .ok_or(RecoveryError::Usage(recovery_usage()))
}

fn reject_extra_arguments(
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<(), RecoveryError> {
    if arguments.next().is_some() {
        return Err(RecoveryError::Usage(recovery_usage()));
    }
    Ok(())
}

const fn recovery_usage() -> &'static str {
    "usage:\n  dara recovery list <data-directory>\n  dara recovery verify <manifest>\n  dara recovery restore <manifest> <data-directory>"
}

fn list_snapshots(paths: &DatabasePaths) -> Result<Vec<SnapshotReport>, RecoveryError> {
    let entries = match fs::read_dir(&paths.backups) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut reports = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }
        if let Ok(snapshot) = snapshot::load_and_validate_snapshot(&path) {
            reports.push(snapshot_report(snapshot));
        }
    }
    reports.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.manifest.cmp(&right.manifest))
    });
    Ok(reports)
}

fn snapshot_report(snapshot: ValidatedSnapshot) -> SnapshotReport {
    SnapshotReport {
        manifest: snapshot.manifest_path,
        created_at: snapshot.manifest.created_at,
        application_version: snapshot.manifest.application_version,
        migration_heads: MigrationHeads {
            main: snapshot.manifest.main.migration_head,
            media: snapshot.manifest.media.migration_head,
        },
    }
}

fn canonical_manifest_and_data_root(manifest: &Path) -> Result<(PathBuf, PathBuf), RecoveryError> {
    let canonical_manifest = fs::canonicalize(manifest)?;
    let backups = canonical_manifest
        .parent()
        .filter(|parent| parent.file_name() == Some(OsStr::new("backups")))
        .ok_or_else(|| RecoveryError::ManifestOutsideBackups(canonical_manifest.clone()))?;
    let data_root = backups
        .parent()
        .map(Path::to_owned)
        .ok_or_else(|| RecoveryError::ManifestOutsideBackups(canonical_manifest.clone()))?;
    Ok((canonical_manifest, data_root))
}

fn restore_snapshot(
    manifest_path: &Path,
    data_directory: &Path,
) -> Result<RestoreReport, RecoveryError> {
    let (manifest_path, source_root) = canonical_manifest_and_data_root(manifest_path)?;
    fs::create_dir_all(data_directory)?;
    let target_root = fs::canonicalize(data_directory)?;
    let _locks = acquire_ordered_locks(&source_root, &target_root)?;
    let paths = DatabasePaths::new(&target_root);

    recover_interrupted_restore(&paths)?;
    if restore_intent_path(&paths).exists() {
        return Err(RecoveryError::RestoreAwaitingLaunch);
    }

    let source = snapshot::load_and_validate_snapshot(&manifest_path)?;
    prepare_restore(&paths, source, None)
}

fn acquire_ordered_locks(
    source_root: &Path,
    target_root: &Path,
) -> Result<Vec<AppDataLock>, RecoveryError> {
    let mut roots = vec![source_root.to_owned(), target_root.to_owned()];
    roots.sort();
    roots.dedup();
    roots
        .iter()
        .map(|root| AppDataLock::acquire(root).map_err(RecoveryError::from))
        .collect()
}

fn prepare_restore(
    paths: &DatabasePaths,
    source: ValidatedSnapshot,
    failpoint: Option<RestoreFailpoint>,
) -> Result<RestoreReport, RecoveryError> {
    let initial_files = inspect_restore_target(paths)?;
    let safety_snapshot = if initial_files.is_empty() {
        None
    } else {
        Some(snapshot::create_snapshot_pair(paths, env!("CARGO_PKG_VERSION"))?.manifest_path)
    };
    let rollback_files = inspect_restore_target(paths)?;

    let identifier = Uuid::now_v7();
    let stage_directory = format!(".dara-restore-stage-{identifier}");
    let rollback_directory = format!(".dara-restore-rollback-{identifier}");
    let stage = paths.root().join(&stage_directory);
    let rollback = paths.root().join(&rollback_directory);
    fs::create_dir(&stage)?;
    fs::create_dir(&rollback)?;

    let stage_main = stage.join(RestoreFile::Main.file_name());
    let stage_media = stage.join(RestoreFile::Media.file_name());
    if let Err(error) = (|| -> Result<(), RecoveryError> {
        copy_and_sync(&source.main_path, &stage_main)?;
        copy_and_sync(&source.media_path, &stage_media)?;
        snapshot::validate_snapshot_pair_files(&source.manifest, &stage_main, &stage_media)?;
        sync_directory(&stage)?;
        sync_directory(paths.root())?;
        Ok(())
    })() {
        let _ = fs::remove_dir_all(&stage);
        let _ = fs::remove_dir(&rollback);
        return Err(error);
    }

    let mut intent = RestoreIntent {
        format_version: RESTORE_INTENT_FORMAT_VERSION,
        phase: RestorePhase::Staged,
        source_manifest: source.manifest_path.clone(),
        expected: source.manifest.clone(),
        stage_directory,
        rollback_directory,
        rollback_files,
        safety_snapshot: safety_snapshot.clone(),
    };
    write_restore_intent(paths, &intent)?;
    interrupt_if(RestoreFailpoint::IntentWritten, failpoint)?;
    resume_restore(paths, &mut intent, failpoint)?;

    Ok(RestoreReport {
        restored_from: source.manifest_path,
        data_directory: paths.root().to_owned(),
        snapshot_created_at: source.manifest.created_at,
        safety_snapshot,
        awaiting_application_validation: true,
    })
}

pub(crate) fn recover_interrupted_restore(paths: &DatabasePaths) -> Result<(), RecoveryError> {
    remove_file_if_exists(&restore_intent_temp_path(paths))?;
    let Some(mut intent) = load_restore_intent(paths)? else {
        return Ok(());
    };
    match intent.phase {
        RestorePhase::Staged | RestorePhase::Installing => resume_restore(paths, &mut intent, None),
        RestorePhase::InstalledValidated => Ok(()),
        RestorePhase::RollingBack => complete_rollback(paths, &mut intent),
        RestorePhase::Completed => cleanup_completed_restore(paths, &intent),
    }
}

pub(crate) fn confirm_restored_launch(paths: &DatabasePaths) -> Result<(), RecoveryError> {
    let Some(mut intent) = load_restore_intent(paths)? else {
        return Ok(());
    };
    if intent.phase != RestorePhase::InstalledValidated {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "cannot confirm phase {:?}",
            intent.phase
        )));
    }
    intent.phase = RestorePhase::Completed;
    write_restore_intent(paths, &intent)?;
    cleanup_completed_restore(paths, &intent)
}

fn resume_restore(
    paths: &DatabasePaths,
    intent: &mut RestoreIntent,
    failpoint: Option<RestoreFailpoint>,
) -> Result<(), RecoveryError> {
    let stage = restore_subdirectory(paths, &intent.stage_directory)?;
    let rollback = restore_subdirectory(paths, &intent.rollback_directory)?;
    let stage_main = stage.join(RestoreFile::Main.file_name());
    let stage_media = stage.join(RestoreFile::Media.file_name());

    if intent.phase == RestorePhase::Staged {
        if let Err(error) =
            snapshot::validate_snapshot_pair_files(&intent.expected, &stage_main, &stage_media)
        {
            intent.phase = RestorePhase::RollingBack;
            write_restore_intent(paths, intent)?;
            complete_rollback(paths, intent)?;
            return Err(error.into());
        }
        intent.phase = RestorePhase::Installing;
        write_restore_intent(paths, intent)?;
        interrupt_if(RestoreFailpoint::InstallingRecorded, failpoint)?;
    }
    if intent.phase != RestorePhase::Installing {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "cannot install phase {:?}",
            intent.phase
        )));
    }

    for restore_file in intent.rollback_files.clone() {
        let live = paths.root().join(restore_file.file_name());
        let saved = rollback.join(restore_file.file_name());
        if live.exists() && saved.exists() {
            let expected_hash = match restore_file {
                RestoreFile::Main => Some(intent.expected.main.sha256.as_str()),
                RestoreFile::Media => Some(intent.expected.media.sha256.as_str()),
                RestoreFile::MainWal
                | RestoreFile::MainShm
                | RestoreFile::MediaWal
                | RestoreFile::MediaShm => None,
            };
            let Some(expected_hash) = expected_hash else {
                return Err(RecoveryError::InvalidRestoreJournal(format!(
                    "moving the existing database into rollback storage: both {} and {} exist",
                    live.display(),
                    saved.display()
                )));
            };
            if let Err(error) = snapshot::validate_recorded_hash(&live, expected_hash) {
                intent.phase = RestorePhase::RollingBack;
                write_restore_intent(paths, intent)?;
                complete_rollback(paths, intent)?;
                return Err(error.into());
            }
        } else {
            move_once(
                &live,
                &saved,
                "moving the existing database into rollback storage",
            )?;
        }
        match restore_file {
            RestoreFile::Main => {
                interrupt_if(RestoreFailpoint::RollbackMain, failpoint)?;
            }
            RestoreFile::Media => {
                interrupt_if(RestoreFailpoint::RollbackMedia, failpoint)?;
            }
            RestoreFile::MainWal
            | RestoreFile::MainShm
            | RestoreFile::MediaWal
            | RestoreFile::MediaShm => {}
        }
    }
    interrupt_if(RestoreFailpoint::RollbackCompanions, failpoint)?;
    sync_directory(paths.root())?;
    sync_directory(&rollback)?;

    move_once(
        &stage_media,
        &paths.media,
        "installing the restored media database",
    )?;
    interrupt_if(RestoreFailpoint::MediaInstalled, failpoint)?;
    move_once(
        &stage_main,
        &paths.main,
        "installing the restored main database",
    )?;
    interrupt_if(RestoreFailpoint::MainInstalled, failpoint)?;
    sync_directory(paths.root())?;

    if let Err(error) =
        snapshot::validate_snapshot_pair_files(&intent.expected, &paths.main, &paths.media)
    {
        intent.phase = RestorePhase::RollingBack;
        write_restore_intent(paths, intent)?;
        complete_rollback(paths, intent)?;
        return Err(error.into());
    }

    intent.phase = RestorePhase::InstalledValidated;
    write_restore_intent(paths, intent)?;
    interrupt_if(RestoreFailpoint::InstalledValidated, failpoint)?;
    Ok(())
}

fn complete_rollback(
    paths: &DatabasePaths,
    intent: &mut RestoreIntent,
) -> Result<(), RecoveryError> {
    if intent.phase != RestorePhase::RollingBack {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "cannot roll back phase {:?}",
            intent.phase
        )));
    }
    let rollback = restore_subdirectory(paths, &intent.rollback_directory)?;
    for restore_file in RestoreFile::ALL {
        let target = paths.root().join(restore_file.file_name());
        let saved = rollback.join(restore_file.file_name());
        if intent.rollback_files.contains(&restore_file) {
            if saved.exists() {
                remove_file_if_exists(&target)?;
                fs::rename(&saved, &target)?;
            } else if !target.exists() {
                return Err(RecoveryError::InvalidRestoreJournal(format!(
                    "rollback copy {} is missing",
                    saved.display()
                )));
            }
        } else {
            remove_file_if_exists(&target)?;
        }
    }
    sync_directory(paths.root())?;
    intent.phase = RestorePhase::Completed;
    write_restore_intent(paths, intent)?;
    cleanup_completed_restore(paths, intent)
}

fn cleanup_completed_restore(
    paths: &DatabasePaths,
    intent: &RestoreIntent,
) -> Result<(), RecoveryError> {
    if intent.phase != RestorePhase::Completed {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "cannot clean up phase {:?}",
            intent.phase
        )));
    }
    let stage = restore_subdirectory(paths, &intent.stage_directory)?;
    match fs::remove_dir_all(&stage) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    remove_file_if_exists(&restore_intent_path(paths))?;
    remove_file_if_exists(&restore_intent_temp_path(paths))?;
    sync_directory(paths.root())?;
    Ok(())
}

fn inspect_restore_target(paths: &DatabasePaths) -> Result<Vec<RestoreFile>, RecoveryError> {
    let mut files = Vec::new();
    for restore_file in RestoreFile::ALL {
        let path = paths.root().join(restore_file.file_name());
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => files.push(restore_file),
            Ok(_) => return Err(RecoveryError::InvalidRestoreTarget),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let has_main = files.contains(&RestoreFile::Main);
    let has_media = files.contains(&RestoreFile::Media);
    if has_main != has_media || (!has_main && !files.is_empty()) {
        return Err(RecoveryError::InvalidRestoreTarget);
    }
    Ok(files)
}

fn restore_intent_path(paths: &DatabasePaths) -> PathBuf {
    paths.root().join(RESTORE_INTENT_FILE_NAME)
}

fn restore_intent_temp_path(paths: &DatabasePaths) -> PathBuf {
    paths.root().join(RESTORE_INTENT_TEMP_FILE_NAME)
}

fn write_restore_intent(
    paths: &DatabasePaths,
    intent: &RestoreIntent,
) -> Result<(), RecoveryError> {
    let temporary = restore_intent_temp_path(paths);
    remove_file_if_exists(&temporary)?;
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, intent)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    fs::rename(&temporary, restore_intent_path(paths))?;
    sync_directory(paths.root())?;
    Ok(())
}

fn load_restore_intent(paths: &DatabasePaths) -> Result<Option<RestoreIntent>, RecoveryError> {
    let path = restore_intent_path(paths);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(RecoveryError::InvalidRestoreJournal(
            "intent is not a regular file".into(),
        ));
    }
    let intent: RestoreIntent = serde_json::from_reader(BufReader::new(File::open(&path)?))?;
    if intent.format_version != RESTORE_INTENT_FORMAT_VERSION {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "unsupported format version {}",
            intent.format_version
        )));
    }
    validate_restore_directory_name(&intent.stage_directory, ".dara-restore-stage-")?;
    validate_restore_directory_name(&intent.rollback_directory, ".dara-restore-rollback-")?;
    restore_subdirectory(paths, &intent.stage_directory)?;
    restore_subdirectory(paths, &intent.rollback_directory)?;
    if intent.rollback_files.iter().any(|candidate| {
        intent
            .rollback_files
            .iter()
            .filter(|value| *value == candidate)
            .count()
            > 1
    }) {
        return Err(RecoveryError::InvalidRestoreJournal(
            "rollback file list contains duplicates".into(),
        ));
    }
    let has_main = intent.rollback_files.contains(&RestoreFile::Main);
    let has_media = intent.rollback_files.contains(&RestoreFile::Media);
    if has_main != has_media || (!has_main && !intent.rollback_files.is_empty()) {
        return Err(RecoveryError::InvalidRestoreJournal(
            "rollback file list is not a complete database pair".into(),
        ));
    }
    Ok(Some(intent))
}

fn validate_restore_directory_name(component: &str, prefix: &str) -> Result<(), RecoveryError> {
    let identifier = component
        .strip_prefix(prefix)
        .ok_or_else(|| {
            RecoveryError::InvalidRestoreJournal(format!(
                "restore directory {component:?} does not start with {prefix:?}"
            ))
        })?
        .parse::<Uuid>()
        .map_err(|_| {
            RecoveryError::InvalidRestoreJournal(format!(
                "restore directory {component:?} has an invalid identifier"
            ))
        })?;
    if identifier.get_version_num() != 7 {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "restore directory {component:?} does not use a version 7 identifier"
        )));
    }
    Ok(())
}

fn restore_subdirectory(paths: &DatabasePaths, component: &str) -> Result<PathBuf, RecoveryError> {
    let mut components = Path::new(component).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "unsafe restore directory {component:?}"
        )));
    }
    Ok(paths.root().join(component))
}

fn copy_and_sync(source: &Path, destination: &Path) -> Result<(), RecoveryError> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() {
        return Err(RecoveryError::InvalidRestoreJournal(format!(
            "{} is not a regular file",
            source.display()
        )));
    }
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    Ok(())
}

fn move_once(source: &Path, destination: &Path, operation: &str) -> Result<(), RecoveryError> {
    match (source.exists(), destination.exists()) {
        (true, false) => fs::rename(source, destination).map_err(RecoveryError::from),
        (false, true) => Ok(()),
        (true, true) => Err(RecoveryError::InvalidRestoreJournal(format!(
            "{operation}: both {} and {} exist",
            source.display(),
            destination.display()
        ))),
        (false, false) => Err(RecoveryError::InvalidRestoreJournal(format!(
            "{operation}: neither {} nor {} exists",
            source.display(),
            destination.display()
        ))),
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), RecoveryError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn sync_directory(path: &Path) -> Result<(), RecoveryError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn interrupt_if(
    point: RestoreFailpoint,
    failpoint: Option<RestoreFailpoint>,
) -> Result<(), RecoveryError> {
    #[cfg(test)]
    if failpoint == Some(point) {
        return Err(RecoveryError::InjectedInterruption);
    }
    #[cfg(not(test))]
    let _ = (point, failpoint);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::database::{
        snapshot::SnapshotManifest, CanonicalImage, CardContentDraft, InitializationOptions,
        SearchCardContentInput, SetZoomPercentInput,
    };

    const TEST_MEDIA_LEASE_ID: &str = "01980c8e-6c00-7000-8000-000000000901";

    fn snapshot_fixture() -> (tempfile::TempDir, DatabasePaths, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path());
        drop(
            database::initialize(
                paths.clone(),
                "test",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("database"),
        );
        let created = snapshot::create_snapshot_pair(&paths, "test").expect("snapshot");
        (directory, paths, created.manifest_path)
    }

    #[test]
    fn parser_leaves_normal_application_arguments_untouched() {
        assert_eq!(
            parse_entrypoint(["dara", "--some-tauri-argument"].map(OsString::from))
                .expect("application entrypoint"),
            Entrypoint::Application
        );
    }

    #[test]
    fn parser_accepts_only_named_recovery_operations() {
        assert_eq!(
            parse_entrypoint(["dara", "recovery", "list", "/tmp/dara"].map(OsString::from))
                .expect("list command"),
            Entrypoint::Recovery(RecoveryCommand::List {
                data_directory: PathBuf::from("/tmp/dara")
            })
        );
        assert!(matches!(
            parse_entrypoint(["dara", "recovery", "delete", "/tmp/dara"].map(OsString::from)),
            Err(RecoveryError::Usage(_))
        ));
        assert_eq!(
            parse_entrypoint(
                [
                    "dara",
                    "recovery",
                    "restore",
                    "/tmp/backups/snapshot.json",
                    "/tmp/dara"
                ]
                .map(OsString::from)
            )
            .expect("restore command"),
            Entrypoint::Recovery(RecoveryCommand::Restore {
                manifest: PathBuf::from("/tmp/backups/snapshot.json"),
                data_directory: PathBuf::from("/tmp/dara"),
            })
        );
        assert!(matches!(
            parse_entrypoint(
                ["dara", "recovery", "restore", "/tmp/snapshot.json"].map(OsString::from)
            ),
            Err(RecoveryError::Usage(_))
        ));
    }

    #[test]
    fn list_reports_only_fully_valid_snapshots() {
        let (_directory, paths, manifest_path) = snapshot_fixture();
        fs::write(paths.backups.join("invalid.json"), b"not JSON").expect("invalid manifest");
        let missing_pair = paths.backups.join("missing.json");
        let mut manifest: SnapshotManifest =
            serde_json::from_slice(&fs::read(&manifest_path).expect("snapshot manifest"))
                .expect("snapshot JSON");
        manifest.main.file_name = "missing-main.sqlite3".into();
        fs::write(
            &missing_pair,
            serde_json::to_vec_pretty(&manifest).expect("missing pair JSON"),
        )
        .expect("missing pair manifest");

        let reports = list_snapshots(&paths).expect("snapshot list");

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].manifest, manifest_path);
    }

    #[test]
    fn list_rejects_a_missing_data_directory_without_creating_it() {
        let parent = tempfile::tempdir().expect("parent directory");
        let missing = parent.path().join("mistyped-data-directory");

        assert!(matches!(
            run_from_args([
                OsString::from("dara"),
                OsString::from("recovery"),
                OsString::from("list"),
                missing.clone().into_os_string(),
            ]),
            Err(RecoveryError::InvalidDataDirectory(path)) if path == missing
        ));
        assert!(!missing.exists());
    }

    #[test]
    fn verify_reports_the_validated_pair() {
        let (_directory, _paths, manifest_path) = snapshot_fixture();

        let output = run_from_args([
            OsString::from("dara"),
            OsString::from("recovery"),
            OsString::from("verify"),
            manifest_path.into_os_string(),
        ])
        .expect("verify command")
        .expect("recovery output");
        let output: serde_json::Value = serde_json::from_str(&output).expect("output JSON");

        assert_eq!(output["valid"], true);
        assert_eq!(output["snapshot"]["applicationVersion"], "test");
    }

    #[test]
    fn recovery_commands_refuse_a_live_data_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let _lock = AppDataLock::acquire(directory.path()).expect("application lock");

        assert!(matches!(
            run_from_args(
                [
                    "dara",
                    "recovery",
                    "list",
                    directory.path().to_str().unwrap()
                ]
                .map(OsString::from)
            ),
            Err(RecoveryError::Lock(AppDataLockError::AlreadyLocked(_)))
        ));
    }

    #[test]
    fn restore_refuses_a_live_target_data_directory() {
        let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
        let target_directory = tempfile::tempdir().expect("target directory");
        let _lock = AppDataLock::acquire(target_directory.path()).expect("application lock");

        assert!(matches!(
            restore_snapshot(&manifest_path, target_directory.path()),
            Err(RecoveryError::Lock(AppDataLockError::AlreadyLocked(_)))
        ));
    }

    #[test]
    fn tampered_source_fails_before_mutating_the_target() {
        let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
        let source = snapshot::load_and_validate_snapshot(&manifest_path).expect("source");
        let target_directory = tempfile::tempdir().expect("target directory");
        let target_paths = DatabasePaths::new(target_directory.path());
        drop(
            database::initialize(
                target_paths.clone(),
                "target",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("target database"),
        );
        let main_before = fs::read(&target_paths.main).expect("target main before");
        let media_before = fs::read(&target_paths.media).expect("target media before");
        OpenOptions::new()
            .append(true)
            .open(source.main_path)
            .expect("tampered source")
            .write_all(b"tampered")
            .expect("tamper bytes");

        assert!(matches!(
            restore_snapshot(&manifest_path, target_directory.path()),
            Err(RecoveryError::Database(
                database::DatabaseError::InvalidSnapshot(_)
            ))
        ));
        assert_eq!(
            fs::read(&target_paths.main).expect("target main after"),
            main_before
        );
        assert_eq!(
            fs::read(&target_paths.media).expect("target media after"),
            media_before
        );
        assert!(!restore_intent_path(&target_paths).exists());
    }

    #[test]
    fn interrupted_restore_converges_at_every_pair_transition() {
        let failpoints = [
            RestoreFailpoint::IntentWritten,
            RestoreFailpoint::InstallingRecorded,
            RestoreFailpoint::RollbackMain,
            RestoreFailpoint::RollbackMedia,
            RestoreFailpoint::RollbackCompanions,
            RestoreFailpoint::MediaInstalled,
            RestoreFailpoint::MainInstalled,
            RestoreFailpoint::InstalledValidated,
        ];
        for failpoint in failpoints {
            let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
            let source = snapshot::load_and_validate_snapshot(&manifest_path).expect("source");
            let target_directory = tempfile::tempdir().expect("target directory");
            let target_paths = DatabasePaths::new(target_directory.path());
            drop(
                database::initialize(
                    target_paths.clone(),
                    "target",
                    InitializationOptions {
                        launch_snapshot: false,
                    },
                )
                .expect("target database"),
            );

            assert!(matches!(
                prepare_restore(&target_paths, source.clone(), Some(failpoint)),
                Err(RecoveryError::InjectedInterruption)
            ));
            recover_interrupted_restore(&target_paths).expect("resumed restore");
            snapshot::validate_snapshot_pair_files(
                &source.manifest,
                &target_paths.main,
                &target_paths.media,
            )
            .expect("restored pair");
            let intent = load_restore_intent(&target_paths)
                .expect("restore intent")
                .expect("pending confirmation");
            assert_eq!(intent.phase, RestorePhase::InstalledValidated);
            let rollback = restore_subdirectory(&target_paths, &intent.rollback_directory)
                .expect("rollback directory");

            confirm_restored_launch(&target_paths).expect("restore confirmation");
            assert!(!restore_intent_path(&target_paths).exists());
            assert!(rollback.is_dir(), "rollback retained for {failpoint:?}");
        }
    }

    #[test]
    fn corrupt_staged_restore_rolls_back_without_mutating_the_target() {
        let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
        let source = snapshot::load_and_validate_snapshot(&manifest_path).expect("source");
        let target_directory = tempfile::tempdir().expect("target directory");
        let target_paths = DatabasePaths::new(target_directory.path());
        drop(
            database::initialize(
                target_paths.clone(),
                "target",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("target database"),
        );
        let main_before = fs::read(&target_paths.main).expect("target main before");
        let media_before = fs::read(&target_paths.media).expect("target media before");

        assert!(matches!(
            prepare_restore(&target_paths, source, Some(RestoreFailpoint::IntentWritten)),
            Err(RecoveryError::InjectedInterruption)
        ));
        let intent = load_restore_intent(&target_paths)
            .expect("restore intent")
            .expect("pending restore");
        snapshot::load_and_validate_snapshot(
            intent
                .safety_snapshot
                .as_deref()
                .expect("pre-restore safety snapshot"),
        )
        .expect("validated safety snapshot");
        let stage = restore_subdirectory(&target_paths, &intent.stage_directory).expect("stage");
        OpenOptions::new()
            .append(true)
            .open(stage.join(RestoreFile::Main.file_name()))
            .expect("staged main")
            .write_all(b"corrupt")
            .expect("corrupt staged main");

        assert!(matches!(
            recover_interrupted_restore(&target_paths),
            Err(RecoveryError::Database(
                database::DatabaseError::InvalidSnapshot(_)
            ))
        ));
        assert_eq!(
            fs::read(&target_paths.main).expect("target main after"),
            main_before
        );
        assert_eq!(
            fs::read(&target_paths.media).expect("target media after"),
            media_before
        );
        assert!(!restore_intent_path(&target_paths).exists());
    }

    #[test]
    fn corrupt_partially_installed_restore_recovers_the_original_pair() {
        let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
        let source = snapshot::load_and_validate_snapshot(&manifest_path).expect("source");
        let target_directory = tempfile::tempdir().expect("target directory");
        let target_paths = DatabasePaths::new(target_directory.path());
        drop(
            database::initialize(
                target_paths.clone(),
                "target",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("target database"),
        );
        let main_before = fs::read(&target_paths.main).expect("target main before");
        let media_before = fs::read(&target_paths.media).expect("target media before");

        assert!(matches!(
            prepare_restore(&target_paths, source, Some(RestoreFailpoint::MainInstalled)),
            Err(RecoveryError::InjectedInterruption)
        ));
        OpenOptions::new()
            .append(true)
            .open(&target_paths.main)
            .expect("installed main")
            .write_all(b"corrupt")
            .expect("corrupt installed main");

        assert!(matches!(
            recover_interrupted_restore(&target_paths),
            Err(RecoveryError::Database(
                database::DatabaseError::InvalidSnapshot(_)
            ))
        ));
        assert_eq!(
            fs::read(&target_paths.main).expect("target main after"),
            main_before
        );
        assert_eq!(
            fs::read(&target_paths.media).expect("target media after"),
            media_before
        );
        assert!(!restore_intent_path(&target_paths).exists());
    }

    #[test]
    fn restore_moves_database_companions_out_of_the_live_pair() {
        let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
        let source = snapshot::load_and_validate_snapshot(&manifest_path).expect("source");
        let target_directory = tempfile::tempdir().expect("target directory");
        let target_paths = DatabasePaths::new(target_directory.path());
        drop(
            database::initialize(
                target_paths.clone(),
                "target",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("target database"),
        );
        for restore_file in [
            RestoreFile::MainWal,
            RestoreFile::MainShm,
            RestoreFile::MediaWal,
            RestoreFile::MediaShm,
        ] {
            fs::write(
                target_paths.root().join(restore_file.file_name()),
                restore_file.file_name().as_bytes(),
            )
            .expect("database companion");
        }

        let identifier = Uuid::now_v7();
        let stage_directory = format!(".dara-restore-stage-{identifier}");
        let rollback_directory = format!(".dara-restore-rollback-{identifier}");
        let stage = target_paths.root().join(&stage_directory);
        let rollback = target_paths.root().join(&rollback_directory);
        fs::create_dir(&stage).expect("stage");
        fs::create_dir(&rollback).expect("rollback");
        copy_and_sync(
            &source.main_path,
            &stage.join(RestoreFile::Main.file_name()),
        )
        .expect("stage main");
        copy_and_sync(
            &source.media_path,
            &stage.join(RestoreFile::Media.file_name()),
        )
        .expect("stage media");
        let intent = RestoreIntent {
            format_version: RESTORE_INTENT_FORMAT_VERSION,
            phase: RestorePhase::Staged,
            source_manifest: source.manifest_path.clone(),
            expected: source.manifest.clone(),
            stage_directory,
            rollback_directory,
            rollback_files: inspect_restore_target(&target_paths).expect("target files"),
            safety_snapshot: None,
        };
        write_restore_intent(&target_paths, &intent).expect("restore intent");

        recover_interrupted_restore(&target_paths).expect("restore");

        for restore_file in [
            RestoreFile::MainWal,
            RestoreFile::MainShm,
            RestoreFile::MediaWal,
            RestoreFile::MediaShm,
        ] {
            assert!(!target_paths.root().join(restore_file.file_name()).exists());
            assert!(rollback.join(restore_file.file_name()).is_file());
        }
        snapshot::validate_snapshot_pair_files(
            &source.manifest,
            &target_paths.main,
            &target_paths.media,
        )
        .expect("restored pair");
    }

    #[test]
    fn restore_journal_rejects_directory_traversal() {
        let (_source_directory, _source_paths, manifest_path) = snapshot_fixture();
        let source = snapshot::load_and_validate_snapshot(&manifest_path).expect("source");
        let target_directory = tempfile::tempdir().expect("target directory");
        let target_paths = DatabasePaths::new(target_directory.path());
        let intent = RestoreIntent {
            format_version: RESTORE_INTENT_FORMAT_VERSION,
            phase: RestorePhase::Staged,
            source_manifest: source.manifest_path,
            expected: source.manifest,
            stage_directory: "../outside".into(),
            rollback_directory: ".dara-restore-rollback-test".into(),
            rollback_files: Vec::new(),
            safety_snapshot: None,
        };
        write_restore_intent(&target_paths, &intent).expect("restore intent");

        assert!(matches!(
            recover_interrupted_restore(&target_paths),
            Err(RecoveryError::InvalidRestoreJournal(_))
        ));
    }

    #[test]
    fn restore_preserves_cards_settings_and_media_from_the_snapshot() {
        let directory = tempfile::tempdir().expect("data directory");
        let paths = DatabasePaths::new(directory.path());
        let (snapshot_path, image_id) = {
            let database = database::initialize(
                paths.clone(),
                "test",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("database");
            let client = database.client();
            client
                .create_card_content(
                    CardContentDraft::Basic {
                        front_md: "snapshot question".into(),
                        back_md: "snapshot answer".into(),
                        source: Some("snapshot source".into()),
                    },
                    TEST_MEDIA_LEASE_ID.into(),
                )
                .expect("snapshot card");
            let settings = client.load_settings().expect("settings");
            client
                .set_zoom_percent(SetZoomPercentInput {
                    expected_revision: settings.revision,
                    zoom_percent: 110,
                })
                .expect("snapshot zoom");
            let image = client
                .ingest_image(
                    CanonicalImage {
                        bytes: b"snapshot image bytes".to_vec(),
                        natural_width: 23,
                        natural_height: 17,
                    },
                    TEST_MEDIA_LEASE_ID.into(),
                )
                .expect("snapshot image");
            drop(client);
            drop(database);
            (
                snapshot::create_snapshot_pair(&paths, "test")
                    .expect("snapshot")
                    .manifest_path,
                image.id,
            )
        };
        {
            let database = database::initialize(
                paths.clone(),
                "test",
                InitializationOptions {
                    launch_snapshot: false,
                },
            )
            .expect("database");
            let client = database.client();
            client
                .create_card_content(
                    CardContentDraft::Basic {
                        front_md: "post-snapshot question".into(),
                        back_md: "post-snapshot answer".into(),
                        source: None,
                    },
                    TEST_MEDIA_LEASE_ID.into(),
                )
                .expect("later card");
            let settings = client.load_settings().expect("settings");
            client
                .set_zoom_percent(SetZoomPercentInput {
                    expected_revision: settings.revision,
                    zoom_percent: 130,
                })
                .expect("later zoom");
        }

        let output = run_from_args([
            OsString::from("dara"),
            OsString::from("recovery"),
            OsString::from("restore"),
            snapshot_path.into_os_string(),
            directory.path().as_os_str().to_owned(),
        ])
        .expect("offline restore")
        .expect("restore report");
        let output: serde_json::Value = serde_json::from_str(&output).expect("restore JSON");
        assert_eq!(output["awaitingApplicationValidation"], true);
        let restored = database::initialize(
            paths.clone(),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("restored database");
        confirm_restored_launch(&paths).expect("restore confirmation");
        let client = restored.client();
        let cards = client
            .search_card_content(SearchCardContentInput {
                query: String::new(),
                limit: 50,
                offset: 0,
            })
            .expect("restored cards");
        assert_eq!(cards.len(), 1);
        assert_eq!(
            client
                .load_settings()
                .expect("restored settings")
                .zoom_percent,
            110
        );
        assert_eq!(
            client
                .load_media_payload(image_id)
                .expect("restored media")
                .bytes,
            b"snapshot image bytes"
        );
    }

    #[test]
    fn verify_rejects_unsafe_snapshot_file_names() {
        let (_directory, paths, manifest_path) = snapshot_fixture();
        let mut manifest: SnapshotManifest =
            serde_json::from_slice(&fs::read(&manifest_path).expect("snapshot manifest"))
                .expect("snapshot JSON");
        manifest.main.file_name = "../outside.sqlite3".into();
        let unsafe_manifest = paths.backups.join("unsafe.json");
        let mut file = fs::File::create(&unsafe_manifest).expect("unsafe manifest");
        serde_json::to_writer_pretty(&mut file, &manifest).expect("unsafe manifest JSON");
        file.write_all(b"\n").expect("manifest newline");

        assert!(matches!(
            run_from_args([
                OsString::from("dara"),
                OsString::from("recovery"),
                OsString::from("verify"),
                unsafe_manifest.into_os_string(),
            ],),
            Err(RecoveryError::Database(
                database::DatabaseError::InvalidSnapshot(_)
            ))
        ));
    }
}
