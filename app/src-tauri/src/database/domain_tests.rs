use std::collections::HashMap;

use rusqlite::{params, Connection};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{
    connection::{self, DatabaseKind, FileState},
    domain::{
        CardContent, CardContentDraft, CardContentReviewStatus, CardContentType,
        DeleteCardContentInput, MutationDisposition, OcclusionDefinitionDraft, OcclusionMaskColor,
        OcclusionMaskDraft, OcclusionMaskLayerDraft, OcclusionMode, ReviewCardCache,
        ReviewCardState, ReviewCardStatus, ReviewEventType, ReviewFact, SchedulerLogV1,
        SearchCardContentInput, SetCardContentSuspendedInput, UpdateCardContentInput,
    },
    embedding_index::InstallEmbeddingDisposition,
    initialize, CanonicalImage, Database, DatabaseError, DatabasePaths, InitializationOptions,
    RecordGradeInput, ReviewContext, SearchMaintenanceOperation, UndoLastGradeInput,
};

const FIXTURE_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000101";
const MEDIA_LEASE_A: &str = "01980c8e-6c00-7000-8000-000000000911";
const MEDIA_LEASE_B: &str = "01980c8e-6c00-7000-8000-000000000912";
const MEDIA_LEASE_C: &str = "01980c8e-6c00-7000-8000-000000000913";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistenceFixture {
    schema_version: i64,
    card_id: String,
    scheduler_config_id: String,
    steps: Vec<FixtureStep>,
    undo: FixtureUndo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureStep {
    event_id: String,
    review: ReviewFact,
    expected_elapsed_days: i64,
    expected_cache: ReviewCardCache,
    expected_scheduler_log: SchedulerLogV1,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureUndo {
    event_id: String,
    target_event_id: String,
    expected_cache: ReviewCardCache,
}

fn fixture() -> PersistenceFixture {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/scheduling/review-sequence-v1.json"
    ))
    .expect("persistence fixture")
}

fn test_paths() -> (TempDir, DatabasePaths) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let paths = DatabasePaths::new(directory.path().join("data"));
    (directory, paths)
}

fn initialize_test(paths: &DatabasePaths) -> Database {
    initialize(
        paths.clone(),
        "test",
        InitializationOptions {
            launch_snapshot: false,
        },
    )
    .expect("database initialization")
}

fn open_existing(paths: &DatabasePaths) -> Connection {
    connection::open_writer(&paths.main, DatabaseKind::Main, FileState::Existing)
        .expect("existing main database")
}

fn seed_fixture_card(paths: &DatabasePaths, fixture: &PersistenceFixture) {
    drop(initialize_test(paths));
    let connection = open_existing(paths);
    let created_at = 1_783_920_000_000_i64;
    let search_body = "fixture front\n\u{1e}\nfixture back";
    let content_hash = Sha256::digest(search_body.as_bytes());
    connection
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (?1, ?2, ?2, NULL, ?3, 'fixture front', 'fixture back', NULL)",
            params![
                FIXTURE_CONTENT_ID,
                created_at,
                CardContentType::Basic.as_db_str()
            ],
        )
        .expect("fixture content");
    connection
        .execute(
            "INSERT INTO review_card (
                id, created_at, updated_at, deleted_at, card_content_id, status,
                suspended_at, variant_key, state, due_at, due_study_day,
                last_review_at, reps, lapses, scheduler_config_id,
                scheduler_state_schema_version, scheduler_state_json
             ) VALUES (
                ?1, ?2, ?2, NULL, ?3, ?4, NULL, 'basic', ?5, NULL,
                NULL, NULL, 0, 0, NULL, NULL, NULL
             )",
            params![
                fixture.card_id,
                created_at,
                FIXTURE_CONTENT_ID,
                ReviewCardStatus::Active.as_db_str(),
                ReviewCardState::New.as_db_str()
            ],
        )
        .expect("fixture review card");
    connection
        .execute(
            "INSERT INTO search_document (card_content_id, body, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                FIXTURE_CONTENT_ID,
                search_body,
                content_hash.as_slice(),
                created_at
            ],
        )
        .expect("fixture search document");
}

fn basic_draft(front: &str, back: &str, source: Option<&str>) -> CardContentDraft {
    CardContentDraft::Basic {
        front_md: front.into(),
        back_md: back.into(),
        source: source.map(str::to_owned),
    }
}

fn cloze_draft(
    front: &str,
    back: &str,
    source: Option<&str>,
    variant_keys: &[&str],
    search_md: &str,
) -> CardContentDraft {
    CardContentDraft::Cloze {
        front_md: front.into(),
        back_md: back.into(),
        source: source.map(str::to_owned),
        variant_keys: variant_keys.iter().map(|key| (*key).to_owned()).collect(),
        search_md: search_md.into(),
    }
}

fn occlusion_draft(
    source_image_id: &str,
    definition_id: &str,
    mode: OcclusionMode,
    layers: Vec<OcclusionMaskLayerDraft>,
) -> CardContentDraft {
    CardContentDraft::Occlusion {
        front_md: "Identify the covered structure.".into(),
        back_md: "Anatomy plate".into(),
        source: Some("Atlas".into()),
        occlusion: OcclusionDefinitionDraft {
            id: definition_id.into(),
            source_image_id: source_image_id.into(),
            mode,
            layers,
        },
    }
}

fn occlusion_layer(
    id: &str,
    label: &str,
    masks: &[(&str, f64, f64, f64, f64, OcclusionMaskColor)],
) -> OcclusionMaskLayerDraft {
    OcclusionMaskLayerDraft {
        id: id.into(),
        label: Some(label.into()),
        masks: masks
            .iter()
            .map(|(id, x, y, width, height, color)| OcclusionMaskDraft {
                id: (*id).into(),
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                color: *color,
            })
            .collect(),
    }
}

fn grade_input(context: &ReviewContext, step: &FixtureStep) -> RecordGradeInput {
    RecordGradeInput {
        event_id: step.event_id.clone(),
        review_card_id: context.review_card.id.clone(),
        expected_review_card_updated_at: context.review_card.updated_at,
        expected_card_content_updated_at: context.card_content.updated_at(),
        expected_card_sequence: context.last_card_sequence,
        expected_scheduler_config_id: context.scheduler_config.id.clone(),
        review: step.review.clone(),
        next_cache: step.expected_cache.clone(),
        scheduler_log: step.expected_scheduler_log.clone(),
    }
}

