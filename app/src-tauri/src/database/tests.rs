use std::{fs, path::Path};

use refinery::{Migration, Runner};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::backup::domain::{
    BackupErrorCode, BackupSetId, ContentSha256, R2AccountId, R2BucketName, R2Jurisdiction,
    R2Prefix, R2Target, ReplicaEpochId,
};

use super::{
    connection::{self, DatabaseKind, FileState, MAIN_APPLICATION_ID, MEDIA_APPLICATION_ID},
    domain::{
        CardContentType, ReviewCardState, ReviewCardStatus, ReviewEventType, SchedulerAlgorithm,
        SchedulerLibrary,
    },
    embedding_index, initialize,
    media::OcrQueueState,
    migrations, snapshot, CanonicalImage, DaraCommand, DatabaseError, DatabasePaths,
    ImageOcrStatus, InitializationOptions, OffsiteMediaAttemptOutcome,
    RecordOffsiteMediaAttemptInput, DEFAULT_HOME_ACCELERATOR,
};

const BASIC_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000101";
const BASIC_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000102";
const SECOND_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000103";
const REVIEW_EVENT_ID: &str = "01980c8e-6c00-7000-8000-000000000104";
const IMAGE_ID: &str = "01980c8e-6c00-7000-8000-000000000105";
const DEFAULT_CONFIG_ID: &str = "019f547b-6200-7000-8000-000000000001";
const JINA_INDEX_ID: &str = "019f547b-6200-7000-8000-000000000002";

fn test_paths() -> (TempDir, DatabasePaths) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let paths = DatabasePaths::new(directory.path().join("data"));
    (directory, paths)
}

fn no_launch_snapshot() -> InitializationOptions {
    InitializationOptions {
        launch_snapshot: false,
    }
}

fn initialize_test(paths: &DatabasePaths) -> super::Database {
    initialize(paths.clone(), "test", no_launch_snapshot()).expect("database initialization")
}

fn create_identified_unmigrated_pair(paths: &DatabasePaths) {
    fs::create_dir_all(paths.root()).expect("database root");
    drop(
        connection::open_writer(&paths.main, DatabaseKind::Main, FileState::Fresh)
            .expect("unmigrated main"),
    );
    drop(
        connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Fresh)
            .expect("unmigrated media"),
    );
}

fn open_existing(path: &Path, kind: DatabaseKind) -> Connection {
    connection::open_writer(path, kind, FileState::Existing).expect("existing database")
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()
        .expect("table lookup")
        .is_some()
}

fn insert_basic_content(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (?1, 100, 100, NULL, ?2, 'question', 'answer', NULL)",
            params![BASIC_CONTENT_ID, CardContentType::Basic.as_db_str()],
        )
        .expect("basic content");
}

#[test]
fn fresh_pair_migrates_reopens_and_is_idempotent() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    assert_eq!(database.paths(), &paths);
    drop(database);

    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");
    let main_id: i32 = main
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .expect("main application id");
    let media_id: i32 = media
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .expect("media application id");
    assert_eq!(main_id, MAIN_APPLICATION_ID);
    assert_eq!(media_id, MEDIA_APPLICATION_ID);
    assert_eq!(
        main.query_row(
            "SELECT json_array_length(config_json, '$.parameters') FROM scheduler_config",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("scheduler parameters"),
        21
    );
    assert_eq!(
        main.query_row("SELECT count(*) FROM text_embedding_index", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("shipped index definition"),
        1
    );
    drop(main);
    drop(media);

    let reopened = initialize_test(&paths);
    drop(reopened);

    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let history_rows: i64 = main
        .query_row("SELECT count(*) FROM refinery_schema_history", [], |row| {
            row.get(0)
        })
        .expect("history count");
    assert_eq!(history_rows, 13);
}

#[test]
fn offsite_backup_config_is_non_secret_typed_and_revision_guarded() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let client = database.client();
    assert_eq!(
        client
            .load_offsite_backup_config()
            .expect("load empty config"),
        None
    );

    let target = R2Target {
        account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
        jurisdiction: R2Jurisdiction::Default,
        bucket: R2BucketName::parse("dara-test").expect("bucket"),
        prefix: R2Prefix::parse("dara/primary").expect("prefix"),
    };
    let backup_set_id = BackupSetId::new();
    let replica_epoch_id = ReplicaEpochId::new();
    let saved = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: replica_epoch_id.clone(),
            enabled: false,
            target: target.clone(),
        })
        .expect("save config");
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.backup_set_id, backup_set_id);
    assert_eq!(saved.replica_epoch_id, replica_epoch_id);
    assert_eq!(saved.target, target);
    assert!(!saved.enabled);
    assert!(!client
        .load_offsite_backup_takeover_availability()
        .expect("load takeover availability"));
    assert_eq!(
        client
            .load_offsite_backup_takeover_reason()
            .expect("load takeover reason"),
        None
    );
    assert_eq!(
        client
            .load_offsite_backup_runtime_config()
            .expect("load empty runtime config"),
        None
    );
    client
        .set_offsite_backup_takeover_reason(
            saved.backup_set_id.clone(),
            Some(super::OffsiteBackupTakeoverReason::OwnerMismatch),
        )
        .expect("persist takeover availability");
    assert!(client
        .load_offsite_backup_takeover_availability()
        .expect("reload takeover availability"));
    assert_eq!(
        client
            .load_offsite_backup_takeover_reason()
            .expect("reload takeover reason"),
        Some(super::OffsiteBackupTakeoverReason::OwnerMismatch)
    );
    assert_eq!(
        client
            .load_offsite_backup_runtime_config()
            .expect("load takeover-blocked runtime config"),
        None
    );
    assert_eq!(
        client
            .load_offsite_backup_config()
            .expect("load config")
            .expect("stored config"),
        saved
    );

    assert!(matches!(
        client.save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: saved.backup_set_id.clone(),
            replica_epoch_id: saved.replica_epoch_id.clone(),
            enabled: true,
            target: saved.target.clone(),
        }),
        Err(DatabaseError::StaleOffsiteBackupConfig)
    ));

    let enabled = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: saved.revision,
            backup_set_id: saved.backup_set_id.clone(),
            replica_epoch_id: saved.replica_epoch_id.clone(),
            enabled: true,
            target: saved.target.clone(),
        })
        .expect("enable config");
    assert_eq!(enabled.revision, 2);
    assert!(enabled.enabled);
    assert!(!client
        .load_offsite_backup_takeover_availability()
        .expect("successful config save clears takeover availability"));
    assert_eq!(
        client
            .load_offsite_backup_runtime_config()
            .expect("load enabled runtime config"),
        Some(enabled.clone())
    );
    client
        .set_offsite_backup_takeover_reason(
            enabled.backup_set_id.clone(),
            Some(super::OffsiteBackupTakeoverReason::RestoredBackup),
        )
        .expect("persist restored takeover availability");
    assert_eq!(
        client
            .load_offsite_backup_takeover_reason()
            .expect("load restored takeover reason"),
        Some(super::OffsiteBackupTakeoverReason::RestoredBackup)
    );
    assert_eq!(
        client
            .load_offsite_backup_runtime_config()
            .expect("load blocked enabled runtime config"),
        None
    );
    let disabled = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: enabled.revision,
            backup_set_id: enabled.backup_set_id.clone(),
            replica_epoch_id: enabled.replica_epoch_id.clone(),
            enabled: false,
            target: enabled.target.clone(),
        })
        .expect("disable config");
    assert!(!disabled.enabled);
    assert!(client
        .load_offsite_backup_takeover_availability()
        .expect("disabling preserves takeover availability"));
    assert_eq!(
        client
            .load_offsite_backup_takeover_reason()
            .expect("disabling preserves takeover reason"),
        Some(super::OffsiteBackupTakeoverReason::RestoredBackup)
    );

    let changed_target = R2Target {
        prefix: R2Prefix::parse("dara/other").expect("other prefix"),
        ..disabled.target.clone()
    };
    assert!(matches!(
        client.save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: disabled.revision,
            backup_set_id: disabled.backup_set_id.clone(),
            replica_epoch_id: disabled.replica_epoch_id.clone(),
            enabled: false,
            target: changed_target.clone(),
        }),
        Err(DatabaseError::InvalidOffsiteBackupConfig(_))
    ));
    let changed = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: disabled.revision,
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: false,
            target: changed_target,
        })
        .expect("change target");
    assert_eq!(changed.revision, 4);
    assert!(!client
        .load_offsite_backup_takeover_availability()
        .expect("changing identity clears takeover availability"));
    assert_eq!(
        client
            .load_offsite_backup_takeover_reason()
            .expect("changing identity clears takeover reason"),
        None
    );
    assert_eq!(
        client
            .load_pending_offsite_credential_cleanup()
            .expect("load retired credential cleanup"),
        vec![disabled.backup_set_id.clone()]
    );
    let returned = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: changed.revision,
            backup_set_id: disabled.backup_set_id.clone(),
            replica_epoch_id: disabled.replica_epoch_id,
            enabled: false,
            target: disabled.target,
        })
        .expect("return to previous target");
    assert_eq!(
        client
            .load_pending_offsite_credential_cleanup()
            .expect("load cleanup after returning to previous target"),
        vec![changed.backup_set_id.clone()]
    );
    client
        .complete_offsite_credential_cleanup(returned.backup_set_id)
        .expect("completing the active backup set is harmless");
    assert_eq!(
        client
            .load_pending_offsite_credential_cleanup()
            .expect("active backup set was never queued"),
        vec![changed.backup_set_id.clone()]
    );
    client
        .complete_offsite_credential_cleanup(changed.backup_set_id)
        .expect("complete retired credential cleanup");
    assert!(client
        .load_pending_offsite_credential_cleanup()
        .expect("reload retired credential cleanup")
        .is_empty());
    drop(database);

    let main = open_existing(&paths.main, DatabaseKind::Main);
    let mut statement = main
        .prepare("SELECT name FROM pragma_table_info('offsite_backup_config')")
        .expect("config columns");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("column query")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("columns");
    for prohibited in ["access", "secret", "credential"] {
        assert!(
            columns.iter().all(|column| !column.contains(prohibited)),
            "credential-like column persisted: {prohibited}"
        );
    }
    assert_eq!(
        main.query_row(
            "SELECT revision FROM offsite_backup_content_clock WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("content clock"),
        0
    );
}

