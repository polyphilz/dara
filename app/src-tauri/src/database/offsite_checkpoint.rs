use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::backup::{
    domain::{
        BackupErrorCode, BackupSetId, CheckpointId, CheckpointPhase, ContentSha256, ReplicaEpochId,
    },
    litestream::LitestreamTxid,
};

use super::{migrations, now_millis, DatabaseError, Result};

#[derive(Clone, Debug)]
pub(crate) struct PrepareOffsiteCheckpointInput {
    pub(crate) checkpoint_id: CheckpointId,
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) replica_epoch_id: ReplicaEpochId,
    pub(crate) created_at: i64,
    pub(crate) dara_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointMediaReference {
    pub(crate) sha256: ContentSha256,
    pub(crate) byte_length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedOffsiteCheckpoint {
    pub(crate) checkpoint_id: CheckpointId,
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) replica_epoch_id: ReplicaEpochId,
    pub(crate) created_at: i64,
    pub(crate) dara_version: String,
    pub(crate) config_revision: i64,
    pub(crate) content_revision: u64,
    pub(crate) main_migration_head: u32,
    pub(crate) media_migration_head: u32,
    pub(crate) referenced_media: Vec<CheckpointMediaReference>,
    pub(crate) referenced_total_bytes: u64,
    pub(crate) referenced_hash_set_sha256: ContentSha256,
    pub(crate) litestream_txid: LitestreamTxid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublishedOffsiteCheckpoint {
    pub(crate) checkpoint_id: CheckpointId,
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) replica_epoch_id: ReplicaEpochId,
    pub(crate) config_revision: i64,
    pub(crate) content_revision: u64,
    pub(crate) litestream_txid: LitestreamTxid,
    pub(crate) manifest_object_key: String,
    pub(crate) created_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OffsiteCheckpointScheduleState {
    pub(crate) content_revision: u64,
    pub(crate) last_published: Option<PublishedOffsiteCheckpoint>,
}

pub(crate) trait LocalCheckpointSync: Send + Sync {
    fn sync_local(&self) -> std::result::Result<LitestreamTxid, BackupErrorCode>;
}

pub(super) fn prepare_and_fence(
    main: &mut Connection,
    media: &Connection,
    input: PrepareOffsiteCheckpointInput,
    local_sync: Arc<dyn LocalCheckpointSync>,
) -> Result<PreparedOffsiteCheckpoint> {
    if input.created_at < 0
        || input.dara_version.is_empty()
        || input.dara_version.len() > 64
        || input.dara_version.chars().any(char::is_control)
    {
        return Err(invalid_checkpoint("checkpoint metadata is invalid"));
    }
    let transaction = main.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let config_revision = load_active_config_revision(&transaction, &input)?;
    let referenced_media = load_verified_references(&transaction, &input.backup_set_id)?;
    let referenced_total_bytes = referenced_media
        .iter()
        .try_fold(0_u64, |total, reference| {
            total
                .checked_add(reference.byte_length)
                .ok_or_else(|| invalid_checkpoint("referenced media byte total overflowed"))
        })?;
    let referenced_hash_set_sha256 = hash_reference_set(&referenced_media);
    let content_revision = load_content_revision(&transaction)?;
    let heads = migrations::expected_heads();
    let main_migration_head = positive_head(heads.main, "main")?;
    let media_migration_head = positive_head(heads.media, "media")?;
    validate_recorded_head(&transaction, main_migration_head, "main")?;
    validate_recorded_head(media, media_migration_head, "media")?;
    let stored_content_revision = stored_i64(content_revision, "checkpoint content revision")?;
    let referenced_hash_count = i64::try_from(referenced_media.len()).map_err(|_| {
        invalid_checkpoint("checkpoint referenced media count exceeded SQLite limits")
    })?;
    let stored_referenced_total_bytes = stored_i64(
        referenced_total_bytes,
        "checkpoint referenced media byte total",
    )?;

    transaction.execute(
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
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            NULL, NULL, NULL, ?7
         )",
        params![
            input.checkpoint_id.as_str(),
            input.backup_set_id.as_str(),
            input.replica_epoch_id.as_str(),
            CheckpointPhase::Prepared.as_db_str(),
            config_revision,
            stored_content_revision,
            input.created_at,
            input.dara_version,
            main_migration_head,
            media_migration_head,
            referenced_hash_count,
            stored_referenced_total_bytes,
            referenced_hash_set_sha256.as_bytes().as_slice(),
        ],
    )?;
    transaction.commit()?;

    // The writer loop remains inside this message until the local-only control call
    // returns. No later main transaction can pass the PREPARED row before this TXID.
    let litestream_txid = local_sync
        .sync_local()
        .map_err(DatabaseError::OffsiteCheckpointFence)?;
    Ok(PreparedOffsiteCheckpoint {
        checkpoint_id: input.checkpoint_id,
        backup_set_id: input.backup_set_id,
        replica_epoch_id: input.replica_epoch_id,
        created_at: input.created_at,
        dara_version: input.dara_version,
        config_revision,
        content_revision,
        main_migration_head,
        media_migration_head,
        referenced_media,
        referenced_total_bytes,
        referenced_hash_set_sha256,
        litestream_txid,
    })
}