#[test]
fn creates_a_basic_card_and_loads_its_scheduling_context() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let context = database
        .create_card_content(basic_draft(
            "What is the capital of France?",
            "Paris",
            Some("Geography notes"),
        ))
        .expect("basic card");

    let CardContent::Basic {
        front_md, back_md, ..
    } = &context.card_content
    else {
        panic!("expected BASIC content");
    };
    assert_eq!(front_md, "What is the capital of France?");
    assert_eq!(back_md, "Paris");
    assert_eq!(context.review_card.variant_key, "basic");
    assert_eq!(context.cache.state, ReviewCardState::New);
    assert_eq!(context.last_card_sequence, 0);
    assert!(context.review_history.is_empty());
    assert_eq!(context.scheduler_config.library_version, "5.4.1");
    assert_eq!(
        database
            .load_review_context(context.review_card.id.clone())
            .expect("reloaded context"),
        context
    );

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let row: (i64, String) = connection
        .query_row(
            "SELECT count(*), body FROM search_document WHERE card_content_id = ?1",
            [context.card_content.id()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("search document");
    assert_eq!(row.0, 1);
    assert_eq!(
        row.1,
        "What is the capital of France?\n\u{1e}\nParis\n\u{1e}\nGeography notes"
    );
}

#[test]
fn image_ingestion_deduplicates_and_ocr_fans_out_into_search() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let canonical = CanonicalImage {
        bytes: b"canonical test webp".to_vec(),
        natural_width: 800,
        natural_height: 600,
    };
    let first = database
        .ingest_image(canonical.clone())
        .expect("first image ingestion");
    let duplicate = database
        .ingest_image(canonical)
        .expect("deduplicated image ingestion");
    assert_eq!(duplicate, first);

    let token = format!("{{{{image:{};width=75%}}}}", first.id);
    let context = database
        .create_card_content(basic_draft(&token, "The mitochondrion", None))
        .expect("image-only card front");
    let cloze_front = format!("The {{{{c1::mitochondrion}}}} is shown below.\n\n{token}");
    let cloze_search = format!("The mitochondrion is shown below.\n\n{token}");
    let cloze = database
        .create_card_content(cloze_draft(
            &cloze_front,
            "",
            None,
            &["cloze:1"],
            &cloze_search,
        ))
        .expect("cloze card with inline image");

    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main database");
    let media =
        connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media database");
    assert_eq!(
        main.query_row("SELECT count(*) FROM image", [], |row| row.get::<_, i64>(0))
            .expect("image count"),
        1
    );
    assert_eq!(
        media
            .query_row("SELECT count(*) FROM media_blob", [], |row| row
                .get::<_, i64>(0))
            .expect("blob count"),
        1
    );
    assert_eq!(
        main.query_row(
            "SELECT count(*) FROM card_content_image WHERE image_id = ?1",
            [&first.id],
            |row| row.get::<_, i64>(0),
        )
        .expect("derived image references"),
        2
    );
    let before_ocr: String = main
        .query_row(
            "SELECT body FROM search_document WHERE card_content_id = ?1",
            [context.card_content.id()],
            |row| row.get(0),
        )
        .expect("search document before OCR");
    assert!(!before_ocr.contains("{{image:"));
    assert!(!before_ocr.contains("ATP synthase"));
    drop(main);
    drop(media);

    let claimed = database
        .claim_next_ocr_job(super::media::now_millis().expect("claim time"))
        .expect("OCR claim")
        .expect("pending OCR job");
    assert_eq!(claimed.image_id, first.id);
    database
        .complete_image_ocr(
            &claimed,
            Ok("ATP synthase inner membrane".into()),
            super::media::now_millis().expect("completion time"),
        )
        .expect("OCR completion");
    let results = database
        .search_card_content(SearchCardContentInput {
            query: "ATP synthase".into(),
            limit: 10,
            offset: 0,
        })
        .expect("OCR lexical search");
    let result_ids = results
        .iter()
        .map(|result| result.card_content.id())
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(result_ids.contains(&context.card_content.id()));
    assert!(result_ids.contains(&cloze.card_content.id()));
}

#[test]
fn image_ocr_queue_retries_with_backoff_and_stops_at_the_attempt_limit() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .ingest_image(CanonicalImage {
            bytes: b"retryable canonical test webp".to_vec(),
            natural_width: 640,
            natural_height: 480,
        })
        .expect("image ingestion");
    let mut claim_at = super::media::now_millis().expect("initial claim time");

    for attempt_count in 1..=super::media::MAX_OCR_ATTEMPTS {
        let job = database
            .claim_next_ocr_job(claim_at)
            .expect("OCR claim")
            .expect("eligible OCR job");
        assert_eq!(job.image_id, image.id);
        assert_eq!(job.attempt_count, attempt_count);

        let running = image_ocr_queue_row(&paths, &image.id);
        assert_eq!(running.0, super::media::OcrQueueState::Running.as_db_str());
        assert_eq!(running.1, attempt_count);
        assert_eq!(running.2, None);
        assert_eq!(running.3, Some(claim_at));

        let completed_at = claim_at + 1;
        database
            .complete_image_ocr(&job, Err("temporary Vision failure".into()), completed_at)
            .expect("failed OCR completion");
        let waiting = image_ocr_queue_row(&paths, &image.id);
        assert_eq!(waiting.1, attempt_count);
        assert_eq!(waiting.3, None);
        assert_eq!(waiting.4.as_deref(), Some("temporary Vision failure"));

        if attempt_count < super::media::MAX_OCR_ATTEMPTS {
            let retry_at =
                completed_at + super::media::OCR_RETRY_DELAYS_MILLIS[(attempt_count - 1) as usize];
            assert_eq!(
                waiting.0,
                super::media::OcrQueueState::RetryWait.as_db_str()
            );
            assert_eq!(waiting.2, Some(retry_at));
            assert!(database
                .claim_next_ocr_job(retry_at - 1)
                .expect("early OCR claim")
                .is_none());
            claim_at = retry_at;
        } else {
            assert_eq!(waiting.0, super::ImageOcrStatus::Failed.as_db_str());
            assert_eq!(waiting.2, None);
        }
    }

    assert!(database
        .claim_next_ocr_job(claim_at + 1)
        .expect("terminal OCR claim")
        .is_none());
}

#[test]
fn image_ocr_queue_recovers_only_abandoned_running_attempts() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .ingest_image(CanonicalImage {
            bytes: b"interrupted canonical test webp".to_vec(),
            natural_width: 720,
            natural_height: 540,
        })
        .expect("image ingestion");
    let claimed_at = super::media::now_millis().expect("claim time");
    let first = database
        .claim_next_ocr_job(claimed_at)
        .expect("OCR claim")
        .expect("pending OCR job");

    let untouched = database
        .recover_interrupted_ocr_jobs(claimed_at - 1, claimed_at + 10)
        .expect("non-stale recovery");
    assert_eq!(untouched, super::OcrQueueRecovery::default());
    assert_eq!(
        image_ocr_queue_row(&paths, &image.id).0,
        super::media::OcrQueueState::Running.as_db_str()
    );

    let recovered_at = claimed_at + 20;
    let recovered = database
        .recover_interrupted_ocr_jobs(claimed_at, recovered_at)
        .expect("stale recovery");
    assert_eq!(
        recovered,
        super::OcrQueueRecovery {
            requeued: 1,
            terminally_failed: 0,
        }
    );
    let row = image_ocr_queue_row(&paths, &image.id);
    assert_eq!(row.0, super::media::OcrQueueState::RetryWait.as_db_str());
    assert_eq!(row.1, first.attempt_count);
    assert_eq!(row.2, Some(recovered_at));
    assert_eq!(row.3, None);

    let mut current = database
        .claim_next_ocr_job(recovered_at)
        .expect("recovered OCR claim")
        .expect("recovered OCR job");
    assert_eq!(current.image_id, image.id);
    assert_eq!(current.attempt_count, first.attempt_count + 1);

    let mut current_started_at = recovered_at;
    while current.attempt_count < super::media::MAX_OCR_ATTEMPTS {
        let next_recovery_at = current_started_at + 1;
        let recovery = database
            .recover_interrupted_ocr_jobs(current_started_at, next_recovery_at)
            .expect("repeat stale recovery");
        assert_eq!(recovery.requeued, 1);
        assert_eq!(recovery.terminally_failed, 0);
        current = database
            .claim_next_ocr_job(next_recovery_at)
            .expect("repeat recovered OCR claim")
            .expect("repeat recovered OCR job");
        current_started_at = next_recovery_at;
    }

    let terminal = database
        .recover_interrupted_ocr_jobs(current_started_at, current_started_at + 1)
        .expect("terminal stale recovery");
    assert_eq!(terminal.requeued, 0);
    assert_eq!(terminal.terminally_failed, 1);
    let failed = image_ocr_queue_row(&paths, &image.id);
    assert_eq!(failed.0, super::ImageOcrStatus::Failed.as_db_str());
    assert_eq!(failed.1, super::media::MAX_OCR_ATTEMPTS);
    assert_eq!(failed.2, None);
    assert_eq!(failed.3, None);
}