#[test]
fn offsite_media_work_survives_offline_restart_and_converges_without_duplicates() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let client = database.client();
    let first_bytes = b"first-canonical-image".to_vec();
    let second_bytes = b"second-canonical-image".to_vec();
    client
        .ingest_image(
            CanonicalImage {
                bytes: first_bytes.clone(),
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("first image before backup configuration");

    let target = R2Target {
        account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
        jurisdiction: R2Jurisdiction::Default,
        bucket: R2BucketName::parse("dara-test").expect("bucket"),
        prefix: R2Prefix::parse("dara/media-reconciliation").expect("prefix"),
    };
    let backup_set_id = BackupSetId::new();
    let saved = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            target: target.clone(),
        })
        .expect("enabled backup configuration");
    client
        .ingest_image(
            CanonicalImage {
                bytes: second_bytes.clone(),
                natural_width: 12,
                natural_height: 12,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("second image after backup configuration");

    let attempt_time = super::now_millis().expect("attempt time").saturating_add(1);
    let offline = client
        .load_next_offsite_media(backup_set_id.clone(), attempt_time)
        .expect("first desired object")
        .expect("queued object");
    let retry_at = attempt_time.saturating_add(60_000);
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: offline.sha256,
            expected_attempt_count: offline.attempt_count,
            attempted_at: attempt_time,
            outcome: OffsiteMediaAttemptOutcome::RetryWait {
                error_code: BackupErrorCode::NetworkOffline,
                next_attempt_at: retry_at,
            },
        })
        .expect("persist offline retry");

    let other = client
        .load_next_offsite_media(backup_set_id.clone(), attempt_time)
        .expect("second desired object")
        .expect("pending object remains eligible");
    assert_ne!(other.sha256, offline.sha256);
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: other.sha256,
            expected_attempt_count: other.attempt_count,
            attempted_at: attempt_time.saturating_add(1),
            outcome: OffsiteMediaAttemptOutcome::Verified,
        })
        .expect("verify second object");
    assert!(client
        .load_next_offsite_media(backup_set_id.clone(), retry_at.saturating_sub(1))
        .expect("work before retry")
        .is_none());
    drop(database);

    let reopened = initialize_test(&paths);
    let client = reopened.client();
    let retried = client
        .load_next_offsite_media(backup_set_id.clone(), retry_at)
        .expect("work after restart")
        .expect("retry survived restart");
    assert_eq!(retried.sha256, offline.sha256);
    assert_eq!(retried.attempt_count, 1);
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: retried.sha256,
            expected_attempt_count: retried.attempt_count,
            attempted_at: retry_at,
            outcome: OffsiteMediaAttemptOutcome::Verified,
        })
        .expect("verify retried object");
    let summary = client
        .load_offsite_media_summary(backup_set_id.clone())
        .expect("media summary");
    assert_eq!(summary.verified_count, 2);
    assert_eq!(summary.pending_count, 0);
    assert_eq!(summary.retry_wait_count, 0);
    assert_eq!(summary.blocked_count, 0);

    let new_backup_set_id = BackupSetId::new();
    let changed = client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: saved.revision,
            backup_set_id: new_backup_set_id.clone(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: false,
            target: R2Target {
                prefix: R2Prefix::parse("dara/new-target").expect("new prefix"),
                ..target
            },
        })
        .expect("new backup target");
    assert_eq!(changed.backup_set_id, new_backup_set_id);
    let new_summary = client
        .load_offsite_media_summary(changed.backup_set_id)
        .expect("new target summary");
    assert_eq!(new_summary.pending_count, 2);
    assert_eq!(
        new_summary.pending_bytes,
        (first_bytes.len() + second_bytes.len()) as u64
    );
}