pub(super) fn mark_fenced(
    connection: &mut Connection,
    checkpoint_id: &CheckpointId,
    txid: LitestreamTxid,
) -> Result<()> {
    transition(
        connection,
        checkpoint_id,
        CheckpointPhase::Prepared,
        CheckpointPhase::Fenced,
        Some(txid),
        None,
        None,
    )
}

pub(super) fn mark_replicated(
    connection: &mut Connection,
    checkpoint_id: &CheckpointId,
) -> Result<()> {
    transition(
        connection,
        checkpoint_id,
        CheckpointPhase::Fenced,
        CheckpointPhase::Replicated,
        None,
        None,
        None,
    )
}

pub(super) fn mark_published(
    connection: &mut Connection,
    checkpoint_id: &CheckpointId,
    manifest_object_key: &str,
) -> Result<()> {
    if manifest_object_key.is_empty() || manifest_object_key.len() > 1_024 {
        return Err(invalid_checkpoint("checkpoint manifest key is invalid"));
    }
    let now = now_millis()?;
    let changed = connection.execute(
        "UPDATE offsite_backup_checkpoint
         SET phase = ?1,
             manifest_object_key = ?2,
             publication_sequence = (
                 SELECT coalesce(max(publication_sequence), 0) + 1
                 FROM offsite_backup_checkpoint
             ),
             last_error_code = NULL,
             updated_at = max(updated_at, ?3)
         WHERE checkpoint_id = ?4 AND phase = ?5",
        params![
            CheckpointPhase::Published.as_db_str(),
            manifest_object_key,
            now,
            checkpoint_id.as_str(),
            CheckpointPhase::Replicated.as_db_str(),
        ],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleOffsiteCheckpoint);
    }
    Ok(())
}

pub(super) fn mark_failed(
    connection: &mut Connection,
    checkpoint_id: &CheckpointId,
    error_code: BackupErrorCode,
) -> Result<()> {
    let now = now_millis()?;
    let changed = connection.execute(
        "UPDATE offsite_backup_checkpoint
         SET phase = ?1, last_error_code = ?2, updated_at = max(updated_at, ?3)
         WHERE checkpoint_id = ?4
           AND phase IN (?5, ?6, ?7)",
        params![
            CheckpointPhase::Failed.as_db_str(),
            error_code.as_db_str(),
            now,
            checkpoint_id.as_str(),
            CheckpointPhase::Prepared.as_db_str(),
            CheckpointPhase::Fenced.as_db_str(),
            CheckpointPhase::Replicated.as_db_str(),
        ],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleOffsiteCheckpoint);
    }
    Ok(())
}

pub(super) fn fail_incomplete(
    connection: &mut Connection,
    error_code: BackupErrorCode,
) -> Result<u64> {
    let now = now_millis()?;
    let changed = connection.execute(
        "UPDATE offsite_backup_checkpoint
         SET phase = ?1, last_error_code = ?2, updated_at = max(updated_at, ?3)
         WHERE phase IN (?4, ?5, ?6)",
        params![
            CheckpointPhase::Failed.as_db_str(),
            error_code.as_db_str(),
            now,
            CheckpointPhase::Prepared.as_db_str(),
            CheckpointPhase::Fenced.as_db_str(),
            CheckpointPhase::Replicated.as_db_str(),
        ],
    )?;
    Ok(changed as u64)
}