fn image_ocr_queue_row(
    paths: &DatabasePaths,
    image_id: &str,
) -> (String, u32, Option<i64>, Option<i64>, Option<String>) {
    connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("main database")
        .query_row(
            "SELECT ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at,
                    ocr_started_at, ocr_error
             FROM image WHERE id = ?1",
            [image_id],
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
        .expect("image OCR queue row")
}

#[test]
fn expired_draft_media_is_aged_before_reaping_and_is_not_claimed_for_ocr() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let bytes = b"cancelled quick add image".to_vec();
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: bytes.clone(),
                natural_width: 800,
                natural_height: 600,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("leased image ingestion");
    let lease_expires_at = media_lease_expiry(&paths, MEDIA_LEASE_A);
    assert!(database
        .claim_next_ocr_job(lease_expires_at)
        .expect("orphan OCR claim")
        .is_none());

    let expired = database
        .maintain_media(lease_expires_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("lease-expiry maintenance");
    assert_eq!(expired.integrity.orphaned_image_ids, vec![image.id.clone()]);
    assert_eq!(expired.cleanup.retired_image_count, 0);

    let before_grace = database
        .maintain_media(
            lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS - 1,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("pre-grace maintenance");
    assert_eq!(
        before_grace.integrity.orphaned_image_ids,
        vec![image.id.clone()]
    );
    assert_eq!(before_grace.cleanup.retired_image_count, 0);

    let reaped = database
        .maintain_media(
            lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("post-grace maintenance");
    assert_eq!(reaped.cleanup.retired_image_count, 1);
    assert_eq!(reaped.cleanup.deleted_blob_count, 1);
    assert_eq!(reaped.cleanup.reclaimed_bytes, bytes.len() as u64);
    assert_eq!(media_row_counts(&paths), (0, 0));

    let removed_before_save = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"removed before card save".to_vec(),
                natural_width: 400,
                natural_height: 300,
            },
            MEDIA_LEASE_B.into(),
        )
        .expect("removed draft image");
    database
        .client()
        .create_card_content(
            basic_draft("saved without the image", "answer", None),
            MEDIA_LEASE_B.into(),
        )
        .expect("card saved after image removal");
    let orphaned_at: i64 = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("main database")
        .query_row(
            "SELECT orphaned_at FROM image WHERE id = ?1",
            [&removed_before_save.id],
            |row| row.get(0),
        )
        .expect("durable orphan timestamp");
    let removed_reap = database
        .maintain_media(
            orphaned_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("removed image maintenance");
    assert_eq!(removed_reap.cleanup.retired_image_count, 1);
    assert_eq!(media_row_counts(&paths), (0, 0));
}

#[test]
fn reattachment_and_saved_deduplication_cancel_orphan_eligibility() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let canonical = CanonicalImage {
        bytes: b"reattached deduplicated image".to_vec(),
        natural_width: 900,
        natural_height: 700,
    };
    let image = database
        .client()
        .ingest_image(canonical.clone(), MEDIA_LEASE_A.into())
        .expect("first draft image");
    let first_expiry = media_lease_expiry(&paths, MEDIA_LEASE_A);
    database
        .maintain_media(first_expiry, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("first lease expiry");

    let duplicate = database
        .client()
        .ingest_image(canonical, MEDIA_LEASE_B.into())
        .expect("reattached image");
    assert_eq!(duplicate.id, image.id);
    let old_grace_end = first_expiry + super::media::MEDIA_ORPHAN_GRACE_MILLIS;
    database
        .client()
        .renew_media_lease(MEDIA_LEASE_B.into(), old_grace_end)
        .expect("lease renewal");
    let protected = database
        .maintain_media(old_grace_end, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("reattachment maintenance");
    assert!(protected.integrity.orphaned_image_ids.is_empty());
    assert_eq!(
        protected.cleanup,
        super::media::MediaCleanupResult::default()
    );

    let token = format!("{{{{image:{};width=75%}}}}", image.id);
    database
        .client()
        .create_card_content(
            basic_draft(&token, "saved answer", None),
            MEDIA_LEASE_B.into(),
        )
        .expect("saved image card");
    let third = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"reattached deduplicated image".to_vec(),
                natural_width: 900,
                natural_height: 700,
            },
            MEDIA_LEASE_C.into(),
        )
        .expect("saved image deduplication");
    assert_eq!(third.id, image.id);
    let third_expiry = media_lease_expiry(&paths, MEDIA_LEASE_C);
    let retained = database
        .maintain_media(
            third_expiry + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("saved media maintenance");
    assert!(retained.integrity.orphaned_image_ids.is_empty());
    assert_eq!(retained.cleanup.retired_image_count, 0);
    assert_eq!(media_row_counts(&paths), (1, 1));
}

#[test]
fn a_save_during_orphan_grace_restores_an_expired_draft_image() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"long-running editor image".to_vec(),
                natural_width: 900,
                natural_height: 600,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("draft image");
    let lease_expires_at = media_lease_expiry(&paths, MEDIA_LEASE_A);
    let expired = database
        .maintain_media(lease_expires_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("lease-expiry maintenance");
    assert_eq!(expired.integrity.orphaned_image_ids, vec![image.id.clone()]);

    let token = format!("{{{{image:{};width=80%}}}}", image.id);
    database
        .client()
        .create_card_content(
            basic_draft(&token, "committed after 25 hours", None),
            MEDIA_LEASE_A.into(),
        )
        .expect("save during orphan grace");
    let orphaned_at: Option<i64> = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("main database")
        .query_row(
            "SELECT orphaned_at FROM image WHERE id = ?1",
            [&image.id],
            |row| row.get(0),
        )
        .expect("restored image");
    assert_eq!(orphaned_at, None);

    let retained = database
        .maintain_media(
            lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("post-save maintenance");
    assert_eq!(retained.cleanup.retired_image_count, 0);
    assert_eq!(media_row_counts(&paths), (1, 1));
}

#[test]
fn an_active_editor_can_renew_an_expired_lease_during_orphan_grace() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"renewed long-running editor image".to_vec(),
                natural_width: 700,
                natural_height: 500,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("draft image");
    let lease_expires_at = media_lease_expiry(&paths, MEDIA_LEASE_A);
    database
        .maintain_media(lease_expires_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("lease-expiry maintenance");

    let renewed_at = lease_expires_at + 1;
    assert_eq!(
        database
            .client()
            .renew_media_lease(MEDIA_LEASE_A.into(), renewed_at)
            .expect("expired lease renewal"),
        1
    );
    let report = database
        .maintain_media(renewed_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("renewed lease maintenance");
    assert!(report.integrity.orphaned_image_ids.is_empty());
    assert_eq!(report.cleanup.retired_image_count, 0);
    assert_eq!(media_row_counts(&paths), (1, 1));
    assert_eq!(
        database
            .client()
            .load_media_payload(image.id)
            .unwrap()
            .bytes,
        b"renewed long-running editor image"
    );
}

#[test]
fn abandoned_leases_survive_restart_and_running_ocr_completion_is_discarded_after_reap() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"abandoned occlusion draft".to_vec(),
                natural_width: 720,
                natural_height: 540,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("abandoned image");
    let lease_expires_at: i64 = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("main database")
        .query_row(
            "SELECT expires_at FROM image_draft_lease WHERE lease_id = ?1",
            [MEDIA_LEASE_A],
            |row| row.get(0),
        )
        .expect("lease expiry");
    drop(database);

    let reopened = initialize_test(&paths);
    let first_launch = reopened
        .maintain_media(lease_expires_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("launch reconciliation");
    assert_eq!(first_launch.integrity.orphaned_image_ids, vec![image.id]);
    assert_eq!(first_launch.cleanup.retired_image_count, 0);
    drop(reopened);

    let reopened = initialize_test(&paths);
    let reaped = reopened
        .maintain_media(
            lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("restart post-grace reconciliation");
    assert_eq!(reaped.cleanup.retired_image_count, 1);
    assert_eq!(media_row_counts(&paths), (0, 0));

    let running = reopened
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"running OCR orphan".to_vec(),
                natural_width: 640,
                natural_height: 480,
            },
            MEDIA_LEASE_B.into(),
        )
        .expect("running OCR image");
    let claimed_at = lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS + 1;
    reopened
        .client()
        .renew_media_lease(MEDIA_LEASE_B.into(), claimed_at)
        .expect("running lease renewal");
    let job = reopened
        .claim_next_ocr_job(claimed_at)
        .expect("OCR claim")
        .expect("running OCR job");
    assert_eq!(job.image_id, running.id);
    let running_lease_expires_at = media_lease_expiry(&paths, MEDIA_LEASE_B);
    reopened
        .maintain_media(
            running_lease_expires_at,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("running OCR lease expiry");
    let reaped_at = running_lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS;
    reopened
        .maintain_media(reaped_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("running OCR reap");
    reopened
        .complete_image_ocr(&job, Ok("late OCR result".into()), reaped_at + 1)
        .expect("discarded late OCR completion");
    assert_eq!(media_row_counts(&paths), (0, 0));
}

#[test]
fn media_delete_failure_leaves_a_retryable_extra_blob_without_missing_references() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"media failure boundary".to_vec(),
                natural_width: 500,
                natural_height: 500,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("failure-boundary image");
    let lease_expires_at = media_lease_expiry(&paths, MEDIA_LEASE_A);
    database
        .maintain_media(lease_expires_at, super::media::MEDIA_ORPHAN_GRACE_MILLIS)
        .expect("lease-expiry maintenance");

    let media = connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Existing)
        .expect("media database");
    assert!(media.execute("DELETE FROM media_blob", []).is_err());
    media
        .execute_batch(
            "CREATE TRIGGER test_abort_media_reap
             BEFORE DELETE ON media_blob
             BEGIN
                 SELECT RAISE(ABORT, 'injected media delete failure');
             END;",
        )
        .expect("failure trigger");
    drop(media);

    assert!(database
        .maintain_media(
            lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .is_err());
    assert_eq!(media_row_counts(&paths), (0, 1));
    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main database");
    assert_eq!(
        main.query_row(
            "SELECT count(*) FROM media_blob_reap_candidate",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .expect("durable blob candidate"),
        1
    );
    drop(main);

    let media = connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Existing)
        .expect("media database");
    media
        .execute_batch("DROP TRIGGER test_abort_media_reap")
        .expect("remove failure trigger");
    drop(media);
    let retried = database
        .maintain_media(
            lease_expires_at + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("retry maintenance");
    assert_eq!(retried.integrity.extra_blob_sha256.len(), 1);
    assert_eq!(retried.cleanup.deleted_blob_count, 1);
    assert_eq!(media_row_counts(&paths), (0, 0));
    assert!(matches!(
        database.client().load_media_payload(image.id),
        Err(DatabaseError::NotFound { .. })
    ));
}

#[test]
fn tombstoned_occlusion_history_retains_its_source_media() {
    const DEFINITION_ID: &str = "01980c8e-6c00-7000-8000-000000000921";
    const LAYER_ID: &str = "01980c8e-6c00-7000-8000-000000000922";
    const MASK_ID: &str = "01980c8e-6c00-7000-8000-000000000923";

    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"retained occlusion history".to_vec(),
                natural_width: 1_000,
                natural_height: 800,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("occlusion source");
    let context = database
        .client()
        .create_card_content(
            occlusion_draft(
                &image.id,
                DEFINITION_ID,
                OcclusionMode::HideOneGuessOne,
                vec![occlusion_layer(
                    LAYER_ID,
                    "retained",
                    &[(MASK_ID, 0.1, 0.1, 0.2, 0.2, OcclusionMaskColor::White)],
                )],
            ),
            MEDIA_LEASE_A.into(),
        )
        .expect("occlusion card");
    let item = database
        .search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 10,
            offset: 0,
        })
        .expect("saved content")
        .into_iter()
        .find(|item| item.card_content.id() == context.card_content.id())
        .expect("occlusion list item");
    database
        .delete_card_content(DeleteCardContentInput {
            card_content_id: context.card_content.id().into(),
            expected_updated_at: context.card_content.updated_at(),
            expected_lifecycle_updated_at: item.lifecycle_updated_at,
        })
        .expect("occlusion tombstone");

    let now = super::media::now_millis().expect("maintenance time");
    let report = database
        .maintain_media(
            now + super::media::MEDIA_ORPHAN_GRACE_MILLIS,
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("history-aware maintenance");
    assert!(report.integrity.orphaned_image_ids.is_empty());
    assert_eq!(report.cleanup.retired_image_count, 0);
    assert_eq!(media_row_counts(&paths), (1, 1));
}

#[test]
fn maintenance_reports_a_missing_blob_for_referenced_content() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .client()
        .ingest_image(
            CanonicalImage {
                bytes: b"missing referenced diagnostic".to_vec(),
                natural_width: 600,
                natural_height: 400,
            },
            MEDIA_LEASE_A.into(),
        )
        .expect("diagnostic image");
    let token = format!("{{{{image:{};width=80%}}}}", image.id);
    database
        .client()
        .create_card_content(
            basic_draft(&token, "diagnostic answer", None),
            MEDIA_LEASE_A.into(),
        )
        .expect("referenced image card");

    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main database");
    let hash: Vec<u8> = main
        .query_row(
            "SELECT sha256 FROM image WHERE id = ?1",
            [&image.id],
            |row| row.get(0),
        )
        .expect("image hash");
    drop(main);
    let mut media = connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Existing)
        .expect("media database");
    let transaction = media.transaction().expect("media transaction");
    transaction
        .execute(
            "INSERT INTO media_blob_reap_authorization(sha256) VALUES (?1)",
            [&hash],
        )
        .expect("test delete authorization");
    transaction
        .execute("DELETE FROM media_blob WHERE sha256 = ?1", [&hash])
        .expect("injected missing blob");
    transaction.commit().expect("missing blob commit");

    let report = database
        .maintain_media(
            super::media::now_millis().expect("maintenance time"),
            super::media::MEDIA_ORPHAN_GRACE_MILLIS,
        )
        .expect("integrity maintenance");
    assert_eq!(
        report.integrity.missing_referenced_blob_image_ids,
        vec![image.id]
    );
    assert_eq!(report.cleanup.deleted_blob_count, 0);
}

fn media_row_counts(paths: &DatabasePaths) -> (i64, i64) {
    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main database");
    let media =
        connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media database");
    let images = main
        .query_row("SELECT count(*) FROM image", [], |row| row.get(0))
        .expect("image count");
    let blobs = media
        .query_row("SELECT count(*) FROM media_blob", [], |row| row.get(0))
        .expect("blob count");
    (images, blobs)
}

fn media_lease_expiry(paths: &DatabasePaths, lease_id: &str) -> i64 {
    connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("main database")
        .query_row(
            "SELECT max(expires_at) FROM image_draft_lease WHERE lease_id = ?1",
            [lease_id],
            |row| row.get(0),
        )
        .expect("media lease expiry")
}

#[test]
fn malformed_or_missing_image_references_fail_without_creating_a_card() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let missing_id = "01980c8e-6c00-7000-8000-000000000299";

    assert!(matches!(
        database.create_card_content(basic_draft(
            &format!("{{{{image:{missing_id};width=50%}}}}"),
            "answer",
            None,
        )),
        Err(DatabaseError::InvalidInput(_))
    ));
    assert!(matches!(
        database.create_card_content(basic_draft(
            &format!("prefix {{{{image:{missing_id};width=50%}}}}"),
            "answer",
            None,
        )),
        Err(DatabaseError::InvalidInput(_))
    ));

    let connection =
        connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main database");
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM card_content", [], |row| row
                .get::<_, i64>(0))
            .expect("card count"),
        0
    );
}