#[test]
fn invalid_remote_evidence_requeues_verified_media_for_repair() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let client = database.client();
    let backup_set_id = BackupSetId::new();
    client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            target: R2Target {
                account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                    .expect("account ID"),
                jurisdiction: R2Jurisdiction::Default,
                bucket: R2BucketName::parse("dara-test").expect("bucket"),
                prefix: R2Prefix::parse("dara/remote-evidence-repair").expect("prefix"),
            },
        })
        .expect("backup configuration");
    client
        .ingest_image(
            CanonicalImage {
                bytes: b"remote-evidence-media".to_vec(),
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("image");
    let now = super::now_millis().expect("time");
    let candidate = client
        .load_next_offsite_media(backup_set_id.clone(), now)
        .expect("load candidate")
        .expect("candidate");
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: candidate.sha256,
            expected_attempt_count: candidate.attempt_count,
            attempted_at: now,
            outcome: OffsiteMediaAttemptOutcome::Verified,
        })
        .expect("verify candidate");

    assert_eq!(
        client
            .requeue_offsite_media_evidence(
                backup_set_id.clone(),
                vec![candidate.sha256],
                BackupErrorCode::RemoteMediaMissing,
                now.saturating_add(1),
            )
            .expect("requeue invalid evidence"),
        1
    );
    let requeued = client
        .load_next_offsite_media(backup_set_id, now.saturating_add(1))
        .expect("load requeued candidate")
        .expect("requeued candidate");
    assert_eq!(requeued.sha256, candidate.sha256);
    assert_eq!(requeued.attempt_count, 1);
}

#[test]
fn periodic_offsite_media_reconciliation_repairs_missing_desired_rows() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let client = database.client();
    let bytes = b"periodic-reconciliation-image".to_vec();
    client
        .ingest_image(
            CanonicalImage {
                bytes: bytes.clone(),
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("image");
    let backup_set_id = BackupSetId::new();
    client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            target: R2Target {
                account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                    .expect("account ID"),
                jurisdiction: R2Jurisdiction::Default,
                bucket: R2BucketName::parse("dara-test").expect("bucket"),
                prefix: R2Prefix::parse("dara/periodic-reconciliation").expect("prefix"),
            },
        })
        .expect("backup configuration");
    drop(database);

    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let media = open_existing(&paths.media, DatabaseKind::Media);
    let hash = ContentSha256::from_bytes(Sha256::digest(&bytes).into());
    main.execute(
        "DELETE FROM offsite_media_object
         WHERE backup_set_id = ?1 AND sha256 = ?2",
        params![backup_set_id.as_str(), hash.as_bytes().as_slice()],
    )
    .expect("simulate missed ingestion hook");
    let report = super::offsite_media::reconcile(
        &mut main,
        &media,
        super::now_millis().expect("reconciliation time"),
    )
    .expect("periodic reconciliation");
    assert_eq!(report.backup_set_id, Some(backup_set_id.clone()));
    assert_eq!(report.inserted, 1);
    assert_eq!(report.missing_local_blobs, 0);
    assert_eq!(
        main.query_row(
            "SELECT count(*)
             FROM offsite_media_object
             WHERE backup_set_id = ?1 AND sha256 = ?2",
            params![backup_set_id.as_str(), hash.as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )
        .expect("repaired desired row"),
        1
    );
}

#[test]
fn available_local_media_requeues_only_local_missing_blocks() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let client = database.client();
    let bytes = b"returned-local-media".to_vec();
    let backup_set_id = BackupSetId::new();
    client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            target: R2Target {
                account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                    .expect("account ID"),
                jurisdiction: R2Jurisdiction::Default,
                bucket: R2BucketName::parse("dara-test").expect("bucket"),
                prefix: R2Prefix::parse("dara/requeue-local-media").expect("prefix"),
            },
        })
        .expect("backup configuration");
    client
        .ingest_image(
            CanonicalImage {
                bytes: bytes.clone(),
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("image");
    let first_attempt_at = super::now_millis().expect("first attempt time");
    let candidate = client
        .load_next_offsite_media(backup_set_id.clone(), first_attempt_at)
        .expect("pending media")
        .expect("candidate");
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: candidate.sha256,
            expected_attempt_count: candidate.attempt_count,
            attempted_at: first_attempt_at,
            outcome: OffsiteMediaAttemptOutcome::Blocked {
                error_code: BackupErrorCode::LocalMediaMissing,
            },
        })
        .expect("record local-missing block");

    client
        .ingest_image(
            CanonicalImage {
                bytes: bytes.clone(),
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("reingested image");
    let requeued_by_ingest = client
        .load_next_offsite_media(backup_set_id.clone(), first_attempt_at)
        .expect("reingested media")
        .expect("requeued candidate");
    assert_eq!(requeued_by_ingest.sha256, candidate.sha256);
    assert_eq!(requeued_by_ingest.attempt_count, 1);

    let second_attempt_at = first_attempt_at.saturating_add(1);
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: requeued_by_ingest.sha256,
            expected_attempt_count: requeued_by_ingest.attempt_count,
            attempted_at: second_attempt_at,
            outcome: OffsiteMediaAttemptOutcome::Blocked {
                error_code: BackupErrorCode::LocalMediaMissing,
            },
        })
        .expect("record repeated local-missing block");
    let report = client
        .reconcile_offsite_media(second_attempt_at.saturating_add(1))
        .expect("periodic reconciliation");
    assert_eq!(report.inserted, 0);
    assert_eq!(report.missing_local_blobs, 0);

    let requeued_by_reconciliation = client
        .load_next_offsite_media(backup_set_id.clone(), second_attempt_at.saturating_add(1))
        .expect("reconciled media")
        .expect("periodically requeued candidate");
    assert_eq!(requeued_by_reconciliation.sha256, candidate.sha256);
    assert_eq!(requeued_by_reconciliation.attempt_count, 2);
    let summary = client
        .load_offsite_media_summary(backup_set_id.clone())
        .expect("media summary");
    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.blocked_count, 0);
    assert_eq!(summary.last_error_code, None);

    let third_attempt_at = second_attempt_at.saturating_add(2);
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: requeued_by_reconciliation.sha256,
            expected_attempt_count: requeued_by_reconciliation.attempt_count,
            attempted_at: third_attempt_at,
            outcome: OffsiteMediaAttemptOutcome::Blocked {
                error_code: BackupErrorCode::ImmutableObjectConflict,
            },
        })
        .expect("record immutable conflict");
    client
        .ingest_image(
            CanonicalImage {
                bytes,
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("reingest after immutable conflict");
    client
        .reconcile_offsite_media(third_attempt_at.saturating_add(1))
        .expect("reconcile immutable conflict");
    assert!(client
        .load_next_offsite_media(backup_set_id.clone(), third_attempt_at.saturating_add(1))
        .expect("work after immutable conflict")
        .is_none());
    let summary = client
        .load_offsite_media_summary(backup_set_id)
        .expect("immutable-conflict summary");
    assert_eq!(summary.pending_count, 0);
    assert_eq!(summary.blocked_count, 1);
    assert_eq!(
        summary.last_error_code,
        Some(BackupErrorCode::ImmutableObjectConflict)
    );
}

#[test]
fn retired_media_work_stays_append_only_but_does_not_block_referenced_media() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let client = database.client();
    let backup_set_id = BackupSetId::new();
    client
        .save_offsite_backup_config(super::SaveOffsiteBackupConfigInput {
            expected_revision: 0,
            backup_set_id: backup_set_id.clone(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            target: R2Target {
                account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                    .expect("account ID"),
                jurisdiction: R2Jurisdiction::Default,
                bucket: R2BucketName::parse("dara-test").expect("bucket"),
                prefix: R2Prefix::parse("dara/append-only-media").expect("prefix"),
            },
        })
        .expect("backup configuration");
    client
        .ingest_image(
            CanonicalImage {
                bytes: b"eventually-reaped-local-image".to_vec(),
                natural_width: 10,
                natural_height: 10,
            },
            super::TEST_MEDIA_LEASE_ID.into(),
        )
        .expect("image");
    let base = super::now_millis().expect("base time");
    let candidate = client
        .load_next_offsite_media(backup_set_id.clone(), base)
        .expect("load candidate")
        .expect("pending candidate");
    client
        .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
            backup_set_id: backup_set_id.clone(),
            sha256: candidate.sha256,
            expected_attempt_count: candidate.attempt_count,
            attempted_at: base,
            outcome: OffsiteMediaAttemptOutcome::Blocked {
                error_code: BackupErrorCode::ImmutableObjectConflict,
            },
        })
        .expect("block media");
    let orphaned_at = base
        .saturating_add(super::media::MEDIA_LEASE_DURATION_MILLIS)
        .saturating_add(1);
    client
        .maintain_media(orphaned_at, super::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("mark orphaned");
    let reaped_at = orphaned_at
        .saturating_add(super::MEDIA_ORPHAN_GRACE_MILLIS)
        .saturating_add(1);
    let report = client
        .maintain_media(reaped_at, super::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("reap local media");
    assert_eq!(report.cleanup.retired_image_count, 1);
    assert_eq!(report.cleanup.deleted_blob_count, 1);

    let summary = client
        .load_offsite_media_summary(backup_set_id.clone())
        .expect("off-site summary");
    assert_eq!(summary.blocked_count, 1);
    assert_eq!(summary.verified_count, 0);
    assert_eq!(
        summary.last_error_code,
        Some(BackupErrorCode::ImmutableObjectConflict)
    );

    let referenced = client
        .load_referenced_offsite_media_summary(backup_set_id)
        .expect("referenced media summary");
    assert_eq!(referenced.pending_count, 0);
    assert_eq!(referenced.retry_wait_count, 0);
    assert_eq!(referenced.blocked_count, 0);
    assert_eq!(referenced.last_error_code, None);
}

#[test]
fn diagnostics_report_the_writer_serialized_database_state() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);

    let diagnostics = database
        .client()
        .load_database_diagnostics()
        .expect("database diagnostics");

    assert_eq!(diagnostics.migration_heads, migrations::expected_heads());
    assert_eq!(diagnostics.scheduler.algorithm, SchedulerAlgorithm::Fsrs);
    assert_eq!(diagnostics.scheduler.algorithm_version, 6);
    assert_eq!(
        diagnostics.scheduler.scheduler_library,
        SchedulerLibrary::TsFsrs
    );
    assert_eq!(diagnostics.scheduler.library_version, "5.4.1");
    assert_eq!(diagnostics.scheduler.desired_retention, 0.9);
    assert_eq!(
        diagnostics.semantic_index.id,
        embedding_index::jina_v1_manifest().id
    );
    assert!(!diagnostics.semantic_index.active);
    assert_eq!(diagnostics.semantic_index.indexed_documents, 0);
    assert_eq!(diagnostics.semantic_index.total_documents, 0);
}

