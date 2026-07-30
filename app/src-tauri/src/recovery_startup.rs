use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroize;

use crate::{
    app_lock::AppDataLock,
    backup::{
        credentials::R2Credentials,
        domain::{BackupErrorCode, R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix, R2Target},
        restore::{RemoteCheckpointCatalog, RemoteRecoveryEngine},
    },
    database::{self, DatabasePaths, InitializationOptions},
};

#[cfg(feature = "e2e")]
const E2E_START_FRESH_ENV: &str = "DARA_E2E_START_FRESH";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ApplicationLaunchMode {
    Normal,
    Recovery,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplicationLaunchContext {
    pub(crate) mode: ApplicationLaunchMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DatabasePairState {
    Fresh,
    Existing,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RecoveryStartupError {
    #[error("Dara found an incomplete database pair")]
    IncompleteDatabasePair,

    #[error("could not inspect the Dara data directory: {0}")]
    Io(#[from] std::io::Error),
}

pub(crate) fn inspect_database_pair(
    paths: &DatabasePaths,
) -> Result<DatabasePairState, RecoveryStartupError> {
    let main_exists = path_exists(&paths.main)?;
    let media_exists = path_exists(&paths.media)?;
    match (main_exists, media_exists) {
        (false, false) => Ok(DatabasePairState::Fresh),
        (true, true) => Ok(DatabasePairState::Existing),
        _ => Err(RecoveryStartupError::IncompleteDatabasePair),
    }
}

fn path_exists(path: &std::path::Path) -> Result<bool, std::io::Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(crate) fn launch_context(pair: DatabasePairState) -> ApplicationLaunchContext {
    ApplicationLaunchContext {
        mode: match pair {
            DatabasePairState::Fresh => ApplicationLaunchMode::Recovery,
            DatabasePairState::Existing => ApplicationLaunchMode::Normal,
        },
    }
}

#[cfg(feature = "e2e")]
pub(crate) fn e2e_start_fresh_requested() -> bool {
    std::env::var(E2E_START_FRESH_ENV).as_deref() == Ok("1")
}

#[derive(Clone)]
pub(crate) struct FreshInstallRecoveryState {
    inner: Arc<RecoveryStateInner>,
}

struct RecoveryStateInner {
    operation_active: AtomicBool,
}

impl Default for FreshInstallRecoveryState {
    fn default() -> Self {
        Self {
            inner: Arc::new(RecoveryStateInner {
                operation_active: AtomicBool::new(false),
            }),
        }
    }
}

impl FreshInstallRecoveryState {
    fn begin_operation(&self) -> Result<RecoveryOperationGuard, RecoveryCommandError> {
        if self.inner.operation_active.swap(true, Ordering::AcqRel) {
            return Err(RecoveryCommandError::operation_in_progress());
        }
        Ok(RecoveryOperationGuard {
            state: self.clone(),
        })
    }
}

struct RecoveryOperationGuard {
    state: FreshInstallRecoveryState,
}

impl Drop for RecoveryOperationGuard {
    fn drop(&mut self) {
        self.state
            .inner
            .operation_active
            .store(false, Ordering::Release);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DiscoverRemoteBackupsInput {
    account_id: String,
    jurisdiction: String,
    bucket: String,
    credentials: RecoveryCredentialsInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryCredentialsInput {
    access_key_id: String,
    secret_access_key: String,
}

impl Drop for RecoveryCredentialsInput {
    fn drop(&mut self) {
        self.access_key_id.zeroize();
        self.secret_access_key.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RecoveryCommandErrorCode {
    InvalidInput,
    NotFreshInstall,
    OperationInProgress,
    BackupFailed,
    Internal,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryCommandError {
    code: RecoveryCommandErrorCode,
    backup_error_code: Option<BackupErrorCode>,
    message: String,
}

impl RecoveryCommandError {
    fn invalid_input() -> Self {
        Self {
            code: RecoveryCommandErrorCode::InvalidInput,
            backup_error_code: None,
            message: "Check the R2 connection details and try again.".to_owned(),
        }
    }

    fn not_fresh_install() -> Self {
        Self {
            code: RecoveryCommandErrorCode::NotFreshInstall,
            backup_error_code: None,
            message: "Dara data already exists in this location.".to_owned(),
        }
    }

    fn operation_in_progress() -> Self {
        Self {
            code: RecoveryCommandErrorCode::OperationInProgress,
            backup_error_code: None,
            message: "A recovery task is already running.".to_owned(),
        }
    }

    fn backup(error: BackupErrorCode) -> Self {
        Self {
            code: RecoveryCommandErrorCode::BackupFailed,
            backup_error_code: Some(error),
            message: "Dara could not inspect that R2 backup.".to_owned(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: RecoveryCommandErrorCode::Internal,
            backup_error_code: None,
            message: message.into(),
        }
    }
}

#[tauri::command]
pub(crate) fn load_application_launch_context(
    context: State<'_, ApplicationLaunchContext>,
) -> ApplicationLaunchContext {
    *context
}

#[tauri::command]
pub(crate) fn start_fresh_install(
    app: AppHandle,
    context: State<'_, ApplicationLaunchContext>,
    data_lock: State<'_, AppDataLock>,
) -> Result<(), RecoveryCommandError> {
    if context.mode != ApplicationLaunchMode::Recovery {
        return Err(RecoveryCommandError::not_fresh_install());
    }
    let paths = DatabasePaths::new(data_lock.data_root());
    if inspect_database_pair(&paths).map_err(|error| {
        RecoveryCommandError::internal(format!("Could not inspect Dara data: {error}"))
    })? != DatabasePairState::Fresh
    {
        return Err(RecoveryCommandError::not_fresh_install());
    }
    let database = database::initialize(
        paths,
        env!("CARGO_PKG_VERSION"),
        InitializationOptions {
            launch_snapshot: false,
        },
    )
    .map_err(|error| {
        RecoveryCommandError::internal(format!("Could not create Dara data: {error}"))
    })?;
    drop(database);
    app.restart()
}

#[tauri::command]
pub(crate) async fn discover_remote_backups(
    app: AppHandle,
    context: State<'_, ApplicationLaunchContext>,
    state: State<'_, FreshInstallRecoveryState>,
    input: DiscoverRemoteBackupsInput,
) -> Result<RemoteCheckpointCatalog, RecoveryCommandError> {
    if context.mode != ApplicationLaunchMode::Recovery {
        return Err(RecoveryCommandError::not_fresh_install());
    }
    let state = state.inner().clone();
    let operation = state.begin_operation()?;
    let resource_dir = app.path().resource_dir().map_err(|error| {
        RecoveryCommandError::internal(format!("Could not find Dara resources: {error}"))
    })?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (target, credentials) = parse_connection(input)?;
        let engine = RemoteRecoveryEngine::system(
            target.clone(),
            credentials
                .try_clone()
                .map_err(|_| RecoveryCommandError::invalid_input())?,
            &resource_dir,
        )
        .map_err(RecoveryCommandError::backup)?;
        let catalog = engine
            .list_checkpoints()
            .map_err(RecoveryCommandError::backup)?;
        Ok::<_, RecoveryCommandError>(catalog)
    })
    .await
    .map_err(|_| RecoveryCommandError::internal("The recovery task stopped unexpectedly."))?;
    drop(operation);
    result
}

fn parse_connection(
    mut input: DiscoverRemoteBackupsInput,
) -> Result<(R2Target, R2Credentials), RecoveryCommandError> {
    let account_id = R2AccountId::parse(std::mem::take(&mut input.account_id))
        .map_err(|_| RecoveryCommandError::invalid_input())?;
    let jurisdiction = R2Jurisdiction::from_db(&input.jurisdiction)
        .map_err(|_| RecoveryCommandError::invalid_input())?;
    let bucket = R2BucketName::parse(std::mem::take(&mut input.bucket))
        .map_err(|_| RecoveryCommandError::invalid_input())?;
    let credentials = R2Credentials::new(
        std::mem::take(&mut input.credentials.access_key_id),
        std::mem::take(&mut input.credentials.secret_access_key),
    )
    .map_err(|_| RecoveryCommandError::invalid_input())?;
    Ok((
        R2Target {
            account_id,
            jurisdiction,
            bucket,
            prefix: R2Prefix::primary(),
        },
        credentials,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_directory_enters_recovery_without_creating_databases() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path());

        assert_eq!(
            inspect_database_pair(&paths).expect("pair state"),
            DatabasePairState::Fresh
        );
        assert_eq!(
            launch_context(DatabasePairState::Fresh).mode,
            ApplicationLaunchMode::Recovery
        );
        assert!(!paths.main.exists());
        assert!(!paths.media.exists());
    }

    #[test]
    fn complete_pair_enters_the_normal_application() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path());
        fs::write(&paths.main, b"main").expect("main");
        fs::write(&paths.media, b"media").expect("media");

        assert_eq!(
            inspect_database_pair(&paths).expect("pair state"),
            DatabasePairState::Existing
        );
        assert_eq!(
            launch_context(DatabasePairState::Existing).mode,
            ApplicationLaunchMode::Normal
        );
    }

    #[test]
    fn incomplete_pair_fails_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path());
        fs::write(&paths.main, b"main").expect("main");

        assert!(matches!(
            inspect_database_pair(&paths),
            Err(RecoveryStartupError::IncompleteDatabasePair)
        ));
    }

    #[test]
    fn only_one_recovery_operation_can_run_at_a_time() {
        let state = FreshInstallRecoveryState::default();
        let operation = state.begin_operation().expect("first operation");

        assert!(matches!(
            state.begin_operation(),
            Err(RecoveryCommandError {
                code: RecoveryCommandErrorCode::OperationInProgress,
                ..
            })
        ));

        drop(operation);
        state.begin_operation().expect("operation after release");
    }
}
