use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{
    connection::{self, DatabaseKind, FileState},
    domain::{CardContentType, ReviewCardState, ReviewCardStatus, ReviewEventType},
    initialize,
    queue::ReviewQueueLane,
    Database, DatabaseError, DatabasePaths, InitializationOptions, LoadHomeStatsInput,
    ReviewQueueSelection, SelectNextReviewCardInput,
};

const DEFAULT_CONFIG_ID: &str = "019f547b-6200-7000-8000-000000000001";
const NOW: i64 = 1_800_000_000_000;
const STUDY_DAY: i64 = 20_000;

#[derive(Clone, Copy)]
enum SeedStatus {
    Active,
    Suspended,
}

#[derive(Clone, Copy)]
enum SeedState {
    New,
    Learning { due_at: i64 },
    Relearning { due_at: i64 },
    Review { due_study_day: i64 },
}

#[derive(Clone, Copy)]
struct SeedCard {
    suffix: u64,
    created_at: i64,
    status: SeedStatus,
    state: SeedState,
}

impl SeedCard {
    fn new(suffix: u64, created_at: i64) -> Self {
        Self {
            suffix,
            created_at,
            status: SeedStatus::Active,
            state: SeedState::New,
        }
    }

    fn learning(suffix: u64, due_at: i64) -> Self {
        Self {
            suffix,
            created_at: NOW - 10_000,
            status: SeedStatus::Active,
            state: SeedState::Learning { due_at },
        }
    }

    fn relearning(suffix: u64, due_at: i64) -> Self {
        Self {
            suffix,
            created_at: NOW - 10_000,
            status: SeedStatus::Active,
            state: SeedState::Relearning { due_at },
        }
    }

    fn review(suffix: u64, due_study_day: i64) -> Self {
        Self {
            suffix,
            created_at: NOW - 10_000,
            status: SeedStatus::Active,
            state: SeedState::Review { due_study_day },
        }
    }

    fn suspended(mut self) -> Self {
        self.status = SeedStatus::Suspended;
        self
    }
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

fn seed_database(seeds: &[SeedCard]) -> (TempDir, DatabasePaths, Database) {
    let (directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let connection = open_existing(&paths);
    for seed in seeds {
        insert_seed_card(&connection, *seed);
    }
    drop(connection);
    let database = initialize_test(&paths);
    (directory, paths, database)
}

fn insert_seed_card(connection: &Connection, seed: SeedCard) {
    let content_id = uuid_v7(0x100_000 + seed.suffix);
    let review_card_id = card_id(seed.suffix);
    let event_id = uuid_v7(0x300_000 + seed.suffix);
    let front = format!("front {}", seed.suffix);
    let back = format!("back {}", seed.suffix);
    let search_body = format!("{front}\n\u{1e}\n{back}");
    let content_hash = Sha256::digest(search_body.as_bytes());
    let (status, suspended_at) = match seed.status {
        SeedStatus::Active => (ReviewCardStatus::Active, None),
        SeedStatus::Suspended => (ReviewCardStatus::Suspended, Some(seed.created_at)),
    };

    connection
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (?1, ?2, ?2, NULL, ?3, ?4, ?5, NULL)",
            params![
                content_id,
                seed.created_at,
                CardContentType::Basic.as_db_str(),
                front,
                back
            ],
        )
        .expect("seed content");

    match seed.state {
        SeedState::New => {
            connection
                .execute(
                    "INSERT INTO review_card (
                    id, created_at, updated_at, deleted_at, card_content_id,
                    status, suspended_at, variant_key, state, due_at,
                    due_study_day, last_review_at, reps, lapses,
                    scheduler_config_id, scheduler_state_schema_version,
                    scheduler_state_json
                 ) VALUES (
                    ?1, ?2, ?2, NULL, ?3, ?4, ?5, 'basic', ?6, NULL,
                    NULL, NULL, 0, 0, NULL, NULL, NULL
                 )",
                    params![
                        review_card_id,
                        seed.created_at,
                        content_id,
                        status.as_db_str(),
                        suspended_at,
                        ReviewCardState::New.as_db_str()
                    ],
                )
                .expect("seed new card");
        }
        SeedState::Learning { due_at } => {
            insert_scheduled_card(
                connection,
                seed,
                &content_id,
                &review_card_id,
                &event_id,
                status,
                suspended_at,
                ReviewCardState::Learning,
                Some(due_at),
                None,
                0,
                1,
            );
        }
        SeedState::Relearning { due_at } => {
            insert_scheduled_card(
                connection,
                seed,
                &content_id,
                &review_card_id,
                &event_id,
                status,
                suspended_at,
                ReviewCardState::Relearning,
                Some(due_at),
                None,
                1,
                1,
            );
        }
        SeedState::Review { due_study_day } => {
            insert_scheduled_card(
                connection,
                seed,
                &content_id,
                &review_card_id,
                &event_id,
                status,
                suspended_at,
                ReviewCardState::Review,
                None,
                Some(due_study_day),
                0,
                3,
            );
        }
    }