#[test]
fn latest_finalized_snapshot_ignores_invalid_manifests() {
    let (directory, paths) = test_paths();
    fs::create_dir_all(&paths.backups).expect("backups directory");

    let finalized = snapshot::SnapshotManifest {
        format_version: 1,
        created_at: 100,
        application_version: "0.1.0".into(),
        main: snapshot_file("complete-main.sqlite3"),
        media: snapshot_file("complete-media.sqlite3"),
        relationship_validated: true,
    };
    fs::write(paths.backups.join(&finalized.main.file_name), b"main").expect("finalized main");
    fs::write(paths.backups.join(&finalized.media.file_name), b"media").expect("finalized media");
    write_snapshot_manifest(&paths.backups.join("complete.json"), &finalized);

    let unfinalized = snapshot::SnapshotManifest {
        created_at: 300,
        relationship_validated: false,
        ..finalized.clone()
    };
    write_snapshot_manifest(&paths.backups.join("unfinalized.json"), &unfinalized);

    let unrenderable_timestamp = snapshot::SnapshotManifest {
        created_at: i64::MAX,
        ..finalized.clone()
    };
    write_snapshot_manifest(
        &paths.backups.join("unrenderable-timestamp.json"),
        &unrenderable_timestamp,
    );

    let missing_pair = snapshot::SnapshotManifest {
        created_at: 200,
        main: snapshot_file("missing-main.sqlite3"),
        media: snapshot_file("missing-media.sqlite3"),
        ..finalized
    };
    write_snapshot_manifest(&paths.backups.join("missing.json"), &missing_pair);

    assert_eq!(
        snapshot::latest_finalized_snapshot(&paths.backups).expect("latest finalized snapshot"),
        Some(snapshot::FinalizedSnapshotSummary {
            created_at: 100,
            application_version: "0.1.0".into(),
        })
    );
    drop(directory);
}

#[test]
fn snapshot_retention_ignores_unrenderable_timestamp_manifests() {
    let (_directory, paths) = test_paths();
    fs::create_dir_all(&paths.backups).expect("backups directory");
    let manifest_path = paths.backups.join("unrenderable-timestamp.json");
    write_snapshot_manifest(
        &manifest_path,
        &snapshot::SnapshotManifest {
            format_version: 1,
            created_at: i64::MAX,
            application_version: "0.1.0".into(),
            main: snapshot_file("unrenderable-main.sqlite3"),
            media: snapshot_file("unrenderable-media.sqlite3"),
            relationship_validated: true,
        },
    );

    snapshot::prune_snapshots(&paths.backups).expect("retention ignores invalid timestamp");

    assert!(manifest_path.exists());
}

#[test]
fn migration_safety_snapshot_replaces_the_same_heads_with_current_data() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));

    let first =
        snapshot::create_migration_safety_snapshot_pair(&paths, "test").expect("first safety pair");
    let main = open_existing(&paths.main, DatabaseKind::Main);
    insert_basic_content(&main);
    drop(main);
    let second =
        snapshot::create_migration_safety_snapshot_pair(&paths, "test").expect("new safety pair");

    assert_ne!(second.manifest_path, first.manifest_path);
    assert!(!first.manifest_path.exists());
    assert!(!paths.backups.join(first.manifest.main.file_name).exists());
    assert!(!paths.backups.join(first.manifest.media.file_name).exists());
    assert_eq!(
        fs::read_dir(&paths.backups)
            .expect("backups")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("migration-safety-"))
                    && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count(),
        1
    );

    let restored = DatabasePaths::new(paths.root().join("restored"));
    snapshot::restore_snapshot_pair(&second.manifest_path, &restored)
        .expect("restore replacement safety pair");
    let restored_main = open_existing(&restored.main, DatabaseKind::Main);
    assert_eq!(
        restored_main
            .query_row(
                "SELECT count(*) FROM card_content WHERE id = ?1",
                [BASIC_CONTENT_ID],
                |row| row.get::<_, i64>(0),
            )
            .expect("restored current data"),
        1
    );
}

