use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

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

#[derive(Clone, Debug, Eq, PartialEq)]
enum RecoveryCommand {
    List { data_directory: PathBuf },
    Verify { manifest: PathBuf },
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

pub fn run_from_args(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<String>, RecoveryError> {
    let command = match parse_entrypoint(arguments)? {
        Entrypoint::Application => return Ok(None),
        Entrypoint::Recovery(command) => command,
    };
    let output = match command {
        RecoveryCommand::List { data_directory } => {
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
    };
    Ok(Some(output))
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
    let path = arguments
        .next()
        .ok_or(RecoveryError::Usage(recovery_usage()))?;
    if arguments.next().is_some() {
        return Err(RecoveryError::Usage(recovery_usage()));
    }
    let command = if operation == OsStr::new(LIST_ARGUMENT) {
        RecoveryCommand::List {
            data_directory: path.into(),
        }
    } else if operation == OsStr::new(VERIFY_ARGUMENT) {
        RecoveryCommand::Verify {
            manifest: path.into(),
        }
    } else {
        return Err(RecoveryError::Usage(recovery_usage()));
    };
    Ok(Entrypoint::Recovery(command))
}

const fn recovery_usage() -> &'static str {
    "usage:\n  dara recovery list <data-directory>\n  dara recovery verify <manifest>"
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::database::{snapshot::SnapshotManifest, InitializationOptions};

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