    connection
        .execute(
            "INSERT INTO search_document (card_content_id, body, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                content_id,
                search_body,
                content_hash.as_slice(),
                seed.created_at
            ],
        )
        .expect("seed search document");
}

#[allow(clippy::too_many_arguments)]
fn insert_scheduled_card(
    connection: &Connection,
    seed: SeedCard,
    content_id: &str,
    review_card_id: &str,
    event_id: &str,
    status: ReviewCardStatus,
    suspended_at: Option<i64>,
    state: ReviewCardState,
    due_at: Option<i64>,
    due_study_day: Option<i64>,
    lapses: i64,
    grade: i64,
) {
    let last_review_at = seed.created_at + 1;
    let review_study_day = due_study_day.map_or(STUDY_DAY - 1, |due| due - 1);
    connection
        .execute(
            r#"INSERT INTO review_card (
                id, created_at, updated_at, deleted_at, card_content_id,
                status, suspended_at, variant_key, state, due_at,
                due_study_day, last_review_at, reps, lapses,
                scheduler_config_id, scheduler_state_schema_version,
                scheduler_state_json
             ) VALUES (
                ?1, ?2, ?3, NULL, ?4, ?5, ?6, 'basic', ?7, ?8, ?9, ?3,
                1, ?10, ?11, 1,
                '{"stability":1.0,"difficulty":5.0,"scheduledDays":1,"learningSteps":0}'
             )"#,
            params![
                review_card_id,
                seed.created_at,
                last_review_at,
                content_id,
                status.as_db_str(),
                suspended_at,
                state.as_db_str(),
                due_at,
                due_study_day,
                lapses,
                DEFAULT_CONFIG_ID,
            ],
        )
        .expect("seed scheduled card");
    connection
        .execute(
            r#"INSERT INTO review_event (
                id, created_at, event_schema_version, event_type,
                review_card_id, card_sequence, reviewed_at, study_day,
                timezone_id, utc_offset_minutes, grade, scheduler_config_id,
                scheduler_log_json, target_event_id
             ) VALUES (
                ?1, ?2, 1, ?3, ?4, 1, ?2, ?5, 'UTC', 0, ?6, ?7,
                '{"stateBefore":"NEW","dueAtBefore":null,"dueStudyDayBefore":null,"stabilityBefore":null,"difficultyBefore":null,"scheduledDaysBefore":null,"learningStepsBefore":null}',
                NULL
             )"#,
            params![
                event_id,
                last_review_at,
                ReviewEventType::Review.as_db_str(),
                review_card_id,
                review_study_day,
                grade,
                DEFAULT_CONFIG_ID,
            ],
        )
        .expect("seed review event");
}

fn select(database: &Database, cursor: i64) -> ReviewQueueSelection {
    database
        .select_next_review_card(SelectNextReviewCardInput {
            now: NOW,
            study_day: STUDY_DAY,
            normal_lane_cursor: cursor,
        })
        .expect("queue selection")
}

fn selected_card(selection: ReviewQueueSelection) -> (ReviewQueueLane, i64, String) {
    match selection {
        ReviewQueueSelection::Card {
            lane,
            next_normal_lane_cursor,
            context,
        } => (lane, next_normal_lane_cursor, context.review_card.id),
        ReviewQueueSelection::CaughtUp { .. } => panic!("expected a selected card"),
    }
}