#[test]
fn snapshot_retention_keeps_one_migration_safety_pair_per_heads() {
    let (_directory, paths) = test_paths();
    fs::create_dir_all(&paths.backups).expect("backups directory");

    let write_pair = |stem: &str, created_at: i64, main_head: i32, media_head: i32| {
        let main_name = format!("{stem}-main.sqlite3");
        let media_name = format!("{stem}-media.sqlite3");
        fs::write(paths.backups.join(&main_name), b"main").expect("retention main");
        fs::write(paths.backups.join(&media_name), b"media").expect("retention media");
        write_snapshot_manifest(
            &paths.backups.join(format!("{stem}.json")),
            &snapshot::SnapshotManifest {
                format_version: 1,
                created_at,
                application_version: "test".into(),
                main: snapshot::SnapshotFile {
                    file_name: main_name,
                    sha256: "not-read-by-retention".into(),
                    migration_head: Some(main_head),
                },
                media: snapshot::SnapshotFile {
                    file_name: media_name,
                    sha256: "not-read-by-retention".into(),
                    migration_head: Some(media_head),
                },
                relationship_validated: true,
            },
        );
    };

    write_pair("migration-safety-old", 100, 6, 1);
    write_pair("migration-safety-new", 200, 6, 1);
    write_pair("migration-safety-other-heads", 150, 5, 1);
    write_pair("restore-safety-first", 50, 6, 1);
    write_pair("restore-safety-second", 75, 6, 1);

    snapshot::prune_snapshots(&paths.backups).expect("bounded safety retention");

    assert!(!paths.backups.join("migration-safety-old.json").exists());
    assert!(!paths
        .backups
        .join("migration-safety-old-main.sqlite3")
        .exists());
    assert!(!paths
        .backups
        .join("migration-safety-old-media.sqlite3")
        .exists());
    for stem in [
        "migration-safety-new",
        "migration-safety-other-heads",
        "restore-safety-first",
        "restore-safety-second",
    ] {
        assert!(
            paths.backups.join(format!("{stem}.json")).exists(),
            "{stem} should remain"
        );
    }
}

fn snapshot_file(file_name: &str) -> snapshot::SnapshotFile {
    snapshot::SnapshotFile {
        file_name: file_name.into(),
        sha256: "not-read-by-cheap-diagnostics".into(),
        migration_head: None,
    }
}

fn write_snapshot_manifest(path: &Path, manifest: &snapshot::SnapshotManifest) {
    fs::write(
        path,
        serde_json::to_vec(manifest).expect("snapshot manifest JSON"),
    )
    .expect("snapshot manifest");
}