#[test]
fn cloze_edits_reconcile_variants_without_losing_retained_history() {
    let fixture = fixture();
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let initial = database
        .create_card_content(cloze_draft(
            "The {{c1::capital}} of France is {{c2::Paris}}.",
            "A geography prompt.",
            Some("Geography notes"),
            &["cloze:1", "cloze:2"],
            "The capital of France is Paris.",
        ))
        .expect("cloze card");
    assert_eq!(initial.review_card.variant_key, "cloze:1");
    let CardContent::Cloze { front_md, .. } = &initial.card_content else {
        panic!("expected CLOZE content");
    };
    assert!(front_md.contains("{{c1::capital}}"));

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let initial_variants = connection
        .prepare(
            "SELECT variant_key, id
             FROM review_card
             WHERE card_content_id = ?1 AND deleted_at IS NULL
             ORDER BY variant_key",
        )
        .expect("variant query")
        .query_map([initial.card_content.id()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("variant rows")
        .collect::<rusqlite::Result<HashMap<_, _>>>()
        .expect("initial variants");
    assert_eq!(initial_variants.len(), 2);
    assert_eq!(
        initial_variants.get("cloze:1"),
        Some(&initial.review_card.id)
    );
    drop(connection);

    let graded = database
        .record_grade(grade_input(&initial, &fixture.steps[0]))
        .expect("grade retained variant")
        .context;
    let updated = database
        .update_card_content(UpdateCardContentInput {
            id: initial.card_content.id().into(),
            expected_updated_at: initial.card_content.updated_at(),
            content: cloze_draft(
                "The {{c1::capital}} of France is Paris, in {{c3::Europe}}.",
                "Updated explanation.",
                Some("Geography notes"),
                &["cloze:1", "cloze:3"],
                "The capital of France is Paris, in Europe.",
            ),
        })
        .expect("cloze edit");
    assert!(updated.card_content.updated_at() > initial.card_content.updated_at());

    let retained = database
        .load_review_context(graded.review_card.id.clone())
        .expect("retained c1 context");
    assert_eq!(retained.review_card.id, graded.review_card.id);
    assert_eq!(retained.review_card.variant_key, "cloze:1");
    assert_eq!(retained.review_history.len(), 1);
    assert_eq!(retained.cache, graded.cache);
    assert_eq!(retained.card_content, updated.card_content);

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let variants = connection
        .prepare(
            "SELECT variant_key, id, deleted_at, state
             FROM review_card
             WHERE card_content_id = ?1
             ORDER BY created_at, id",
        )
        .expect("reconciled variant query")
        .query_map([initial.card_content.id()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("reconciled rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("reconciled variants");
    assert_eq!(variants.len(), 3);
    assert!(variants.iter().any(|(key, id, deleted_at, _)| {
        key == "cloze:1"
            && id == initial_variants.get("cloze:1").expect("initial c1")
            && deleted_at.is_none()
    }));
    assert!(variants.iter().any(|(key, id, deleted_at, _)| {
        key == "cloze:2"
            && id == initial_variants.get("cloze:2").expect("initial c2")
            && deleted_at.is_some()
    }));
    assert!(variants.iter().any(|(key, _, deleted_at, state)| {
        key == "cloze:3" && deleted_at.is_none() && state == ReviewCardState::New.as_db_str()
    }));
    let search_body: String = connection
        .query_row(
            "SELECT body FROM search_document WHERE card_content_id = ?1",
            [initial.card_content.id()],
            |row| row.get(0),
        )
        .expect("cloze search document");
    assert!(search_body.contains("The capital of France is Paris, in Europe."));
    assert!(!search_body.contains("{{c"));
}

#[test]
fn image_occlusion_persists_layers_and_reconciles_siblings_by_stable_layer_id() {
    const DEFINITION_ID: &str = "01980c8e-6c00-7000-8000-000000000301";
    const LAYER_ONE_ID: &str = "01980c8e-6c00-7000-8000-000000000302";
    const LAYER_TWO_ID: &str = "01980c8e-6c00-7000-8000-000000000303";
    const LAYER_THREE_ID: &str = "01980c8e-6c00-7000-8000-000000000304";
    const MASK_ONE_ID: &str = "01980c8e-6c00-7000-8000-000000000305";
    const MASK_TWO_A_ID: &str = "01980c8e-6c00-7000-8000-000000000306";
    const MASK_TWO_B_ID: &str = "01980c8e-6c00-7000-8000-000000000307";
    const MASK_THREE_ID: &str = "01980c8e-6c00-7000-8000-000000000308";

    let fixture = fixture();
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let image = database
        .ingest_image(CanonicalImage {
            bytes: b"canonical image occlusion source".to_vec(),
            natural_width: 1200,
            natural_height: 800,
        })
        .expect("source image");
    let initial = database
        .create_card_content(occlusion_draft(
            &image.id,
            DEFINITION_ID,
            OcclusionMode::HideOneGuessOne,
            vec![
                occlusion_layer(
                    LAYER_ONE_ID,
                    "Aorta",
                    &[(MASK_ONE_ID, 0.1, 0.2, 0.15, 0.1, OcclusionMaskColor::White)],
                ),
                occlusion_layer(
                    LAYER_TWO_ID,
                    "Pulmonary veins",
                    &[
                        (
                            MASK_TWO_A_ID,
                            0.4,
                            0.2,
                            0.12,
                            0.08,
                            OcclusionMaskColor::Black,
                        ),
                        (
                            MASK_TWO_B_ID,
                            0.62,
                            0.21,
                            0.12,
                            0.08,
                            OcclusionMaskColor::White,
                        ),
                    ],
                ),
            ],
        ))
        .expect("image occlusion card");

    let CardContent::Occlusion { occlusion, .. } = &initial.card_content else {
        panic!("expected OCCLUSION content");
    };
    assert_eq!(occlusion.id, DEFINITION_ID);
    assert_eq!(occlusion.source_image, image);
    assert_eq!(occlusion.layers.len(), 2);
    assert_eq!(occlusion.layers[1].masks.len(), 2);
    assert_eq!(
        initial.review_card.variant_key,
        format!("layer:{LAYER_ONE_ID}")
    );

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let initial_variants = connection
        .prepare(
            "SELECT variant_key, id
             FROM review_card
             WHERE card_content_id = ?1 AND deleted_at IS NULL
             ORDER BY variant_key",
        )
        .expect("variant query")
        .query_map([initial.card_content.id()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("variant rows")
        .collect::<rusqlite::Result<HashMap<_, _>>>()
        .expect("initial variants");
    assert_eq!(initial_variants.len(), 2);
    drop(connection);

    let graded = database
        .record_grade(grade_input(&initial, &fixture.steps[0]))
        .expect("grade retained layer")
        .context;
    let updated = database
        .update_card_content(UpdateCardContentInput {
            id: initial.card_content.id().into(),
            expected_updated_at: initial.card_content.updated_at(),
            content: occlusion_draft(
                &image.id,
                DEFINITION_ID,
                OcclusionMode::HideAllGuessOne,
                vec![
                    occlusion_layer(
                        LAYER_ONE_ID,
                        "Ascending aorta",
                        &[(
                            MASK_ONE_ID,
                            0.12,
                            0.22,
                            0.16,
                            0.11,
                            OcclusionMaskColor::Black,
                        )],
                    ),
                    occlusion_layer(
                        LAYER_THREE_ID,
                        "Vena cava",
                        &[(
                            MASK_THREE_ID,
                            0.72,
                            0.45,
                            0.1,
                            0.12,
                            OcclusionMaskColor::White,
                        )],
                    ),
                ],
            ),
        })
        .expect("occlusion edit");

    let retained = database
        .load_review_context(graded.review_card.id.clone())
        .expect("retained layer context");
    assert_eq!(retained.review_card.id, graded.review_card.id);
    assert_eq!(retained.review_history.len(), 1);
    assert_eq!(retained.cache, graded.cache);
    let CardContent::Occlusion { occlusion, .. } = &updated.card_content else {
        panic!("expected updated OCCLUSION content");
    };
    assert_eq!(occlusion.mode, OcclusionMode::HideAllGuessOne);
    assert_eq!(occlusion.layers[0].id, LAYER_ONE_ID);
    assert_eq!(occlusion.layers[0].masks[0].id, MASK_ONE_ID);
    assert_eq!(occlusion.layers[0].masks[0].x, 0.12);
    assert_eq!(retained.card_content, updated.card_content);
    assert_eq!(updated.review_cards.len(), 2);
    let retained_summary = updated
        .review_cards
        .iter()
        .find(|card| card.id == graded.review_card.id)
        .expect("retained layer summary");
    assert_eq!(retained_summary.state, graded.cache.state);
    assert_eq!(retained_summary.due_at, graded.cache.due_at);
    assert_eq!(retained_summary.due_study_day, graded.cache.due_study_day);
    assert_eq!(retained_summary.last_review_at, graded.cache.last_review_at);

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let variants = connection
        .prepare(
            "SELECT variant_key, id, deleted_at
             FROM review_card
             WHERE card_content_id = ?1
             ORDER BY created_at, id",
        )
        .expect("reconciled variants")
        .query_map([initial.card_content.id()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .expect("variant rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("variant values");
    assert!(variants.iter().any(|(key, id, deleted_at)| {
        key == &format!("layer:{LAYER_ONE_ID}")
            && id
                == initial_variants
                    .get(&format!("layer:{LAYER_ONE_ID}"))
                    .expect("initial layer one")
            && deleted_at.is_none()
    }));
    assert!(variants.iter().any(|(key, _, deleted_at)| {
        key == &format!("layer:{LAYER_TWO_ID}") && deleted_at.is_some()
    }));
    assert!(variants.iter().any(|(key, _, deleted_at)| {
        key == &format!("layer:{LAYER_THREE_ID}") && deleted_at.is_none()
    }));
    let removed_layer_deleted_at: Option<i64> = connection
        .query_row(
            "SELECT deleted_at FROM card_occlusion_mask_layer WHERE id = ?1",
            [LAYER_TWO_ID],
            |row| row.get(0),
        )
        .expect("removed layer tombstone");
    assert!(removed_layer_deleted_at.is_some());
    let removed_mask_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM card_occlusion_mask
             WHERE card_occlusion_mask_layer_id = ?1 AND deleted_at IS NOT NULL",
            [LAYER_TWO_ID],
            |row| row.get(0),
        )
        .expect("removed mask tombstones");
    assert_eq!(removed_mask_count, 2);
}

#[test]
fn cloze_commands_reject_invalid_variants_and_card_type_changes() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    for variant_keys in [Vec::new(), vec!["cloze:0"], vec!["cloze:1", "cloze:1"]] {
        assert!(matches!(
            database.create_card_content(cloze_draft(
                "{{c1::answer}}",
                "",
                None,
                &variant_keys,
                "answer",
            )),
            Err(DatabaseError::InvalidInput(_))
        ));
    }

    let basic = database
        .create_card_content(basic_draft("front", "back", None))
        .expect("basic card");
    assert!(matches!(
        database.update_card_content(UpdateCardContentInput {
            id: basic.card_content.id().into(),
            expected_updated_at: basic.card_content.updated_at(),
            content: cloze_draft("{{c1::front}}", "", None, &["cloze:1"], "front"),
        }),
        Err(DatabaseError::InvalidInput(_))
    ));
}

#[test]
fn browse_search_paginates_without_hiding_authored_items() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    for index in 1..=3 {
        database
            .create_card_content(basic_draft(
                &format!("front {index}"),
                &format!("back {index}"),
                None,
            ))
            .expect("paginated card");
    }

    let first_page = database
        .search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 2,
            offset: 0,
        })
        .expect("first page");
    let second_page = database
        .search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 2,
            offset: 2,
        })
        .expect("second page");
    assert_eq!(first_page.len(), 2);
    assert_eq!(second_page.len(), 1);
    let mut ids = first_page
        .iter()
        .chain(&second_page)
        .map(|item| item.card_content.id())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3);
    assert!(first_page.iter().all(|item| item.review_cards.len() == 1));

    assert!(matches!(
        database.search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 2,
            offset: -1,
        }),
        Err(DatabaseError::InvalidInput(_))
    ));
}

