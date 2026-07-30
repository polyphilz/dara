use std::{
    fs::{self, OpenOptions},
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroize;

use crate::{
    app_lock::AppDataLock,
    backup::{
        credentials::{
            CredentialError, CredentialStore, MacOsKeychainCredentialStore, R2Credentials,
        },
        domain::{
            BackupErrorCode, BackupSetId, CheckpointId, R2AccountId, R2BucketName, R2Jurisdiction,
            R2Prefix, R2Target,
        },
        restore::{RemoteCheckpointCatalog, RemoteCheckpointSelector, RemoteRecoveryEngine},
    },
    database::{
        self,
        connection::{self, FileState},
        DatabasePaths, InitializationOptions,
    },
};

#[cfg(feature = "e2e")]
const E2E_START_FRESH_ENV: &str = "DARA_E2E_START_FRESH";
const SHOW_MAIN_AFTER_RESTART_FILE_NAME: &str = ".show-main-after-restart";

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
    Io(#[from] io::Error),

    #[error("could not inspect the Dara databases: {0}")]
    Database(#[from] database::DatabaseError),
}

pub(crate) fn inspect_database_pair(
    paths: &DatabasePaths,
) -> Result<DatabasePairState, RecoveryStartupError> {
    let main_state = connection::inspect_file(&paths.main)?;
    let media_state = connection::inspect_file(&paths.media)?;
    match (main_state, media_state) {
        (FileState::Fresh, FileState::Fresh) => Ok(DatabasePairState::Fresh),
        (FileState::Existing, FileState::Existing) => Ok(DatabasePairState::Existing),
        _ => Err(RecoveryStartupError::IncompleteDatabasePair),
    }
}

fn request_show_main_after_restart(paths: &DatabasePaths) -> Result<(), io::Error> {
    let marker_path = paths.root().join(SHOW_MAIN_AFTER_RESTART_FILE_NAME);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&marker_path)?;
            if metadata.file_type().is_file() {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn take_show_main_after_restart_request(
    paths: &DatabasePaths,
) -> Result<bool, io::Error> {
    let marker_path = paths.root().join(SHOW_MAIN_AFTER_RESTART_FILE_NAME);
    match fs::remove_file(marker_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
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
    session: Mutex<Option<RecoverySession>>,
}

struct RecoverySession {
    target: R2Target,
    credentials: R2Credentials,
    backup_set_id: BackupSetId,
}

struct RestoreSessionFailure {
    error: RecoveryCommandError,
    session: RecoverySession,
}

impl Default for FreshInstallRecoveryState {
    fn default() -> Self {
        Self {
            inner: Arc::new(RecoveryStateInner {
                operation_active: AtomicBool::new(false),
                session: Mutex::new(None),
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

    fn replace_session(&self, session: RecoverySession) {
        *self
            .inner
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(session);
    }

    fn clear_session(&self) {
        *self
            .inner
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn take_session(&self) -> Result<RecoverySession, RecoveryCommandError> {
        self.inner
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(RecoveryCommandError::discovery_required)
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
    jurisdiction: R2Jurisdiction,
    bucket: String,
    credentials: RecoveryCredentialsInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RestoreRemoteBackupInput {
    checkpoint_id: String,
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
    DiscoveryRequired,
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

    fn discovery_required() -> Self {
        Self {
            code: RecoveryCommandErrorCode::DiscoveryRequired,
            backup_error_code: None,
            message: "Find the available R2 backups again before restoring.".to_owned(),
        }
    }

    fn backup(error: BackupErrorCode) -> Self {
        Self {
            code: RecoveryCommandErrorCode::BackupFailed,
            backup_error_code: Some(error),
            message: "Dara could not safely use that R2 backup.".to_owned(),
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
    state: State<'_, FreshInstallRecoveryState>,
) -> Result<(), RecoveryCommandError> {
    if context.mode != ApplicationLaunchMode::Recovery {
        return Err(RecoveryCommandError::not_fresh_install());
    }
    let _operation = state.begin_operation()?;
    let paths = DatabasePaths::new(data_lock.data_root());
    if inspect_database_pair(&paths).map_err(|error| {
        RecoveryCommandError::internal(format!("Could not inspect Dara data: {error}"))
    })? != DatabasePairState::Fresh
    {
        return Err(RecoveryCommandError::not_fresh_install());
    }
    let database = database::initialize(
        paths.clone(),
        env!("CARGO_PKG_VERSION"),
        InitializationOptions {
            launch_snapshot: false,
        },
    )
    .map_err(|error| {
        RecoveryCommandError::internal(format!("Could not create Dara data: {error}"))
    })?;
    drop(database);
    request_show_main_after_restart(&paths).map_err(|error| {
        RecoveryCommandError::internal(format!(
            "Could not prepare Dara to show the new library: {error}"
        ))
    })?;
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
    state.clear_session();
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
        let session = RecoverySession {
            target,
            credentials,
            backup_set_id: catalog.backup_set_id.clone(),
        };
        Ok::<_, RecoveryCommandError>((catalog, session))
    })
    .await
    .map_err(|_| RecoveryCommandError::internal("The recovery task stopped unexpectedly."))?;
    drop(operation);
    let (catalog, session) = result?;
    state.replace_session(session);
    Ok(catalog)
}

#[tauri::command]
pub(crate) async fn restore_remote_backup(
    app: AppHandle,
    context: State<'_, ApplicationLaunchContext>,
    state: State<'_, FreshInstallRecoveryState>,
    data_lock: State<'_, AppDataLock>,
    input: RestoreRemoteBackupInput,
) -> Result<(), RecoveryCommandError> {
    if context.mode != ApplicationLaunchMode::Recovery {
        return Err(RecoveryCommandError::not_fresh_install());
    }
    let state = state.inner().clone();
    let operation = state.begin_operation()?;
    let paths = DatabasePaths::new(data_lock.data_root());
    let selector = CheckpointId::parse(input.checkpoint_id)
        .map(RemoteCheckpointSelector::Checkpoint)
        .map_err(|_| RecoveryCommandError::invalid_input())?;
    let resource_dir = app.path().resource_dir().map_err(|error| {
        RecoveryCommandError::internal(format!("Could not find Dara resources: {error}"))
    })?;
    let data_lock = data_lock.try_clone().map_err(|error| {
        RecoveryCommandError::internal(format!("Could not retain the Dara data lock: {error}"))
    })?;
    request_show_main_after_restart(&paths).map_err(|error| {
        RecoveryCommandError::internal(format!(
            "Could not prepare Dara to show the restored library: {error}"
        ))
    })?;
    let session = state.take_session()?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        restore_session(&data_lock, &resource_dir, &selector, session)
    })
    .await
    .map_err(|_| RecoveryCommandError::internal("The restore task stopped unexpectedly."))?;
    drop(operation);
    match result {
        Ok(()) => app.restart(),
        Err(failure) => {
            let RestoreSessionFailure { error, session } = *failure;
            state.replace_session(session);
            Err(error)
        }
    }
}

fn restore_session(
    data_lock: &AppDataLock,
    resource_dir: &std::path::Path,
    selector: &RemoteCheckpointSelector,
    session: RecoverySession,
) -> Result<(), Box<RestoreSessionFailure>> {
    let paths = DatabasePaths::new(data_lock.data_root());
    match inspect_database_pair(&paths) {
        Ok(DatabasePairState::Fresh) => {}
        Ok(DatabasePairState::Existing) => {
            return Err(Box::new(RestoreSessionFailure {
                error: RecoveryCommandError::not_fresh_install(),
                session,
            }));
        }
        Err(error) => {
            return Err(Box::new(RestoreSessionFailure {
                error: RecoveryCommandError::internal(format!(
                    "Could not inspect Dara data: {error}"
                )),
                session,
            }));
        }
    }
    let engine_credentials = match session.credentials.try_clone() {
        Ok(credentials) => credentials,
        Err(_) => {
            return Err(Box::new(RestoreSessionFailure {
                error: RecoveryCommandError::invalid_input(),
                session,
            }));
        }
    };
    let engine = match RemoteRecoveryEngine::system(
        session.target.clone(),
        engine_credentials,
        resource_dir,
    ) {
        Ok(engine) => engine,
        Err(error) => {
            return Err(Box::new(RestoreSessionFailure {
                error: RecoveryCommandError::backup(error),
                session,
            }));
        }
    };
    if let Err(error) = engine.restore_fresh_to_locked(data_lock, selector, &session.backup_set_id)
    {
        return Err(Box::new(RestoreSessionFailure {
            error: RecoveryCommandError::backup(error),
            session,
        }));
    }
    // Discovery credentials may be read-only. Preserve any canonical credentials
    // already saved for this backup set so an explicit takeover can still write.
    // A post-install Keychain failure is recoverable from Settings: restored
    // backups remain paused until the user explicitly takes ownership.
    if let Err(error) = save_restore_credentials_if_missing(
        &MacOsKeychainCredentialStore,
        &session.backup_set_id,
        &session.credentials,
    ) {
        log::warn!("restored Dara data but could not save its R2 credentials: {error}");
    }
    Ok(())
}

fn save_restore_credentials_if_missing(
    store: &impl CredentialStore,
    backup_set_id: &BackupSetId,
    credentials: &R2Credentials,
) -> Result<(), CredentialError> {
    match store.load(backup_set_id) {
        Ok(_) => Ok(()),
        Err(CredentialError::Missing) => store.save(backup_set_id, credentials),
        Err(error) => Err(error),
    }
}

fn parse_connection(
    mut input: DiscoverRemoteBackupsInput,
) -> Result<(R2Target, R2Credentials), RecoveryCommandError> {
    let account_id = R2AccountId::parse(std::mem::take(&mut input.account_id))
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
            jurisdiction: input.jurisdiction,
            bucket,
            prefix: R2Prefix::primary(),
        },
        credentials,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXISTING_ACCESS_KEY: &str = "11111111111111111111111111111111";
    const EXISTING_SECRET_KEY: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";
    const RECOVERY_ACCESS_KEY: &str = "22222222222222222222222222222222";
    const RECOVERY_SECRET_KEY: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";

    #[derive(Clone, Copy)]
    enum TestCredentialState {
        Existing,
        Missing,
        Unavailable,
    }

    struct TestCredentialStore {
        state: TestCredentialState,
        saved_access_keys: Mutex<Vec<String>>,
    }

    impl TestCredentialStore {
        fn new(state: TestCredentialState) -> Self {
            Self {
                state,
                saved_access_keys: Mutex::new(Vec::new()),
            }
        }
    }

    impl CredentialStore for TestCredentialStore {
        fn save(
            &self,
            _backup_set_id: &BackupSetId,
            credentials: &R2Credentials,
        ) -> Result<(), CredentialError> {
            self.saved_access_keys
                .lock()
                .expect("saved credentials")
                .push(credentials.access_key_id().to_owned());
            Ok(())
        }

        fn load(&self, _backup_set_id: &BackupSetId) -> Result<R2Credentials, CredentialError> {
            match self.state {
                TestCredentialState::Existing => {
                    R2Credentials::new(EXISTING_ACCESS_KEY, EXISTING_SECRET_KEY)
                }
                TestCredentialState::Missing => Err(CredentialError::Missing),
                TestCredentialState::Unavailable => Err(CredentialError::Unavailable),
            }
        }

        fn remove(&self, _backup_set_id: &BackupSetId) -> Result<(), CredentialError> {
            Ok(())
        }
    }

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
    fn zero_length_database_placeholders_are_fresh() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path());
        fs::write(&paths.main, []).expect("empty main placeholder");

        assert_eq!(
            inspect_database_pair(&paths).expect("pair state"),
            DatabasePairState::Fresh
        );

        fs::write(&paths.media, []).expect("empty media placeholder");
        assert_eq!(
            inspect_database_pair(&paths).expect("pair state"),
            DatabasePairState::Fresh
        );
    }

    #[test]
    fn show_main_after_restart_request_is_consumed_once() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path());

        assert!(!take_show_main_after_restart_request(&paths).expect("no request"));
        request_show_main_after_restart(&paths).expect("request main window");
        assert!(take_show_main_after_restart_request(&paths).expect("pending request"));
        assert!(!take_show_main_after_restart_request(&paths).expect("consumed request"));
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

    #[test]
    fn restore_preserves_existing_write_credentials() {
        let store = TestCredentialStore::new(TestCredentialState::Existing);
        let backup_set_id = BackupSetId::new();
        let recovery_credentials =
            R2Credentials::new(RECOVERY_ACCESS_KEY, RECOVERY_SECRET_KEY).expect("credentials");

        save_restore_credentials_if_missing(&store, &backup_set_id, &recovery_credentials)
            .expect("preserve credentials");

        assert!(store
            .saved_access_keys
            .lock()
            .expect("saved credentials")
            .is_empty());
    }

    #[test]
    fn restore_saves_credentials_when_none_exist() {
        let store = TestCredentialStore::new(TestCredentialState::Missing);
        let backup_set_id = BackupSetId::new();
        let recovery_credentials =
            R2Credentials::new(RECOVERY_ACCESS_KEY, RECOVERY_SECRET_KEY).expect("credentials");

        save_restore_credentials_if_missing(&store, &backup_set_id, &recovery_credentials)
            .expect("save credentials");

        assert_eq!(
            store
                .saved_access_keys
                .lock()
                .expect("saved credentials")
                .as_slice(),
            [RECOVERY_ACCESS_KEY]
        );
    }

    #[test]
    fn restore_does_not_overwrite_credentials_when_keychain_is_unavailable() {
        let store = TestCredentialStore::new(TestCredentialState::Unavailable);
        let backup_set_id = BackupSetId::new();
        let recovery_credentials =
            R2Credentials::new(RECOVERY_ACCESS_KEY, RECOVERY_SECRET_KEY).expect("credentials");

        assert!(matches!(
            save_restore_credentials_if_missing(&store, &backup_set_id, &recovery_credentials),
            Err(CredentialError::Unavailable)
        ));
        assert!(store
            .saved_access_keys
            .lock()
            .expect("saved credentials")
            .is_empty());
    }
}
