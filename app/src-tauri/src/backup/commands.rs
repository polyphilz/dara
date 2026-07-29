use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
use zeroize::Zeroize;

use super::{
    checkpoint::{CheckpointBackupStatus, CheckpointCoordinator},
    credentials::{CredentialError, CredentialStore, MacOsKeychainCredentialStore, R2Credentials},
    domain::{
        BackupErrorCode, BackupSetId, CheckpointBackupPhase, IdentityManifestV1, InstallationId,
        MediaBackupPhase, OwnerManifestV1, R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix,
        R2Target, RelationalBackupPhase, ReplicaEpochId,
    },
    installation::InstallationIdentityStore,
    litestream::VerifiedLitestreamBinary,
    litestream_runtime::{LitestreamRuntimeService, RelationalBackupStatus},
    media_reconciliation::{MediaBackupCoordinator, MediaBackupStatus},
    object_store::{ObjectStore, ObjectStoreErrorCode, R2ObjectStore},
    probe::{
        verify_connection_with_progress, ConnectionProbeError, LitestreamRelationalProbe,
        ProbeCleanupStatus, ProbeStage, RelationalProbeErrorCode,
    },
    remote_authority::{
        create_or_validate_backup_authority, map_credential_error, map_store_error,
        take_over_backup_authority, validate_backup_identity,
    },
    restore::{
        load_restore_drill_report, restore_drill_report_updated_at, RemoteCheckpointSelector,
        RemoteRecoveryEngine, RestoreDrillReport,
    },
};
use crate::database::{
    Database, DatabaseClient, DatabaseError, OffsiteBackupConfig, SaveOffsiteBackupConfigInput,
};

pub(crate) const OFFSITE_BACKUP_PROGRESS_EVENT: &str = "offsite-backup-progress";
const RESTORE_DRILL_DIRECTORY: &str = "offsite-restore-drills";

#[derive(Default)]
pub(crate) struct OffsiteBackupOperationRegistry {
    state: Mutex<OperationRegistryState>,
}

#[derive(Default)]
struct OperationRegistryState {
    active: Option<ActiveOperation>,
    takeover_available: bool,
}

impl OffsiteBackupOperationRegistry {
    fn begin(&self, kind: OffsiteBackupOperationKind) -> OperationStart {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(operation) = state.active.as_ref() {
            return OperationStart {
                operation_id: operation.operation_id.clone(),
                operation: operation.kind,
                reused: true,
            };
        }
        let operation_id = Uuid::now_v7().to_string();
        state.active = Some(ActiveOperation {
            operation_id: operation_id.clone(),
            kind,
        });
        OperationStart {
            operation_id,
            operation: kind,
            reused: false,
        }
    }

    fn finish(&self, operation_id: &str, result: Result<(), BackupErrorCode>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(operation) = state
            .active
            .as_ref()
            .filter(|operation| operation.operation_id == operation_id)
            .cloned()
        else {
            return;
        };
        state.active = None;
        match (operation.kind, result) {
            (OffsiteBackupOperationKind::TestAndEnable, Err(BackupErrorCode::OwnerMismatch)) => {
                state.takeover_available = true
            }
            (
                OffsiteBackupOperationKind::TestAndEnable
                | OffsiteBackupOperationKind::ChangeTarget
                | OffsiteBackupOperationKind::TakeOver
                | OffsiteBackupOperationKind::RemoveCredentials,
                Ok(()),
            ) => state.takeover_available = false,
            _ => {}
        }
    }

    fn active(&self) -> Option<ActiveOperation> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active
            .clone()
    }

    fn takeover_available(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .takeover_available
    }
}

#[derive(Clone)]
struct ActiveOperation {
    operation_id: String,
    kind: OffsiteBackupOperationKind,
}