#[test]
fn lexical_search_and_protected_edits_share_one_plain_text_document() {
    let fixture = fixture();
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let original = database
        .create_card_content(basic_draft(
            "Why is **copper** used for wire?",
            "It conducts electricity well.",
            Some("EE notes"),
        ))
        .expect("basic card");

    let matches = database
        .search_card_content(SearchCardContentInput {
            query: "copp".into(),
            limit: 20,
            offset: 0,
        })
        .expect("substring search");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].card_content.id(), original.card_content.id());
    assert_eq!(matches[0].review_status, CardContentReviewStatus::Active);

    assert_eq!(
        database
            .search_card_content(SearchCardContentInput {
                query: "opp".into(),
                limit: 20,
                offset: 0,
            })
            .expect("infix search")
            .len(),
        1
    );
    assert_eq!(
        database
            .search_card_content(SearchCardContentInput {
                query: "op".into(),
                limit: 20,
                offset: 0,
            })
            .expect("short literal search")
            .len(),
        1
    );

    let updated = database
        .update_card_content(UpdateCardContentInput {
            id: original.card_content.id().into(),
            expected_updated_at: original.card_content.updated_at(),
            content: basic_draft(
                "Why is **aluminum** used for overhead wire?",
                "It has a favorable mass-to-conductivity tradeoff.",
                Some("Power systems notes"),
            ),
        })
        .expect("protected edit");
    assert!(updated.card_content.updated_at() > original.card_content.updated_at());

    assert!(database
        .search_card_content(SearchCardContentInput {
            query: "copper".into(),
            limit: 20,
            offset: 0,
        })
        .expect("old search")
        .is_empty());
    assert_eq!(
        database
            .search_card_content(SearchCardContentInput {
                query: "alum".into(),
                limit: 20,
                offset: 0,
            })
            .expect("new search")
            .len(),
        1
    );

    let stale_edit = database.update_card_content(UpdateCardContentInput {
        id: original.card_content.id().into(),
        expected_updated_at: original.card_content.updated_at(),
        content: basic_draft("stale front", "stale back", None),
    });
    assert!(matches!(
        stale_edit,
        Err(DatabaseError::StaleCardContent(_))
    ));

    let stale_grade = database.record_grade(grade_input(&original, &fixture.steps[0]));
    assert!(matches!(
        stale_grade,
        Err(DatabaseError::StaleReviewContext(_))
    ));

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let body: String = connection
        .query_row(
            "SELECT body FROM search_document WHERE card_content_id = ?1",
            [original.card_content.id()],
            |row| row.get(0),
        )
        .expect("search document");
    assert!(body.contains("Why is aluminum used for overhead wire?"));
    assert!(!body.contains("**"));
}