#[test]
fn settings_migration_adds_defaults_to_an_existing_database() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let first_five = migrations::main_runner()
        .get_migrations()
        .iter()
        .filter(|migration| migration.version() <= 5)
        .cloned()
        .collect::<Vec<_>>();
    Runner::new(&first_five)
        .set_grouped(true)
        .run(&mut main)
        .expect("pre-settings migrations");
    main.execute(
        "UPDATE app_settings SET updated_at = updated_at + 1 WHERE singleton_id = 1",
        [],
    )
    .expect("existing application state");

    migrations::run_main(&mut main).expect("settings migration");

    let preferences = main
        .query_row(
            "SELECT revision, appearance, automatic_update_checks_enabled, zoom_percent, legacy_zoom_migrated
             FROM user_preferences WHERE singleton_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .expect("migrated preferences");
    assert_eq!(preferences, (1, "SYSTEM".into(), true, 100, false));
    assert_eq!(
        main.query_row("SELECT count(*) FROM keyboard_binding", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("default keyboard bindings"),
        2
    );
    assert_eq!(
        main.query_row(
            "SELECT command, accelerator
             FROM keyboard_binding
             WHERE command = ?1",
            [DaraCommand::Home.as_db_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("default home binding"),
        (
            DaraCommand::Home.as_db_str().to_owned(),
            DEFAULT_HOME_ACCELERATOR.to_owned(),
        )
    );
    assert_eq!(
        main.query_row(
            "SELECT updated_at FROM app_settings WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("preserved application state"),
        1_783_828_800_001
    );
}

#[test]
fn home_shortcut_migration_preserves_a_custom_review_accelerator() {
    const LEGACY_REVIEW_COMMAND: &str = "REVIEW";
    const CUSTOM_ACCELERATOR: &str = "control+alt+super+KeyW";

    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let through_user_settings = migrations::main_runner()
        .get_migrations()
        .iter()
        .filter(|migration| migration.version() <= 6)
        .cloned()
        .collect::<Vec<_>>();
    Runner::new(&through_user_settings)
        .set_grouped(true)
        .run(&mut main)
        .expect("user-settings migrations");
    main.execute(
        "UPDATE keyboard_binding SET accelerator = ?1 WHERE command = ?2",
        params![CUSTOM_ACCELERATOR, LEGACY_REVIEW_COMMAND],
    )
    .expect("custom legacy binding");

    migrations::run_main(&mut main).expect("home-shortcut migration");

    assert_eq!(
        main.query_row(
            "SELECT command, accelerator
             FROM keyboard_binding
             WHERE command = ?1",
            [DaraCommand::Home.as_db_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("migrated home binding"),
        (
            DaraCommand::Home.as_db_str().to_owned(),
            CUSTOM_ACCELERATOR.to_owned(),
        )
    );
}

#[test]
fn applied_durable_image_ocr_migration_checksum_stays_stable() {
    let runner = migrations::main_runner();
    let migration = runner
        .get_migrations()
        .iter()
        .find(|migration| migration.version() == 3)
        .expect("V3 migration");
    assert_eq!(migration.checksum(), 8_411_579_165_145_044_789);
}

#[test]
fn jina_v1_seed_matches_the_canonical_manifest_and_starts_inactive() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    let manifest = embedding_index::jina_v1_manifest();

    let row = main
        .query_row(
            "SELECT
                id, created_at, index_key, model_name, model_revision,
                lower(hex(model_file_sha256)), dimension, distance_metric, normalized,
                index_schema_version, config_json
             FROM text_embedding_index
             WHERE id = ?1",
            [JINA_INDEX_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, u32>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .expect("Jina v1 seed");

    assert_eq!(row.0, manifest.id);
    assert_eq!(row.1, manifest.created_at);
    assert_eq!(row.2, manifest.index_key);
    assert_eq!(row.3, manifest.model_name);
    assert_eq!(row.4, manifest.model_revision);
    assert_eq!(row.5, manifest.model_file_sha256);
    assert_eq!(row.6, manifest.dimension);
    assert_eq!(row.7, manifest.distance_metric);
    assert_eq!(row.8, manifest.normalized);
    assert_eq!(row.9, manifest.index_schema_version);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&row.10).expect("seed config JSON"),
        serde_json::to_value(&manifest.config).expect("manifest config JSON")
    );
    assert_eq!(manifest.manifest_version, 1);

    let active: Option<String> = main
        .query_row(
            "SELECT active_text_embedding_index_id FROM app_settings WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("active embedding index");
    assert_eq!(active, None);

    assert!(main
        .execute(
            "UPDATE text_embedding_index SET model_revision = 'changed' WHERE id = ?1",
            [JINA_INDEX_ID],
        )
        .is_err());
    assert!(main
        .execute(
            "DELETE FROM text_embedding_index WHERE id = ?1",
            [JINA_INDEX_ID],
        )
        .is_err());
}

#[test]
fn jina_v1_golden_fixtures_match_the_manifest() {
    let manifest = embedding_index::jina_v1_manifest();
    let fixtures: serde_json::Value =
        serde_json::from_str(embedding_index::JINA_V1_GOLDEN_JSON).expect("golden fixture JSON");

    assert_eq!(fixtures["fixtureVersion"], 1);
    assert_eq!(
        fixtures["modelFileSha256"],
        manifest.model_file_sha256.as_str()
    );
    let cases = fixtures["cases"].as_array().expect("golden cases");
    assert_eq!(cases.len(), 2);

    for case in cases {
        let name = case["name"].as_str().expect("fixture name");
        let input = case["input"].as_str().expect("fixture input");
        let expected_prefix = match name {
            "query" => &manifest.config.query_prefix,
            "document" => &manifest.config.document_prefix,
            _ => panic!("unexpected fixture {name}"),
        };
        assert!(input.starts_with(expected_prefix));

        let embedding = case["embedding"].as_array().expect("fixture embedding");
        assert_eq!(embedding.len(), manifest.dimension as usize);
        let norm = embedding
            .iter()
            .map(|value| {
                let value = value.as_f64().expect("finite embedding component");
                value * value
            })
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() <= 0.00001, "{name} norm is {norm}");
    }
}

#[test]
fn ordinary_tables_are_strict_and_media_stores_only_blob_lifecycle_state() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");

    for table in [
        "card_content",
        "image",
        "image_draft_lease",
        "media_blob_reap_candidate",
        "review_card",
        "review_event",
        "scheduler_config",
        "search_document",
        "text_embedding",
        "text_embedding_index",
    ] {
        let strict: i64 = main
            .query_row(
                "SELECT strict FROM pragma_table_list WHERE name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("strict table");
        assert_eq!(strict, 1, "{table} must be STRICT");
    }

    let media_tables: Vec<String> = media
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .expect("media tables")
        .query_map([], |row| row.get(0))
        .expect("media rows")
        .collect::<rusqlite::Result<_>>()
        .expect("media table names");
    assert_eq!(
        media_tables,
        vec![
            "media_blob",
            "media_blob_reap_authorization",
            "refinery_schema_history"
        ]
    );
}

#[test]
fn fts_triggers_work_with_trusted_schema_disabled() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    insert_basic_content(&main);
    main.execute(
        "INSERT INTO search_document(rowid, card_content_id, body, content_hash, updated_at)
         VALUES(1, ?1, 'mitochondria produce energy', zeroblob(32), 100)",
        [BASIC_CONTENT_ID],
    )
    .expect("search document insert through FTS trigger");

    let match_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'mitochondria'",
            [],
            |row| row.get(0),
        )
        .expect("FTS query");
    assert_eq!(match_count, 1);

    main.execute(
        "UPDATE search_document SET body = 'ribosome translation', updated_at = 101 WHERE rowid = 1",
        [],
    )
    .expect("search document update through FTS trigger");
    let old_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'mitochondria'",
            [],
            |row| row.get(0),
        )
        .expect("old FTS query");
    let new_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'ribosome'",
            [],
            |row| row.get(0),
        )
        .expect("new FTS query");
    assert_eq!((old_count, new_count), (0, 1));
}

#[test]
fn trigram_search_migration_rebuilds_existing_documents() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let v1 = migrations::main_runner()
        .get_migrations()
        .iter()
        .find(|migration| migration.version() == 1)
        .expect("V1 migration")
        .clone();
    Runner::new(&[v1])
        .set_grouped(true)
        .run(&mut main)
        .expect("V1 schema");

    insert_basic_content(&main);
    main.execute(
        "INSERT INTO search_document(rowid, card_content_id, body, content_hash, updated_at)
         VALUES(1, ?1, 'prefix ffsef suffix', zeroblob(32), 100)",
        [BASIC_CONTENT_ID],
    )
    .expect("pre-migration search document");

    migrations::run_main(&mut main).expect("trigram migration");
    let match_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'sef'",
            [],
            |row| row.get(0),
        )
        .expect("substring query after rebuild");
    assert_eq!(match_count, 1);
}

#[test]
fn durable_ocr_migration_queues_unrecognized_images_and_preserves_existing_text() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let available = migrations::main_runner().get_migrations().clone();
    let first_two = [1, 2]
        .map(|version| {
            available
                .iter()
                .find(|migration| migration.version() == version)
                .unwrap_or_else(|| panic!("V{version} migration"))
                .clone()
        })
        .to_vec();
    Runner::new(&first_two)
        .set_grouped(true)
        .run(&mut main)
        .expect("V1 and V2 schema");

    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text
         ) VALUES (?1, 100, 120, NULL, zeroblob(32), 'image/webp', 10, 20, '')",
        [IMAGE_ID],
    )
    .expect("unrecognized image");
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text
         ) VALUES (
            '01980c8e-6c00-7000-8000-000000000106', 100, 130, NULL,
            randomblob(32), 'image/webp', 10, 20, 'existing OCR'
         )",
        [],
    )
    .expect("recognized image");

    migrations::run_main(&mut main).expect("durable OCR migration");
    let pending = main
        .query_row(
            "SELECT ocr_status, ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at,
                    ocr_started_at, ocr_error
             FROM image WHERE id = ?1",
            [IMAGE_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .expect("pending migrated image");
    assert_eq!(
        pending,
        (
            ImageOcrStatus::Pending.as_db_str().into(),
            OcrQueueState::Pending.as_db_str().into(),
            0,
            Some(120),
            None,
            None,
        )
    );
    let ready = main
        .query_row(
            "SELECT ocr_status, ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at,
                    ocr_started_at, ocr_error, ocr_text
             FROM image WHERE id = '01980c8e-6c00-7000-8000-000000000106'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .expect("ready migrated image");
    assert_eq!(
        ready,
        (
            ImageOcrStatus::Ready.as_db_str().into(),
            OcrQueueState::Ready.as_db_str().into(),
            0,
            None,
            None,
            None,
            "existing OCR".into(),
        )
    );
}

#[test]
fn state_checks_partial_uniqueness_and_append_only_history_are_enforced() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    assert!(main
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (
                ?1, 100, 100, NULL, ?2, 'question', 'answer', NULL
             )",
            params![
                "01980c8e-6c00-7000-8000-00000000010-",
                CardContentType::Basic.as_db_str()
            ],
        )
        .is_err());
    insert_basic_content(&main);

    let invalid_new = main.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
            variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
            scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, 100, 100, NULL, ?2, ?3, NULL, 'basic', ?4, 100, NULL, NULL,
            0, 0, NULL, NULL, NULL
         )",
        params![
            BASIC_CARD_ID,
            BASIC_CONTENT_ID,
            ReviewCardStatus::Active.as_db_str(),
            ReviewCardState::New.as_db_str()
        ],
    );
    assert!(invalid_new.is_err());

    main.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
            variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
            scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, 100, 100, NULL, ?2, ?3, NULL, 'basic', ?4, NULL, 200,
            100, 1, 0, ?5, 1, '{}'
         )",
        params![
            BASIC_CARD_ID,
            BASIC_CONTENT_ID,
            ReviewCardStatus::Active.as_db_str(),
            ReviewCardState::Review.as_db_str(),
            DEFAULT_CONFIG_ID
        ],
    )
    .expect("valid review card");

    let duplicate = main.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
            variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
            scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, 101, 101, NULL, ?2, ?3, NULL, 'basic', ?4, NULL, NULL, NULL,
            0, 0, NULL, NULL, NULL
         )",
        params![
            SECOND_CARD_ID,
            BASIC_CONTENT_ID,
            ReviewCardStatus::Active.as_db_str(),
            ReviewCardState::New.as_db_str()
        ],
    );
    assert!(duplicate.is_err());

    main.execute(
        "INSERT INTO review_event (
            id, created_at, event_schema_version, event_type, review_card_id, card_sequence,
            reviewed_at, study_day, timezone_id, utc_offset_minutes, grade,
            scheduler_config_id, scheduler_log_json, target_event_id
         ) VALUES (
            ?1, 100, 1, ?2, ?3, 1, 100, 20000, 'America/New_York', -240, 3,
            ?4, '{}', NULL
         )",
        params![
            REVIEW_EVENT_ID,
            ReviewEventType::Review.as_db_str(),
            BASIC_CARD_ID,
            DEFAULT_CONFIG_ID
        ],
    )
    .expect("review event");
    assert!(main
        .execute(
            "UPDATE review_event SET grade = 4 WHERE id = ?1",
            [REVIEW_EVENT_ID]
        )
        .is_err());
    assert!(main
        .execute("DELETE FROM review_card WHERE id = ?1", [BASIC_CARD_ID])
        .is_err());
    assert!(main.execute("DELETE FROM app_settings", []).is_err());
}