pub(super) fn schedule_state(connection: &Connection) -> Result<OffsiteCheckpointScheduleState> {
    let content_revision = load_content_revision(connection)?;
    let stored = connection
        .query_row(
            "SELECT
                checkpoint_id,
                backup_set_id,
                replica_epoch_id,
                config_revision,
                content_revision,
                litestream_txid,
                manifest_object_key,
                created_at
             FROM offsite_backup_checkpoint
             WHERE phase = ?1
             ORDER BY coalesce(publication_sequence, 0) DESC, checkpoint_id DESC
             LIMIT 1",
            [CheckpointPhase::Published.as_db_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    let last_published = stored
        .map(
            |(
                checkpoint_id,
                backup_set_id,
                replica_epoch_id,
                config_revision,
                revision,
                txid,
                manifest_object_key,
                created_at,
            )| {
                Ok::<PublishedOffsiteCheckpoint, DatabaseError>(PublishedOffsiteCheckpoint {
                    checkpoint_id: CheckpointId::parse(checkpoint_id).map_err(invalid_domain)?,
                    backup_set_id: BackupSetId::parse(backup_set_id).map_err(invalid_domain)?,
                    replica_epoch_id: ReplicaEpochId::parse(replica_epoch_id)
                        .map_err(invalid_domain)?,
                    config_revision,
                    content_revision: non_negative_u64(revision, "checkpoint content revision")?,
                    litestream_txid: txid
                        .parse()
                        .map_err(|_| invalid_checkpoint("checkpoint TXID is invalid"))?,
                    manifest_object_key,
                    created_at,
                })
            },
        )
        .transpose()?;
    Ok(OffsiteCheckpointScheduleState {
        content_revision,
        last_published,
    })
}

fn transition(
    connection: &mut Connection,
    checkpoint_id: &CheckpointId,
    expected: CheckpointPhase,
    next: CheckpointPhase,
    txid: Option<LitestreamTxid>,
    manifest_object_key: Option<&str>,
    error_code: Option<BackupErrorCode>,
) -> Result<()> {
    let now = now_millis()?;
    let txid = txid.map(|value| value.to_string());
    let changed = connection.execute(
        "UPDATE offsite_backup_checkpoint
         SET phase = ?1,
             litestream_txid = coalesce(?2, litestream_txid),
             manifest_object_key = coalesce(?3, manifest_object_key),
             last_error_code = ?4,
             updated_at = max(updated_at, ?5)
         WHERE checkpoint_id = ?6 AND phase = ?7",
        params![
            next.as_db_str(),
            txid,
            manifest_object_key,
            error_code.map(BackupErrorCode::as_db_str),
            now,
            checkpoint_id.as_str(),
            expected.as_db_str(),
        ],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleOffsiteCheckpoint);
    }
    Ok(())
}

fn load_active_config_revision(
    transaction: &Transaction<'_>,
    input: &PrepareOffsiteCheckpointInput,
) -> Result<i64> {
    let revision = transaction
        .query_row(
            "SELECT revision
         FROM offsite_backup_config
         WHERE singleton_id = 1
           AND enabled = 1
           AND backup_set_id = ?1
           AND replica_epoch_id = ?2",
            params![
                input.backup_set_id.as_str(),
                input.replica_epoch_id.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    match revision {
        Some(revision) if revision > 0 => Ok(revision),
        _ => Err(invalid_checkpoint(
            "checkpoint target is not the active enabled backup",
        )),
    }
}

fn load_verified_references(
    transaction: &Transaction<'_>,
    backup_set_id: &BackupSetId,
) -> Result<Vec<CheckpointMediaReference>> {
    let mut statement = transaction.prepare(
        "SELECT referenced.sha256, object.byte_length, object.state
         FROM (SELECT DISTINCT sha256 FROM image) AS referenced
         LEFT JOIN offsite_media_object AS object
           ON object.backup_set_id = ?1
          AND object.sha256 = referenced.sha256
         ORDER BY referenced.sha256",
    )?;
    let rows = statement.query_map([backup_set_id.as_str()], |row| {
        Ok((
            row.get::<_, Vec<u8>>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut references = Vec::new();
    for row in rows {
        let (sha256, byte_length, state) = row?;
        let Some((byte_length, state)) = byte_length.zip(state) else {
            return Err(DatabaseError::OffsiteCheckpointMediaIncomplete);
        };
        if state != crate::backup::domain::OffsiteMediaState::Verified.as_db_str() {
            return Err(DatabaseError::OffsiteCheckpointMediaIncomplete);
        }
        references.push(CheckpointMediaReference {
            sha256: ContentSha256::from_slice(&sha256).map_err(invalid_domain)?,
            byte_length: positive_u64(byte_length, "referenced media byte length")?,
        });
    }
    Ok(references)
}

fn hash_reference_set(references: &[CheckpointMediaReference]) -> ContentSha256 {
    let mut digest = Sha256::new();
    for reference in references {
        digest.update(reference.sha256.as_bytes());
    }
    ContentSha256::from_bytes(digest.finalize().into())
}

fn load_content_revision(connection: &Connection) -> Result<u64> {
    let revision = connection.query_row(
        "SELECT revision FROM offsite_backup_content_clock WHERE singleton_id = 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    non_negative_u64(revision, "off-site backup content revision")
}

fn validate_recorded_head(connection: &Connection, expected: u32, label: &str) -> Result<()> {
    let actual = connection.query_row(
        "SELECT max(version) FROM refinery_schema_history",
        [],
        |row| row.get::<_, Option<i32>>(0),
    )?;
    if actual.and_then(|value| u32::try_from(value).ok()) != Some(expected) {
        return Err(invalid_checkpoint(&format!(
            "{label} migration head changed during checkpoint"
        )));
    }
    Ok(())
}

fn positive_head(value: Option<i32>, label: &str) -> Result<u32> {
    value
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid_checkpoint(&format!("{label} migration head is invalid")))
}

fn non_negative_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid_checkpoint(&format!("{field} is negative")))
}

fn positive_u64(value: i64, field: &str) -> Result<u64> {
    non_negative_u64(value, field).and_then(|value| {
        (value > 0)
            .then_some(value)
            .ok_or_else(|| invalid_checkpoint(field))
    })
}

fn stored_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid_checkpoint(&format!("{field} exceeded SQLite limits")))
}

fn invalid_domain(error: impl std::fmt::Display) -> DatabaseError {
    invalid_checkpoint(&error.to_string())
}

fn invalid_checkpoint(reason: &str) -> DatabaseError {
    DatabaseError::InvalidOffsiteCheckpoint(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            mpsc::{self, Receiver},
            Mutex,
        },
        thread,
        time::Duration,
    };

    use rusqlite::OpenFlags;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backup::domain::{
            BackupProvider, R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix, R2Target,
        },
        database::{
            initialize, settings::Appearance, CanonicalImage, Database, DatabasePaths,
            InitializationOptions, OffsiteBackupConfig, SaveOffsiteBackupConfigInput,
            SetAppearanceInput,
        },
    };

    fn test_txid() -> LitestreamTxid {
        LitestreamTxid::from_local(42)
    }

    struct InspectingSync {
        main_path: PathBuf,
        checkpoint_id: CheckpointId,
        entered: mpsc::SyncSender<()>,
        release: Mutex<Receiver<()>>,
    }

    impl LocalCheckpointSync for InspectingSync {
        fn sync_local(&self) -> std::result::Result<LitestreamTxid, BackupErrorCode> {
            let connection = Connection::open_with_flags(
                &self.main_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
            let phase = connection
                .query_row(
                    "SELECT phase FROM offsite_backup_checkpoint WHERE checkpoint_id = ?1",
                    [self.checkpoint_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
            if phase != CheckpointPhase::Prepared.as_db_str() {
                return Err(BackupErrorCode::RestoreValidationFailed);
            }
            self.entered
                .send(())
                .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
            self.release
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .recv()
                .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
            Ok(test_txid())
        }
    }

    struct ImmediateSync;

    impl LocalCheckpointSync for ImmediateSync {
        fn sync_local(&self) -> std::result::Result<LitestreamTxid, BackupErrorCode> {
            Ok(test_txid())
        }
    }

    struct FailingSync;

    impl LocalCheckpointSync for FailingSync {
        fn sync_local(&self) -> std::result::Result<LitestreamTxid, BackupErrorCode> {
            Err(BackupErrorCode::FenceTimeout)
        }
    }

    fn enabled_database() -> (TempDir, Database, OffsiteBackupConfig) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path().join("data"));
        let database = initialize(
            paths,
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database");
        let config = database
            .client()
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: 0,
                backup_set_id: BackupSetId::new(),
                replica_epoch_id: ReplicaEpochId::new(),
                enabled: true,
                target: R2Target {
                    account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                        .expect("account ID"),
                    jurisdiction: R2Jurisdiction::Default,
                    bucket: R2BucketName::parse("dara-test").expect("bucket"),
                    prefix: R2Prefix::parse("dara/checkpoint-test").expect("prefix"),
                },
            })
            .expect("save config");
        assert_eq!(config.provider, BackupProvider::R2);
        (directory, database, config)
    }

    fn prepare_input(
        checkpoint_id: CheckpointId,
        config: &OffsiteBackupConfig,
        created_at: i64,
    ) -> PrepareOffsiteCheckpointInput {
        PrepareOffsiteCheckpointInput {
            checkpoint_id,
            backup_set_id: config.backup_set_id.clone(),
            replica_epoch_id: config.replica_epoch_id.clone(),
            created_at,
            dara_version: "test".into(),
        }
    }

    fn publish_checkpoint(
        database: &Database,
        config: &OffsiteBackupConfig,
        created_at: i64,
        manifest_object_key: &str,
    ) -> CheckpointId {
        let client = database.client();
        let checkpoint_id = CheckpointId::new();
        let prepared = client
            .prepare_offsite_checkpoint(
                prepare_input(checkpoint_id.clone(), config, created_at),
                Arc::new(ImmediateSync),
            )
            .expect("prepare checkpoint");
        client
            .mark_offsite_checkpoint_fenced(checkpoint_id.clone(), prepared.litestream_txid)
            .expect("mark fenced");
        client
            .mark_offsite_checkpoint_replicated(checkpoint_id.clone())
            .expect("mark replicated");
        client
            .mark_offsite_checkpoint_published(
                checkpoint_id.clone(),
                manifest_object_key.to_owned(),
            )
            .expect("mark published");
        checkpoint_id
    }

    #[test]
    fn prepared_row_commits_before_local_sync_and_next_writer_waits_behind_fence() {
        let (_directory, database, config) = enabled_database();
        let checkpoint_id = CheckpointId::new();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let sync = Arc::new(InspectingSync {
            main_path: database.paths().main.clone(),
            checkpoint_id: checkpoint_id.clone(),
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });
        let checkpoint_client = database.client();
        let config_for_thread = config.clone();
        let prepare_thread = thread::spawn(move || {
            checkpoint_client.prepare_offsite_checkpoint(
                prepare_input(checkpoint_id, &config_for_thread, 1),
                sync,
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("local sync entered");

        let (read_started_tx, read_started_rx) = mpsc::sync_channel(1);
        let (read_done_tx, read_done_rx) = mpsc::sync_channel(1);
        let read_client = database.client();
        let read_thread = thread::spawn(move || {
            read_started_tx.send(()).expect("read started");
            let result = read_client.load_settings();
            read_done_tx.send(result).expect("read result");
        });
        read_started_rx.recv().expect("read queued");
        assert!(matches!(
            read_done_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        release_tx.send(()).expect("release local sync");
        let prepared = prepare_thread
            .join()
            .expect("prepare thread")
            .expect("prepared checkpoint");
        assert_eq!(prepared.litestream_txid, test_txid());
        read_done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("read unblocked")
            .expect("settings");
        read_thread.join().expect("read thread");
    }

    #[test]
    fn failed_attempt_preserves_the_last_published_checkpoint() {
        let (_directory, database, config) = enabled_database();
        let client = database.client();
        let published_id = CheckpointId::new();
        let published = client
            .prepare_offsite_checkpoint(
                prepare_input(published_id.clone(), &config, 1),
                Arc::new(ImmediateSync),
            )
            .expect("prepare published checkpoint");
        client
            .mark_offsite_checkpoint_fenced(published_id.clone(), published.litestream_txid)
            .expect("mark fenced");
        client
            .mark_offsite_checkpoint_replicated(published_id.clone())
            .expect("mark replicated");
        client
            .mark_offsite_checkpoint_published(
                published_id.clone(),
                "dara/checkpoint-test/checkpoints/v1/test.json".into(),
            )
            .expect("mark published");

        let failed_id = CheckpointId::new();
        let error = client
            .prepare_offsite_checkpoint(
                prepare_input(failed_id.clone(), &config, 2),
                Arc::new(FailingSync),
            )
            .expect_err("fence must fail");
        assert!(matches!(
            error,
            DatabaseError::OffsiteCheckpointFence(BackupErrorCode::FenceTimeout)
        ));
        client
            .mark_offsite_checkpoint_failed(failed_id, BackupErrorCode::FenceTimeout)
            .expect("mark failed");

        let state = client
            .load_offsite_checkpoint_schedule_state()
            .expect("schedule state");
        assert_eq!(
            state.last_published.expect("last published").checkpoint_id,
            published_id
        );
    }

    #[test]
    fn publication_sequence_selects_the_latest_checkpoint_after_clock_rollback() {
        let (_directory, database, config) = enabled_database();
        let first = publish_checkpoint(
            &database,
            &config,
            200,
            "dara/checkpoint-test/checkpoints/v1/first.json",
        );
        let second = publish_checkpoint(
            &database,
            &config,
            100,
            "dara/checkpoint-test/checkpoints/v1/second.json",
        );

        let state = database
            .client()
            .load_offsite_checkpoint_schedule_state()
            .expect("schedule state");
        assert_ne!(first, second);
        assert_eq!(
            state.last_published.expect("last published").checkpoint_id,
            second
        );
    }

    #[test]
    fn content_clock_advances_for_authored_state_but_not_backup_bookkeeping() {
        let (_directory, database, config) = enabled_database();
        let client = database.client();
        let initial = client
            .load_offsite_checkpoint_schedule_state()
            .expect("initial clock")
            .content_revision;
        assert_eq!(initial, 0);

        let settings = client.load_settings().expect("settings");
        client
            .set_appearance(SetAppearanceInput {
                expected_revision: settings.revision,
                appearance: Appearance::Dark,
            })
            .expect("authored setting");
        let authored = client
            .load_offsite_checkpoint_schedule_state()
            .expect("authored clock")
            .content_revision;
        assert!(authored > initial);

        let checkpoint_id = CheckpointId::new();
        let prepared = client
            .prepare_offsite_checkpoint(
                prepare_input(checkpoint_id.clone(), &config, 3),
                Arc::new(ImmediateSync),
            )
            .expect("prepare");
        client
            .mark_offsite_checkpoint_fenced(checkpoint_id.clone(), prepared.litestream_txid)
            .expect("fenced");
        client
            .mark_offsite_checkpoint_replicated(checkpoint_id.clone())
            .expect("replicated");
        client
            .mark_offsite_checkpoint_published(
                checkpoint_id,
                "dara/checkpoint-test/checkpoints/v1/bookkeeping.json".into(),
            )
            .expect("published");
        assert_eq!(
            client
                .load_offsite_checkpoint_schedule_state()
                .expect("bookkeeping clock")
                .content_revision,
            authored
        );
    }

    #[test]
    fn writer_fence_rechecks_media_and_rejects_a_new_unverified_image() {
        let (_directory, database, config) = enabled_database();
        let client = database.client();
        client
            .ingest_image(
                CanonicalImage {
                    bytes: b"new-image-before-fence".to_vec(),
                    natural_width: 10,
                    natural_height: 10,
                },
                "01980c8e-6c00-7000-8000-000000000901".into(),
            )
            .expect("image");
        assert!(matches!(
            client.prepare_offsite_checkpoint(
                prepare_input(CheckpointId::new(), &config, 4),
                Arc::new(ImmediateSync),
            ),
            Err(DatabaseError::OffsiteCheckpointMediaIncomplete)
        ));
    }
}
