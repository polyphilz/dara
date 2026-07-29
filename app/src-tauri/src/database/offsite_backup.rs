use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::backup::domain::{
    BackupProvider, BackupSetId, R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix, R2Target,
    ReplicaEpochId,
};

use super::{now_millis, offsite_media, DatabaseError, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OffsiteBackupConfig {
    pub(crate) revision: i64,
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) replica_epoch_id: ReplicaEpochId,
    pub(crate) enabled: bool,
    pub(crate) provider: BackupProvider,
    pub(crate) target: R2Target,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct SaveOffsiteBackupConfigInput {
    pub(crate) expected_revision: i64,
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) replica_epoch_id: ReplicaEpochId,
    pub(crate) enabled: bool,
    pub(crate) target: R2Target,
}

pub(super) fn load(connection: &Connection) -> Result<Option<OffsiteBackupConfig>> {
    let stored = connection
        .query_row(
            "SELECT
                revision,
                backup_set_id,
                replica_epoch_id,
                enabled,
                provider,
                jurisdiction,
                account_id,
                bucket,
                prefix,
                created_at,
                updated_at
             FROM offsite_backup_config
             WHERE singleton_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .optional()?;
    stored.map(parse_stored).transpose()
}

pub(super) fn load_takeover_available(connection: &Connection) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT takeover_available
             FROM offsite_backup_config
             WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(false))
}

pub(super) fn set_takeover_available(
    connection: &mut Connection,
    backup_set_id: &BackupSetId,
    available: bool,
) -> Result<()> {
    let changed = connection.execute(
        "UPDATE offsite_backup_config
         SET takeover_available = ?1
         WHERE singleton_id = 1 AND backup_set_id = ?2",
        params![available, backup_set_id.as_str()],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleOffsiteBackupConfig);
    }
    Ok(())
}

pub(super) fn save(
    connection: &mut Connection,
    media: &Connection,
    input: SaveOffsiteBackupConfigInput,
) -> Result<OffsiteBackupConfig> {
    if input.expected_revision < 0 {
        return Err(DatabaseError::InvalidOffsiteBackupConfig(
            "expected revision must not be negative".into(),
        ));
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = load(&transaction)?;
    validate_change(current.as_ref(), &input)?;
    let now = now_millis()?;

    match current {
        None => {
            if input.expected_revision != 0 {
                return Err(DatabaseError::StaleOffsiteBackupConfig);
            }
            transaction.execute(
                "INSERT INTO offsite_backup_config (
                    singleton_id,
                    revision,
                    backup_set_id,
                    replica_epoch_id,
                    enabled,
                    provider,
                    jurisdiction,
                    account_id,
                    bucket,
                    prefix,
                    created_at,
                    updated_at
                 ) VALUES (1, 1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                params![
                    input.backup_set_id.as_str(),
                    input.replica_epoch_id.as_str(),
                    input.enabled,
                    BackupProvider::R2.as_db_str(),
                    input.target.jurisdiction.as_db_str(),
                    input.target.account_id.as_str(),
                    input.target.bucket.as_str(),
                    input.target.prefix.as_str(),
                    now,
                ],
            )?;
        }
        Some(_) => {
            let changed = transaction.execute(
                "UPDATE offsite_backup_config
                 SET revision = revision + 1,
                     backup_set_id = ?1,
                     replica_epoch_id = ?2,
                     enabled = ?3,
                     provider = ?4,
                     jurisdiction = ?5,
                     account_id = ?6,
                     bucket = ?7,
                     prefix = ?8,
                     updated_at = ?9,
                     takeover_available = CASE
                         WHEN backup_set_id = ?1
                              AND replica_epoch_id = ?2
                              AND ?3 = 0
                         THEN takeover_available
                         ELSE 0
                     END
                 WHERE singleton_id = 1 AND revision = ?10",
                params![
                    input.backup_set_id.as_str(),
                    input.replica_epoch_id.as_str(),
                    input.enabled,
                    BackupProvider::R2.as_db_str(),
                    input.target.jurisdiction.as_db_str(),
                    input.target.account_id.as_str(),
                    input.target.bucket.as_str(),
                    input.target.prefix.as_str(),
                    now,
                    input.expected_revision,
                ],
            )?;
            if changed != 1 {
                return Err(DatabaseError::StaleOffsiteBackupConfig);
            }
        }
    }

    let saved = load(&transaction)?.ok_or_else(|| {
        DatabaseError::InvalidOffsiteBackupConfig("saved configuration is unavailable".into())
    })?;
    offsite_media::seed_for_backup_set(&transaction, media, &saved.backup_set_id, now)?;
    transaction.commit()?;
    Ok(saved)
}

fn validate_change(
    current: Option<&OffsiteBackupConfig>,
    input: &SaveOffsiteBackupConfigInput,
) -> Result<()> {
    let Some(current) = current else {
        return Ok(());
    };
    if input.expected_revision != current.revision {
        return Err(DatabaseError::StaleOffsiteBackupConfig);
    }
    let target_changed = current.target != input.target;
    let backup_set_changed = current.backup_set_id != input.backup_set_id;
    if target_changed != backup_set_changed {
        return Err(DatabaseError::InvalidOffsiteBackupConfig(
            "a target change must create exactly one new backup set".into(),
        ));
    }
    Ok(())
}

fn parse_stored(
    stored: (
        i64,
        String,
        String,
        bool,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
    ),
) -> Result<OffsiteBackupConfig> {
    let provider = BackupProvider::from_db(&stored.4).map_err(invalid_stored)?;
    let jurisdiction = R2Jurisdiction::from_db(&stored.5).map_err(invalid_stored)?;
    let backup_set_id = BackupSetId::parse(stored.1).map_err(invalid_stored)?;
    let replica_epoch_id = ReplicaEpochId::parse(stored.2).map_err(invalid_stored)?;
    let account_id = R2AccountId::parse(stored.6).map_err(invalid_stored)?;
    let bucket = R2BucketName::parse(stored.7).map_err(invalid_stored)?;
    let prefix = R2Prefix::parse(stored.8).map_err(invalid_stored)?;
    if stored.0 <= 0 || stored.9 < 0 || stored.10 < stored.9 {
        return Err(DatabaseError::InvalidOffsiteBackupConfig(
            "stored configuration metadata is invalid".into(),
        ));
    }
    Ok(OffsiteBackupConfig {
        revision: stored.0,
        backup_set_id,
        replica_epoch_id,
        enabled: stored.3,
        provider,
        target: R2Target {
            account_id,
            jurisdiction,
            bucket,
            prefix,
        },
        created_at: stored.9,
        updated_at: stored.10,
    })
}

fn invalid_stored(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::InvalidOffsiteBackupConfig(error.to_string())
}