struct OperationStart {
    operation_id: String,
    operation: OffsiteBackupOperationKind,
    reused: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum OffsiteBackupOperationKind {
    TestAndEnable,
    ReplaceCredentials,
    ChangeTarget,
    TakeOver,
    Disable,
    RemoveCredentials,
    BackupNow,
    RestoreDrill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum OffsiteBackupProgressPhase {
    ValidatingConfig,
    TestingObjectStore,
    TestingLitestream,
    SavingConfiguration,
    ReconcilingMedia,
    FencingDatabase,
    WaitingForReplication,
    PublishingCheckpoint,
    RestoringRelational,
    RestoringMedia,
    ValidatingPair,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OffsiteBackupProgress {
    operation_id: String,
    operation: OffsiteBackupOperationKind,
    phase: OffsiteBackupProgressPhase,
    error_code: Option<BackupErrorCode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OffsiteBackupOperation {
    operation_id: String,
    operation: OffsiteBackupOperationKind,
    reused: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OffsiteBackupCommandError {
    code: BackupErrorCode,
    message: &'static str,
}

impl From<BackupErrorCode> for OffsiteBackupCommandError {
    fn from(code: BackupErrorCode) -> Self {
        Self {
            code,
            message: safe_error_message(code),
        }
    }
}

type CommandResult<T> = Result<T, OffsiteBackupCommandError>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OffsiteBackupTargetInput {
    account_id: String,
    jurisdiction: R2Jurisdiction,
    bucket: String,
    prefix: String,
}

impl OffsiteBackupTargetInput {
    fn parse(self) -> Result<R2Target, BackupErrorCode> {
        Ok(R2Target {
            account_id: R2AccountId::parse(self.account_id)
                .map_err(|_| BackupErrorCode::InvalidTarget)?,
            jurisdiction: self.jurisdiction,
            bucket: R2BucketName::parse(self.bucket).map_err(|_| BackupErrorCode::InvalidTarget)?,
            prefix: R2Prefix::parse(self.prefix).map_err(|_| BackupErrorCode::InvalidTarget)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OffsiteBackupCredentialsInput {
    access_key_id: String,
    secret_access_key: String,
}

impl OffsiteBackupCredentialsInput {
    fn parse(mut self) -> Result<R2Credentials, BackupErrorCode> {
        let access_key_id = std::mem::take(&mut self.access_key_id);
        let secret_access_key = std::mem::take(&mut self.secret_access_key);
        R2Credentials::new(access_key_id, secret_access_key)
            .map_err(|_| BackupErrorCode::InvalidTarget)
    }
}

impl Drop for OffsiteBackupCredentialsInput {
    fn drop(&mut self) {
        self.access_key_id.zeroize();
        self.secret_access_key.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TestAndEnableOffsiteBackupInput {
    target: OffsiteBackupTargetInput,
    credentials: OffsiteBackupCredentialsInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReplaceOffsiteBackupCredentialsInput {
    credentials: OffsiteBackupCredentialsInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeOffsiteBackupTargetInput {
    target: OffsiteBackupTargetInput,
    credentials: OffsiteBackupCredentialsInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TakeOverRestoredOffsiteBackupInput {
    confirmed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CredentialAvailability {
    Present,
    Missing,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OffsiteBackupTargetStatus {
    account_id: String,
    jurisdiction: R2Jurisdiction,
    bucket: String,
    prefix: String,
}

impl From<&R2Target> for OffsiteBackupTargetStatus {
    fn from(target: &R2Target) -> Self {
        Self {
            account_id: target.account_id.as_str().to_owned(),
            jurisdiction: target.jurisdiction,
            bucket: target.bucket.as_str().to_owned(),
            prefix: target.prefix.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RelationalBackupStatusDto {
    phase: RelationalBackupPhase,
    latest_local_txid: Option<String>,
    latest_remote_txid: Option<String>,
    last_remote_confirmed_at: Option<i64>,
    restart_count: u32,
    last_error_code: Option<BackupErrorCode>,
}

impl From<RelationalBackupStatus> for RelationalBackupStatusDto {
    fn from(status: RelationalBackupStatus) -> Self {
        Self {
            phase: status.phase,
            latest_local_txid: status.latest_local_txid.map(|txid| txid.to_string()),
            latest_remote_txid: status.latest_remote_txid.map(|txid| txid.to_string()),
            last_remote_confirmed_at: status.last_remote_confirmed_at,
            restart_count: status.restart_count,
            last_error_code: status.last_error_code,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaBackupStatusDto {
    phase: MediaBackupPhase,
    pending_count: u64,
    pending_bytes: u64,
    retry_wait_count: u64,
    verified_count: u64,
    verified_bytes: u64,
    blocked_count: u64,
    last_error_code: Option<BackupErrorCode>,
}

impl From<MediaBackupStatus> for MediaBackupStatusDto {
    fn from(status: MediaBackupStatus) -> Self {
        Self {
            phase: status.phase,
            pending_count: status.pending_count,
            pending_bytes: status.pending_bytes,
            retry_wait_count: status.retry_wait_count,
            verified_count: status.verified_count,
            verified_bytes: status.verified_bytes,
            blocked_count: status.blocked_count,
            last_error_code: status.last_error_code,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckpointBackupStatusDto {
    phase: CheckpointBackupPhase,
    in_progress_checkpoint_id: Option<String>,
    last_complete_checkpoint_id: Option<String>,
    last_complete_at: Option<i64>,
    last_error_code: Option<BackupErrorCode>,
}

impl From<CheckpointBackupStatus> for CheckpointBackupStatusDto {
    fn from(status: CheckpointBackupStatus) -> Self {
        Self {
            phase: status.phase,
            in_progress_checkpoint_id: status
                .in_progress_checkpoint_id
                .map(|checkpoint_id| checkpoint_id.to_string()),
            last_complete_checkpoint_id: status
                .last_complete_checkpoint_id
                .map(|checkpoint_id| checkpoint_id.to_string()),
            last_complete_at: status.last_complete_at,
            last_error_code: status.last_error_code,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OffsiteBackupStatus {
    configured: bool,
    enabled: bool,
    revision: Option<i64>,
    target: Option<OffsiteBackupTargetStatus>,
    credentials: CredentialAvailability,
    relational: RelationalBackupStatusDto,
    media: MediaBackupStatusDto,
    checkpoint: CheckpointBackupStatusDto,
    last_restore_drill: Option<RestoreDrillReport>,
    last_restore_drill_at: Option<i64>,
    last_restore_drill_error: Option<BackupErrorCode>,
    takeover_available: bool,
    active_operation: Option<OffsiteBackupOperation>,
}

#[tauri::command]
pub(crate) async fn load_offsite_backup_status(
    database: State<'_, Database>,
    media: State<'_, MediaBackupCoordinator>,
    litestream: State<'_, LitestreamRuntimeService>,
    checkpoint: State<'_, CheckpointCoordinator>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
) -> CommandResult<OffsiteBackupStatus> {
    let client = database.client();
    let report_directory = database.paths().backups.join(RESTORE_DRILL_DIRECTORY);
    let media_status = media.status();
    let relational_status = litestream.status();
    let checkpoint_status = checkpoint.status();
    let active_operation = operations.active();
    let takeover_available = operations.takeover_available();
    tauri::async_runtime::spawn_blocking(move || {
        load_status(
            client,
            report_directory,
            media_status,
            relational_status,
            checkpoint_status,
            active_operation,
            takeover_available,
        )
    })
    .await
    .map_err(|_| OffsiteBackupCommandError::from(BackupErrorCode::WorkerUnavailable))?
}

#[tauri::command]
pub(crate) async fn test_and_enable_offsite_backup(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
    input: TestAndEnableOffsiteBackupInput,
) -> CommandResult<OffsiteBackupOperation> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let started = operations.begin(OffsiteBackupOperationKind::TestAndEnable);
    if started.reused {
        return Ok(started.into());
    }
    let operation_id = started.operation_id.clone();
    let client = database.client();
    let data_root = database.paths().root.clone();
    let task_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        configure_backup(
            &task_app,
            &operation_id,
            OffsiteBackupOperationKind::TestAndEnable,
            client,
            data_root,
            resource_dir,
            input.target,
            input.credentials,
            ConfigurationMode::Enable,
        )
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    if result.is_ok() {
        reload_services(&app);
    }
    finish_operation(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::TestAndEnable,
        result,
    )?;
    Ok(started.into())
}

#[tauri::command]
pub(crate) async fn replace_offsite_backup_credentials(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
    input: ReplaceOffsiteBackupCredentialsInput,
) -> CommandResult<OffsiteBackupOperation> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let started = operations.begin(OffsiteBackupOperationKind::ReplaceCredentials);
    if started.reused {
        return Ok(started.into());
    }
    let operation_id = started.operation_id.clone();
    let client = database.client();
    let data_root = database.paths().root.clone();
    let task_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        replace_credentials(
            &task_app,
            &operation_id,
            client,
            data_root,
            resource_dir,
            input,
        )
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    if result.is_ok() {
        reload_services(&app);
    }
    finish_operation(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::ReplaceCredentials,
        result,
    )?;
    Ok(started.into())
}

#[tauri::command]
pub(crate) async fn change_offsite_backup_target(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
    input: ChangeOffsiteBackupTargetInput,
) -> CommandResult<OffsiteBackupOperation> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let started = operations.begin(OffsiteBackupOperationKind::ChangeTarget);
    if started.reused {
        return Ok(started.into());
    }
    let operation_id = started.operation_id.clone();
    let client = database.client();
    let data_root = database.paths().root.clone();
    let task_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        configure_backup(
            &task_app,
            &operation_id,
            OffsiteBackupOperationKind::ChangeTarget,
            client,
            data_root,
            resource_dir,
            input.target,
            input.credentials,
            ConfigurationMode::ChangeTarget,
        )
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    if result.is_ok() {
        reload_services(&app);
    }
    finish_operation(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::ChangeTarget,
        result,
    )?;
    Ok(started.into())
}

#[tauri::command]
pub(crate) async fn take_over_restored_offsite_backup(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
    input: TakeOverRestoredOffsiteBackupInput,
) -> CommandResult<OffsiteBackupOperation> {
    if !input.confirmed {
        return Err(BackupErrorCode::InvalidTarget.into());
    }
    let started = operations.begin(OffsiteBackupOperationKind::TakeOver);
    if started.reused {
        return Ok(started.into());
    }
    let operation_id = started.operation_id.clone();
    let client = database.client();
    let data_root = database.paths().root.clone();
    let task_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        emit_progress(
            &task_app,
            &operation_id,
            OffsiteBackupOperationKind::TakeOver,
            OffsiteBackupProgressPhase::ValidatingConfig,
            None,
        );
        take_over_backup(client, data_root)
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    // A successful remote owner CAS fences the old local configuration even
    // if persisting the new epoch is the step that failed.
    reload_services(&app);
    finish_operation(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::TakeOver,
        result,
    )?;
    Ok(started.into())
}

#[tauri::command]
pub(crate) async fn disable_offsite_backup(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
) -> CommandResult<OffsiteBackupOperation> {
    run_local_configuration_operation(
        app,
        database.client(),
        &operations,
        OffsiteBackupOperationKind::Disable,
        false,
    )
    .await
}

#[tauri::command]
pub(crate) async fn remove_offsite_backup_credentials(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
) -> CommandResult<OffsiteBackupOperation> {
    run_local_configuration_operation(
        app,
        database.client(),
        &operations,
        OffsiteBackupOperationKind::RemoveCredentials,
        true,
    )
    .await
}

#[tauri::command]
pub(crate) async fn create_offsite_backup_now(
    app: AppHandle,
    operations: State<'_, OffsiteBackupOperationRegistry>,
) -> CommandResult<OffsiteBackupOperation> {
    let started = operations.begin(OffsiteBackupOperationKind::BackupNow);
    if started.reused {
        return Ok(started.into());
    }
    emit_progress(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::BackupNow,
        OffsiteBackupProgressPhase::FencingDatabase,
        None,
    );
    let task_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        task_app
            .state::<CheckpointCoordinator>()
            .backup_now()
            .map(|_| ())
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    finish_operation(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::BackupNow,
        result,
    )?;
    Ok(started.into())
}

#[tauri::command]
pub(crate) async fn run_offsite_restore_drill(
    app: AppHandle,
    database: State<'_, Database>,
    operations: State<'_, OffsiteBackupOperationRegistry>,
) -> CommandResult<OffsiteBackupOperation> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let started = operations.begin(OffsiteBackupOperationKind::RestoreDrill);
    if started.reused {
        return Ok(started.into());
    }
    let operation_id = started.operation_id.clone();
    let client = database.client();
    let report_directory = database.paths().backups.join(RESTORE_DRILL_DIRECTORY);
    let task_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_restore_drill(
            &task_app,
            &operation_id,
            client,
            resource_dir,
            report_directory,
        )
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    finish_operation(
        &app,
        &started.operation_id,
        OffsiteBackupOperationKind::RestoreDrill,
        result,
    )?;
    Ok(started.into())
}

impl From<OperationStart> for OffsiteBackupOperation {
    fn from(started: OperationStart) -> Self {
        Self {
            operation_id: started.operation_id,
            operation: started.operation,
            reused: started.reused,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ConfigurationMode {
    Enable,
    ChangeTarget,
}

#[allow(clippy::too_many_arguments)]
fn configure_backup(
    app: &AppHandle,
    operation_id: &str,
    operation: OffsiteBackupOperationKind,
    client: DatabaseClient,
    data_root: std::path::PathBuf,
    resource_dir: std::path::PathBuf,
    target_input: OffsiteBackupTargetInput,
    credentials_input: OffsiteBackupCredentialsInput,
    mode: ConfigurationMode,
) -> Result<(), BackupErrorCode> {
    emit_progress(
        app,
        operation_id,
        operation,
        OffsiteBackupProgressPhase::ValidatingConfig,
        None,
    );
    let target = target_input.parse()?;
    let credentials = credentials_input.parse()?;
    let current = client
        .load_offsite_backup_config()
        .map_err(map_database_error)?;
    let expected_revision = match (mode, current.as_ref()) {
        (ConfigurationMode::Enable, Some(config)) if config.target == target => config.revision,
        (ConfigurationMode::Enable, Some(_)) => return Err(BackupErrorCode::InvalidTarget),
        (ConfigurationMode::Enable, None) => 0,
        (ConfigurationMode::ChangeTarget, Some(config)) if config.target != target => {
            config.revision
        }
        (ConfigurationMode::ChangeTarget, _) => return Err(BackupErrorCode::InvalidTarget),
    };
    let binary = VerifiedLitestreamBinary::resolve(&resource_dir)
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let store = R2ObjectStore::new(target.clone(), &credentials)
        .map_err(|error| map_store_error(error.code))?;
    emit_progress(
        app,
        operation_id,
        operation,
        OffsiteBackupProgressPhase::TestingObjectStore,
        None,
    );
    let relational_probe =
        LitestreamRelationalProbe::new(&binary, &data_root, &target, &credentials);
    let report =
        verify_connection_with_progress(&store, &relational_probe, &target.keyspace(), |stage| {
            if stage == ProbeStage::LitestreamRoundTrip {
                emit_progress(
                    app,
                    operation_id,
                    operation,
                    OffsiteBackupProgressPhase::TestingLitestream,
                    None,
                );
            }
        })
        .map_err(map_probe_error)?;
    if let ProbeCleanupStatus::Failed(code) = report.cleanup {
        return Err(map_store_error(code));
    }
    let installation_id = InstallationIdentityStore::new(&data_root)
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?
        .load_or_create()
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let (backup_set_id, replica_epoch_id) =
        select_configuration_authority(mode, current.as_ref(), &store, &target, &installation_id)?;
    let candidate = OffsiteBackupConfig {
        revision: expected_revision,
        backup_set_id: backup_set_id.clone(),
        replica_epoch_id: replica_epoch_id.clone(),
        enabled: true,
        provider: super::domain::BackupProvider::R2,
        target: target.clone(),
        created_at: current.as_ref().map_or(0, |config| config.created_at),
        updated_at: current.as_ref().map_or(0, |config| config.updated_at),
    };
    let credential_store = MacOsKeychainCredentialStore;
    if let Err(error) = create_or_validate_backup_authority(&store, &candidate, &installation_id) {
        if mode == ConfigurationMode::Enable
            && current.is_some()
            && error == BackupErrorCode::OwnerMismatch
        {
            // The connection probe and backup-set identity both passed. Keep
            // these tested credentials so the explicit takeover action can
            // authenticate without enabling the old ownership epoch.
            credential_store
                .save(&backup_set_id, &credentials)
                .map_err(map_credential_error)?;
            client
                .set_offsite_backup_takeover_availability(backup_set_id.clone(), true)
                .map_err(map_database_error)?;
        }
        return Err(error);
    }

    emit_progress(
        app,
        operation_id,
        operation,
        OffsiteBackupProgressPhase::SavingConfiguration,
        None,
    );
    let previous_credentials = credential_store.load(&backup_set_id).ok();
    credential_store
        .save(&backup_set_id, &credentials)
        .map_err(map_credential_error)?;
    let saved = client.save_offsite_backup_config(SaveOffsiteBackupConfigInput {
        expected_revision,
        backup_set_id: backup_set_id.clone(),
        replica_epoch_id,
        enabled: true,
        target,
    });
    if let Err(error) = saved {
        if let Some(previous_credentials) = previous_credentials {
            let _ = credential_store.save(&backup_set_id, &previous_credentials);
        } else {
            let _ = credential_store.remove(&backup_set_id);
        }
        return Err(map_database_error(error));
    }
    if let Some(previous) = current {
        if previous.backup_set_id != backup_set_id {
            let _ = credential_store.remove(&previous.backup_set_id);
        }
    }
    emit_progress(
        app,
        operation_id,
        operation,
        OffsiteBackupProgressPhase::ReconcilingMedia,
        None,
    );
    Ok(())
}

fn select_configuration_authority(
    mode: ConfigurationMode,
    current: Option<&OffsiteBackupConfig>,
    store: &dyn ObjectStore,
    target: &R2Target,
    installation_id: &InstallationId,
) -> Result<(BackupSetId, ReplicaEpochId), BackupErrorCode> {
    match (mode, current) {
        (ConfigurationMode::Enable, Some(config)) => Ok((
            config.backup_set_id.clone(),
            config.replica_epoch_id.clone(),
        )),
        (ConfigurationMode::Enable, None) | (ConfigurationMode::ChangeTarget, Some(_)) => Ok(
            recover_uncommitted_authority(store, target, installation_id)?
                .unwrap_or_else(|| (BackupSetId::new(), ReplicaEpochId::new())),
        ),
        (ConfigurationMode::ChangeTarget, None) => Err(BackupErrorCode::InvalidTarget),
    }
}

fn recover_uncommitted_authority(
    store: &dyn ObjectStore,
    target: &R2Target,
    installation_id: &InstallationId,
) -> Result<Option<(BackupSetId, ReplicaEpochId)>, BackupErrorCode> {
    let keyspace = target.keyspace();
    let identity = match store.get(&keyspace.identity()) {
        Ok(stored) => IdentityManifestV1::from_json(&stored.bytes)
            .map_err(|_| BackupErrorCode::MalformedManifest)?,
        Err(error) if error.code == ObjectStoreErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(map_store_error(error.code)),
    };
    if identity.original_installation_id() != installation_id {
        return Ok(None);
    }
    let owner = match store.get(&keyspace.owner()) {
        Ok(stored) => Some(
            OwnerManifestV1::from_json(&stored.bytes)
                .map_err(|_| BackupErrorCode::MalformedManifest)?,
        ),
        Err(error) if error.code == ObjectStoreErrorCode::NotFound => None,
        Err(error) => return Err(map_store_error(error.code)),
    };
    match owner {
        Some(owner)
            if owner.backup_set_id() == identity.backup_set_id()
                && owner.installation_id() == installation_id =>
        {
            Ok(Some((
                identity.backup_set_id().clone(),
                owner.replica_epoch_id().clone(),
            )))
        }
        Some(_) => Ok(None),
        None => Ok(Some((
            identity.backup_set_id().clone(),
            ReplicaEpochId::new(),
        ))),
    }
}

fn replace_credentials(
    app: &AppHandle,
    operation_id: &str,
    client: DatabaseClient,
    data_root: std::path::PathBuf,
    resource_dir: std::path::PathBuf,
    input: ReplaceOffsiteBackupCredentialsInput,
) -> Result<(), BackupErrorCode> {
    emit_progress(
        app,
        operation_id,
        OffsiteBackupOperationKind::ReplaceCredentials,
        OffsiteBackupProgressPhase::ValidatingConfig,
        None,
    );
    let config = require_config(&client)?;
    let credentials = input.credentials.parse()?;
    let binary = VerifiedLitestreamBinary::resolve(&resource_dir)
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let store = R2ObjectStore::new(config.target.clone(), &credentials)
        .map_err(|error| map_store_error(error.code))?;
    emit_progress(
        app,
        operation_id,
        OffsiteBackupOperationKind::ReplaceCredentials,
        OffsiteBackupProgressPhase::TestingObjectStore,
        None,
    );
    let relational_probe =
        LitestreamRelationalProbe::new(&binary, &data_root, &config.target, &credentials);
    let report = verify_connection_with_progress(
        &store,
        &relational_probe,
        &config.target.keyspace(),
        |stage| {
            if stage == ProbeStage::LitestreamRoundTrip {
                emit_progress(
                    app,
                    operation_id,
                    OffsiteBackupOperationKind::ReplaceCredentials,
                    OffsiteBackupProgressPhase::TestingLitestream,
                    None,
                );
            }
        },
    )
    .map_err(map_probe_error)?;
    if let ProbeCleanupStatus::Failed(code) = report.cleanup {
        return Err(map_store_error(code));
    }
    validate_backup_identity(&store, &config)?;
    MacOsKeychainCredentialStore
        .save(&config.backup_set_id, &credentials)
        .map_err(map_credential_error)?;
    Ok(())
}

fn take_over_backup(
    client: DatabaseClient,
    data_root: std::path::PathBuf,
) -> Result<(), BackupErrorCode> {
    let config = require_config(&client)?;
    let credentials = MacOsKeychainCredentialStore
        .load(&config.backup_set_id)
        .map_err(map_credential_error)?;
    let store = R2ObjectStore::new(config.target.clone(), &credentials)
        .map_err(|error| map_store_error(error.code))?;
    let installation_id = InstallationIdentityStore::new(&data_root)
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?
        .load_or_create()
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let replica_epoch_id =
        take_over_backup_authority(&store, &config, &installation_id, &ReplicaEpochId::new())?;
    client
        .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
            expected_revision: config.revision,
            backup_set_id: config.backup_set_id,
            replica_epoch_id,
            enabled: true,
            target: config.target,
        })
        .map_err(map_database_error)?;
    Ok(())
}

async fn run_local_configuration_operation(
    app: AppHandle,
    client: DatabaseClient,
    operations: &OffsiteBackupOperationRegistry,
    kind: OffsiteBackupOperationKind,
    remove_credentials: bool,
) -> CommandResult<OffsiteBackupOperation> {
    let started = operations.begin(kind);
    if started.reused {
        return Ok(started.into());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let config = require_config(&client)?;
        if config.enabled {
            client
                .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                    expected_revision: config.revision,
                    backup_set_id: config.backup_set_id.clone(),
                    replica_epoch_id: config.replica_epoch_id,
                    enabled: false,
                    target: config.target,
                })
                .map_err(map_database_error)?;
        }
        if remove_credentials {
            MacOsKeychainCredentialStore
                .remove(&config.backup_set_id)
                .map_err(map_credential_error)?;
            client
                .set_offsite_backup_takeover_availability(config.backup_set_id, false)
                .map_err(map_database_error)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| BackupErrorCode::WorkerUnavailable)
    .and_then(|result| result);
    operations.finish(&started.operation_id, result);
    // The database config may already be disabled when a later Keychain
    // removal fails, so always apply the stored configuration.
    reload_services(&app);
    finish_operation(&app, &started.operation_id, kind, result)?;
    Ok(started.into())
}

fn run_restore_drill(
    app: &AppHandle,
    operation_id: &str,
    client: DatabaseClient,
    resource_dir: std::path::PathBuf,
    report_root: std::path::PathBuf,
) -> Result<(), BackupErrorCode> {
    emit_progress(
        app,
        operation_id,
        OffsiteBackupOperationKind::RestoreDrill,
        OffsiteBackupProgressPhase::ValidatingConfig,
        None,
    );
    let config = require_config(&client)?;
    let credentials = MacOsKeychainCredentialStore
        .load(&config.backup_set_id)
        .map_err(map_credential_error)?;
    let store = R2ObjectStore::new(config.target.clone(), &credentials)
        .map_err(|error| map_store_error(error.code))?;
    validate_backup_identity(&store, &config)?;
    let report_directory = restore_drill_directory(&report_root, &config);
    let engine = RemoteRecoveryEngine::system(config.target.clone(), credentials, &resource_dir)?;
    emit_progress(
        app,
        operation_id,
        OffsiteBackupOperationKind::RestoreDrill,
        OffsiteBackupProgressPhase::RestoringRelational,
        None,
    );
    let report = engine.run_scoped_restore_drill(
        &report_directory,
        &RemoteCheckpointSelector::Latest,
        &config.backup_set_id,
        &config.replica_epoch_id,
    )?;
    if report.outcome != super::restore::RestoreDrillOutcome::Success {
        return Err(report
            .error_code
            .unwrap_or(BackupErrorCode::RestoreValidationFailed));
    }
    emit_progress(
        app,
        operation_id,
        OffsiteBackupOperationKind::RestoreDrill,
        OffsiteBackupProgressPhase::ValidatingPair,
        None,
    );
    Ok(())
}

fn require_config(client: &DatabaseClient) -> Result<OffsiteBackupConfig, BackupErrorCode> {
    client
        .load_offsite_backup_config()
        .map_err(map_database_error)?
        .ok_or(BackupErrorCode::InvalidTarget)
}

fn load_status(
    client: DatabaseClient,
    report_root: std::path::PathBuf,
    media_status: MediaBackupStatus,
    relational_status: RelationalBackupStatus,
    checkpoint_status: CheckpointBackupStatus,
    active_operation: Option<ActiveOperation>,
    takeover_hint: bool,
) -> CommandResult<OffsiteBackupStatus> {
    let config = client
        .load_offsite_backup_config()
        .map_err(map_database_error)?;
    let persisted_takeover_available = client
        .load_offsite_backup_takeover_availability()
        .map_err(map_database_error)?;
    let credentials = config
        .as_ref()
        .map_or(
            CredentialAvailability::Missing,
            |config| match MacOsKeychainCredentialStore.load(&config.backup_set_id) {
                Ok(_) => CredentialAvailability::Present,
                Err(CredentialError::Missing) => CredentialAvailability::Missing,
                Err(_) => CredentialAvailability::Unavailable,
            },
        );
    let (last_restore_drill, last_restore_drill_at, last_restore_drill_error) =
        config.as_ref().map_or((None, None, None), |config| {
            let report_directory = restore_drill_directory(&report_root, config);
            match load_restore_drill_report(&report_directory) {
                Ok(Some(report))
                    if report.matches_scope(&config.backup_set_id, &config.replica_epoch_id) =>
                {
                    // The report itself is the useful durable result. A platform that
                    // cannot expose its modification time should not make all backup
                    // status unavailable.
                    let updated_at = restore_drill_report_updated_at(&report_directory)
                        .ok()
                        .flatten();
                    (Some(report), updated_at, None)
                }
                Ok(Some(_)) => (None, None, Some(BackupErrorCode::RestoreValidationFailed)),
                Ok(None) => (None, None, None),
                Err(error) => (None, None, Some(error)),
            }
        });
    let takeover_available = config.is_some()
        && (persisted_takeover_available
            || takeover_hint
            || relational_status.last_error_code == Some(BackupErrorCode::OwnerMismatch));
    let active_operation = active_operation.map(|operation| OffsiteBackupOperation {
        operation_id: operation.operation_id,
        operation: operation.kind,
        reused: false,
    });
    Ok(OffsiteBackupStatus {
        configured: config.is_some(),
        enabled: config.as_ref().is_some_and(|config| config.enabled),
        revision: config.as_ref().map(|config| config.revision),
        target: config
            .as_ref()
            .map(|config| OffsiteBackupTargetStatus::from(&config.target)),
        credentials,
        relational: relational_status.into(),
        media: media_status.into(),
        checkpoint: checkpoint_status.scoped_to(config.as_ref()).into(),
        last_restore_drill,
        last_restore_drill_at,
        last_restore_drill_error,
        takeover_available,
        active_operation,
    })
}

fn restore_drill_directory(
    report_root: &std::path::Path,
    config: &OffsiteBackupConfig,
) -> std::path::PathBuf {
    report_root
        .join(config.backup_set_id.as_str())
        .join(config.replica_epoch_id.as_str())
}

fn reload_services(app: &AppHandle) {
    app.state::<MediaBackupCoordinator>().reload_configuration();
    app.state::<LitestreamRuntimeService>()
        .reload_configuration();
    app.state::<CheckpointCoordinator>().wake();
}

fn finish_operation(
    app: &AppHandle,
    operation_id: &str,
    operation: OffsiteBackupOperationKind,
    result: Result<(), BackupErrorCode>,
) -> CommandResult<()> {
    match result {
        Ok(()) => {
            emit_progress(
                app,
                operation_id,
                operation,
                OffsiteBackupProgressPhase::Complete,
                None,
            );
            Ok(())
        }
        Err(code) => {
            emit_progress(
                app,
                operation_id,
                operation,
                OffsiteBackupProgressPhase::Failed,
                Some(code),
            );
            Err(code.into())
        }
    }
}

fn emit_progress(
    app: &AppHandle,
    operation_id: &str,
    operation: OffsiteBackupOperationKind,
    phase: OffsiteBackupProgressPhase,
    error_code: Option<BackupErrorCode>,
) {
    let _ = app.emit_to(
        "main",
        OFFSITE_BACKUP_PROGRESS_EVENT,
        OffsiteBackupProgress {
            operation_id: operation_id.to_owned(),
            operation,
            phase,
            error_code,
        },
    );
}

fn map_probe_error(error: ConnectionProbeError) -> BackupErrorCode {
    match error.stage {
        ProbeStage::LitestreamRoundTrip => match error.relational_code {
            Some(RelationalProbeErrorCode::Prepare)
            | Some(RelationalProbeErrorCode::Start)
            | Some(RelationalProbeErrorCode::SocketUnavailable) => {
                BackupErrorCode::LitestreamUnavailable
            }
            Some(RelationalProbeErrorCode::Sync)
            | Some(RelationalProbeErrorCode::Shutdown)
            | Some(RelationalProbeErrorCode::Restore)
            | Some(RelationalProbeErrorCode::Validate)
            | None => BackupErrorCode::LitestreamFailed,
        },
        ProbeStage::ObjectPut
        | ProbeStage::ObjectHead
        | ProbeStage::ObjectGet
        | ProbeStage::ObjectList => error
            .object_store_code
            .map(map_store_error)
            .unwrap_or(BackupErrorCode::ServiceUnavailable),
    }
}

fn map_database_error(error: DatabaseError) -> BackupErrorCode {
    match error {
        DatabaseError::InvalidOffsiteBackupConfig(_) => BackupErrorCode::InvalidTarget,
        DatabaseError::StaleOffsiteBackupConfig | DatabaseError::WriterUnavailable => {
            BackupErrorCode::WorkerUnavailable
        }
        _ => BackupErrorCode::WorkerUnavailable,
    }
}

const fn safe_error_message(code: BackupErrorCode) -> &'static str {
    match code {
        BackupErrorCode::NetworkOffline => "Dara could not reach Cloudflare R2.",
        BackupErrorCode::NetworkTimeout => "Cloudflare R2 did not respond in time.",
        BackupErrorCode::RateLimited => "Cloudflare R2 asked Dara to try again later.",
        BackupErrorCode::ServiceUnavailable => "Cloudflare R2 is temporarily unavailable.",
        BackupErrorCode::KeychainCredentialMissing => "Saved R2 credentials are missing.",
        BackupErrorCode::KeychainUnavailable => "Dara could not use macOS Keychain.",
        BackupErrorCode::InvalidTarget => "Check the R2 account, bucket, prefix, and credentials.",
        BackupErrorCode::AuthenticationRejected => "Cloudflare rejected the R2 credentials.",
        BackupErrorCode::AuthorizationRejected => {
            "The R2 credentials cannot read and write this bucket."
        }
        BackupErrorCode::PrefixIdentityMismatch => {
            "This R2 prefix belongs to a different Dara backup."
        }
        BackupErrorCode::OwnerMismatch => "Another Dara installation currently owns this backup.",
        BackupErrorCode::ImmutableObjectConflict => {
            "Remote backup data conflicts with the local data."
        }
        BackupErrorCode::LocalMediaMissing => "A local image needed for backup is missing.",
        BackupErrorCode::LocalMediaTooLarge => "A local image is too large to back up.",
        BackupErrorCode::LocalMediaHashMismatch => "A local image failed its integrity check.",
        BackupErrorCode::WorkerUnavailable => "The backup service is unavailable. Restart Dara.",
        BackupErrorCode::LitestreamUnavailable => "Dara could not start its bundled backup helper.",
        BackupErrorCode::LitestreamFailed => "The relational backup test failed.",
        BackupErrorCode::FenceTimeout => "Dara could not safely pause the database for backup.",
        BackupErrorCode::ReplicaBehind => "Relational replication has not caught up yet.",
        BackupErrorCode::CheckpointNotFound => "No recoverable backup checkpoint was found.",
        BackupErrorCode::ExactTxidUnavailable => {
            "The selected relational backup point is no longer available."
        }
        BackupErrorCode::MalformedManifest => "Remote backup metadata is invalid.",
        BackupErrorCode::RemoteMediaMissing => "A backed-up image is missing from R2.",
        BackupErrorCode::RemoteMediaCorrupt => "A backed-up image failed its integrity check.",
        BackupErrorCode::RestoreValidationFailed => "The restore drill did not validate.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::object_store::fake::FakeObjectStore;

    #[test]
    fn target_input_uses_the_domain_validators() {
        let target = OffsiteBackupTargetInput {
            account_id: "0123456789abcdef0123456789abcdef".into(),
            jurisdiction: R2Jurisdiction::Default,
            bucket: "dara-local".into(),
            prefix: "dara/primary".into(),
        }
        .parse()
        .expect("valid target");
        assert_eq!(target.bucket.as_str(), "dara-local");

        assert_eq!(
            OffsiteBackupTargetInput {
                account_id: "not-an-account".into(),
                jurisdiction: R2Jurisdiction::Default,
                bucket: "dara-local".into(),
                prefix: "dara/primary".into(),
            }
            .parse(),
            Err(BackupErrorCode::InvalidTarget)
        );
    }

    #[test]
    fn operation_registry_reuses_an_active_operation() {
        let registry = OffsiteBackupOperationRegistry::default();
        let first = registry.begin(OffsiteBackupOperationKind::BackupNow);
        let second = registry.begin(OffsiteBackupOperationKind::RestoreDrill);
        assert!(!first.reused);
        assert!(second.reused);
        assert_eq!(first.operation_id, second.operation_id);

        registry.finish(&first.operation_id, Ok(()));

        let third = registry.begin(OffsiteBackupOperationKind::RestoreDrill);
        assert!(!third.reused);
        assert_ne!(first.operation_id, third.operation_id);
    }

    #[test]
    fn operation_registry_exposes_owner_mismatch_until_recovery_succeeds() {
        let registry = OffsiteBackupOperationRegistry::default();
        let reenable = registry.begin(OffsiteBackupOperationKind::TestAndEnable);
        registry.finish(&reenable.operation_id, Err(BackupErrorCode::OwnerMismatch));
        assert!(registry.takeover_available());

        let failed_takeover = registry.begin(OffsiteBackupOperationKind::TakeOver);
        registry.finish(
            &failed_takeover.operation_id,
            Err(BackupErrorCode::NetworkOffline),
        );
        assert!(registry.takeover_available());

        let successful_takeover = registry.begin(OffsiteBackupOperationKind::TakeOver);
        registry.finish(&successful_takeover.operation_id, Ok(()));
        assert!(!registry.takeover_available());
    }

    #[test]
    fn setup_retry_recovers_only_authority_created_by_this_installation() {
        let target = OffsiteBackupTargetInput {
            account_id: "0123456789abcdef0123456789abcdef".into(),
            jurisdiction: R2Jurisdiction::Default,
            bucket: "dara-local".into(),
            prefix: "dara/primary".into(),
        }
        .parse()
        .expect("target");
        let installation_id = InstallationId::new();
        let config = OffsiteBackupConfig {
            revision: 0,
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            provider: super::super::domain::BackupProvider::R2,
            target: target.clone(),
            created_at: 0,
            updated_at: 0,
        };
        let store = FakeObjectStore::default();
        create_or_validate_backup_authority(&store, &config, &installation_id).expect("authority");

        assert_eq!(
            recover_uncommitted_authority(&store, &target, &installation_id)
                .expect("recovered authority"),
            Some((
                config.backup_set_id.clone(),
                config.replica_epoch_id.clone()
            ))
        );
        assert_eq!(
            recover_uncommitted_authority(&store, &target, &InstallationId::new())
                .expect("foreign authority"),
            None
        );
    }

    #[test]
    fn target_change_retry_recovers_authority_created_by_this_installation() {
        let target = OffsiteBackupTargetInput {
            account_id: "0123456789abcdef0123456789abcdef".into(),
            jurisdiction: R2Jurisdiction::Default,
            bucket: "dara-local".into(),
            prefix: "dara/new-target".into(),
        }
        .parse()
        .expect("target");
        let installation_id = InstallationId::new();
        let remote = OffsiteBackupConfig {
            revision: 3,
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            provider: super::super::domain::BackupProvider::R2,
            target: target.clone(),
            created_at: 0,
            updated_at: 0,
        };
        let current = OffsiteBackupConfig {
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            target: R2Target {
                prefix: R2Prefix::parse("dara/old-target").expect("old prefix"),
                ..target.clone()
            },
            ..remote.clone()
        };
        let store = FakeObjectStore::default();
        create_or_validate_backup_authority(&store, &remote, &installation_id).expect("authority");

        assert_eq!(
            select_configuration_authority(
                ConfigurationMode::ChangeTarget,
                Some(&current),
                &store,
                &target,
                &installation_id,
            )
            .expect("recovered authority"),
            (
                remote.backup_set_id.clone(),
                remote.replica_epoch_id.clone()
            )
        );
    }

    #[test]
    fn restore_drill_directories_are_scoped_to_backup_set_and_epoch() {
        let target = OffsiteBackupTargetInput {
            account_id: "0123456789abcdef0123456789abcdef".into(),
            jurisdiction: R2Jurisdiction::Default,
            bucket: "dara-local".into(),
            prefix: "dara/primary".into(),
        }
        .parse()
        .expect("target");
        let config = OffsiteBackupConfig {
            revision: 1,
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            provider: super::super::domain::BackupProvider::R2,
            target,
            created_at: 0,
            updated_at: 0,
        };
        let changed_target = OffsiteBackupConfig {
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            ..config.clone()
        };
        let taken_over = OffsiteBackupConfig {
            replica_epoch_id: ReplicaEpochId::new(),
            ..config.clone()
        };
        let root = std::path::Path::new("/tmp/dara-restore-drills");

        assert_ne!(
            restore_drill_directory(root, &config),
            restore_drill_directory(root, &changed_target)
        );
        assert_ne!(
            restore_drill_directory(root, &config),
            restore_drill_directory(root, &taken_over)
        );
    }

    #[test]
    fn safe_errors_never_include_provider_or_local_details() {
        for code in [
            BackupErrorCode::AuthenticationRejected,
            BackupErrorCode::LitestreamFailed,
            BackupErrorCode::RestoreValidationFailed,
        ] {
            let message = safe_error_message(code);
            assert!(!message.contains("http"));
            assert!(!message.contains('/'));
            assert!(!message.contains("xml"));
        }
    }
}