fn suspend(connection: &Connection, review_card_id: &str) {
    let changed = connection
        .execute(
            "UPDATE review_card
             SET status = ?1, suspended_at = ?2, updated_at = updated_at + 1
             WHERE id = ?3 AND status = ?4",
            params![
                ReviewCardStatus::Suspended.as_db_str(),
                NOW,
                review_card_id,
                ReviewCardStatus::Active.as_db_str()
            ],
        )
        .expect("suspend selected card");
    assert_eq!(changed, 1);
}

fn unsuspend(connection: &Connection, review_card_id: &str) {
    let changed = connection
        .execute(
            "UPDATE review_card
             SET status = ?1, suspended_at = NULL, updated_at = updated_at + 1
             WHERE id = ?2 AND status = ?3",
            params![
                ReviewCardStatus::Active.as_db_str(),
                review_card_id,
                ReviewCardStatus::Suspended.as_db_str()
            ],
        )
        .expect("unsuspend card");
    assert_eq!(changed, 1);
}

fn card_id(suffix: u64) -> String {
    uuid_v7(0x200_000 + suffix)
}

fn uuid_v7(suffix: u64) -> String {
    format!("01980c8e-6c00-7000-8000-{suffix:012x}")
}

#[test]
fn due_intraday_cards_preempt_without_moving_the_normal_cursor() {
    let seeds = [
        SeedCard::learning(1, NOW - 50),
        SeedCard::relearning(2, NOW - 100),
        SeedCard::learning(3, NOW + 100),
        SeedCard::new(4, NOW - 500),
    ];
    let (_directory, paths, database) = seed_database(&seeds);
    let external_writer = open_existing(&paths);

    let first = selected_card(select(&database, 3));
    assert_eq!(first, (ReviewQueueLane::Intraday, 3, card_id(2)));
    suspend(&external_writer, &first.2);

    let second = selected_card(select(&database, 3));
    assert_eq!(second, (ReviewQueueLane::Intraday, 3, card_id(1)));
    suspend(&external_writer, &second.2);

    let normal = selected_card(select(&database, 3));
    assert_eq!(normal, (ReviewQueueLane::New, 0, card_id(4)));
}

#[test]
fn cadence_is_three_review_slots_then_one_new_with_lane_fallbacks() {
    let seeds = [
        SeedCard::review(10, STUDY_DAY),
        SeedCard::review(11, STUDY_DAY),
        SeedCard::review(12, STUDY_DAY),
        SeedCard::review(13, STUDY_DAY),
        SeedCard::review(14, STUDY_DAY),
        SeedCard::new(20, NOW - 500),
        SeedCard::new(21, NOW - 400),
    ];
    let (_directory, paths, database) = seed_database(&seeds);
    let external_writer = open_existing(&paths);
    let mut cursor = 0;

    for expected_lane in [
        ReviewQueueLane::Review,
        ReviewQueueLane::Review,
        ReviewQueueLane::Review,
        ReviewQueueLane::New,
    ] {
        let selected = selected_card(select(&database, cursor));
        assert_eq!(selected.0, expected_lane);
        assert_eq!(selected.1, (cursor + 1) % 4);
        cursor = selected.1;
        suspend(&external_writer, &selected.2);
    }
    assert_eq!(cursor, 0);

    suspend(&external_writer, &card_id(21));
    let review_on_new_slot = selected_card(select(&database, 3));
    assert_eq!(review_on_new_slot.0, ReviewQueueLane::Review);
    assert_eq!(review_on_new_slot.1, 0);
    suspend(&external_writer, &review_on_new_slot.2);

    while let ReviewQueueSelection::Card { context, .. } = select(&database, 0) {
        if context.cache.state != super::domain::ReviewCardState::Review {
            break;
        }
        suspend(&external_writer, &context.review_card.id);
    }
    unsuspend(&external_writer, &card_id(21));
    let new_on_review_slot = selected_card(select(&database, 1));
    assert_eq!(new_on_review_slot.0, ReviewQueueLane::New);
    assert_eq!(new_on_review_slot.1, 2);
}