#[test]
fn pending_migrations_require_a_valid_snapshot_before_changes() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);

    let database = initialize_test(&paths);
    drop(database);

    let manifests: Vec<_> = fs::read_dir(&paths.backups)
        .expect("backups")
        .map(|entry| entry.expect("backup entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    assert_eq!(manifests.len(), 1);
    let manifest = snapshot::load_and_validate_manifest(&manifests[0]).expect("manifest");
    assert_eq!(manifest.main.migration_head, None);
    assert_eq!(manifest.media.migration_head, None);

    let restore_directory = tempfile::tempdir().expect("restore directory");
    let restored = DatabasePaths::new(restore_directory.path().join("restored"));
    snapshot::restore_snapshot_pair(&manifests[0], &restored).expect("restore");
    let mut restored_main =
        connection::open_read_only(&restored.main, DatabaseKind::Main).expect("restored main");
    let mut restored_media =
        connection::open_read_only(&restored.media, DatabaseKind::Media).expect("restored media");
    assert_eq!(
        migrations::current_heads(&mut restored_main, &mut restored_media).expect("restored heads"),
        migrations::MigrationHeads {
            main: None,
            media: None
        }
    );
}

#[test]
fn failed_snapshot_gate_leaves_unmigrated_files_unchanged() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    fs::write(&paths.backups, b"not a directory").expect("blocking backup path");

    assert!(initialize(paths.clone(), "test", no_launch_snapshot()).is_err());
    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");
    assert!(!table_exists(&main, "refinery_schema_history"));
    assert!(!table_exists(&media, "refinery_schema_history"));
}

#[test]
fn swapped_or_incomplete_database_pairs_fail_closed() {
    let (_directory, paths) = test_paths();
    fs::create_dir_all(paths.root()).expect("database root");
    drop(
        connection::open_writer(&paths.main, DatabaseKind::Media, FileState::Fresh)
            .expect("swapped main"),
    );
    drop(
        connection::open_writer(&paths.media, DatabaseKind::Main, FileState::Fresh)
            .expect("swapped media"),
    );
    assert!(matches!(
        initialize(paths.clone(), "test", no_launch_snapshot()),
        Err(DatabaseError::WrongApplicationId { .. })
    ));

    let (_directory, incomplete) = test_paths();
    fs::create_dir_all(incomplete.root()).expect("database root");
    drop(
        connection::open_writer(&incomplete.main, DatabaseKind::Main, FileState::Fresh)
            .expect("main only"),
    );
    assert!(matches!(
        initialize(incomplete, "test", no_launch_snapshot()),
        Err(DatabaseError::IncompletePair { .. })
    ));
}

#[test]
fn changed_checksums_and_future_heads_are_rejected() {
    let (_directory, divergent) = test_paths();
    drop(initialize_test(&divergent));
    let main = open_existing(&divergent.main, DatabaseKind::Main);
    main.execute(
        "UPDATE refinery_schema_history SET checksum = '0' WHERE version = 1",
        [],
    )
    .expect("change checksum");
    drop(main);
    assert!(matches!(
        initialize(divergent, "test", no_launch_snapshot()),
        Err(DatabaseError::IncompatibleMigrationHistory { .. })
    ));

    let (_directory, future) = test_paths();
    drop(initialize_test(&future));
    let main = open_existing(&future.main, DatabaseKind::Main);
    main.execute(
        "INSERT INTO refinery_schema_history(version, name, applied_on, checksum)
         SELECT 14, 'future', applied_on, '0'
         FROM refinery_schema_history WHERE version = 1",
        [],
    )
    .expect("future migration");
    drop(main);
    assert!(matches!(
        initialize(future, "test", no_launch_snapshot()),
        Err(DatabaseError::IncompatibleMigrationHistory { .. })
    ));
}

#[test]
fn grouped_refinery_run_rolls_back_all_pending_migrations() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let mut main = open_existing(&paths.main, DatabaseKind::Main);

    let mut all = migrations::main_runner().get_migrations().clone();
    all.push(
        Migration::unapplied(
            "V14__grouped_good.sql",
            "CREATE TABLE grouped_good(id INTEGER PRIMARY KEY) STRICT;",
        )
        .expect("V14 migration"),
    );
    all.push(
        Migration::unapplied(
            "V15__grouped_failure.sql",
            "CREATE TABLE grouped_failure(id INTEGER) STRICT; THIS IS NOT SQL;",
        )
        .expect("V15 migration"),
    );
    let runner = Runner::new(&all).set_grouped(true);
    assert!(runner.run(&mut main).is_err());
    assert!(!table_exists(&main, "grouped_good"));
    assert_eq!(
        migrations::main_runner()
            .get_last_applied_migration(&mut main)
            .expect("last migration")
            .expect("V13")
            .version(),
        13
    );
}