#[test]
fn semantic_index_reconciles_by_hash_activates_atomically_and_supports_hybrid_search() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let copper = database
        .create_card_content(basic_draft(
            "Why is copper conductive?",
            "Its electrons are mobile.",
            None,
        ))
        .expect("copper card");
    database
        .create_card_content(basic_draft(
            "What causes ocean tides?",
            "Mostly the Moon's gravity.",
            None,
        ))
        .expect("tides card");

    let client = database.client();
    let pending = client
        .load_embedding_reconciliation_batch(32)
        .expect("pending embeddings");
    assert_eq!(pending.len(), 2);
    let copper_document = pending
        .iter()
        .find(|document| document.body.contains("copper"))
        .expect("copper search document")
        .clone();
    for document in pending {
        let dimension = if document.rowid == copper_document.rowid {
            0
        } else {
            1
        };
        assert_eq!(
            client
                .install_text_embedding(document, unit_embedding(dimension))
                .expect("install embedding"),
            InstallEmbeddingDisposition::Installed
        );
    }
    let progress = client
        .load_embedding_index_progress()
        .expect("embedding progress");
    assert_eq!(progress.current_documents, 2);
    assert_eq!(progress.total_documents, 2);
    assert!(!progress.active);
    assert!(client
        .activate_embedding_index_if_complete()
        .expect("activate complete index"));

    let semantic = client
        .hybrid_search_card_content(
            SearchCardContentInput {
                query: "???".into(),
                limit: 20,
                offset: 0,
            },
            unit_embedding(0),
        )
        .expect("semantic-only ranked query");
    assert_eq!(semantic[0].card_content.id(), copper.card_content.id());

    let updated = database
        .update_card_content(UpdateCardContentInput {
            id: copper.card_content.id().into(),
            expected_updated_at: copper.card_content.updated_at(),
            content: basic_draft(
                "Why is aluminum useful for transmission lines?",
                "It is light for its conductivity.",
                None,
            ),
        })
        .expect("edit invalidates embedding");
    assert_eq!(
        client
            .install_text_embedding(copper_document, unit_embedding(0))
            .expect("stale write is handled"),
        InstallEmbeddingDisposition::Stale
    );
    let pending = client
        .load_embedding_reconciliation_batch(32)
        .expect("edited document is pending");
    assert_eq!(pending.len(), 1);
    assert!(pending[0].body.contains("aluminum"));
    assert_eq!(
        client
            .install_text_embedding(pending[0].clone(), unit_embedding(0))
            .expect("install refreshed embedding"),
        InstallEmbeddingDisposition::Installed
    );
    assert_eq!(
        database
            .search_card_content(SearchCardContentInput {
                query: "aluminum".into(),
                limit: 20,
                offset: 0,
            })
            .expect("updated lexical search")[0]
            .card_content
            .updated_at(),
        updated.card_content.updated_at()
    );

    let report = client
        .maintain_search(SearchMaintenanceOperation::RebuildFts)
        .expect("rebuild FTS");
    assert_eq!(report.search_documents, 2);
    assert_eq!(report.fts_rows, 2);
    assert_eq!(report.indexed_documents, 2);
    assert!(report.semantic_index_active);
}

