use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, ExitStatus, Stdio},
    str::FromStr,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    credentials::R2Credentials,
    domain::{
        BackupErrorCode, BackupSetId, CheckpointId, CheckpointManifestV1, CheckpointPhase,
        ContentSha256, IdentityManifestV1, OffsiteMediaState, OwnerManifestV1, R2AccountId,
        R2BucketName, R2Jurisdiction, R2Keyspace, R2Prefix, R2Target, ReplicaEpochId, UtcTimestamp,
        OBJECT_FORMAT_VERSION,
    },
    litestream::{
        configure_credentials_environment, parse_restore_plan_json, parse_restore_result_json,
        LitestreamConfig, LitestreamRuntimePaths, LitestreamTxid, ReplicaKind,
        VerifiedLitestreamBinary,
    },
    object_store::{
        GetObjectResult, ListedObject, ObjectContentType, ObjectStore, ObjectStoreErrorCode,
        R2ObjectStore, MAX_OBJECT_BYTES,
    },
    remote_authority::map_store_error,
};
use crate::{
    app_lock::AppDataLock,
    database::{
        connection::{self, DatabaseKind, FileState},
        migrations::{self, MigrationHeads},
        snapshot, validation,
    },
    recovery,
};

const MAX_CHECKPOINT_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_CHECKPOINT_MANIFEST_CANDIDATES: usize = 100;
const MAX_CHECKPOINT_LIST_PAGES: usize = 100;
const MAX_RECOVERY_CATALOG_CHECKPOINTS: usize = 20;
const MAX_RESTORE_MEDIA_OBJECTS: usize = 100_000;
const MAX_RESTORE_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const RESTORE_DRY_RUN_TIMEOUT: Duration = Duration::from_secs(35);
const RESTORE_EXECUTION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const RESTORE_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);
const DRILL_REPORT_FILE_NAME: &str = "restore-drill-report-v2.json";
const DRILL_REPORT_FORMAT_VERSION: u32 = 2;
const MAX_DRILL_REPORT_BYTES: u64 = 64 * 1024;
const ACCOUNT_ID_ENV: &str = "DARA_LITESTREAM_R2_ACCOUNT_ID";
const JURISDICTION_ENV: &str = "DARA_LITESTREAM_R2_JURISDICTION";
const BUCKET_ENV: &str = "DARA_LITESTREAM_R2_BUCKET";
const ACCESS_KEY_ID_ENV: &str = "DARA_LITESTREAM_R2_ACCESS_KEY_ID";
const SECRET_ACCESS_KEY_ENV: &str = "DARA_LITESTREAM_R2_SECRET_ACCESS_KEY";
const RESTORE_RUNTIME_PREFIX: &str = ".dara-ls-restore-";
const RESTORE_RUNTIME_CREATE_ATTEMPTS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteCheckpointSelector {
    Latest,
    Checkpoint(CheckpointId),
}