#[test]
fn refinery_rejects_a_missing_migration_from_an_applied_history() {
    let mut connection = Connection::open_in_memory().expect("in-memory database");
    let first = Migration::unapplied(
        "V1__first.sql",
        "CREATE TABLE first(id INTEGER PRIMARY KEY) STRICT;",
    )
    .expect("first migration");
    let second = Migration::unapplied(
        "V2__second.sql",
        "CREATE TABLE second(id INTEGER PRIMARY KEY) STRICT;",
    )
    .expect("second migration");
    Runner::new(&[first.clone(), second.clone()])
        .set_grouped(true)
        .run(&mut connection)
        .expect("initial history");

    let missing_runner = Runner::new(&[second])
        .set_abort_missing(true)
        .set_abort_divergent(true);
    assert!(missing_runner.run(&mut connection).is_err());
}

#[test]
fn launch_snapshot_runs_in_background_and_retention_keeps_seven_daily_points() {
    let (_directory, paths) = test_paths();
    let database = initialize(
        paths.clone(),
        "test",
        InitializationOptions {
            launch_snapshot: true,
        },
    )
    .expect("database with launch snapshot");
    let launch = database
        .wait_for_launch_snapshot()
        .expect("launch snapshot result")
        .expect("launch snapshot");
    assert!(launch.manifest_path.exists());
    assert_eq!(launch.manifest.main.migration_head, Some(13));
    drop(database);

    let base = launch.manifest.created_at;
    for age_days in 1_i64..10 {
        let mut created = snapshot::create_snapshot_pair(&paths, "test").expect("snapshot");
        created.manifest.created_at = base - age_days * 86_400_000;
        fs::write(
            &created.manifest_path,
            serde_json::to_vec_pretty(&created.manifest).expect("manifest JSON"),
        )
        .expect("rewrite test timestamp");
    }
    snapshot::prune_snapshots(&paths.backups).expect("retention");
    let manifests = fs::read_dir(&paths.backups)
        .expect("backups")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .count();
    assert!((7..10).contains(&manifests));
}

#[test]
fn snapshot_hash_tampering_is_detected_before_restore() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let created = snapshot::create_snapshot_pair(&paths, "test").expect("snapshot");
    let main_path = paths.backups.join(&created.manifest.main.file_name);
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(main_path)
        .expect("snapshot file");
    file.write_all(b"tamper").expect("tamper snapshot");
    assert!(matches!(
        snapshot::load_and_validate_manifest(&created.manifest_path),
        Err(DatabaseError::InvalidSnapshot(_))
    ));
}

#[test]
fn foreign_keys_mask_bounds_and_fts_rebuild_are_enforced() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    insert_basic_content(&main);

    assert!(main
        .execute(
            "INSERT INTO card_content_image(card_content_id, image_id) VALUES(?1, ?2)",
            params![BASIC_CONTENT_ID, IMAGE_ID],
        )
        .is_err());

    main.execute(
        "INSERT INTO search_document(rowid, card_content_id, body, content_hash, updated_at)
         VALUES(1, ?1, 'deterministic rebuild', zeroblob(32), 100)",
        [BASIC_CONTENT_ID],
    )
    .expect("search document");
    main.execute(
        "INSERT INTO search_document_fts(search_document_fts) VALUES('rebuild')",
        [],
    )
    .expect("FTS rebuild");
    assert_eq!(
        main.query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'deterministic'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("rebuilt FTS query"),
        1
    );

    let media = open_existing(&paths.media, DatabaseKind::Media);
    let bytes = b"occlusion image";
    let hash = Sha256::digest(bytes).to_vec();
    media
        .execute(
            "INSERT INTO media_blob(sha256, bytes) VALUES(?1, ?2)",
            params![&hash, bytes.as_slice()],
        )
        .expect("media blob");
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_next_attempt_at
         ) VALUES (?1, 100, 100, NULL, ?2, 'image/webp', 100, 100, '', 100)",
        params![IMAGE_ID, &hash],
    )
    .expect("image");
    assert!(main
        .execute(
            "UPDATE image SET ocr_queue_state = ?1 WHERE id = ?2",
            params![OcrQueueState::Running.as_db_str(), IMAGE_ID],
        )
        .is_err());
    main.execute(
        "INSERT INTO card_occlusion_content (
            id, created_at, updated_at, deleted_at, card_content_id, source_image_id, mode
         ) VALUES (
            '01980c8e-6c00-7000-8000-000000000106', 100, 100, NULL, ?1, ?2,
            'HIDE_ONE_GUESS_ONE'
         )",
        params![BASIC_CONTENT_ID, IMAGE_ID],
    )
    .expect("occlusion content");
    main.execute(
        "INSERT INTO card_occlusion_mask_layer (
            id, created_at, updated_at, deleted_at, card_occlusion_content_id, label, sort_order
         ) VALUES (
            '01980c8e-6c00-7000-8000-000000000107', 100, 100, NULL,
            '01980c8e-6c00-7000-8000-000000000106', NULL, 0
         )",
        [],
    )
    .expect("mask layer");
    assert!(main
        .execute(
            "INSERT INTO card_occlusion_mask (
                id, created_at, updated_at, deleted_at, card_occlusion_mask_layer_id,
                x, y, width, height, color
             ) VALUES (
                '01980c8e-6c00-7000-8000-000000000108', 100, 100, NULL,
                '01980c8e-6c00-7000-8000-000000000107', 0.8, 0.1, 0.3, 0.2, 'WHITE'
             )",
            [],
        )
        .is_err());
}

#[test]
fn media_reconciliation_detects_missing_or_corrupt_blobs_and_tolerates_extras() {
    let (_directory, valid) = test_paths();
    drop(initialize_test(&valid));
    let media = open_existing(&valid.media, DatabaseKind::Media);
    let bytes = b"canonical webp bytes";
    let hash = Sha256::digest(bytes).to_vec();
    media
        .execute(
            "INSERT INTO media_blob(sha256, bytes) VALUES(?1, ?2)",
            params![&hash, bytes.as_slice()],
        )
        .expect("media first");
    drop(media);
    let main = open_existing(&valid.main, DatabaseKind::Main);
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_next_attempt_at
         ) VALUES (?1, 100, 100, NULL, ?2, 'image/webp', 10, 20, '', 100)",
        params![IMAGE_ID, &hash],
    )
    .expect("image metadata second");
    drop(main);
    let database = initialize_test(&valid);
    drop(database);

    let (_directory, missing) = test_paths();
    drop(initialize_test(&missing));
    let main = open_existing(&missing.main, DatabaseKind::Main);
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_next_attempt_at
         ) VALUES (?1, 100, 100, NULL, zeroblob(32), 'image/webp', 10, 20, '', 100)",
        [IMAGE_ID],
    )
    .expect("dangling image metadata");
    drop(main);
    assert!(matches!(
        initialize(missing, "test", no_launch_snapshot()),
        Err(DatabaseError::Validation {
            kind: "database pair",
            ..
        })
    ));

    let (_directory, extra) = test_paths();
    drop(initialize_test(&extra));
    let media = open_existing(&extra.media, DatabaseKind::Media);
    media
        .execute(
            "INSERT INTO media_blob(sha256, bytes) VALUES(?1, ?2)",
            params![&hash, bytes.as_slice()],
        )
        .expect("unreferenced blob");
    drop(media);
    drop(initialize_test(&extra));
}