fn unit_embedding(dimension: usize) -> Vec<f32> {
    let mut embedding = vec![0.0; 768];
    embedding[dimension] = 1.0;
    embedding
}

#[test]
fn suspend_unsuspend_and_tombstone_delete_preserve_scheduler_history() {
    let fixture = fixture();
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let initial = database
        .create_card_content(basic_draft("front", "back", None))
        .expect("basic card");
    let recent = database
        .search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 20,
            offset: 0,
        })
        .expect("recent cards")
        .pop()
        .expect("created card");

    let suspended = database
        .set_card_content_suspended(SetCardContentSuspendedInput {
            card_content_id: initial.card_content.id().into(),
            expected_lifecycle_updated_at: recent.lifecycle_updated_at,
            suspended: true,
        })
        .expect("suspend content");
    assert_eq!(suspended.review_status, CardContentReviewStatus::Suspended);
    assert!(suspended.lifecycle_updated_at > recent.lifecycle_updated_at);

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let suspended_row: (String, Option<i64>) = connection
        .query_row(
            "SELECT status, suspended_at FROM review_card WHERE id = ?1",
            [&initial.review_card.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("suspended row");
    assert_eq!(suspended_row.0, ReviewCardStatus::Suspended.as_db_str());
    assert!(suspended_row.1.is_some());
    drop(connection);

    let resumed = database
        .set_card_content_suspended(SetCardContentSuspendedInput {
            card_content_id: initial.card_content.id().into(),
            expected_lifecycle_updated_at: suspended.lifecycle_updated_at,
            suspended: false,
        })
        .expect("unsuspend content");
    assert_eq!(resumed.review_status, CardContentReviewStatus::Active);

    let current_context = database
        .load_review_context(initial.review_card.id.clone())
        .expect("context after resume");
    let graded = database
        .record_grade(grade_input(&current_context, &fixture.steps[0]))
        .expect("grade before deletion")
        .context;
    let current_item = database
        .search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 20,
            offset: 0,
        })
        .expect("current card")
        .pop()
        .expect("current item");
    database
        .delete_card_content(DeleteCardContentInput {
            card_content_id: graded.card_content.id().into(),
            expected_updated_at: graded.card_content.updated_at(),
            expected_lifecycle_updated_at: current_item.lifecycle_updated_at,
        })
        .expect("tombstone content");

    assert!(database
        .search_card_content(SearchCardContentInput {
            query: String::new(),
            limit: 20,
            offset: 0,
        })
        .expect("recent cards after deletion")
        .is_empty());
    assert!(matches!(
        database.load_review_context(initial.review_card.id.clone()),
        Err(DatabaseError::NotFound { .. })
    ));

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let counts: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT count(*) FROM card_content WHERE id = ?1 AND deleted_at IS NOT NULL),
                (SELECT count(*) FROM review_card WHERE card_content_id = ?1 AND deleted_at IS NOT NULL),
                (SELECT count(*) FROM review_event WHERE review_card_id = ?2)",
            params![initial.card_content.id(), initial.review_card.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("tombstone and history counts");
    assert_eq!(counts, (1, 1, 1));
    let search_documents: i64 = connection
        .query_row(
            "SELECT count(*) FROM search_document WHERE card_content_id = ?1",
            [initial.card_content.id()],
            |row| row.get(0),
        )
        .expect("derived search document count");
    assert_eq!(search_documents, 0);
}

