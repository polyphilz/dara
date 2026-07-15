use rusqlite::{params, Connection};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{
    connection::{self, DatabaseKind, FileState},
    domain::{
        CardContent, CardContentDraft, CardContentReviewStatus, CardContentType,
        DeleteCardContentInput, MutationDisposition, ReviewCardCache, ReviewCardState,
        ReviewCardStatus, ReviewEventType, ReviewFact, SchedulerLogV1, SearchCardContentInput,
        SetCardContentSuspendedInput, UpdateCardContentInput,
    },
    initialize, Database, DatabaseError, DatabasePaths, InitializationOptions, RecordGradeInput,
    ReviewContext, UndoLastGradeInput,
};

const FIXTURE_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000101";

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
    } = &context.card_content;
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
        })
        .expect("old search")
        .is_empty());
    assert_eq!(
        database
            .search_card_content(SearchCardContentInput {
                query: "alum".into(),
                limit: 20,
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