#[test]
fn review_and_new_lanes_have_deterministic_ordering() {
    let seeds = [
        SeedCard::review(30, STUDY_DAY - 5),
        SeedCard::review(31, STUDY_DAY - 5),
        SeedCard::review(32, STUDY_DAY - 2),
        SeedCard::review(33, STUDY_DAY),
        SeedCard::new(40, NOW - 100),
        SeedCard::new(41, NOW - 300),
        SeedCard::new(42, NOW - 300),
    ];
    let (_directory, paths, database) = seed_database(&seeds);
    let external_writer = open_existing(&paths);

    let first = selected_card(select(&database, 0));
    let repeated = selected_card(select(&database, 0));
    assert_eq!(first, repeated);
    assert_eq!(first.0, ReviewQueueLane::Review);
    assert!([card_id(30), card_id(31)].contains(&first.2));
    suspend(&external_writer, &first.2);

    let second = selected_card(select(&database, 0));
    assert!([card_id(30), card_id(31)].contains(&second.2));
    assert_ne!(first.2, second.2);
    suspend(&external_writer, &second.2);

    let third = selected_card(select(&database, 0));
    assert_eq!(third.2, card_id(32));

    let first_new = selected_card(select(&database, 3));
    assert_eq!(first_new.2, card_id(41));
    suspend(&external_writer, &first_new.2);
    let second_new = selected_card(select(&database, 3));
    assert_eq!(second_new.2, card_id(42));
    suspend(&external_writer, &second_new.2);
    let third_new = selected_card(select(&database, 3));
    assert_eq!(third_new.2, card_id(40));
}

#[test]
fn caught_up_reports_only_the_next_active_intraday_deadline() {
    let seeds = [
        SeedCard::learning(50, NOW + 500),
        SeedCard::relearning(51, NOW + 200),
        SeedCard::learning(52, NOW + 100).suspended(),
    ];
    let (_directory, paths, database) = seed_database(&seeds);
    let external_writer = open_existing(&paths);

    assert_eq!(
        select(&database, 2),
        ReviewQueueSelection::CaughtUp {
            next_due_at: Some(NOW + 200),
            next_normal_lane_cursor: 2,
        }
    );

    suspend(&external_writer, &card_id(50));
    suspend(&external_writer, &card_id(51));
    assert_eq!(
        select(&database, 2),
        ReviewQueueSelection::CaughtUp {
            next_due_at: None,
            next_normal_lane_cursor: 2,
        }
    );

    assert!(matches!(
        database.select_next_review_card(SelectNextReviewCardInput {
            now: NOW,
            study_day: STUDY_DAY,
            normal_lane_cursor: 4,
        }),
        Err(DatabaseError::InvalidInput(_))
    ));
}

#[test]
fn home_stats_group_review_activity_and_count_the_current_queue() {
    let seeds = [
        SeedCard::learning(60, NOW - 50),
        SeedCard::relearning(61, NOW + 500),
        SeedCard::review(62, STUDY_DAY),
        SeedCard::review(63, STUDY_DAY).suspended(),
        SeedCard::review(64, STUDY_DAY + 1),
        SeedCard::new(65, NOW - 100),
    ];
    let (_directory, _paths, database) = seed_database(&seeds);

    let stats = database
        .load_home_stats(LoadHomeStatsInput {
            now: NOW,
            study_day: STUDY_DAY,
            activity_start_study_day: STUDY_DAY - 1,
        })
        .expect("home stats");

    assert_eq!(stats.reviewed_today, 1);
    assert_eq!(stats.queue.new, 1);
    assert_eq!(stats.queue.learning, 1);
    assert_eq!(stats.queue.review, 1);
    assert_eq!(stats.next_learning_due_at, Some(NOW + 500));
    assert_eq!(stats.activity.len(), 2);
    assert_eq!(stats.activity[0].study_day, STUDY_DAY - 1);
    assert_eq!(stats.activity[0].count, 4);
    assert_eq!(stats.activity[1].study_day, STUDY_DAY);
    assert_eq!(stats.activity[1].count, 1);
}