#[test]
fn shared_fixture_round_trips_through_the_event_log_and_undo() {
    let fixture = fixture();
    assert_eq!(fixture.schema_version, 1);
    assert!(fixture
        .steps
        .iter()
        .all(|step| step.expected_elapsed_days >= 0));
    let (_directory, paths) = test_paths();
    seed_fixture_card(&paths, &fixture);
    let database = initialize_test(&paths);
    let mut context = database
        .load_review_context(fixture.card_id.clone())
        .expect("initial context");

    for (index, step) in fixture.steps.iter().enumerate() {
        let result = database
            .record_grade(grade_input(&context, step))
            .expect("record fixture grade");
        assert_eq!(result.disposition, MutationDisposition::Applied);
        assert_eq!(result.card_sequence, i64::try_from(index + 1).unwrap());
        assert_eq!(result.context.cache, step.expected_cache);
        assert_eq!(result.context.review_history.len(), index + 1);
        assert_eq!(
            result.context.review_history[index].scheduler_log,
            step.expected_scheduler_log
        );
        context = result.context;
    }

    let undo_input = UndoLastGradeInput {
        event_id: fixture.undo.event_id.clone(),
        review_card_id: fixture.card_id.clone(),
        target_event_id: fixture.undo.target_event_id.clone(),
        expected_review_card_updated_at: context.review_card.updated_at,
        expected_card_sequence: context.last_card_sequence,
        expected_scheduler_config_id: fixture.scheduler_config_id.clone(),
        next_cache: fixture.undo.expected_cache.clone(),
    };
    let undone = database
        .undo_last_grade(undo_input.clone())
        .expect("undo last grade");
    assert_eq!(undone.disposition, MutationDisposition::Applied);
    assert_eq!(undone.card_sequence, 5);
    assert_eq!(undone.context.cache, fixture.undo.expected_cache);
    assert_eq!(undone.context.review_history.len(), 3);
    assert_eq!(undone.context.last_card_sequence, 5);

    let duplicate = database
        .undo_last_grade(undo_input.clone())
        .expect("idempotent undo retry");
    assert_eq!(duplicate.disposition, MutationDisposition::AlreadyApplied);
    assert_eq!(duplicate.context, undone.context);

    let mut conflict = undo_input;
    conflict.target_event_id = fixture.steps[0].event_id.clone();
    assert!(matches!(
        database.undo_last_grade(conflict),
        Err(DatabaseError::IdempotencyConflict { .. })
    ));

    let connection = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let counts: (i64, i64) = connection
        .query_row(
            "SELECT
                count(*) FILTER (WHERE event_type = ?1),
                count(*) FILTER (WHERE event_type = ?2)
             FROM review_event
             WHERE review_card_id = ?3",
            params![
                ReviewEventType::Review.as_db_str(),
                ReviewEventType::Revoke.as_db_str(),
                fixture.card_id
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("event counts");
    assert_eq!(counts, (4, 1));
}

#[test]
fn grade_is_atomic_and_an_idempotent_retry_writes_one_event() {
    let fixture = fixture();
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let context = database
        .create_card_content(basic_draft("front", "back", None))
        .expect("basic card");
    let input = grade_input(&context, &fixture.steps[0]);

    let external_writer = open_existing(&paths);
    external_writer
        .execute_batch(
            "CREATE TRIGGER test_abort_review_card_cache_update
             BEFORE UPDATE OF state ON review_card
             BEGIN
                 SELECT RAISE(ABORT, 'injected cache failure');
             END;",
        )
        .expect("failure trigger");
    assert!(matches!(
        database.record_grade(input.clone()),
        Err(DatabaseError::Sqlite(_))
    ));
    external_writer
        .execute_batch("DROP TRIGGER test_abort_review_card_cache_update")
        .expect("drop failure trigger");

    let read_only = connection::open_read_only(&paths.main, DatabaseKind::Main)
        .expect("read-only main database");
    let event_count: i64 = read_only
        .query_row(
            "SELECT count(*) FROM review_event WHERE review_card_id = ?1",
            [&context.review_card.id],
            |row| row.get(0),
        )
        .expect("event count after rollback");
    let state: String = read_only
        .query_row(
            "SELECT state FROM review_card WHERE id = ?1",
            [&context.review_card.id],
            |row| row.get(0),
        )
        .expect("card state after rollback");
    assert_eq!(event_count, 0);
    assert_eq!(state, ReviewCardState::New.as_db_str());
    drop(read_only);

    let applied = database
        .record_grade(input.clone())
        .expect("grade after rollback");
    assert_eq!(applied.disposition, MutationDisposition::Applied);
    let duplicate = database
        .record_grade(input.clone())
        .expect("idempotent grade retry");
    assert_eq!(duplicate.disposition, MutationDisposition::AlreadyApplied);
    assert_eq!(duplicate.context, applied.context);

    let mut conflict = input;
    conflict.review.grade = 2;
    assert!(matches!(
        database.record_grade(conflict),
        Err(DatabaseError::IdempotencyConflict { .. })
    ));
}

#[test]
fn stale_timestamp_sequence_and_scheduler_config_are_rejected_without_writes() {
    let fixture = fixture();
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    let initial = database
        .create_card_content(basic_draft("front", "back", None))
        .expect("basic card");
    let current = database
        .record_grade(grade_input(&initial, &fixture.steps[0]))
        .expect("first grade")
        .context;

    let mut stale_timestamp = grade_input(&initial, &fixture.steps[1]);
    stale_timestamp.review_card_id = current.review_card.id.clone();
    assert!(matches!(
        database.record_grade(stale_timestamp),
        Err(DatabaseError::StaleReviewContext(_))
    ));

    let mut stale_sequence = grade_input(&current, &fixture.steps[1]);
    stale_sequence.expected_card_sequence = 0;
    assert!(matches!(
        database.record_grade(stale_sequence),
        Err(DatabaseError::StaleReviewContext(_))
    ));

    let mut stale_config = grade_input(&current, &fixture.steps[1]);
    stale_config.event_id = fixture.steps[2].event_id.clone();
    stale_config.expected_scheduler_config_id = "01980c8e-6c00-7000-8000-000000000299".into();
    assert!(matches!(
        database.record_grade(stale_config),
        Err(DatabaseError::StaleReviewContext(_))
    ));

    let reloaded = database
        .load_review_context(current.review_card.id.clone())
        .expect("unchanged context");
    assert_eq!(reloaded.last_card_sequence, 1);
    assert_eq!(reloaded.review_history.len(), 1);
}