impl RemoteCheckpointSelector {
    pub(crate) fn parse(value: &str) -> Result<Self, BackupErrorCode> {
        if value == "latest" {
            Ok(Self::Latest)
        } else {
            CheckpointId::parse(value)
                .map(Self::Checkpoint)
                .map_err(|_| BackupErrorCode::CheckpointNotFound)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RemoteCheckpointAvailability {
    Restorable,
    ExactTxidUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteCheckpointSummary {
    pub(crate) checkpoint_id: CheckpointId,
    pub(crate) created_at: String,
    pub(crate) dara_version: String,
    pub(crate) txid: String,
    pub(crate) main_migration_head: u32,
    pub(crate) media_migration_head: u32,
    pub(crate) referenced_media_count: u64,
    pub(crate) referenced_media_bytes: u64,
    pub(crate) availability: RemoteCheckpointAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteCheckpointCatalog {
    pub(crate) checkpoints: Vec<RemoteCheckpointSummary>,
    pub(crate) malformed_objects_ignored: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RestoreDrillOutcome {
    Success,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RestoreValidationStage {
    CheckpointDiscovered,
    ExactTxidRestored,
    RelationalValidated,
    MediaReconstructed,
    PairValidated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RestoreDrillReport {
    pub(crate) format_version: u32,
    pub(crate) backup_set_id: Option<BackupSetId>,
    pub(crate) replica_epoch_id: Option<ReplicaEpochId>,
    pub(crate) outcome: RestoreDrillOutcome,
    pub(crate) checkpoint_id: Option<CheckpointId>,
    pub(crate) checkpoint_created_at: Option<String>,
    pub(crate) restored_txid: Option<String>,
    pub(crate) main_migration_head: Option<u32>,
    pub(crate) media_migration_head: Option<u32>,
    pub(crate) referenced_media_count: Option<u64>,
    pub(crate) referenced_media_bytes: Option<u64>,
    pub(crate) validation_stages: Vec<RestoreValidationStage>,
    pub(crate) duration_ms: u64,
    pub(crate) dara_version: String,
    pub(crate) error_code: Option<BackupErrorCode>,
}

impl RestoreDrillReport {
    pub(crate) fn matches_scope(
        &self,
        backup_set_id: &BackupSetId,
        replica_epoch_id: &ReplicaEpochId,
    ) -> bool {
        self.backup_set_id.as_ref() == Some(backup_set_id)
            && self.replica_epoch_id.as_ref() == Some(replica_epoch_id)
    }

    fn validate(&self) -> Result<(), BackupErrorCode> {
        const ALL_STAGES: &[RestoreValidationStage] = &[
            RestoreValidationStage::CheckpointDiscovered,
            RestoreValidationStage::ExactTxidRestored,
            RestoreValidationStage::RelationalValidated,
            RestoreValidationStage::MediaReconstructed,
            RestoreValidationStage::PairValidated,
        ];
        let valid_prefix = self.validation_stages.len() <= ALL_STAGES.len()
            && self
                .validation_stages
                .iter()
                .zip(ALL_STAGES)
                .all(|(actual, expected)| actual == expected);
        let metadata_complete = self.checkpoint_id.is_some()
            && self.backup_set_id.is_some()
            && self.replica_epoch_id.is_some()
            && self.checkpoint_created_at.is_some()
            && self.restored_txid.is_some()
            && self.main_migration_head.is_some()
            && self.media_migration_head.is_some()
            && self.referenced_media_count.is_some()
            && self.referenced_media_bytes.is_some();
        let outcome_valid = match self.outcome {
            RestoreDrillOutcome::Success => {
                self.error_code.is_none()
                    && metadata_complete
                    && self.validation_stages == ALL_STAGES
            }
            RestoreDrillOutcome::Failed => self.error_code.is_some(),
        };
        if self.format_version != DRILL_REPORT_FORMAT_VERSION
            || self.dara_version.is_empty()
            || self.dara_version.len() > 64
            || self.dara_version.chars().any(char::is_control)
            || !valid_prefix
            || !outcome_valid
        {
            return Err(BackupErrorCode::RestoreValidationFailed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteRestoreReport {
    pub(crate) checkpoint_id: CheckpointId,
    pub(crate) checkpoint_created_at: String,
    pub(crate) restored_txid: String,
    pub(crate) referenced_media_count: u64,
    pub(crate) referenced_media_bytes: u64,
    pub(crate) data_directory: PathBuf,
    pub(crate) awaiting_application_validation: bool,
}

#[derive(Clone)]
struct RemoteCheckpoint {
    manifest: CheckpointManifestV1,
    created_at_millis: i64,
}

struct ListedCheckpointCandidate {
    listed: ListedObject,
    checkpoint_id: CheckpointId,
    created_at_millis: i64,
}

impl RemoteCheckpoint {
    fn summary(&self, availability: RemoteCheckpointAvailability) -> RemoteCheckpointSummary {
        RemoteCheckpointSummary {
            checkpoint_id: self.manifest.checkpoint_id().clone(),
            created_at: self.manifest.created_at().as_str().to_owned(),
            dara_version: self.manifest.dara_version().to_owned(),
            txid: self.manifest.txid().to_owned(),
            main_migration_head: self.manifest.main_migration_head(),
            media_migration_head: self.manifest.media_migration_head(),
            referenced_media_count: self.manifest.referenced_hash_count(),
            referenced_media_bytes: self.manifest.referenced_total_bytes(),
            availability,
        }
    }
}

struct DiscoveredCheckpoints {
    checkpoints: Vec<RemoteCheckpoint>,
    malformed_objects_ignored: u64,
    backup_set_id: BackupSetId,
    epoch: ReplicaEpochId,
}

trait RelationalRestore: Send + Sync {
    fn dry_run(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
        txid: LitestreamTxid,
        output: &Path,
    ) -> Result<(), BackupErrorCode>;

    fn restore(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
        txid: LitestreamTxid,
        output: &Path,
    ) -> Result<(), BackupErrorCode>;
}

pub(crate) struct RemoteRecoveryEngine {
    store: Arc<dyn ObjectStore>,
    keyspace: R2Keyspace,
    target: R2Target,
    relational: Arc<dyn RelationalRestore>,
}

impl RemoteRecoveryEngine {
    pub(crate) fn system_from_environment(resource_dir: &Path) -> Result<Self, BackupErrorCode> {
        let target = R2Target {
            account_id: R2AccountId::parse(required_environment(ACCOUNT_ID_ENV)?)
                .map_err(|_| BackupErrorCode::InvalidTarget)?,
            jurisdiction: R2Jurisdiction::from_db(&required_environment(JURISDICTION_ENV)?)
                .map_err(|_| BackupErrorCode::InvalidTarget)?,
            bucket: R2BucketName::parse(required_environment(BUCKET_ENV)?)
                .map_err(|_| BackupErrorCode::InvalidTarget)?,
            prefix: R2Prefix::primary(),
        };
        let access_key_id = std::env::var(ACCESS_KEY_ID_ENV)
            .map_err(|_| BackupErrorCode::KeychainCredentialMissing)?;
        let secret_access_key = std::env::var(SECRET_ACCESS_KEY_ENV)
            .map_err(|_| BackupErrorCode::KeychainCredentialMissing)?;
        let credentials = R2Credentials::new(access_key_id, secret_access_key)
            .map_err(|_| BackupErrorCode::KeychainUnavailable)?;
        Self::system(target, credentials, resource_dir)
    }

    pub(crate) fn system(
        target: R2Target,
        credentials: R2Credentials,
        resource_dir: &Path,
    ) -> Result<Self, BackupErrorCode> {
        let binary = VerifiedLitestreamBinary::resolve(resource_dir)
            .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        let store = R2ObjectStore::new(target.clone(), &credentials)
            .map_err(|error| map_store_error(error.code))?;
        Ok(Self {
            store: Arc::new(store),
            keyspace: target.keyspace(),
            target: target.clone(),
            relational: Arc::new(SystemRelationalRestore {
                binary: binary.path().to_owned(),
                target,
                credentials,
            }),
        })
    }

    #[cfg(test)]
    fn with_parts(
        store: Arc<dyn ObjectStore>,
        target: R2Target,
        relational: Arc<dyn RelationalRestore>,
    ) -> Self {
        Self {
            store,
            keyspace: target.keyspace(),
            target,
            relational,
        }
    }

    pub(crate) fn list_checkpoints(&self) -> Result<RemoteCheckpointCatalog, BackupErrorCode> {
        let scratch = RestoreTask::create(&std::env::temp_dir(), ".dara-remote-inspect-")?;
        let discovered = self.discover_checkpoints()?;
        let catalog_length = discovered
            .checkpoints
            .len()
            .min(MAX_RECOVERY_CATALOG_CHECKPOINTS);
        let mut checkpoints = Vec::with_capacity(catalog_length);
        for (index, checkpoint) in discovered
            .checkpoints
            .iter()
            .take(MAX_RECOVERY_CATALOG_CHECKPOINTS)
            .enumerate()
        {
            let availability =
                match self.dry_run_checkpoint(scratch.path(), &discovered.epoch, checkpoint, index)
                {
                    Ok(()) => RemoteCheckpointAvailability::Restorable,
                    Err(BackupErrorCode::ExactTxidUnavailable) => {
                        RemoteCheckpointAvailability::ExactTxidUnavailable
                    }
                    Err(error) => return Err(error),
                };
            checkpoints.push(checkpoint.summary(availability));
        }
        Ok(RemoteCheckpointCatalog {
            checkpoints,
            malformed_objects_ignored: discovered.malformed_objects_ignored,
        })
    }

    pub(crate) fn inspect_checkpoint(
        &self,
        selector: &RemoteCheckpointSelector,
    ) -> Result<RemoteCheckpointSummary, BackupErrorCode> {
        let scratch = RestoreTask::create(&std::env::temp_dir(), ".dara-remote-inspect-")?;
        let discovered = self.discover_checkpoints()?;
        let checkpoint = self.select_checkpoint(scratch.path(), &discovered, selector)?;
        Ok(checkpoint.summary(RemoteCheckpointAvailability::Restorable))
    }

    pub(crate) fn run_restore_drill(
        &self,
        report_directory: &Path,
        selector: &RemoteCheckpointSelector,
    ) -> Result<RestoreDrillReport, BackupErrorCode> {
        self.run_restore_drill_with_scope(report_directory, selector, None)
    }

    pub(crate) fn run_scoped_restore_drill(
        &self,
        report_directory: &Path,
        selector: &RemoteCheckpointSelector,
        backup_set_id: &BackupSetId,
        replica_epoch_id: &ReplicaEpochId,
    ) -> Result<RestoreDrillReport, BackupErrorCode> {
        self.run_restore_drill_with_scope(
            report_directory,
            selector,
            Some((backup_set_id.clone(), replica_epoch_id.clone())),
        )
    }

    fn run_restore_drill_with_scope(
        &self,
        report_directory: &Path,
        selector: &RemoteCheckpointSelector,
        expected_scope: Option<(BackupSetId, ReplicaEpochId)>,
    ) -> Result<RestoreDrillReport, BackupErrorCode> {
        fs::create_dir_all(report_directory)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let report_directory = fs::canonicalize(report_directory)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let task = RestoreTask::create(&report_directory, ".dara-restore-drill-")?;
        let started = Instant::now();
        let mut stages = Vec::new();
        let mut selected: Option<RemoteCheckpoint> = None;
        let mut report_scope = expected_scope.clone();
        let attempt = (|| {
            let discovered = self.discover_checkpoints()?;
            if let Some((expected_backup_set_id, expected_replica_epoch_id)) =
                expected_scope.as_ref()
            {
                if &discovered.backup_set_id != expected_backup_set_id {
                    return Err(BackupErrorCode::PrefixIdentityMismatch);
                }
                if &discovered.epoch != expected_replica_epoch_id {
                    return Err(BackupErrorCode::OwnerMismatch);
                }
            } else {
                report_scope = Some((discovered.backup_set_id.clone(), discovered.epoch.clone()));
            }
            let checkpoint = self.select_checkpoint(task.path(), &discovered, selector)?;
            stages.push(RestoreValidationStage::CheckpointDiscovered);
            selected = Some(checkpoint.clone());
            let restored =
                self.reconstruct(task.path(), &discovered.epoch, &checkpoint, &mut stages)?;
            snapshot::load_and_validate_manifest(&restored.manifest_path)
                .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
            Ok::<(), BackupErrorCode>(())
        })();
        let duration_ms = elapsed_millis(started);
        let report = match attempt {
            Ok(()) => {
                let checkpoint = selected
                    .as_ref()
                    .expect("successful drill selected a checkpoint");
                successful_drill_report(checkpoint, stages, duration_ms)
            }
            Err(error) => failed_drill_report(
                selected.as_ref(),
                report_scope.as_ref(),
                stages,
                duration_ms,
                error,
            ),
        };
        write_drill_report(&report_directory, &report)?;
        Ok(report)
    }

    pub(crate) fn restore_to(
        &self,
        data_directory: &Path,
        selector: &RemoteCheckpointSelector,
    ) -> Result<RemoteRestoreReport, BackupErrorCode> {
        fs::create_dir_all(data_directory).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let data_directory = fs::canonicalize(data_directory)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let lock = AppDataLock::acquire(&data_directory)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        recovery::prepare_offsite_restore_target(&lock)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let task = RestoreTask::create(&data_directory, ".dara-remote-restore-")?;
        let discovered = self.discover_checkpoints()?;
        let checkpoint = self.select_checkpoint(task.path(), &discovered, selector)?;
        let mut stages = vec![RestoreValidationStage::CheckpointDiscovered];
        let restored =
            self.reconstruct(task.path(), &discovered.epoch, &checkpoint, &mut stages)?;
        recovery::install_offsite_snapshot(&lock, &restored.manifest_path)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        Ok(RemoteRestoreReport {
            checkpoint_id: checkpoint.manifest.checkpoint_id().clone(),
            checkpoint_created_at: checkpoint.manifest.created_at().as_str().to_owned(),
            restored_txid: checkpoint.manifest.txid().to_owned(),
            referenced_media_count: checkpoint.manifest.referenced_hash_count(),
            referenced_media_bytes: checkpoint.manifest.referenced_total_bytes(),
            data_directory,
            awaiting_application_validation: true,
        })
    }

    fn discover_checkpoints(&self) -> Result<DiscoveredCheckpoints, BackupErrorCode> {
        let identity_object = self
            .store
            .get(&self.keyspace.identity())
            .map_err(|error| map_store_error(error.code))?;
        validate_json_object(&identity_object)?;
        let identity = IdentityManifestV1::from_json(&identity_object.bytes)
            .map_err(|_| BackupErrorCode::MalformedManifest)?;

        let owner_object = self
            .store
            .get(&self.keyspace.owner())
            .map_err(|error| map_store_error(error.code))?;
        validate_json_object(&owner_object)?;
        let owner = OwnerManifestV1::from_json(&owner_object.bytes)
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        if owner.backup_set_id() != identity.backup_set_id() {
            return Err(BackupErrorCode::PrefixIdentityMismatch);
        }

        let prefix = self.keyspace.checkpoints(owner.replica_epoch_id());
        let mut continuation = None;
        let mut candidates = Vec::new();
        let mut checkpoint_id_counts = HashMap::new();
        let mut malformed_objects_ignored = 0_u64;
        let mut listing_complete = false;
        for _ in 0..MAX_CHECKPOINT_LIST_PAGES {
            let page = self
                .store
                .list(&prefix, continuation.as_ref())
                .map_err(|error| map_store_error(error.code))?;
            for listed in page.objects {
                if listed.byte_length == 0 || listed.byte_length > MAX_CHECKPOINT_MANIFEST_BYTES {
                    malformed_objects_ignored = malformed_objects_ignored.saturating_add(1);
                    continue;
                }
                let candidate = match listed_checkpoint_candidate(
                    &self.keyspace,
                    owner.replica_epoch_id(),
                    listed,
                ) {
                    Ok(candidate) => candidate,
                    Err(_) => {
                        malformed_objects_ignored = malformed_objects_ignored.saturating_add(1);
                        continue;
                    }
                };
                *checkpoint_id_counts
                    .entry(candidate.checkpoint_id.clone())
                    .or_insert(0_u64) += 1;
                candidates.push(candidate);
            }
            retain_most_recent_checkpoint_candidates(
                &mut candidates,
                MAX_CHECKPOINT_MANIFEST_CANDIDATES,
            );
            match page.next {
                Some(next) => continuation = Some(next),
                None => {
                    listing_complete = true;
                    break;
                }
            }
        }
        if !listing_complete {
            return Err(BackupErrorCode::RestoreValidationFailed);
        }
        malformed_objects_ignored = malformed_objects_ignored.saturating_add(
            discard_duplicate_checkpoint_candidates(&mut candidates, &checkpoint_id_counts),
        );
        sort_checkpoint_candidates_most_recent_first(&mut candidates);

        let mut checkpoints = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let listed = candidate.listed;
            let stored = match self.store.get(&listed.key) {
                Ok(stored) => stored,
                Err(error) if error.code == ObjectStoreErrorCode::NotFound => {
                    malformed_objects_ignored = malformed_objects_ignored.saturating_add(1);
                    continue;
                }
                Err(error) => return Err(map_store_error(error.code)),
            };
            let manifest = match validate_json_object(&stored).and_then(|()| {
                CheckpointManifestV1::from_json(&stored.bytes, &self.keyspace)
                    .map_err(|_| BackupErrorCode::MalformedManifest)
            }) {
                Ok(manifest) => manifest,
                Err(_) => {
                    malformed_objects_ignored = malformed_objects_ignored.saturating_add(1);
                    continue;
                }
            };
            let valid = manifest.backup_set_id() == identity.backup_set_id()
                && manifest.replica_epoch_id() == owner.replica_epoch_id()
                && manifest
                    .object_key(&self.keyspace)
                    .is_ok_and(|expected| expected == listed.key);
            let created_at_millis = manifest.created_at().unix_timestamp_millis();
            if !valid || created_at_millis.is_err() {
                malformed_objects_ignored = malformed_objects_ignored.saturating_add(1);
                continue;
            }
            checkpoints.push(RemoteCheckpoint {
                manifest,
                created_at_millis: created_at_millis.expect("validated checkpoint timestamp"),
            });
        }
        sort_checkpoints_most_recent_first(&mut checkpoints);
        Ok(DiscoveredCheckpoints {
            checkpoints,
            malformed_objects_ignored,
            backup_set_id: identity.backup_set_id().clone(),
            epoch: owner.replica_epoch_id().clone(),
        })
    }

    fn select_checkpoint(
        &self,
        task_root: &Path,
        discovered: &DiscoveredCheckpoints,
        selector: &RemoteCheckpointSelector,
    ) -> Result<RemoteCheckpoint, BackupErrorCode> {
        let candidates = discovered
            .checkpoints
            .iter()
            .filter(|checkpoint| match selector {
                RemoteCheckpointSelector::Latest => true,
                RemoteCheckpointSelector::Checkpoint(checkpoint_id) => {
                    checkpoint.manifest.checkpoint_id() == checkpoint_id
                }
            });
        let mut found = false;
        for (index, checkpoint) in candidates.enumerate() {
            found = true;
            match self.dry_run_checkpoint(task_root, &discovered.epoch, checkpoint, index) {
                Ok(()) => return Ok(checkpoint.clone()),
                Err(BackupErrorCode::ExactTxidUnavailable)
                    if matches!(selector, RemoteCheckpointSelector::Latest) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        if found {
            Err(BackupErrorCode::ExactTxidUnavailable)
        } else {
            Err(BackupErrorCode::CheckpointNotFound)
        }
    }

    fn dry_run_checkpoint(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
        checkpoint: &RemoteCheckpoint,
        index: usize,
    ) -> Result<(), BackupErrorCode> {
        let txid = LitestreamTxid::from_str(checkpoint.manifest.txid())
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        let output = task_root.join(format!("dry-run-{index}.sqlite3"));
        self.relational.dry_run(task_root, epoch, txid, &output)
    }

    fn reconstruct(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
        checkpoint: &RemoteCheckpoint,
        stages: &mut Vec<RestoreValidationStage>,
    ) -> Result<snapshot::CreatedSnapshot, BackupErrorCode> {
        let backups = task_root.join("backups");
        create_private_directory(&backups)?;
        let main_path = backups.join("remote-main.sqlite3");
        let media_path = backups.join("remote-media.sqlite3");
        let txid = LitestreamTxid::from_str(checkpoint.manifest.txid())
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        self.relational
            .restore(task_root, epoch, txid, &main_path)?;
        stages.push(RestoreValidationStage::ExactTxidRestored);
        let references =
            validate_restored_relational(&main_path, &checkpoint.manifest, &self.target)?;
        stages.push(RestoreValidationStage::RelationalValidated);
        reconstruct_media(
            self.store.as_ref(),
            &self.keyspace,
            &media_path,
            checkpoint.manifest.media_migration_head(),
            &references,
        )?;
        stages.push(RestoreValidationStage::MediaReconstructed);
        validate_reconstructed_pair(&main_path, &media_path, &checkpoint.manifest)?;
        stages.push(RestoreValidationStage::PairValidated);
        let heads = MigrationHeads {
            main: Some(
                i32::try_from(checkpoint.manifest.main_migration_head())
                    .map_err(|_| BackupErrorCode::RestoreValidationFailed)?,
            ),
            media: Some(
                i32::try_from(checkpoint.manifest.media_migration_head())
                    .map_err(|_| BackupErrorCode::RestoreValidationFailed)?,
            ),
        };
        snapshot::finalize_external_snapshot_pair(
            &backups,
            &main_path,
            &media_path,
            checkpoint
                .manifest
                .created_at()
                .unix_timestamp_millis()
                .map_err(|_| BackupErrorCode::MalformedManifest)?,
            checkpoint.manifest.dara_version(),
            heads,
        )
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)
    }
}

fn required_environment(name: &'static str) -> Result<String, BackupErrorCode> {
    std::env::var(name).map_err(|_| BackupErrorCode::InvalidTarget)
}

struct SystemRelationalRestore {
    binary: PathBuf,
    target: R2Target,
    credentials: R2Credentials,
}

impl RelationalRestore for SystemRelationalRestore {
    fn dry_run(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
        txid: LitestreamTxid,
        output: &Path,
    ) -> Result<(), BackupErrorCode> {
        let prepared = self.prepare_config(task_root, epoch)?;
        let result = execute_restore(
            &self.binary,
            prepared.runtime.config(),
            &prepared.database_path,
            output,
            txid,
            &self.credentials,
            RestoreCommandMode::DryRun,
        )?;
        match result {
            RestoreCommandOutput::Plan => Ok(()),
            RestoreCommandOutput::Restored => Err(BackupErrorCode::RestoreValidationFailed),
        }
    }

    fn restore(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
        txid: LitestreamTxid,
        output: &Path,
    ) -> Result<(), BackupErrorCode> {
        let prepared = self.prepare_config(task_root, epoch)?;
        let result = execute_restore(
            &self.binary,
            prepared.runtime.config(),
            &prepared.database_path,
            output,
            txid,
            &self.credentials,
            RestoreCommandMode::Restore,
        )?;
        match result {
            RestoreCommandOutput::Restored => Ok(()),
            RestoreCommandOutput::Plan => Err(BackupErrorCode::RestoreValidationFailed),
        }
    }
}

impl SystemRelationalRestore {
    fn prepare_config(
        &self,
        task_root: &Path,
        epoch: &ReplicaEpochId,
    ) -> Result<PreparedRestoreRuntime, BackupErrorCode> {
        let runtime_root = ShortRestoreRuntimeRoot::create()?;
        let runtime = LitestreamRuntimePaths::new(runtime_root.path())
            .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        let database_path = task_root.join("remote-dara.sqlite3");
        let replica_path = self.target.keyspace().litestream(epoch);
        let endpoint = self.target.endpoint();
        let config = LitestreamConfig {
            database_path: &database_path,
            runtime: &runtime,
            bucket: self.target.bucket.as_str(),
            replica_path: replica_path.as_str(),
            endpoint: &endpoint,
        }
        .render()
        .map_err(|_| BackupErrorCode::InvalidTarget)?;
        runtime
            .write_config(&config)
            .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        Ok(PreparedRestoreRuntime {
            _root: runtime_root,
            runtime,
            database_path,
        })
    }
}

struct PreparedRestoreRuntime {
    _root: ShortRestoreRuntimeRoot,
    runtime: LitestreamRuntimePaths,
    database_path: PathBuf,
}

struct ShortRestoreRuntimeRoot {
    path: PathBuf,
}

impl ShortRestoreRuntimeRoot {
    fn create() -> Result<Self, BackupErrorCode> {
        let base = restore_runtime_base();
        let base = fs::canonicalize(base).map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        for _ in 0..RESTORE_RUNTIME_CREATE_ATTEMPTS {
            let path = base.join(format!("{RESTORE_RUNTIME_PREFIX}{}", Uuid::now_v7()));
            match create_private_runtime_directory(&path) {
                Ok(()) => {
                    if LitestreamRuntimePaths::new(&path).is_err() {
                        let _ = fs::remove_dir(&path);
                        return Err(BackupErrorCode::LitestreamUnavailable);
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(BackupErrorCode::LitestreamUnavailable),
            }
        }
        Err(BackupErrorCode::LitestreamUnavailable)
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ShortRestoreRuntimeRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("could not remove off-site restore runtime data: {error}");
            }
        }
    }
}

#[cfg(unix)]
fn restore_runtime_base() -> &'static Path {
    Path::new("/tmp")
}

#[cfg(not(unix))]
fn restore_runtime_base() -> PathBuf {
    std::env::temp_dir()
}

#[cfg(unix)]
fn create_private_runtime_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_runtime_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

#[derive(Clone, Copy)]
enum RestoreCommandMode {
    DryRun,
    Restore,
}

enum RestoreCommandOutput {
    Plan,
    Restored,
}

#[allow(clippy::too_many_arguments)]
fn execute_restore(
    binary: &Path,
    config: &Path,
    database_path: &Path,
    output_path: &Path,
    txid: LitestreamTxid,
    credentials: &R2Credentials,
    mode: RestoreCommandMode,
) -> Result<RestoreCommandOutput, BackupErrorCode> {
    if !binary.is_absolute()
        || !config.is_absolute()
        || !database_path.is_absolute()
        || !output_path.is_absolute()
    {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    remove_file_if_exists(output_path)?;
    let txid_string = txid.to_string();
    let mut command = Command::new(binary);
    command
        .args(["restore", "-config"])
        .arg(config)
        .args(["-txid", &txid_string]);
    if matches!(mode, RestoreCommandMode::DryRun) {
        command.arg("-dry-run");
    } else {
        command.args(["-integrity-check", "full"]);
    }
    command.args(["-json", "-o"]);
    command
        .arg(output_path)
        .arg(database_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_credentials_environment(&mut command, credentials);
    let mut child = command
        .spawn()
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let stdout = child.stdout.take().ok_or_else(|| {
        stop_restore_process(&mut child, None);
        BackupErrorCode::RestoreValidationFailed
    })?;
    let timeout = match mode {
        RestoreCommandMode::DryRun => RESTORE_DRY_RUN_TIMEOUT,
        RestoreCommandMode::Restore => RESTORE_EXECUTION_TIMEOUT,
    };
    let (status, bytes) = collect_restore_output(&mut child, stdout, timeout)?;
    if !status.success() {
        remove_file_if_exists(output_path)?;
        return Err(BackupErrorCode::ExactTxidUnavailable);
    }
    match mode {
        RestoreCommandMode::DryRun => {
            let plan = parse_restore_plan_json(&bytes)
                .map_err(|_| BackupErrorCode::RestoreValidationFailed);
            let cleanup = remove_file_if_exists(output_path);
            let plan = plan?;
            cleanup?;
            if plan.target_path != output_path
                || plan.replica != ReplicaKind::S3
                || plan.max_txid != txid
                || plan.source != database_path.to_string_lossy()
            {
                return Err(BackupErrorCode::RestoreValidationFailed);
            }
            Ok(RestoreCommandOutput::Plan)
        }
        RestoreCommandMode::Restore => {
            let restored = parse_restore_result_json(&bytes)
                .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
            let metadata = fs::symlink_metadata(output_path)
                .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
            if restored.database_path != output_path
                || restored.replica != ReplicaKind::S3
                || restored.txid != txid
                || !metadata.file_type().is_file()
            {
                return Err(BackupErrorCode::RestoreValidationFailed);
            }
            Ok(RestoreCommandOutput::Restored)
        }
    }
}

fn collect_restore_output(
    child: &mut Child,
    mut stdout: ChildStdout,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>), BackupErrorCode> {
    let mut reader = match thread::Builder::new()
        .name("dara-remote-restore-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .by_ref()
                .take(MAX_RESTORE_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        }) {
        Ok(reader) => Some(reader),
        Err(_) => {
            stop_restore_process(child, None);
            return Err(BackupErrorCode::WorkerUnavailable);
        }
    };
    let deadline = Instant::now() + timeout;
    let mut status = None;
    let mut bytes = None;
    loop {
        if bytes.is_none() && reader.as_ref().is_some_and(|reader| reader.is_finished()) {
            let output = match reader.take().expect("finished restore reader").join() {
                Ok(Ok(output)) if output.len() <= MAX_RESTORE_OUTPUT_BYTES => output,
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
                    stop_restore_process(child, reader.take());
                    return Err(BackupErrorCode::RestoreValidationFailed);
                }
            };
            bytes = Some(output);
        }
        if status.is_none() {
            status = match child.try_wait() {
                Ok(status) => status,
                Err(_) => {
                    stop_restore_process(child, reader.take());
                    return Err(BackupErrorCode::LitestreamFailed);
                }
            };
        }
        if status.is_some() && bytes.is_some() {
            return Ok((
                status.take().expect("completed restore status"),
                bytes.take().expect("completed restore output"),
            ));
        }
        if Instant::now() >= deadline {
            stop_restore_process(child, reader.take());
            return Err(BackupErrorCode::NetworkTimeout);
        }
        thread::sleep(RESTORE_PROCESS_POLL_INTERVAL);
    }
}

fn stop_restore_process(
    child: &mut Child,
    reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
) {
    let _ = child.kill();
    let _ = child.wait();
    if let Some(reader) = reader {
        let _ = reader.join();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecoveredMediaReference {
    sha256: ContentSha256,
    byte_length: u64,
}

fn validate_restored_relational(
    path: &Path,
    manifest: &CheckpointManifestV1,
    target: &R2Target,
) -> Result<Vec<RecoveredMediaReference>, BackupErrorCode> {
    let mut main = connection::open_read_only(path, DatabaseKind::Main)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    validation::validate_integrity(&main, DatabaseKind::Main)
        .and_then(|()| validation::validate_foreign_keys(&main, DatabaseKind::Main))
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let migration = migrations::inspect_main(&mut main)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    if migration.head
        != Some(
            i32::try_from(manifest.main_migration_head())
                .map_err(|_| BackupErrorCode::RestoreValidationFailed)?,
        )
    {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    validate_stored_backup_config(&main, manifest, target)?;
    validate_checkpoint_row(&main, manifest)?;
    load_recovered_media_references(&main, manifest)
}

fn validate_stored_backup_config(
    main: &Connection,
    manifest: &CheckpointManifestV1,
    target: &R2Target,
) -> Result<(), BackupErrorCode> {
    let stored = main
        .query_row(
            "SELECT
                backup_set_id,
                replica_epoch_id,
                enabled,
                provider,
                jurisdiction,
                account_id,
                bucket,
                prefix
             FROM offsite_backup_config
             WHERE singleton_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?
        .ok_or(BackupErrorCode::RestoreValidationFailed)?;
    let valid = BackupSetId::parse(stored.0).is_ok_and(|value| &value == manifest.backup_set_id())
        && ReplicaEpochId::parse(stored.1).is_ok_and(|value| &value == manifest.replica_epoch_id())
        && stored.2
        && stored.3 == super::domain::BackupProvider::R2.as_db_str()
        && stored.4 == target.jurisdiction.as_db_str()
        && stored.5 == target.account_id.as_str()
        && stored.6 == target.bucket.as_str()
        && stored.7 == target.prefix.as_str();
    if !valid {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    Ok(())
}

fn validate_checkpoint_row(
    main: &Connection,
    manifest: &CheckpointManifestV1,
) -> Result<(), BackupErrorCode> {
    let stored = main
        .query_row(
            "SELECT
                backup_set_id,
                replica_epoch_id,
                phase,
                content_revision,
                dara_version,
                main_migration_head,
                media_migration_head,
                referenced_hash_count,
                referenced_total_bytes,
                referenced_hash_set_sha256,
                litestream_txid,
                manifest_object_key
             FROM offsite_backup_checkpoint
             WHERE checkpoint_id = ?1",
            [manifest.checkpoint_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Vec<u8>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            },
        )
        .optional()
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?
        .ok_or(BackupErrorCode::RestoreValidationFailed)?;
    let phase = CheckpointPhase::from_db(&stored.2)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let valid = BackupSetId::parse(stored.0).is_ok_and(|value| &value == manifest.backup_set_id())
        && ReplicaEpochId::parse(stored.1).is_ok_and(|value| &value == manifest.replica_epoch_id())
        && phase == CheckpointPhase::Prepared
        && u64::try_from(stored.3).ok() == Some(manifest.content_revision())
        && stored.4 == manifest.dara_version()
        && u32::try_from(stored.5).ok() == Some(manifest.main_migration_head())
        && u32::try_from(stored.6).ok() == Some(manifest.media_migration_head())
        && u64::try_from(stored.7).ok() == Some(manifest.referenced_hash_count())
        && u64::try_from(stored.8).ok() == Some(manifest.referenced_total_bytes())
        && ContentSha256::from_slice(&stored.9)
            .is_ok_and(|value| value == manifest.referenced_hash_set_sha256())
        && stored.10.is_none()
        && stored.11.is_none();
    if !valid {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    Ok(())
}

fn load_recovered_media_references(
    main: &Connection,
    manifest: &CheckpointManifestV1,
) -> Result<Vec<RecoveredMediaReference>, BackupErrorCode> {
    if manifest.referenced_hash_count() > MAX_RESTORE_MEDIA_OBJECTS as u64 {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    let mut statement = main
        .prepare(
            "SELECT referenced.sha256, object.byte_length, object.state
             FROM (SELECT DISTINCT sha256 FROM image) AS referenced
             LEFT JOIN offsite_media_object AS object
               ON object.backup_set_id = ?1
              AND object.sha256 = referenced.sha256
             ORDER BY referenced.sha256",
        )
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let rows = statement
        .query_map([manifest.backup_set_id().as_str()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let mut references = Vec::new();
    for row in rows {
        if references.len() >= MAX_RESTORE_MEDIA_OBJECTS {
            return Err(BackupErrorCode::RestoreValidationFailed);
        }
        let (sha256, byte_length, state) =
            row.map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let byte_length = byte_length
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or(BackupErrorCode::RestoreValidationFailed)?;
        if state.as_deref() != Some(OffsiteMediaState::Verified.as_db_str()) {
            return Err(BackupErrorCode::RestoreValidationFailed);
        }
        references.push(RecoveredMediaReference {
            sha256: ContentSha256::from_slice(&sha256)
                .map_err(|_| BackupErrorCode::RestoreValidationFailed)?,
            byte_length,
        });
    }
    let total = references.iter().try_fold(0_u64, |total, reference| {
        total.checked_add(reference.byte_length)
    });
    let mut digest = Sha256::new();
    for reference in &references {
        digest.update(reference.sha256.as_bytes());
    }
    if references.len() as u64 != manifest.referenced_hash_count()
        || total != Some(manifest.referenced_total_bytes())
        || ContentSha256::from_bytes(digest.finalize().into())
            != manifest.referenced_hash_set_sha256()
    {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    Ok(references)
}

fn reconstruct_media(
    store: &dyn ObjectStore,
    keyspace: &R2Keyspace,
    media_path: &Path,
    migration_head: u32,
    references: &[RecoveredMediaReference],
) -> Result<(), BackupErrorCode> {
    let mut media = connection::open_writer(media_path, DatabaseKind::Media, FileState::Fresh)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    migrations::run_media_to(&mut media, migration_head)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let transaction = media
        .transaction()
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    for reference in references {
        if reference.byte_length > MAX_OBJECT_BYTES as u64 {
            return Err(BackupErrorCode::RemoteMediaCorrupt);
        }
        let key = keyspace.media(reference.sha256);
        let stored = store.get(&key).map_err(|error| {
            if error.code == ObjectStoreErrorCode::NotFound {
                BackupErrorCode::RemoteMediaMissing
            } else {
                map_store_error(error.code)
            }
        })?;
        validate_media_object(&stored, reference)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO media_blob(sha256, bytes) VALUES (?1, ?2)",
                params![reference.sha256.as_bytes().as_slice(), &stored.bytes],
            )
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let round_trip: Vec<u8> = transaction
            .query_row(
                "SELECT bytes FROM media_blob WHERE sha256 = ?1",
                [reference.sha256.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        if Sha256::digest(&round_trip).as_slice() != reference.sha256.as_bytes() {
            return Err(BackupErrorCode::RestoreValidationFailed);
        }
    }
    transaction
        .commit()
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)
}

fn validate_media_object(
    stored: &GetObjectResult,
    reference: &RecoveredMediaReference,
) -> Result<(), BackupErrorCode> {
    let actual = ContentSha256::from_bytes(Sha256::digest(&stored.bytes).into());
    if stored.metadata.byte_length != reference.byte_length
        || stored.bytes.len() as u64 != reference.byte_length
        || stored.metadata.content_type != Some(ObjectContentType::Webp)
        || stored.metadata.dara_sha256 != Some(reference.sha256)
        || stored.metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
        || actual != reference.sha256
    {
        return Err(BackupErrorCode::RemoteMediaCorrupt);
    }
    Ok(())
}

fn validate_reconstructed_pair(
    main_path: &Path,
    media_path: &Path,
    manifest: &CheckpointManifestV1,
) -> Result<(), BackupErrorCode> {
    let mut main = connection::open_read_only(main_path, DatabaseKind::Main)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let mut media = connection::open_read_only(media_path, DatabaseKind::Media)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    validation::validate_snapshot_pair(&mut main, &mut media, main_path, media_path)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let heads = migrations::current_heads(&mut main, &mut media)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let expected = MigrationHeads {
        main: i32::try_from(manifest.main_migration_head()).ok(),
        media: i32::try_from(manifest.media_migration_head()).ok(),
    };
    if heads != expected {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    Ok(())
}

fn validate_json_object(stored: &GetObjectResult) -> Result<(), BackupErrorCode> {
    if stored.bytes.is_empty()
        || stored.bytes.len() as u64 > MAX_CHECKPOINT_MANIFEST_BYTES
        || stored.metadata.byte_length != stored.bytes.len() as u64
        || stored.metadata.content_type != Some(ObjectContentType::Json)
        || stored.metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
        || stored.metadata.dara_sha256.is_some()
    {
        return Err(BackupErrorCode::MalformedManifest);
    }
    Ok(())
}

fn successful_drill_report(
    checkpoint: &RemoteCheckpoint,
    stages: Vec<RestoreValidationStage>,
    duration_ms: u64,
) -> RestoreDrillReport {
    RestoreDrillReport {
        format_version: DRILL_REPORT_FORMAT_VERSION,
        backup_set_id: Some(checkpoint.manifest.backup_set_id().clone()),
        replica_epoch_id: Some(checkpoint.manifest.replica_epoch_id().clone()),
        outcome: RestoreDrillOutcome::Success,
        checkpoint_id: Some(checkpoint.manifest.checkpoint_id().clone()),
        checkpoint_created_at: Some(checkpoint.manifest.created_at().as_str().to_owned()),
        restored_txid: Some(checkpoint.manifest.txid().to_owned()),
        main_migration_head: Some(checkpoint.manifest.main_migration_head()),
        media_migration_head: Some(checkpoint.manifest.media_migration_head()),
        referenced_media_count: Some(checkpoint.manifest.referenced_hash_count()),
        referenced_media_bytes: Some(checkpoint.manifest.referenced_total_bytes()),
        validation_stages: stages,
        duration_ms,
        dara_version: env!("CARGO_PKG_VERSION").to_owned(),
        error_code: None,
    }
}

fn failed_drill_report(
    checkpoint: Option<&RemoteCheckpoint>,
    discovered_scope: Option<&(BackupSetId, ReplicaEpochId)>,
    stages: Vec<RestoreValidationStage>,
    duration_ms: u64,
    error: BackupErrorCode,
) -> RestoreDrillReport {
    RestoreDrillReport {
        format_version: DRILL_REPORT_FORMAT_VERSION,
        backup_set_id: checkpoint
            .map(|value| value.manifest.backup_set_id().clone())
            .or_else(|| discovered_scope.map(|(backup_set_id, _)| backup_set_id.clone())),
        replica_epoch_id: checkpoint
            .map(|value| value.manifest.replica_epoch_id().clone())
            .or_else(|| discovered_scope.map(|(_, replica_epoch_id)| replica_epoch_id.clone())),
        outcome: RestoreDrillOutcome::Failed,
        checkpoint_id: checkpoint.map(|value| value.manifest.checkpoint_id().clone()),
        checkpoint_created_at: checkpoint
            .map(|value| value.manifest.created_at().as_str().to_owned()),
        restored_txid: checkpoint.map(|value| value.manifest.txid().to_owned()),
        main_migration_head: checkpoint.map(|value| value.manifest.main_migration_head()),
        media_migration_head: checkpoint.map(|value| value.manifest.media_migration_head()),
        referenced_media_count: checkpoint.map(|value| value.manifest.referenced_hash_count()),
        referenced_media_bytes: checkpoint.map(|value| value.manifest.referenced_total_bytes()),
        validation_stages: stages,
        duration_ms,
        dara_version: env!("CARGO_PKG_VERSION").to_owned(),
        error_code: Some(error),
    }
}

fn write_drill_report(
    directory: &Path,
    report: &RestoreDrillReport,
) -> Result<(), BackupErrorCode> {
    report.validate()?;
    let final_path = directory.join(DRILL_REPORT_FILE_NAME);
    let temporary_path = directory.join(format!(".{DRILL_REPORT_FILE_NAME}.tmp"));
    remove_file_if_exists(&temporary_path)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&temporary_path)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, report)
        .and_then(|()| writer.write_all(b"\n").map_err(serde_json::Error::io))
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    writer
        .flush()
        .and_then(|()| writer.get_ref().sync_all())
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    fs::rename(&temporary_path, &final_path)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)
}

pub(crate) fn load_restore_drill_report(
    directory: &Path,
) -> Result<Option<RestoreDrillReport>, BackupErrorCode> {
    let path = directory.join(DRILL_REPORT_FILE_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(BackupErrorCode::RestoreValidationFailed),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_DRILL_REPORT_BYTES {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_DRILL_REPORT_BYTES + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    if bytes.len() as u64 > MAX_DRILL_REPORT_BYTES {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    let report: RestoreDrillReport =
        serde_json::from_slice(&bytes).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    report.validate()?;
    Ok(Some(report))
}

pub(crate) fn restore_drill_report_updated_at(
    directory: &Path,
) -> Result<Option<i64>, BackupErrorCode> {
    let path = directory.join(DRILL_REPORT_FILE_NAME);
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(BackupErrorCode::RestoreValidationFailed),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_DRILL_REPORT_BYTES {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    let modified = metadata
        .modified()
        .and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    i64::try_from(modified.as_millis())
        .map(Some)
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)
}

struct RestoreTask {
    path: PathBuf,
    lock: Option<File>,
}

impl RestoreTask {
    fn create(base: &Path, prefix: &str) -> Result<Self, BackupErrorCode> {
        fs::create_dir_all(base).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let base = fs::canonicalize(base).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        sweep_stale_restore_tasks(&base, prefix)?;
        let path = base.join(format!("{prefix}{}", Uuid::now_v7()));
        create_private_directory(&path)?;
        let lock_path = path.join(".dara-restore-task.lock");
        let lock = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        lock.try_lock()
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        Ok(Self {
            path,
            lock: Some(lock),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RestoreTask {
    fn drop(&mut self) {
        drop(self.lock.take());
        if let Err(error) = fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("could not remove off-site restore staging data: {error}");
            }
        }
    }
}

fn sweep_stale_restore_tasks(base: &Path, prefix: &str) -> Result<(), BackupErrorCode> {
    let entries = fs::read_dir(base).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    for entry in entries {
        let entry = entry.map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        let name = match entry.file_name().to_str() {
            Some(name) => name.to_owned(),
            None => continue,
        };
        let Some(identifier_text) = name.strip_prefix(prefix) else {
            continue;
        };
        let Ok(identifier) = Uuid::parse_str(identifier_text) else {
            continue;
        };
        if identifier.get_version_num() != 7 || identifier.to_string() != identifier_text {
            continue;
        }
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        if !metadata.file_type().is_dir() {
            continue;
        }
        let lock_path = path.join(".dara-restore-task.lock");
        let lock = match OpenOptions::new().read(true).write(true).open(&lock_path) {
            Ok(lock) => match lock.try_lock() {
                Ok(()) => Some(lock),
                Err(std::fs::TryLockError::WouldBlock) => continue,
                Err(std::fs::TryLockError::Error(_)) => {
                    return Err(BackupErrorCode::RestoreValidationFailed);
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let recently_created = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age < Duration::from_secs(60));
                if recently_created {
                    continue;
                }
                None
            }
            Err(_) => return Err(BackupErrorCode::RestoreValidationFailed),
        };
        fs::remove_dir_all(&path).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
        drop(lock);
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), BackupErrorCode> {
    fs::create_dir(path).map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), BackupErrorCode> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(BackupErrorCode::RestoreValidationFailed),
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn sort_checkpoints_most_recent_first(checkpoints: &mut [RemoteCheckpoint]) {
    checkpoints.sort_by(|left, right| {
        right
            .created_at_millis
            .cmp(&left.created_at_millis)
            .then_with(|| {
                right
                    .manifest
                    .checkpoint_id()
                    .cmp(left.manifest.checkpoint_id())
            })
    });
}

fn listed_checkpoint_candidate(
    keyspace: &R2Keyspace,
    epoch: &ReplicaEpochId,
    listed: ListedObject,
) -> Result<ListedCheckpointCandidate, BackupErrorCode> {
    let prefix = keyspace.checkpoints(epoch);
    let file_name = listed
        .key
        .as_str()
        .strip_prefix(prefix.as_str())
        .and_then(|value| value.strip_suffix(".json"))
        .ok_or(BackupErrorCode::MalformedManifest)?;
    let (basic_timestamp, checkpoint_id) = file_name
        .split_once('-')
        .ok_or(BackupErrorCode::MalformedManifest)?;
    let created_at = UtcTimestamp::parse_basic_utc(basic_timestamp)
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    let checkpoint_id =
        CheckpointId::parse(checkpoint_id).map_err(|_| BackupErrorCode::MalformedManifest)?;
    let expected = keyspace
        .checkpoint(epoch, &checkpoint_id, &created_at)
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    if expected != listed.key {
        return Err(BackupErrorCode::MalformedManifest);
    }
    let created_at_millis = created_at
        .unix_timestamp_millis()
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    Ok(ListedCheckpointCandidate {
        listed,
        checkpoint_id,
        created_at_millis,
    })
}

fn sort_checkpoint_candidates_most_recent_first(candidates: &mut [ListedCheckpointCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .created_at_millis
            .cmp(&left.created_at_millis)
            .then_with(|| right.checkpoint_id.cmp(&left.checkpoint_id))
    });
}

fn retain_most_recent_checkpoint_candidates(
    candidates: &mut Vec<ListedCheckpointCandidate>,
    maximum: usize,
) {
    if candidates.len() <= maximum {
        return;
    }
    sort_checkpoint_candidates_most_recent_first(candidates);
    candidates.truncate(maximum);
}

fn discard_duplicate_checkpoint_candidates(
    candidates: &mut Vec<ListedCheckpointCandidate>,
    counts: &HashMap<CheckpointId, u64>,
) -> u64 {
    candidates.retain(|candidate| counts.get(&candidate.checkpoint_id).copied() == Some(1));
    counts
        .values()
        .filter(|count| **count > 1)
        .fold(0_u64, |total, count| total.saturating_add(*count))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    use super::*;
    use crate::{
        backup::{
            credentials::R2Credentials,
            domain::{
                CheckpointManifestInput, InstallationId, R2AccountId, R2BucketName, R2Jurisdiction,
                R2ObjectKey, R2Prefix, UtcTimestamp,
            },
            litestream::{parse_sync_json, LitestreamConfig, LitestreamRuntimePaths},
            object_store::{
                fake::{FakeObjectStore, ObjectOperation},
                PutCondition, PutObjectOutcome, PutObjectRequest, R2ObjectStore,
            },
        },
        database::{
            self, CanonicalImage, CardContentDraft, DatabasePaths, InitializationOptions,
            SaveOffsiteBackupConfigInput,
        },
    };

    const MEDIA_LEASE_ID: &str = "01980c8e-6c00-7000-8000-000000000901";

    struct FakeRelationalRestore {
        main_source: PathBuf,
        unavailable: Mutex<HashSet<u64>>,
        dry_run_count: AtomicUsize,
    }

    impl RelationalRestore for FakeRelationalRestore {
        fn dry_run(
            &self,
            _task_root: &Path,
            _epoch: &ReplicaEpochId,
            txid: LitestreamTxid,
            _output: &Path,
        ) -> Result<(), BackupErrorCode> {
            self.dry_run_count.fetch_add(1, Ordering::Relaxed);
            if self
                .unavailable
                .lock()
                .expect("unavailable TXIDs")
                .contains(&txid.value())
            {
                Err(BackupErrorCode::ExactTxidUnavailable)
            } else {
                Ok(())
            }
        }

        fn restore(
            &self,
            _task_root: &Path,
            _epoch: &ReplicaEpochId,
            txid: LitestreamTxid,
            output: &Path,
        ) -> Result<(), BackupErrorCode> {
            self.dry_run(Path::new("/"), &ReplicaEpochId::new(), txid, output)?;
            fs::copy(&self.main_source, output)
                .map(|_| ())
                .map_err(|_| BackupErrorCode::RestoreValidationFailed)
        }
    }

    struct RemoteFixture {
        _source: tempfile::TempDir,
        engine: RemoteRecoveryEngine,
        store: Arc<FakeObjectStore>,
        keyspace: R2Keyspace,
        media_key: R2ObjectKey,
        relational: Arc<FakeRelationalRestore>,
        manifest: CheckpointManifestV1,
        epoch: ReplicaEpochId,
        checkpoint_id: CheckpointId,
        image_bytes: Vec<u8>,
    }

    struct R2CanaryCleanup {
        store: R2ObjectStore,
        keyspace: R2Keyspace,
        armed: bool,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    enum R2CanaryCheckOutcome {
        Passed,
    }

    impl R2CanaryCleanup {
        fn new(store: R2ObjectStore, keyspace: R2Keyspace) -> Self {
            Self {
                store,
                keyspace,
                armed: true,
            }
        }

        fn cleanup(&mut self) -> Result<u64, &'static str> {
            let mut keys = Vec::new();
            let mut continuation = None;
            for _ in 0..100 {
                let page = self
                    .store
                    .list(&self.keyspace.root_prefix(), continuation.as_ref())
                    .map_err(|_| "could not list the unique R2 canary prefix")?;
                keys.extend(page.objects.into_iter().map(|object| object.key));
                match page.next {
                    Some(next) => continuation = Some(next),
                    None => break,
                }
            }
            for key in &keys {
                self.store
                    .delete(key)
                    .map_err(|_| "could not delete an object from the unique R2 canary prefix")?;
            }
            let residue = self
                .store
                .list(&self.keyspace.root_prefix(), None)
                .map_err(|_| "could not verify R2 canary cleanup")?
                .objects
                .len();
            if residue != 0 {
                return Err("the unique R2 canary prefix still contains objects");
            }
            self.armed = false;
            u64::try_from(keys.len()).map_err(|_| "R2 canary object count overflow")
        }
    }

    impl Drop for R2CanaryCleanup {
        fn drop(&mut self) {
            if self.armed {
                let _ = self.cleanup();
            }
        }
    }

    struct R2CanaryLitestreamChild {
        child: Option<Child>,
    }

    impl R2CanaryLitestreamChild {
        fn new(child: Child) -> Self {
            Self { child: Some(child) }
        }

        fn child_mut(&mut self) -> &mut Child {
            self.child.as_mut().expect("canary child is present")
        }

        fn kill_and_wait(mut self) {
            self.child_mut()
                .kill()
                .expect("interrupt first canary daemon");
            self.child
                .take()
                .expect("canary child is present")
                .wait()
                .expect("reap interrupted canary daemon");
        }

        fn terminate_and_wait(mut self) {
            #[cfg(unix)]
            {
                let result = unsafe { libc::kill(self.child_mut().id() as i32, libc::SIGTERM) };
                assert_eq!(result, 0, "stop canary daemon");
            }
            #[cfg(not(unix))]
            self.child_mut().kill().expect("stop canary daemon");
            assert!(
                self.child
                    .take()
                    .expect("canary child is present")
                    .wait()
                    .expect("reap canary daemon")
                    .success(),
                "canary daemon did not stop cleanly"
            );
        }
    }

    impl Drop for R2CanaryLitestreamChild {
        fn drop(&mut self) {
            let Some(mut child) = self.child.take() else {
                return;
            };
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn target() -> R2Target {
        R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/remote-restore-test").expect("prefix"),
        }
    }

    fn put_json(store: &FakeObjectStore, key: R2ObjectKey, bytes: Vec<u8>) {
        assert_eq!(
            store
                .put(PutObjectRequest {
                    key,
                    bytes,
                    content_type: ObjectContentType::Json,
                    dara_sha256: None,
                    condition: PutCondition::IfAbsent,
                })
                .expect("put JSON"),
            PutObjectOutcome::Stored
        );
    }

    fn rich_remote_fixture() -> RemoteFixture {
        let source = tempfile::tempdir().expect("source");
        let paths = DatabasePaths::new(source.path().join("live"));
        let database = database::initialize(
            paths.clone(),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database");
        let client = database.client();
        let image_bytes = b"canonical webp fixture bytes".to_vec();
        let image = client
            .ingest_image(
                CanonicalImage {
                    bytes: image_bytes.clone(),
                    natural_width: 640,
                    natural_height: 480,
                },
                MEDIA_LEASE_ID.into(),
            )
            .expect("image");
        let basic = client
            .create_card_content(
                CardContentDraft::Basic {
                    front_md: format!("What is shown?\n\n{{{{image:{};width=50%}}}}", image.id),
                    back_md: "A durable restore fixture".into(),
                    source: Some("restore drill".into()),
                },
                MEDIA_LEASE_ID.into(),
            )
            .expect("basic card");
        let cloze = client
            .create_card_content(
                CardContentDraft::Cloze {
                    front_md: "{{c1::Paris}} is the capital of France.".into(),
                    back_md: String::new(),
                    source: Some("restore drill".into()),
                    variant_keys: vec!["cloze:1".into()],
                    search_md: "Paris is the capital of France.".into(),
                },
                MEDIA_LEASE_ID.into(),
            )
            .expect("cloze card");
        let occlusion: CardContentDraft = serde_json::from_value(serde_json::json!({
            "type": "OCCLUSION",
            "frontMd": "Identify the covered area.",
            "backMd": "Restore fixture",
            "source": "restore drill",
            "occlusion": {
                "id": Uuid::now_v7().to_string(),
                "sourceImageId": image.id,
                "mode": "HIDE_ONE_GUESS_ONE",
                "layers": [{
                    "id": Uuid::now_v7().to_string(),
                    "label": "fixture",
                    "masks": [{
                        "id": Uuid::now_v7().to_string(),
                        "x": 0.1,
                        "y": 0.1,
                        "width": 0.2,
                        "height": 0.2,
                        "color": "WHITE"
                    }]
                }]
            }
        }))
        .expect("occlusion draft");
        let occlusion = client
            .create_card_content(occlusion, MEDIA_LEASE_ID.into())
            .expect("occlusion card");

        let review_event_id = Uuid::now_v7().to_string();
        let reviewed_at = database::now_millis().expect("review timestamp");
        let grade: database::RecordGradeInput = serde_json::from_value(serde_json::json!({
            "eventId": review_event_id,
            "reviewCardId": basic.review_card.id,
            "expectedReviewCardUpdatedAt": basic.review_card.updated_at,
            "expectedCardContentUpdatedAt": basic.card_content.updated_at(),
            "expectedCardSequence": basic.last_card_sequence,
            "expectedSchedulerConfigId": basic.scheduler_config.id,
            "review": {
                "grade": 3,
                "reviewedAt": reviewed_at,
                "studyDay": 20_640,
                "timezoneId": "America/New_York",
                "utcOffsetMinutes": -240
            },
            "nextCache": {
                "state": "LEARNING",
                "dueAt": reviewed_at + 60_000,
                "dueStudyDay": null,
                "lastReviewAt": reviewed_at,
                "reps": 1,
                "lapses": 0,
                "schedulerState": {
                    "stability": 1.0,
                    "difficulty": 5.0,
                    "scheduledDays": 0,
                    "learningSteps": 1
                }
            },
            "schedulerLog": {
                "stateBefore": "NEW",
                "dueAtBefore": null,
                "dueStudyDayBefore": null,
                "stabilityBefore": null,
                "difficultyBefore": null,
                "scheduledDaysBefore": null,
                "learningStepsBefore": null
            }
        }))
        .expect("grade input");
        let graded = client.record_grade(grade).expect("record grade");
        let undo: database::UndoLastGradeInput = serde_json::from_value(serde_json::json!({
            "eventId": Uuid::now_v7().to_string(),
            "reviewCardId": graded.context.review_card.id,
            "targetEventId": review_event_id,
            "expectedReviewCardUpdatedAt": graded.context.review_card.updated_at,
            "expectedCardSequence": graded.context.last_card_sequence,
            "expectedSchedulerConfigId": graded.context.scheduler_config.id,
            "nextCache": basic.cache
        }))
        .expect("undo input");
        client.undo_last_grade(undo).expect("undo grade");

        let recent = client
            .search_card_content(database::SearchCardContentInput {
                query: String::new(),
                limit: 20,
                offset: 0,
            })
            .expect("card list");
        let cloze_item = recent
            .iter()
            .find(|item| item.card_content.id() == cloze.card_content.id())
            .expect("cloze list item");
        client
            .set_card_content_suspended(database::SetCardContentSuspendedInput {
                card_content_id: cloze.card_content.id().into(),
                expected_lifecycle_updated_at: cloze_item.lifecycle_updated_at,
                suspended: true,
            })
            .expect("suspend cloze");
        let occlusion_item = recent
            .iter()
            .find(|item| item.card_content.id() == occlusion.card_content.id())
            .expect("occlusion list item");
        client
            .delete_card_content(database::DeleteCardContentInput {
                card_content_id: occlusion.card_content.id().into(),
                expected_updated_at: occlusion.card_content.updated_at(),
                expected_lifecycle_updated_at: occlusion_item.lifecycle_updated_at,
            })
            .expect("delete occlusion");

        let settings = client.load_settings().expect("settings");
        client
            .set_zoom_percent(database::SetZoomPercentInput {
                expected_revision: settings.revision,
                zoom_percent: 120,
            })
            .expect("zoom setting");

        let target = target();
        let keyspace = target.keyspace();
        let backup_set_id = BackupSetId::new();
        let epoch = ReplicaEpochId::new();
        client
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: 0,
                backup_set_id: backup_set_id.clone(),
                replica_epoch_id: epoch.clone(),
                enabled: true,
                target: target.clone(),
            })
            .expect("backup config");
        drop(database);

        let now = database::now_millis().expect("time");
        let main = connection::open_writer(&paths.main, DatabaseKind::Main, FileState::Existing)
            .expect("main");
        main.execute(
            "UPDATE offsite_media_object
             SET state = ?1,
                 last_verified_at = ?2,
                 updated_at = max(updated_at, ?2)
             WHERE backup_set_id = ?3",
            params![
                OffsiteMediaState::Verified.as_db_str(),
                now,
                backup_set_id.as_str()
            ],
        )
        .expect("verified media");
        let (sha256, byte_length): (Vec<u8>, i64) = main
            .query_row(
                "SELECT sha256, byte_length
                 FROM offsite_media_object
                 WHERE backup_set_id = ?1",
                [backup_set_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("media reference");
        let sha256 = ContentSha256::from_slice(&sha256).expect("sha256");
        let mut digest = Sha256::new();
        digest.update(sha256.as_bytes());
        let reference_digest = ContentSha256::from_bytes(digest.finalize().into());
        let content_revision: i64 = main
            .query_row(
                "SELECT revision
                 FROM offsite_backup_content_clock
                 WHERE singleton_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("content revision");
        let checkpoint_id = CheckpointId::new();
        let heads = migrations::expected_heads();
        let main_head = heads.main.expect("main head");
        let media_head = heads.media.expect("media head");
        main.execute(
            "INSERT INTO offsite_backup_checkpoint (
                checkpoint_id,
                backup_set_id,
                replica_epoch_id,
                phase,
                config_revision,
                content_revision,
                created_at,
                dara_version,
                main_migration_head,
                media_migration_head,
                referenced_hash_count,
                referenced_total_bytes,
                referenced_hash_set_sha256,
                litestream_txid,
                manifest_object_key,
                last_error_code,
                updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, 1, ?5, ?6, 'test', ?7, ?8, 1, ?9, ?10,
                NULL, NULL, NULL, ?6
             )",
            params![
                checkpoint_id.as_str(),
                backup_set_id.as_str(),
                epoch.as_str(),
                CheckpointPhase::Prepared.as_db_str(),
                content_revision,
                now,
                main_head,
                media_head,
                byte_length,
                reference_digest.as_bytes().as_slice(),
            ],
        )
        .expect("checkpoint row");
        drop(main);

        let snapshot =
            snapshot::create_snapshot_pair(&paths, "test").expect("relational source snapshot");
        let source_main = snapshot
            .manifest_path
            .parent()
            .expect("snapshot directory")
            .join(&snapshot.manifest.main.file_name);
        let created_at = UtcTimestamp::now().expect("checkpoint timestamp");
        let manifest = CheckpointManifestV1::new(CheckpointManifestInput {
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: epoch.clone(),
            checkpoint_id: checkpoint_id.clone(),
            created_at: created_at.clone(),
            dara_version: "test".into(),
            content_revision: u64::try_from(content_revision).expect("content revision"),
            main_migration_head: u32::try_from(main_head).expect("main head"),
            litestream_path: keyspace.litestream(&epoch),
            txid: "0000000000000042".into(),
            media_migration_head: u32::try_from(media_head).expect("media head"),
            referenced_hash_count: 1,
            referenced_total_bytes: u64::try_from(byte_length).expect("byte length"),
            referenced_hash_set_sha256: reference_digest,
        })
        .expect("checkpoint manifest");

        let store = Arc::new(FakeObjectStore::default());
        put_json(
            store.as_ref(),
            keyspace.identity(),
            IdentityManifestV1::new(backup_set_id.clone(), InstallationId::new())
                .to_json()
                .expect("identity JSON"),
        );
        put_json(
            store.as_ref(),
            keyspace.owner(),
            OwnerManifestV1::new(
                backup_set_id,
                InstallationId::new(),
                epoch.clone(),
                UtcTimestamp::now().expect("owner timestamp"),
            )
            .to_json()
            .expect("owner JSON"),
        );
        put_json(
            store.as_ref(),
            keyspace
                .checkpoint(&epoch, &checkpoint_id, &created_at)
                .expect("checkpoint key"),
            manifest.to_json().expect("checkpoint JSON"),
        );
        let media_key = keyspace.media(sha256);
        assert_eq!(
            store
                .put(PutObjectRequest {
                    key: media_key.clone(),
                    bytes: image_bytes.clone(),
                    content_type: ObjectContentType::Webp,
                    dara_sha256: Some(sha256),
                    condition: PutCondition::IfAbsent,
                })
                .expect("media object"),
            PutObjectOutcome::Stored
        );
        let relational = Arc::new(FakeRelationalRestore {
            main_source: source_main,
            unavailable: Mutex::new(HashSet::new()),
            dry_run_count: AtomicUsize::new(0),
        });
        let engine = RemoteRecoveryEngine::with_parts(store.clone(), target, relational.clone());
        RemoteFixture {
            _source: source,
            engine,
            store,
            keyspace,
            media_key,
            relational,
            manifest,
            epoch,
            checkpoint_id,
            image_bytes,
        }
    }

    #[test]
    #[ignore = "requires bucket-scoped R2 credentials and the pinned Litestream binary"]
    fn live_r2_canary_restores_complete_checkpoint_and_cleans_unique_prefix() {
        assert_eq!(
            required_canary_environment("DARA_RUN_R2_CANARY"),
            "1",
            "set DARA_RUN_R2_CANARY=1 explicitly"
        );
        let run_id = Uuid::now_v7();
        let prefix = format!(
            "{}/canary/{run_id}",
            required_canary_environment("DARA_LITESTREAM_R2_PREFIX").trim_end_matches('/')
        );
        let target = R2Target {
            account_id: R2AccountId::parse(required_canary_environment(
                "DARA_LITESTREAM_R2_ACCOUNT_ID",
            ))
            .expect("canary account ID"),
            jurisdiction: R2Jurisdiction::from_db(&required_canary_environment(
                "DARA_LITESTREAM_R2_JURISDICTION",
            ))
            .expect("canary jurisdiction"),
            bucket: R2BucketName::parse(required_canary_environment("DARA_LITESTREAM_R2_BUCKET"))
                .expect("canary bucket"),
            prefix: R2Prefix::parse(prefix).expect("unique canary prefix"),
        };
        let keyspace = target.keyspace();
        let credentials = canary_credentials();
        let store = R2ObjectStore::new(target.clone(), &credentials).expect("canary object store");
        let mut cleanup = R2CanaryCleanup::new(store, keyspace.clone());
        let local_root = PathBuf::from(required_canary_environment("DARA_R2_CANARY_DATA_DIR"));
        assert!(
            !local_root.exists(),
            "R2 canary data directory must be unique"
        );
        assert!(
            local_root.parent().is_some_and(|parent| parent.is_dir()),
            "R2 canary data parent must already exist"
        );
        create_private_directory(&local_root).expect("canary data directory");

        let fixture = rich_remote_fixture();
        let source_main = fixture.relational.main_source.clone();
        rewrite_canary_target(&source_main, &target);
        let manifest = replicate_canary_source(
            &source_main,
            &local_root,
            &target,
            &credentials,
            &cleanup.store,
            &fixture.manifest,
        );
        let installation_id = InstallationId::new();
        put_canary_json(
            &cleanup.store,
            keyspace.identity(),
            IdentityManifestV1::new(manifest.backup_set_id().clone(), installation_id.clone())
                .to_json()
                .expect("canary identity JSON"),
        );
        put_canary_json(
            &cleanup.store,
            keyspace.owner(),
            OwnerManifestV1::new(
                manifest.backup_set_id().clone(),
                installation_id,
                manifest.replica_epoch_id().clone(),
                UtcTimestamp::now().expect("canary owner timestamp"),
            )
            .to_json()
            .expect("canary owner JSON"),
        );
        let main = connection::open_read_only(&source_main, DatabaseKind::Main)
            .expect("canary source main");
        let sha256 = main
            .query_row(
                "SELECT sha256 FROM offsite_media_object WHERE backup_set_id = ?1 LIMIT 1",
                [manifest.backup_set_id().as_str()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .map(|bytes| ContentSha256::from_slice(&bytes).expect("canary media SHA"))
            .expect("canary media reference");
        drop(main);
        let media_key = keyspace.media(sha256);
        assert_eq!(
            cleanup
                .store
                .put(PutObjectRequest {
                    key: media_key.clone(),
                    bytes: fixture.image_bytes.clone(),
                    content_type: ObjectContentType::Webp,
                    dara_sha256: Some(sha256),
                    condition: PutCondition::IfAbsent,
                })
                .expect("canary media upload"),
            PutObjectOutcome::Stored
        );
        assert_eq!(
            cleanup
                .store
                .put(PutObjectRequest {
                    key: media_key.clone(),
                    bytes: fixture.image_bytes.clone(),
                    content_type: ObjectContentType::Webp,
                    dara_sha256: Some(sha256),
                    condition: PutCondition::IfAbsent,
                })
                .expect("idempotent canary media retry"),
            PutObjectOutcome::ConditionNotMet
        );
        assert_eq!(
            cleanup
                .store
                .get(&media_key)
                .expect("canary media read")
                .bytes,
            fixture.image_bytes
        );
        put_canary_json(
            &cleanup.store,
            keyspace
                .checkpoint(
                    manifest.replica_epoch_id(),
                    manifest.checkpoint_id(),
                    manifest.created_at(),
                )
                .expect("canary checkpoint key"),
            manifest.to_json().expect("canary checkpoint JSON"),
        );

        let engine = RemoteRecoveryEngine::system(
            target,
            canary_credentials(),
            Path::new("/unused-with-development-binary-override"),
        )
        .expect("canary recovery engine");
        let report_directory = local_root.join("drill");
        let report = engine
            .run_restore_drill(&report_directory, &RemoteCheckpointSelector::Latest)
            .expect("real R2 restore drill");
        assert_eq!(report.outcome, RestoreDrillOutcome::Success);
        assert!(report.matches_scope(manifest.backup_set_id(), manifest.replica_epoch_id(),));

        let restored_root = local_root.join("restored");
        let restored = engine
            .restore_to(&restored_root, &RemoteCheckpointSelector::Latest)
            .expect("real R2 restore");
        assert_eq!(restored.checkpoint_id, *manifest.checkpoint_id());
        let restored_paths = DatabasePaths::new(&restored_root);
        let restored_main = connection::open_read_only(&restored_paths.main, DatabaseKind::Main)
            .expect("restored canary main");
        assert_eq!(
            restored_main
                .query_row("SELECT count(*) FROM card_content", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("restored canary card count"),
            3
        );
        drop(restored_main);
        let restored_media = connection::open_read_only(&restored_paths.media, DatabaseKind::Media)
            .expect("restored canary media");
        assert_eq!(
            restored_media
                .query_row("SELECT bytes FROM media_blob", [], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .expect("restored canary media bytes"),
            fixture.image_bytes
        );
        drop(restored_media);
        drop(engine);

        let removed_objects = cleanup.cleanup().expect("unique canary prefix cleanup");
        fs::write(
            local_root.join("canary-report-v1.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "formatVersion": 1,
                "runId": run_id,
                "checkpointId": manifest.checkpoint_id(),
                "restoreDrill": R2CanaryCheckOutcome::Passed,
                "remoteRestore": R2CanaryCheckOutcome::Passed,
                "interruptedReplicationRetry": R2CanaryCheckOutcome::Passed,
                "idempotentMediaRetry": R2CanaryCheckOutcome::Passed,
                "removedObjectCount": removed_objects,
                "cleanupResidueCount": 0,
            }))
            .expect("canary report JSON"),
        )
        .expect("canary report");
    }

    fn rewrite_canary_target(database_path: &Path, target: &R2Target) {
        let main = Connection::open(database_path).expect("canary source database");
        main.execute(
            "UPDATE offsite_backup_config
             SET jurisdiction = ?1,
                 account_id = ?2,
                 bucket = ?3,
                 prefix = ?4",
            params![
                target.jurisdiction.as_db_str(),
                target.account_id.as_str(),
                target.bucket.as_str(),
                target.prefix.as_str(),
            ],
        )
        .expect("canary target update");
        main.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .expect("canary source checkpoint");
    }

    fn replicate_canary_source(
        database_path: &Path,
        local_root: &Path,
        target: &R2Target,
        credentials: &R2Credentials,
        store: &R2ObjectStore,
        fixture_manifest: &CheckpointManifestV1,
    ) -> CheckpointManifestV1 {
        let binary_path = PathBuf::from(required_canary_environment("DARA_LITESTREAM_PATH"));
        let binary = super::super::litestream::VerifiedLitestreamBinary::resolve_staged_for_test(
            &binary_path,
        )
        .expect("pinned canary Litestream binary");
        let runtime_root = ShortRestoreRuntimeRoot::create().expect("canary runtime root");
        let runtime =
            LitestreamRuntimePaths::new(runtime_root.path()).expect("canary runtime paths");
        let replica_path = target
            .keyspace()
            .litestream(fixture_manifest.replica_epoch_id());
        let endpoint = target.endpoint();
        let config = LitestreamConfig {
            database_path,
            runtime: &runtime,
            bucket: target.bucket.as_str(),
            replica_path: replica_path.as_str(),
            endpoint: &endpoint,
        }
        .render()
        .expect("canary Litestream config");
        runtime.write_config(&config).expect("write canary config");

        let mut interrupted = spawn_canary_litestream(binary.path(), runtime.config(), credentials);
        wait_for_canary_socket(interrupted.child_mut(), runtime.socket());
        wait_for_canary_replication_progress(
            interrupted.child_mut(),
            store,
            &target.keyspace(),
            replica_path.as_str(),
        );
        interrupted.kill_and_wait();
        remove_file_if_exists(runtime.socket()).expect("remove interrupted canary socket");

        let mut daemon = spawn_canary_litestream(binary.path(), runtime.config(), credentials);
        wait_for_canary_socket(daemon.child_mut(), runtime.socket());
        let mut sync = Command::new(binary.path());
        sync.args(["sync", "-wait", "-timeout", "60", "-json", "-socket"])
            .arg(runtime.socket())
            .arg(database_path)
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        configure_credentials_environment(&mut sync, credentials);
        let output = sync.output().expect("execute canary remote sync");
        assert!(output.status.success(), "canary remote sync failed");
        let synced = parse_sync_json(&output.stdout, true).expect("canary sync contract");
        assert_eq!(synced.database_path, database_path);
        assert_eq!(synced.replica_txid, Some(synced.txid));

        daemon.terminate_and_wait();

        let created_at = UtcTimestamp::now().expect("canary checkpoint timestamp");
        let manifest = CheckpointManifestV1::new(CheckpointManifestInput {
            backup_set_id: fixture_manifest.backup_set_id().clone(),
            replica_epoch_id: fixture_manifest.replica_epoch_id().clone(),
            checkpoint_id: fixture_manifest.checkpoint_id().clone(),
            created_at,
            dara_version: fixture_manifest.dara_version().to_owned(),
            content_revision: fixture_manifest.content_revision(),
            main_migration_head: fixture_manifest.main_migration_head(),
            litestream_path: replica_path,
            txid: synced.txid.to_string(),
            media_migration_head: fixture_manifest.media_migration_head(),
            referenced_hash_count: fixture_manifest.referenced_hash_count(),
            referenced_total_bytes: fixture_manifest.referenced_total_bytes(),
            referenced_hash_set_sha256: fixture_manifest.referenced_hash_set_sha256(),
        })
        .expect("canary checkpoint manifest");
        fs::write(
            local_root.join("checkpoint-manifest-v1.json"),
            manifest.to_json().expect("canary checkpoint manifest JSON"),
        )
        .expect("canary checkpoint evidence");
        manifest
    }

    fn spawn_canary_litestream(
        binary: &Path,
        config: &Path,
        credentials: &R2Credentials,
    ) -> R2CanaryLitestreamChild {
        let mut command = Command::new(binary);
        command
            .args(["replicate", "-config"])
            .arg(config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_credentials_environment(&mut command, credentials);
        R2CanaryLitestreamChild::new(command.spawn().expect("start canary Litestream"))
    }

    fn wait_for_canary_socket(child: &mut Child, socket: &Path) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !socket.exists() {
            assert!(
                child.try_wait().expect("canary daemon status").is_none(),
                "canary Litestream exited before creating its socket"
            );
            assert!(
                Instant::now() < deadline,
                "timed out waiting for canary Litestream socket"
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_for_canary_replication_progress(
        child: &mut Child,
        store: &R2ObjectStore,
        keyspace: &R2Keyspace,
        replica_path: &str,
    ) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                child.try_wait().expect("canary daemon status").is_none(),
                "canary Litestream exited before uploading replication data"
            );
            let page = store
                .list(&keyspace.root_prefix(), None)
                .expect("list canary replication progress");
            if page.objects.iter().any(|object| {
                object.byte_length > 0 && object.key.as_str().starts_with(replica_path)
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for canary replication progress"
            );
            thread::sleep(Duration::from_millis(500));
        }
    }

    fn put_canary_json(store: &R2ObjectStore, key: R2ObjectKey, bytes: Vec<u8>) {
        assert_eq!(
            store
                .put(PutObjectRequest {
                    key,
                    bytes,
                    content_type: ObjectContentType::Json,
                    dara_sha256: None,
                    condition: PutCondition::IfAbsent,
                })
                .expect("put canary JSON"),
            PutObjectOutcome::Stored
        );
    }

    fn canary_credentials() -> R2Credentials {
        R2Credentials::new(
            required_canary_environment("DARA_LITESTREAM_R2_ACCESS_KEY_ID"),
            required_canary_environment("DARA_LITESTREAM_R2_SECRET_ACCESS_KEY"),
        )
        .expect("canary credentials")
    }

    fn required_canary_environment(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
    }

    #[test]
    fn simulated_disk_loss_reconstructs_and_installs_a_complete_pair() {
        let fixture = rich_remote_fixture();
        let target = tempfile::tempdir().expect("restore parent");
        let data_root = target.path().join("restored");

        let report = fixture
            .engine
            .restore_to(&data_root, &RemoteCheckpointSelector::Latest)
            .expect("remote restore");

        assert_eq!(report.checkpoint_id, fixture.checkpoint_id);
        assert!(report.awaiting_application_validation);
        let paths = DatabasePaths::new(&data_root);
        let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
        let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");
        assert_eq!(
            main.query_row("SELECT count(*) FROM card_content", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("card count"),
            3
        );
        let preserved_state: (i64, i64, i64, i64, i64) = main
            .query_row(
                "SELECT
                    (SELECT count(*) FROM review_event),
                    (SELECT count(*) FROM card_content WHERE deleted_at IS NOT NULL),
                    (SELECT count(*) FROM review_card
                     WHERE suspended_at IS NOT NULL AND deleted_at IS NULL),
                    (SELECT count(*) FROM search_document),
                    (SELECT zoom_percent FROM user_preferences WHERE singleton_id = 1)",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("preserved relational state");
        assert_eq!(preserved_state, (2, 1, 1, 2, 120));
        assert_eq!(
            media
                .query_row("SELECT bytes FROM media_blob", [], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .expect("media bytes"),
            fixture.image_bytes
        );
        drop(main);
        drop(media);
        assert!(recovery::restored_offsite_takeover_required(&paths)
            .expect("off-site takeover requirement"));
        recovery::confirm_restored_launch(&paths).expect("launch confirmation");
    }

    #[test]
    fn drill_ignores_malformed_manifests_and_removes_authored_staging_data() {
        let fixture = rich_remote_fixture();
        let malformed_key = fixture
            .keyspace
            .validate_returned_key(format!(
                "{}/checkpoints/v1/{}/malformed.json",
                fixture
                    .keyspace
                    .root_prefix()
                    .as_str()
                    .trim_end_matches('/'),
                fixture
                    .engine
                    .discover_checkpoints()
                    .expect("discovery")
                    .epoch
                    .as_str()
            ))
            .expect("malformed key");
        put_json(
            fixture.store.as_ref(),
            malformed_key,
            b"{\"not\":\"a checkpoint\"}".to_vec(),
        );
        let reports = tempfile::tempdir().expect("reports");

        let report = fixture
            .engine
            .run_restore_drill(reports.path(), &RemoteCheckpointSelector::Latest)
            .expect("drill");

        assert_eq!(report.outcome, RestoreDrillOutcome::Success);
        assert!(report.matches_scope(
            fixture.manifest.backup_set_id(),
            fixture.manifest.replica_epoch_id(),
        ));
        assert!(!report.matches_scope(&BackupSetId::new(), fixture.manifest.replica_epoch_id(),));
        assert!(reports.path().join(DRILL_REPORT_FILE_NAME).is_file());
        assert_eq!(
            load_restore_drill_report(reports.path()).expect("loaded report"),
            Some(report.clone())
        );
        let entries = fs::read_dir(reports.path())
            .expect("report directory")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [std::ffi::OsString::from(DRILL_REPORT_FILE_NAME)]);
        let catalog = fixture.engine.list_checkpoints().expect("catalog");
        assert_eq!(catalog.checkpoints.len(), 1);
        assert_eq!(catalog.malformed_objects_ignored, 1);
    }

    #[test]
    fn missing_remote_media_fails_the_drill_before_installation() {
        let fixture = rich_remote_fixture();
        fixture
            .store
            .delete(&fixture.media_key)
            .expect("remove remote media");
        let reports = tempfile::tempdir().expect("reports");

        let report = fixture
            .engine
            .run_restore_drill(reports.path(), &RemoteCheckpointSelector::Latest)
            .expect("failed drill report");

        assert_eq!(report.outcome, RestoreDrillOutcome::Failed);
        assert_eq!(report.error_code, Some(BackupErrorCode::RemoteMediaMissing));
        assert!(report
            .validation_stages
            .contains(&RestoreValidationStage::RelationalValidated));
        assert!(!report
            .validation_stages
            .contains(&RestoreValidationStage::PairValidated));
        assert!(reports.path().join(DRILL_REPORT_FILE_NAME).is_file());
    }

    #[test]
    fn failed_scoped_drill_before_discovery_keeps_expected_scope() {
        let fixture = rich_remote_fixture();
        let identity_key = fixture.keyspace.identity();
        fixture
            .store
            .delete(&identity_key)
            .expect("remove remote identity");
        put_json(
            fixture.store.as_ref(),
            identity_key,
            b"{\"not\":\"an identity manifest\"}".to_vec(),
        );
        let reports = tempfile::tempdir().expect("reports");

        let report = fixture
            .engine
            .run_scoped_restore_drill(
                reports.path(),
                &RemoteCheckpointSelector::Latest,
                fixture.manifest.backup_set_id(),
                fixture.manifest.replica_epoch_id(),
            )
            .expect("failed drill report");

        assert_eq!(report.outcome, RestoreDrillOutcome::Failed);
        assert_eq!(report.error_code, Some(BackupErrorCode::MalformedManifest));
        assert!(report.matches_scope(
            fixture.manifest.backup_set_id(),
            fixture.manifest.replica_epoch_id(),
        ));
        assert_eq!(
            load_restore_drill_report(reports.path()).expect("load failed report"),
            Some(report)
        );
    }

    #[test]
    fn scoped_drill_rejects_a_different_remote_epoch() {
        let fixture = rich_remote_fixture();
        let expected_epoch = ReplicaEpochId::new();
        let reports = tempfile::tempdir().expect("reports");

        let report = fixture
            .engine
            .run_scoped_restore_drill(
                reports.path(),
                &RemoteCheckpointSelector::Latest,
                fixture.manifest.backup_set_id(),
                &expected_epoch,
            )
            .expect("failed drill report");

        assert_eq!(report.outcome, RestoreDrillOutcome::Failed);
        assert_eq!(report.error_code, Some(BackupErrorCode::OwnerMismatch));
        assert!(report.matches_scope(fixture.manifest.backup_set_id(), &expected_epoch));
        assert!(report.validation_stages.is_empty());
    }

    #[test]
    fn latest_selection_falls_back_when_the_newest_exact_txid_expired() {
        let fixture = rich_remote_fixture();
        let newer_id = CheckpointId::new();
        let newer_created_at =
            UtcTimestamp::parse("2099-07-28T23:59:59Z").expect("newer timestamp");
        let newer = CheckpointManifestV1::new(CheckpointManifestInput {
            backup_set_id: fixture.manifest.backup_set_id().clone(),
            replica_epoch_id: fixture.epoch.clone(),
            checkpoint_id: newer_id.clone(),
            created_at: newer_created_at.clone(),
            dara_version: fixture.manifest.dara_version().into(),
            content_revision: fixture.manifest.content_revision(),
            main_migration_head: fixture.manifest.main_migration_head(),
            litestream_path: fixture.keyspace.litestream(&fixture.epoch),
            txid: "0000000000000043".into(),
            media_migration_head: fixture.manifest.media_migration_head(),
            referenced_hash_count: fixture.manifest.referenced_hash_count(),
            referenced_total_bytes: fixture.manifest.referenced_total_bytes(),
            referenced_hash_set_sha256: fixture.manifest.referenced_hash_set_sha256(),
        })
        .expect("newer checkpoint");
        put_json(
            fixture.store.as_ref(),
            fixture
                .keyspace
                .checkpoint(&fixture.epoch, &newer_id, &newer_created_at)
                .expect("newer key"),
            newer.to_json().expect("newer JSON"),
        );
        fixture
            .relational
            .unavailable
            .lock()
            .expect("unavailable TXIDs")
            .insert(0x43);

        let selected = fixture
            .engine
            .inspect_checkpoint(&RemoteCheckpointSelector::Latest)
            .expect("fallback checkpoint");

        assert_eq!(selected.checkpoint_id, fixture.checkpoint_id);
        let catalog = fixture.engine.list_checkpoints().expect("catalog");
        assert_eq!(catalog.checkpoints.len(), 2);
        assert_eq!(
            catalog.checkpoints[0].availability,
            RemoteCheckpointAvailability::ExactTxidUnavailable
        );
        assert_eq!(
            catalog.checkpoints[1].availability,
            RemoteCheckpointAvailability::Restorable
        );
    }

    #[test]
    fn recovery_catalog_validates_only_a_bounded_recent_set() {
        let fixture = rich_remote_fixture();
        for index in 0..MAX_CHECKPOINT_MANIFEST_CANDIDATES + 25 {
            let checkpoint_id = CheckpointId::new();
            let minute = index / 60;
            let second = index % 60;
            let created_at = UtcTimestamp::parse(format!("2099-07-29T00:{minute:02}:{second:02}Z"))
                .expect("checkpoint timestamp");
            let manifest = CheckpointManifestV1::new(CheckpointManifestInput {
                backup_set_id: fixture.manifest.backup_set_id().clone(),
                replica_epoch_id: fixture.epoch.clone(),
                checkpoint_id: checkpoint_id.clone(),
                created_at: created_at.clone(),
                dara_version: fixture.manifest.dara_version().into(),
                content_revision: fixture.manifest.content_revision(),
                main_migration_head: fixture.manifest.main_migration_head(),
                litestream_path: fixture.keyspace.litestream(&fixture.epoch),
                txid: format!("{:016x}", 0x100 + index),
                media_migration_head: fixture.manifest.media_migration_head(),
                referenced_hash_count: fixture.manifest.referenced_hash_count(),
                referenced_total_bytes: fixture.manifest.referenced_total_bytes(),
                referenced_hash_set_sha256: fixture.manifest.referenced_hash_set_sha256(),
            })
            .expect("checkpoint");
            put_json(
                fixture.store.as_ref(),
                fixture
                    .keyspace
                    .checkpoint(&fixture.epoch, &checkpoint_id, &created_at)
                    .expect("checkpoint key"),
                manifest.to_json().expect("checkpoint JSON"),
            );
        }

        let catalog = fixture.engine.list_checkpoints().expect("catalog");

        assert_eq!(catalog.checkpoints.len(), MAX_RECOVERY_CATALOG_CHECKPOINTS);
        assert_eq!(
            fixture.relational.dry_run_count.load(Ordering::Relaxed),
            MAX_RECOVERY_CATALOG_CHECKPOINTS
        );
        assert_eq!(
            fixture
                .store
                .operations()
                .into_iter()
                .filter(|operation| *operation == ObjectOperation::Get)
                .count(),
            MAX_CHECKPOINT_MANIFEST_CANDIDATES + 2
        );
        assert_eq!(
            catalog
                .checkpoints
                .first()
                .expect("latest checkpoint")
                .created_at,
            "2099-07-29T00:02:04Z"
        );
    }

    #[test]
    fn corrupt_relational_and_future_schema_fail_before_installation() {
        let corrupt = rich_remote_fixture();
        fs::write(&corrupt.relational.main_source, b"not a SQLite database")
            .expect("corrupt relational source");
        let corrupt_target = tempfile::tempdir().expect("corrupt target");
        assert_eq!(
            corrupt.engine.restore_to(
                &corrupt_target.path().join("data"),
                &RemoteCheckpointSelector::Latest,
            ),
            Err(BackupErrorCode::RestoreValidationFailed)
        );
        assert!(!corrupt_target.path().join("data/dara.sqlite3").exists());
        assert!(!corrupt_target.path().join("data/media.sqlite3").exists());

        let future = rich_remote_fixture();
        let connection = Connection::open(&future.relational.main_source).expect("future main");
        connection
            .pragma_update(None, "journal_mode", "DELETE")
            .expect("delete journal");
        connection
            .execute(
                "INSERT INTO refinery_schema_history(version, name, applied_on, checksum)
                 VALUES (999, 'future_schema', '2026-07-28T00:00:00Z', '0')",
                [],
            )
            .expect("future migration");
        drop(connection);
        let future_target = tempfile::tempdir().expect("future target");
        assert_eq!(
            future.engine.restore_to(
                &future_target.path().join("data"),
                &RemoteCheckpointSelector::Latest,
            ),
            Err(BackupErrorCode::RestoreValidationFailed)
        );
        assert!(!future_target.path().join("data/dara.sqlite3").exists());
        assert!(!future_target.path().join("data/media.sqlite3").exists());
    }

    #[test]
    fn corrupt_remote_media_fails_hash_validation() {
        let fixture = rich_remote_fixture();
        let expected_sha = fixture
            .store
            .get(&fixture.media_key)
            .expect("expected media")
            .metadata
            .dara_sha256
            .expect("media SHA");
        fixture
            .store
            .delete(&fixture.media_key)
            .expect("delete media");
        fixture
            .store
            .put(PutObjectRequest {
                key: fixture.media_key.clone(),
                bytes: b"corrupt replacement".to_vec(),
                content_type: ObjectContentType::Webp,
                dara_sha256: Some(expected_sha),
                condition: PutCondition::IfAbsent,
            })
            .expect("corrupt media");
        let reports = tempfile::tempdir().expect("reports");

        let report = fixture
            .engine
            .run_restore_drill(reports.path(), &RemoteCheckpointSelector::Latest)
            .expect("failed drill report");

        assert_eq!(report.outcome, RestoreDrillOutcome::Failed);
        assert_eq!(report.error_code, Some(BackupErrorCode::RemoteMediaCorrupt));
    }

    #[test]
    fn restore_tasks_reap_only_stale_validated_directories() {
        let base = tempfile::tempdir().expect("task base");
        let stale_name = format!(".dara-restore-drill-{}", Uuid::now_v7());
        let stale = base.path().join(&stale_name);
        create_private_directory(&stale).expect("stale task");
        fs::write(stale.join("authored.sqlite3"), b"stale").expect("stale data");
        File::create(stale.join(".dara-restore-task.lock")).expect("stale task lock");
        let unrelated = base.path().join(".dara-restore-drill-not-a-uuid");
        create_private_directory(&unrelated).expect("unrelated directory");

        let first =
            RestoreTask::create(base.path(), ".dara-restore-drill-").expect("first active task");
        assert!(!stale.exists());
        assert!(unrelated.exists());
        let second =
            RestoreTask::create(base.path(), ".dara-restore-drill-").expect("second active task");
        assert!(first.path().exists());
        assert!(second.path().exists());
        let first_path = first.path().to_owned();
        let second_path = second.path().to_owned();
        drop(first);
        drop(second);
        assert!(!first_path.exists());
        assert!(!second_path.exists());
        assert!(unrelated.exists());
    }

    #[cfg(unix)]
    #[test]
    fn system_restore_keeps_litestream_runtime_outside_long_task_roots() {
        use std::os::unix::fs::PermissionsExt;

        let base = tempfile::tempdir().expect("restore base");
        let task_root = base.path().join("x".repeat(160));
        create_private_directory(&task_root).expect("long restore task");
        assert!(LitestreamRuntimePaths::new(&task_root).is_err());
        let restore = SystemRelationalRestore {
            binary: PathBuf::from("/not-used/litestream"),
            target: target(),
            credentials: R2Credentials::new("1".repeat(32), "2".repeat(64)).expect("credentials"),
        };

        let prepared = restore
            .prepare_config(&task_root, &ReplicaEpochId::new())
            .expect("short restore runtime");
        assert!(!prepared.runtime.directory().starts_with(&task_root));
        assert!(prepared.database_path.starts_with(&task_root));
        let runtime_root = prepared._root.path().to_owned();
        assert!(runtime_root.exists());
        assert_eq!(
            fs::metadata(&runtime_root)
                .expect("runtime root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

        drop(prepared);
        assert!(!runtime_root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_restore_process_is_killed_and_reaped() {
        let mut child = Command::new("/bin/sleep")
            .arg("10")
            .stdout(Stdio::piped())
            .spawn()
            .expect("sleep child");
        let pid = child.id();
        let stdout = child.stdout.take().expect("stdout");

        assert_eq!(
            collect_restore_output(&mut child, stdout, Duration::from_millis(20)),
            Err(BackupErrorCode::NetworkTimeout)
        );
        assert!(child.try_wait().expect("reaped child").is_some());
        let result = unsafe { libc::kill(pid as i32, 0) };
        assert_eq!(result, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
