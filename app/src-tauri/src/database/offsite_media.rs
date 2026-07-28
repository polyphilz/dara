use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::backup::domain::{BackupErrorCode, BackupSetId, ContentSha256, OffsiteMediaState};

use super::{offsite_backup, DatabaseError, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OffsiteMediaCandidate {
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) sha256: ContentSha256,
    pub(crate) byte_length: u64,
    pub(crate) attempt_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OffsiteMediaAttemptOutcome {
    Verified,
    RetryWait {
        error_code: BackupErrorCode,
        next_attempt_at: i64,
    },
    Blocked {
        error_code: BackupErrorCode,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecordOffsiteMediaAttemptInput {
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) sha256: ContentSha256,
    pub(crate) expected_attempt_count: u32,
    pub(crate) attempted_at: i64,
    pub(crate) outcome: OffsiteMediaAttemptOutcome,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct OffsiteMediaReconciliationReport {
    pub(crate) backup_set_id: Option<BackupSetId>,
    pub(crate) inserted: u64,
    pub(crate) missing_local_blobs: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct OffsiteMediaSummary {
    pub(crate) pending_count: u64,
    pub(crate) pending_bytes: u64,
    pub(crate) retry_wait_count: u64,
    pub(crate) verified_count: u64,
    pub(crate) blocked_count: u64,
    pub(crate) next_attempt_at: Option<i64>,
    pub(crate) last_error_code: Option<BackupErrorCode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnqueueAvailableResult {
    Inserted,
    Requeued,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OffsiteMediaSummaryScope {
    All,
    Referenced,
}

impl OffsiteMediaSummaryScope {
    const fn referenced_only(self) -> i64 {
        match self {
            Self::All => 0,
            Self::Referenced => 1,
        }
    }
}

pub(super) fn enqueue_ingested(
    transaction: &Transaction<'_>,
    sha256: &[u8],
    byte_length: usize,
    now: i64,
) -> Result<bool> {
    validate_hash(sha256)?;
    let byte_length = positive_i64(byte_length as u64, "ingested media byte length")?;
    validate_timestamp(now, "ingested media timestamp")?;
    let backup_set_id = transaction
        .query_row(
            "SELECT backup_set_id
             FROM offsite_backup_config
             WHERE singleton_id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(backup_set_id) = backup_set_id else {
        return Ok(false);
    };
    Ok(
        enqueue_available(transaction, &backup_set_id, sha256, byte_length, now)?
            != EnqueueAvailableResult::Unchanged,
    )
}

fn enqueue_available(
    transaction: &Transaction<'_>,
    backup_set_id: &str,
    sha256: &[u8],
    byte_length: i64,
    now: i64,
) -> Result<EnqueueAvailableResult> {
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO offsite_media_object (
            backup_set_id,
            sha256,
            byte_length,
            state,
            attempt_count,
            next_attempt_at,
            last_attempt_at,
            last_verified_at,
            last_error_code,
            created_at,
            updated_at
         )
        VALUES (?1, ?2, ?3, ?4, 0, NULL, NULL, NULL, NULL, ?5, ?5)",
        params![
            backup_set_id,
            sha256,
            byte_length,
            OffsiteMediaState::Pending.as_db_str(),
            now
        ],
    )?;
    if inserted == 1 {
        return Ok(EnqueueAvailableResult::Inserted);
    }
    let stored_length: i64 = transaction.query_row(
        "SELECT byte_length
         FROM offsite_media_object
         WHERE backup_set_id = ?1 AND sha256 = ?2",
        params![backup_set_id, sha256],
        |row| row.get(0),
    )?;
    if stored_length != byte_length {
        return Err(invalid_media_state(
            "desired media byte length does not match the canonical blob",
        ));
    }
    let requeued = transaction.execute(
        "UPDATE offsite_media_object
         SET state = ?1,
             next_attempt_at = NULL,
             last_error_code = NULL,
             updated_at = ?2
         WHERE backup_set_id = ?3
           AND sha256 = ?4
           AND state = ?5
           AND last_error_code = ?6",
        params![
            OffsiteMediaState::Pending.as_db_str(),
            now,
            backup_set_id,
            sha256,
            OffsiteMediaState::Blocked.as_db_str(),
            BackupErrorCode::LocalMediaMissing.as_db_str(),
        ],
    )?;
    Ok(if requeued == 1 {
        EnqueueAvailableResult::Requeued
    } else {
        EnqueueAvailableResult::Unchanged
    })
}

pub(super) fn seed_for_backup_set(
    transaction: &Transaction<'_>,
    media: &Connection,
    backup_set_id: &BackupSetId,
    now: i64,
) -> Result<OffsiteMediaReconciliationReport> {
    validate_timestamp(now, "off-site media reconciliation timestamp")?;
    let hashes = transaction
        .prepare("SELECT DISTINCT sha256 FROM image ORDER BY sha256")?
        .query_map([], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut report = OffsiteMediaReconciliationReport {
        backup_set_id: Some(backup_set_id.clone()),
        ..OffsiteMediaReconciliationReport::default()
    };
    for hash in hashes {
        validate_hash(&hash)?;
        let byte_length = media
            .query_row(
                "SELECT length(bytes) FROM media_blob WHERE sha256 = ?1",
                [&hash],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(byte_length) = byte_length else {
            report.missing_local_blobs = report.missing_local_blobs.saturating_add(1);
            continue;
        };
        if byte_length <= 0 {
            return Err(invalid_media_state("media blob has an invalid byte length"));
        }
        match enqueue_available(transaction, backup_set_id.as_str(), &hash, byte_length, now)? {
            EnqueueAvailableResult::Inserted => {
                report.inserted = report.inserted.saturating_add(1);
            }
            EnqueueAvailableResult::Requeued | EnqueueAvailableResult::Unchanged => {}
        }
    }
    Ok(report)
}

pub(super) fn reconcile(
    main: &mut Connection,
    media: &Connection,
    now: i64,
) -> Result<OffsiteMediaReconciliationReport> {
    let transaction = main.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some(config) = offsite_backup::load(&transaction)? else {
        transaction.commit()?;
        return Ok(OffsiteMediaReconciliationReport::default());
    };
    let report = seed_for_backup_set(&transaction, media, &config.backup_set_id, now)?;
    transaction.commit()?;
    Ok(report)
}

pub(super) fn load_next(
    connection: &Connection,
    backup_set_id: &BackupSetId,
    now: i64,
) -> Result<Option<OffsiteMediaCandidate>> {
    validate_timestamp(now, "off-site media claim timestamp")?;
    let stored = connection
        .query_row(
            "SELECT
                object.sha256,
                object.byte_length,
                object.attempt_count,
                object.state
             FROM offsite_media_object AS object
             WHERE object.backup_set_id = ?1
               AND (
                    object.state = ?2
                    OR (
                        object.state = ?3
                        AND object.next_attempt_at <= ?4
                    )
               )
               AND EXISTS (
                    SELECT 1
                    FROM offsite_backup_config AS config
                    WHERE config.singleton_id = 1
                      AND config.enabled = 1
                      AND config.backup_set_id = object.backup_set_id
               )
             ORDER BY
                CASE object.state WHEN ?2 THEN 0 ELSE 1 END,
                coalesce(object.next_attempt_at, 0),
                object.created_at,
                object.sha256
             LIMIT 1",
            params![
                backup_set_id.as_str(),
                OffsiteMediaState::Pending.as_db_str(),
                OffsiteMediaState::RetryWait.as_db_str(),
                now,
            ],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    stored
        .map(|(sha256, byte_length, attempt_count, state)| {
            if !matches!(
                OffsiteMediaState::from_db(&state).map_err(invalid_domain)?,
                OffsiteMediaState::Pending | OffsiteMediaState::RetryWait
            ) {
                return Err(invalid_media_state("selected media object is not eligible"));
            }
            Ok(OffsiteMediaCandidate {
                backup_set_id: backup_set_id.clone(),
                sha256: ContentSha256::from_slice(&sha256).map_err(invalid_domain)?,
                byte_length: u64::try_from(byte_length)
                    .map_err(|_| invalid_media_state("media byte length is negative"))?,
                attempt_count: u32::try_from(attempt_count)
                    .map_err(|_| invalid_media_state("media attempt count is invalid"))?,
            })
        })
        .transpose()
}

pub(super) fn record_attempt(
    connection: &mut Connection,
    input: RecordOffsiteMediaAttemptInput,
) -> Result<()> {
    validate_timestamp(input.attempted_at, "off-site media attempt timestamp")?;
    let next_attempt_count = input
        .expected_attempt_count
        .checked_add(1)
        .ok_or_else(|| invalid_media_state("media attempt count overflowed"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = match input.outcome {
        OffsiteMediaAttemptOutcome::Verified => transaction.execute(
            "UPDATE offsite_media_object
             SET state = ?1,
                 attempt_count = ?2,
                 next_attempt_at = NULL,
                 last_attempt_at = ?3,
                 last_verified_at = ?3,
                 last_error_code = NULL,
                 updated_at = ?3
             WHERE backup_set_id = ?4
               AND sha256 = ?5
               AND attempt_count = ?6
               AND state IN (?7, ?8)",
            params![
                OffsiteMediaState::Verified.as_db_str(),
                next_attempt_count,
                input.attempted_at,
                input.backup_set_id.as_str(),
                input.sha256.as_bytes().as_slice(),
                input.expected_attempt_count,
                OffsiteMediaState::Pending.as_db_str(),
                OffsiteMediaState::RetryWait.as_db_str(),
            ],
        )?,
        OffsiteMediaAttemptOutcome::RetryWait {
            error_code,
            next_attempt_at,
        } => {
            if next_attempt_at <= input.attempted_at {
                return Err(invalid_media_state(
                    "media retry timestamp must follow the attempt",
                ));
            }
            transaction.execute(
                "UPDATE offsite_media_object
                 SET state = ?1,
                     attempt_count = ?2,
                     next_attempt_at = ?3,
                     last_attempt_at = ?4,
                     last_error_code = ?5,
                     updated_at = ?4
                 WHERE backup_set_id = ?6
                   AND sha256 = ?7
                   AND attempt_count = ?8
                   AND state IN (?9, ?10)",
                params![
                    OffsiteMediaState::RetryWait.as_db_str(),
                    next_attempt_count,
                    next_attempt_at,
                    input.attempted_at,
                    error_code.as_db_str(),
                    input.backup_set_id.as_str(),
                    input.sha256.as_bytes().as_slice(),
                    input.expected_attempt_count,
                    OffsiteMediaState::Pending.as_db_str(),
                    OffsiteMediaState::RetryWait.as_db_str(),
                ],
            )?
        }
        OffsiteMediaAttemptOutcome::Blocked { error_code } => transaction.execute(
            "UPDATE offsite_media_object
             SET state = ?1,
                 attempt_count = ?2,
                 next_attempt_at = NULL,
                 last_attempt_at = ?3,
                 last_error_code = ?4,
                 updated_at = ?3
             WHERE backup_set_id = ?5
               AND sha256 = ?6
               AND attempt_count = ?7
               AND state IN (?8, ?9)",
            params![
                OffsiteMediaState::Blocked.as_db_str(),
                next_attempt_count,
                input.attempted_at,
                error_code.as_db_str(),
                input.backup_set_id.as_str(),
                input.sha256.as_bytes().as_slice(),
                input.expected_attempt_count,
                OffsiteMediaState::Pending.as_db_str(),
                OffsiteMediaState::RetryWait.as_db_str(),
            ],
        )?,
    };
    if changed != 1 {
        return Err(DatabaseError::StaleOffsiteMediaAttempt);
    }
    transaction.commit()?;
    Ok(())
}

pub(super) fn summary(
    connection: &Connection,
    backup_set_id: &BackupSetId,
) -> Result<OffsiteMediaSummary> {
    summary_for_scope(connection, backup_set_id, OffsiteMediaSummaryScope::All)
}

pub(super) fn referenced_summary(
    connection: &Connection,
    backup_set_id: &BackupSetId,
) -> Result<OffsiteMediaSummary> {
    summary_for_scope(
        connection,
        backup_set_id,
        OffsiteMediaSummaryScope::Referenced,
    )
}

fn summary_for_scope(
    connection: &Connection,
    backup_set_id: &BackupSetId,
    scope: OffsiteMediaSummaryScope,
) -> Result<OffsiteMediaSummary> {
    let counts = connection.query_row(
        "SELECT
            coalesce(sum(CASE WHEN state = ?2 THEN 1 ELSE 0 END), 0),
            coalesce(sum(CASE WHEN state IN (?2, ?3) THEN byte_length ELSE 0 END), 0),
            coalesce(sum(CASE WHEN state = ?3 THEN 1 ELSE 0 END), 0),
            coalesce(sum(CASE WHEN state = ?4 THEN 1 ELSE 0 END), 0),
            coalesce(sum(CASE WHEN state = ?5 THEN 1 ELSE 0 END), 0),
            min(CASE WHEN state = ?3 THEN next_attempt_at END)
         FROM offsite_media_object AS object
         WHERE object.backup_set_id = ?1
           AND (
                ?6 = 0
                OR EXISTS (
                    SELECT 1
                    FROM image AS referenced
                    WHERE referenced.sha256 = object.sha256
                )
           )",
        params![
            backup_set_id.as_str(),
            OffsiteMediaState::Pending.as_db_str(),
            OffsiteMediaState::RetryWait.as_db_str(),
            OffsiteMediaState::Verified.as_db_str(),
            OffsiteMediaState::Blocked.as_db_str(),
            scope.referenced_only(),
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        },
    )?;
    let last_error = connection
        .query_row(
            "SELECT object.last_error_code
             FROM offsite_media_object AS object
             WHERE object.backup_set_id = ?1
               AND object.last_error_code IS NOT NULL
               AND (
                    ?2 = 0
                    OR EXISTS (
                        SELECT 1
                        FROM image AS referenced
                        WHERE referenced.sha256 = object.sha256
                    )
               )
             ORDER BY object.updated_at DESC, object.sha256
             LIMIT 1",
            params![backup_set_id.as_str(), scope.referenced_only()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| BackupErrorCode::from_db(&value).map_err(invalid_domain))
        .transpose()?;
    Ok(OffsiteMediaSummary {
        pending_count: non_negative_u64(counts.0, "pending media count")?,
        pending_bytes: non_negative_u64(counts.1, "pending media bytes")?,
        retry_wait_count: non_negative_u64(counts.2, "retrying media count")?,
        verified_count: non_negative_u64(counts.3, "verified media count")?,
        blocked_count: non_negative_u64(counts.4, "blocked media count")?,
        next_attempt_at: counts.5,
        last_error_code: last_error,
    })
}

pub(super) fn release_transient_retries(
    connection: &mut Connection,
    backup_set_id: &BackupSetId,
    now: i64,
) -> Result<u64> {
    validate_timestamp(now, "connectivity restoration timestamp")?;
    let changed = connection.execute(
        "UPDATE offsite_media_object
         SET next_attempt_at = ?1, updated_at = ?1
         WHERE backup_set_id = ?2
           AND state = ?3
           AND last_error_code IN (?4, ?5, ?6, ?7)",
        params![
            now,
            backup_set_id.as_str(),
            OffsiteMediaState::RetryWait.as_db_str(),
            BackupErrorCode::NetworkOffline.as_db_str(),
            BackupErrorCode::NetworkTimeout.as_db_str(),
            BackupErrorCode::RateLimited.as_db_str(),
            BackupErrorCode::ServiceUnavailable.as_db_str(),
        ],
    )?;
    Ok(changed as u64)
}

pub(super) fn release_all_retries(
    connection: &mut Connection,
    backup_set_id: &BackupSetId,
    now: i64,
) -> Result<u64> {
    validate_timestamp(now, "manual backup timestamp")?;
    let changed = connection.execute(
        "UPDATE offsite_media_object
         SET next_attempt_at = ?1, updated_at = max(updated_at, ?1)
         WHERE backup_set_id = ?2 AND state = ?3",
        params![
            now,
            backup_set_id.as_str(),
            OffsiteMediaState::RetryWait.as_db_str(),
        ],
    )?;
    Ok(changed as u64)
}

pub(super) fn requeue_credential_failures(
    connection: &mut Connection,
    backup_set_id: &BackupSetId,
    now: i64,
) -> Result<u64> {
    validate_timestamp(now, "credential replacement timestamp")?;
    let changed = connection.execute(
        "UPDATE offsite_media_object
         SET state = ?1,
             next_attempt_at = NULL,
             last_error_code = NULL,
             updated_at = ?2
         WHERE backup_set_id = ?3
           AND state = ?4
           AND last_error_code IN (?5, ?6)",
        params![
            OffsiteMediaState::Pending.as_db_str(),
            now,
            backup_set_id.as_str(),
            OffsiteMediaState::Blocked.as_db_str(),
            BackupErrorCode::AuthenticationRejected.as_db_str(),
            BackupErrorCode::AuthorizationRejected.as_db_str(),
        ],
    )?;
    Ok(changed as u64)
}

fn validate_hash(value: &[u8]) -> Result<()> {
    ContentSha256::from_slice(value)
        .map(|_| ())
        .map_err(invalid_domain)
}

fn validate_timestamp(value: i64, name: &'static str) -> Result<()> {
    if value < 0 {
        return Err(invalid_media_state(name));
    }
    Ok(())
}

fn positive_i64(value: u64, name: &'static str) -> Result<i64> {
    let value = i64::try_from(value).map_err(|_| invalid_media_state(name))?;
    if value <= 0 {
        return Err(invalid_media_state(name));
    }
    Ok(value)
}

fn non_negative_u64(value: i64, name: &'static str) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid_media_state(name))
}

fn invalid_domain(error: impl std::fmt::Display) -> DatabaseError {
    invalid_media_state(error.to_string())
}

fn invalid_media_state(reason: impl Into<String>) -> DatabaseError {
    DatabaseError::InvalidOffsiteMediaState(reason.into())
}
