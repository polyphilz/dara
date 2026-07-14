use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{DatabaseError, Result};

const EVENT_SCHEMA_VERSION: i64 = 1;
const SCHEDULER_STATE_SCHEMA_VERSION: i64 = 1;
const MAX_JSON_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
// An explicit, non-token field boundary keeps the aggregate deterministic.
const SEARCH_FIELD_SEPARATOR: &str = "\n\u{1e}\n";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateBasicCardInput {
    pub front_md: String,
    pub back_md: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BasicCardContent {
    pub id: String,
    pub front_md: String,
    pub back_md: String,
    pub source: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewCardStatus {
    Active,
    Suspended,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewCardState {
    New,
    Learning,
    Review,
    Relearning,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulerStateV1 {
    pub stability: f64,
    pub difficulty: f64,
    pub scheduled_days: i64,
    pub learning_steps: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewCardCache {
    pub state: ReviewCardState,
    pub due_at: Option<i64>,
    pub due_study_day: Option<i64>,
    pub last_review_at: Option<i64>,
    pub reps: i64,
    pub lapses: i64,
    pub scheduler_state: Option<SchedulerStateV1>,
}

impl ReviewCardCache {
    fn new() -> Self {
        Self {
            state: ReviewCardState::New,
            due_at: None,
            due_study_day: None,
            last_review_at: None,
            reps: 0,
            lapses: 0,
            scheduler_state: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFact {
    pub grade: u8,
    pub reviewed_at: i64,
    pub study_day: i64,
    pub timezone_id: String,
    pub utc_offset_minutes: i16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulerLogV1 {
    pub state_before: ReviewCardState,
    pub due_at_before: Option<i64>,
    pub due_study_day_before: Option<i64>,
    pub stability_before: Option<f64>,
    pub difficulty_before: Option<f64>,
    pub scheduled_days_before: Option<i64>,
    pub learning_steps_before: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfigRecord {
    pub id: String,
    pub algorithm: String,
    pub algorithm_version: i64,
    pub scheduler_library: String,
    pub library_version: String,
    pub config_schema_version: i64,
    pub config: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCardSummary {
    pub id: String,
    pub status: ReviewCardStatus,
    pub variant_key: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedReviewFact {
    pub event_id: String,
    pub card_sequence: i64,
    pub scheduler_config_id: String,
    pub review: ReviewFact,
    pub scheduler_log: SchedulerLogV1,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewContext {
    pub card_content: BasicCardContent,
    pub review_card: ReviewCardSummary,
    pub cache: ReviewCardCache,
    pub cache_scheduler_config_id: Option<String>,
    pub last_card_sequence: i64,
    pub scheduler_config: SchedulerConfigRecord,
    pub review_history: Vec<PersistedReviewFact>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordGradeInput {
    pub event_id: String,
    pub review_card_id: String,
    pub expected_review_card_updated_at: i64,
    pub expected_card_sequence: i64,
    pub expected_scheduler_config_id: String,
    pub review: ReviewFact,
    pub next_cache: ReviewCardCache,
    pub scheduler_log: SchedulerLogV1,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UndoLastGradeInput {
    pub event_id: String,
    pub review_card_id: String,
    pub target_event_id: String,
    pub expected_review_card_updated_at: i64,
    pub expected_card_sequence: i64,
    pub expected_scheduler_config_id: String,
    pub next_cache: ReviewCardCache,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MutationDisposition {
    Applied,
    AlreadyApplied,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewMutationResult {
    pub disposition: MutationDisposition,
    pub event_id: String,
    pub card_sequence: i64,
    pub context: ReviewContext,
}

struct StoredCard {
    content_id: String,
    front_md: String,
    back_md: String,
    source: Option<String>,
    card_id: String,
    status: String,
    variant_key: String,
    updated_at: i64,
    state: String,
    due_at: Option<i64>,
    due_study_day: Option<i64>,
    last_review_at: Option<i64>,
    reps: i64,
    lapses: i64,
    scheduler_config_id: Option<String>,
    scheduler_state_schema_version: Option<i64>,
    scheduler_state_json: Option<String>,
}

struct StoredEvent {
    event_schema_version: i64,
    event_type: String,
    review_card_id: String,
    card_sequence: i64,
    reviewed_at: Option<i64>,
    study_day: Option<i64>,
    timezone_id: Option<String>,
    utc_offset_minutes: Option<i16>,
    grade: Option<u8>,
    scheduler_config_id: String,
    scheduler_log_json: Option<String>,
    target_event_id: Option<String>,
}

pub(super) fn create_basic_card(
    connection: &mut Connection,
    input: CreateBasicCardInput,
) -> Result<ReviewContext> {
    validate_basic_card(&input)?;
    let now = now_millis()?;
    let content_id = Uuid::now_v7().to_string();
    let review_card_id = Uuid::now_v7().to_string();
    let search_body = search_body(&input);
    let content_hash = Sha256::digest(search_body.as_bytes());

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT INTO card_content (
            id, created_at, updated_at, deleted_at, type, front_md, back_md, source
         ) VALUES (?1, ?2, ?2, NULL, 'BASIC', ?3, ?4, ?5)",
        params![content_id, now, input.front_md, input.back_md, input.source],
    )?;
    transaction.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status,
            suspended_at, variant_key, state, due_at, due_study_day,
            last_review_at, reps, lapses, scheduler_config_id,
            scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, ?2, ?2, NULL, ?3, 'ACTIVE', NULL, 'basic', 'NEW', NULL, NULL,
            NULL, 0, 0, NULL, NULL, NULL
         )",
        params![review_card_id, now, content_id],
    )?;
    transaction.execute(
        "INSERT INTO search_document (card_content_id, body, content_hash, updated_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![content_id, search_body, content_hash.as_slice(), now],
    )?;
    transaction.commit()?;

    load_review_context(connection, &review_card_id)
}

pub(super) fn load_review_context(
    connection: &Connection,
    review_card_id: &str,
) -> Result<ReviewContext> {
    validate_uuid_v7(review_card_id, "reviewCardId")?;
    let stored = load_stored_card(connection, review_card_id)?;
    let status = parse_status(&stored.status)?;
    let cache = cache_from_stored(&stored)?;
    validate_cache(&cache).map_err(DatabaseError::CorruptReviewData)?;
    let scheduler_config = load_active_scheduler_config(connection)?;

    if cache.state != ReviewCardState::New
        && stored.scheduler_config_id.as_deref() != Some(scheduler_config.id.as_str())
    {
        return Err(DatabaseError::StaleReviewContext(format!(
            "card {} cache uses scheduler config {:?}, but {} is active",
            stored.card_id, stored.scheduler_config_id, scheduler_config.id
        )));
    }

    let last_card_sequence = connection.query_row(
        "SELECT coalesce(max(card_sequence), 0)
         FROM review_event
         WHERE review_card_id = ?1",
        [review_card_id],
        |row| row.get(0),
    )?;
    let review_history = load_review_history(connection, review_card_id)?;
    validate_materialized_history(&cache, &review_history)?;

    Ok(ReviewContext {
        card_content: BasicCardContent {
            id: stored.content_id,
            front_md: stored.front_md,
            back_md: stored.back_md,
            source: stored.source,
        },
        review_card: ReviewCardSummary {
            id: stored.card_id,
            status,
            variant_key: stored.variant_key,
            updated_at: stored.updated_at,
        },
        cache,
        cache_scheduler_config_id: stored.scheduler_config_id,
        last_card_sequence,
        scheduler_config,
        review_history,
    })
}

pub(super) fn record_grade(
    connection: &mut Connection,
    input: RecordGradeInput,
) -> Result<ReviewMutationResult> {
    validate_record_input(&input)?;
    let scheduler_log_json = serde_json::to_string(&input.scheduler_log)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    if let Some(existing) = load_event(&transaction, &input.event_id)? {
        if !review_event_matches(&existing, &input, &scheduler_log_json) {
            return Err(DatabaseError::IdempotencyConflict {
                event_id: input.event_id,
            });
        }
        let context = load_review_context(&transaction, &existing.review_card_id)?;
        return Ok(ReviewMutationResult {
            disposition: MutationDisposition::AlreadyApplied,
            event_id: input.event_id,
            card_sequence: existing.card_sequence,
            context,
        });
    }

    let context = load_review_context(&transaction, &input.review_card_id)?;
    validate_preconditions(
        &context,
        input.expected_review_card_updated_at,
        input.expected_card_sequence,
        &input.expected_scheduler_config_id,
    )?;
    let expected_log = scheduler_log_for(&context.cache);
    if input.scheduler_log != expected_log {
        return Err(DatabaseError::StaleReviewContext(
            "scheduler log does not describe the stored card cache".into(),
        ));
    }
    validate_next_review_cache(&context.cache, &input.review, &input.next_cache)?;

    let card_sequence = next_sequence(context.last_card_sequence)?;
    let now = now_millis()?;
    let updated_at = next_updated_at(context.review_card.updated_at, now)?;
    transaction.execute(
        "INSERT INTO review_event (
            id, created_at, event_schema_version, event_type, review_card_id,
            card_sequence, reviewed_at, study_day, timezone_id,
            utc_offset_minutes, grade, scheduler_config_id, scheduler_log_json,
            target_event_id
         ) VALUES (
            ?1, ?2, ?3, 'REVIEW', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL
         )",
        params![
            input.event_id,
            now,
            EVENT_SCHEMA_VERSION,
            input.review_card_id,
            card_sequence,
            input.review.reviewed_at,
            input.review.study_day,
            input.review.timezone_id,
            input.review.utc_offset_minutes,
            input.review.grade,
            input.expected_scheduler_config_id,
            scheduler_log_json,
        ],
    )?;
    update_card_cache(
        &transaction,
        &input.review_card_id,
        context.review_card.updated_at,
        updated_at,
        &input.next_cache,
        Some(&input.expected_scheduler_config_id),
    )?;
    transaction.commit()?;

    Ok(ReviewMutationResult {
        disposition: MutationDisposition::Applied,
        event_id: input.event_id,
        card_sequence,
        context: load_review_context(connection, &input.review_card_id)?,
    })
}

pub(super) fn undo_last_grade(
    connection: &mut Connection,
    input: UndoLastGradeInput,
) -> Result<ReviewMutationResult> {
    validate_undo_input(&input)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    if let Some(existing) = load_event(&transaction, &input.event_id)? {
        if !revoke_event_matches(&existing, &input) {
            return Err(DatabaseError::IdempotencyConflict {
                event_id: input.event_id,
            });
        }
        let context = load_review_context(&transaction, &existing.review_card_id)?;
        return Ok(ReviewMutationResult {
            disposition: MutationDisposition::AlreadyApplied,
            event_id: input.event_id,
            card_sequence: existing.card_sequence,
            context,
        });
    }

    let context = load_review_context(&transaction, &input.review_card_id)?;
    validate_preconditions(
        &context,
        input.expected_review_card_updated_at,
        input.expected_card_sequence,
        &input.expected_scheduler_config_id,
    )?;
    let target = context.review_history.last().ok_or_else(|| {
        DatabaseError::InvalidInput("a card with no active reviews cannot be undone".into())
    })?;
    if target.event_id != input.target_event_id {
        return Err(DatabaseError::StaleReviewContext(format!(
            "{} is not the card's latest non-revoked review",
            input.target_event_id
        )));
    }
    validate_undo_cache(
        &input.next_cache,
        &context.review_history[..context.review_history.len() - 1],
    )?;

    let card_sequence = next_sequence(context.last_card_sequence)?;
    let now = now_millis()?;
    let updated_at = next_updated_at(context.review_card.updated_at, now)?;
    transaction.execute(
        "INSERT INTO review_event (
            id, created_at, event_schema_version, event_type, review_card_id,
            card_sequence, reviewed_at, study_day, timezone_id,
            utc_offset_minutes, grade, scheduler_config_id, scheduler_log_json,
            target_event_id
         ) VALUES (
            ?1, ?2, ?3, 'REVOKE', ?4, ?5, NULL, NULL, NULL, NULL, NULL, ?6, NULL, ?7
         )",
        params![
            input.event_id,
            now,
            EVENT_SCHEMA_VERSION,
            input.review_card_id,
            card_sequence,
            input.expected_scheduler_config_id,
            input.target_event_id,
        ],
    )?;
    let cache_scheduler_config_id = (input.next_cache.state != ReviewCardState::New)
        .then_some(input.expected_scheduler_config_id.as_str());
    update_card_cache(
        &transaction,
        &input.review_card_id,
        context.review_card.updated_at,
        updated_at,
        &input.next_cache,
        cache_scheduler_config_id,
    )?;
    transaction.commit()?;

    Ok(ReviewMutationResult {
        disposition: MutationDisposition::Applied,
        event_id: input.event_id,
        card_sequence,
        context: load_review_context(connection, &input.review_card_id)?,
    })
}

fn validate_basic_card(input: &CreateBasicCardInput) -> Result<()> {
    if input.front_md.trim().is_empty() {
        return Err(DatabaseError::InvalidInput(
            "frontMd must contain visible text".into(),
        ));
    }
    if input.back_md.trim().is_empty() {
        return Err(DatabaseError::InvalidInput(
            "backMd must contain visible text".into(),
        ));
    }
    Ok(())
}

fn search_body(input: &CreateBasicCardInput) -> String {
    let mut fields = vec![input.front_md.as_str(), input.back_md.as_str()];
    if let Some(source) = input.source.as_deref().filter(|source| !source.is_empty()) {
        fields.push(source);
    }
    fields.join(SEARCH_FIELD_SEPARATOR)
}

fn load_stored_card(connection: &Connection, review_card_id: &str) -> Result<StoredCard> {
    connection
        .query_row(
            "SELECT
                content.id, content.front_md, content.back_md, content.source,
                card.id, card.status, card.variant_key, card.updated_at,
                card.state, card.due_at, card.due_study_day, card.last_review_at,
                card.reps, card.lapses, card.scheduler_config_id,
                card.scheduler_state_schema_version, card.scheduler_state_json
             FROM review_card AS card
             JOIN card_content AS content ON content.id = card.card_content_id
             WHERE card.id = ?1
               AND card.deleted_at IS NULL
               AND content.deleted_at IS NULL
               AND content.type = 'BASIC'",
            [review_card_id],
            |row| {
                Ok(StoredCard {
                    content_id: row.get(0)?,
                    front_md: row.get(1)?,
                    back_md: row.get(2)?,
                    source: row.get(3)?,
                    card_id: row.get(4)?,
                    status: row.get(5)?,
                    variant_key: row.get(6)?,
                    updated_at: row.get(7)?,
                    state: row.get(8)?,
                    due_at: row.get(9)?,
                    due_study_day: row.get(10)?,
                    last_review_at: row.get(11)?,
                    reps: row.get(12)?,
                    lapses: row.get(13)?,
                    scheduler_config_id: row.get(14)?,
                    scheduler_state_schema_version: row.get(15)?,
                    scheduler_state_json: row.get(16)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| DatabaseError::NotFound {
            entity: "review card",
            id: review_card_id.to_owned(),
        })
}

fn load_active_scheduler_config(connection: &Connection) -> Result<SchedulerConfigRecord> {
    let tuple = connection.query_row(
        "SELECT
            config.id, config.algorithm, config.algorithm_version,
            config.scheduler_library, config.library_version,
            config.config_schema_version, config.config_json
         FROM app_settings AS settings
         JOIN scheduler_config AS config
           ON config.id = settings.active_scheduler_config_id
         WHERE settings.singleton_id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        },
    )?;
    if tuple.1 != "FSRS"
        || tuple.2 != 6
        || tuple.3 != "ts-fsrs"
        || tuple.4 != "5.4.1"
        || tuple.5 != 1
    {
        return Err(DatabaseError::UnsupportedSchedulerConfig(format!(
            "{} is {}/{} via {} {} with config schema {}",
            tuple.0, tuple.1, tuple.2, tuple.3, tuple.4, tuple.5
        )));
    }
    let config = serde_json::from_str(&tuple.6).map_err(|error| {
        DatabaseError::CorruptReviewData(format!("scheduler config JSON: {error}"))
    })?;
    Ok(SchedulerConfigRecord {
        id: tuple.0,
        algorithm: tuple.1,
        algorithm_version: tuple.2,
        scheduler_library: tuple.3,
        library_version: tuple.4,
        config_schema_version: tuple.5,
        config,
    })
}

fn load_review_history(
    connection: &Connection,
    review_card_id: &str,
) -> Result<Vec<PersistedReviewFact>> {
    let mut statement = connection.prepare(
        "SELECT
            event.id, event.card_sequence, event.scheduler_config_id,
            event.reviewed_at, event.study_day, event.timezone_id,
            event.utc_offset_minutes, event.grade, event.scheduler_log_json
         FROM review_event AS event
         WHERE event.review_card_id = ?1
           AND event.event_type = 'REVIEW'
           AND NOT EXISTS (
               SELECT 1
               FROM review_event AS revoke
               WHERE revoke.event_type = 'REVOKE'
                 AND revoke.target_event_id = event.id
           )
         ORDER BY event.card_sequence",
    )?;
    let rows = statement.query_map([review_card_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i16>(6)?,
            row.get::<_, u8>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;

    let mut history = Vec::new();
    for row in rows {
        let row = row?;
        let scheduler_log = serde_json::from_str(&row.8).map_err(|error| {
            DatabaseError::CorruptReviewData(format!(
                "review event {} scheduler log: {error}",
                row.0
            ))
        })?;
        history.push(PersistedReviewFact {
            event_id: row.0,
            card_sequence: row.1,
            scheduler_config_id: row.2,
            review: ReviewFact {
                reviewed_at: row.3,
                study_day: row.4,
                timezone_id: row.5,
                utc_offset_minutes: row.6,
                grade: row.7,
            },
            scheduler_log,
        });
    }
    Ok(history)
}

fn load_event(connection: &Connection, event_id: &str) -> Result<Option<StoredEvent>> {
    connection
        .query_row(
            "SELECT
                event_schema_version, event_type, review_card_id, card_sequence,
                reviewed_at, study_day, timezone_id, utc_offset_minutes, grade,
                scheduler_config_id, scheduler_log_json, target_event_id
             FROM review_event
             WHERE id = ?1",
            [event_id],
            |row| {
                Ok(StoredEvent {
                    event_schema_version: row.get(0)?,
                    event_type: row.get(1)?,
                    review_card_id: row.get(2)?,
                    card_sequence: row.get(3)?,
                    reviewed_at: row.get(4)?,
                    study_day: row.get(5)?,
                    timezone_id: row.get(6)?,
                    utc_offset_minutes: row.get(7)?,
                    grade: row.get(8)?,
                    scheduler_config_id: row.get(9)?,
                    scheduler_log_json: row.get(10)?,
                    target_event_id: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn cache_from_stored(stored: &StoredCard) -> Result<ReviewCardCache> {
    let state = parse_state(&stored.state)?;
    let scheduler_state = match (
        stored.scheduler_state_schema_version,
        stored.scheduler_state_json.as_deref(),
    ) {
        (None, None) => None,
        (Some(SCHEDULER_STATE_SCHEMA_VERSION), Some(json)) => {
            Some(serde_json::from_str(json).map_err(|error| {
                DatabaseError::CorruptReviewData(format!(
                    "card {} scheduler state: {error}",
                    stored.card_id
                ))
            })?)
        }
        (schema, json) => {
            return Err(DatabaseError::CorruptReviewData(format!(
                "card {} has unsupported scheduler state pair {schema:?}/{:?}",
                stored.card_id,
                json.is_some()
            )));
        }
    };
    Ok(ReviewCardCache {
        state,
        due_at: stored.due_at,
        due_study_day: stored.due_study_day,
        last_review_at: stored.last_review_at,
        reps: stored.reps,
        lapses: stored.lapses,
        scheduler_state,
    })
}

fn update_card_cache(
    connection: &Connection,
    review_card_id: &str,
    expected_updated_at: i64,
    updated_at: i64,
    cache: &ReviewCardCache,
    scheduler_config_id: Option<&str>,
) -> Result<()> {
    let (schema_version, scheduler_state_json) = match cache.scheduler_state.as_ref() {
        Some(state) => (
            Some(SCHEDULER_STATE_SCHEMA_VERSION),
            Some(serde_json::to_string(state)?),
        ),
        None => (None, None),
    };
    let changed = connection.execute(
        "UPDATE review_card
         SET updated_at = ?1,
             state = ?2,
             due_at = ?3,
             due_study_day = ?4,
             last_review_at = ?5,
             reps = ?6,
             lapses = ?7,
             scheduler_config_id = ?8,
             scheduler_state_schema_version = ?9,
             scheduler_state_json = ?10
         WHERE id = ?11 AND updated_at = ?12 AND deleted_at IS NULL",
        params![
            updated_at,
            state_name(cache.state),
            cache.due_at,
            cache.due_study_day,
            cache.last_review_at,
            cache.reps,
            cache.lapses,
            scheduler_config_id,
            schema_version,
            scheduler_state_json,
            review_card_id,
            expected_updated_at,
        ],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleReviewContext(
            "review card changed while the grade was being recorded".into(),
        ));
    }
    Ok(())
}

fn validate_record_input(input: &RecordGradeInput) -> Result<()> {
    validate_uuid_v7(&input.event_id, "eventId")?;
    validate_uuid_v7(&input.review_card_id, "reviewCardId")?;
    validate_uuid_v7(
        &input.expected_scheduler_config_id,
        "expectedSchedulerConfigId",
    )?;
    validate_non_negative_safe(
        input.expected_review_card_updated_at,
        "expectedReviewCardUpdatedAt",
    )?;
    validate_non_negative_safe(input.expected_card_sequence, "expectedCardSequence")?;
    validate_review_fact(&input.review)?;
    validate_cache(&input.next_cache).map_err(DatabaseError::InvalidInput)?;
    if input.next_cache.state == ReviewCardState::New {
        return Err(DatabaseError::InvalidInput(
            "a grade cannot produce a NEW cache".into(),
        ));
    }
    validate_scheduler_log(&input.scheduler_log)?;
    Ok(())
}

fn validate_undo_input(input: &UndoLastGradeInput) -> Result<()> {
    validate_uuid_v7(&input.event_id, "eventId")?;
    validate_uuid_v7(&input.review_card_id, "reviewCardId")?;
    validate_uuid_v7(&input.target_event_id, "targetEventId")?;
    validate_uuid_v7(
        &input.expected_scheduler_config_id,
        "expectedSchedulerConfigId",
    )?;
    validate_non_negative_safe(
        input.expected_review_card_updated_at,
        "expectedReviewCardUpdatedAt",
    )?;
    validate_non_negative_safe(input.expected_card_sequence, "expectedCardSequence")?;
    validate_cache(&input.next_cache).map_err(DatabaseError::InvalidInput)
}

fn validate_preconditions(
    context: &ReviewContext,
    expected_updated_at: i64,
    expected_sequence: i64,
    expected_scheduler_config_id: &str,
) -> Result<()> {
    if context.review_card.status != ReviewCardStatus::Active {
        return Err(DatabaseError::InvalidInput(format!(
            "review card {} is suspended",
            context.review_card.id
        )));
    }
    if context.review_card.updated_at != expected_updated_at {
        return Err(DatabaseError::StaleReviewContext(format!(
            "review card timestamp is {}, expected {}",
            context.review_card.updated_at, expected_updated_at
        )));
    }
    if context.last_card_sequence != expected_sequence {
        return Err(DatabaseError::StaleReviewContext(format!(
            "card sequence is {}, expected {}",
            context.last_card_sequence, expected_sequence
        )));
    }
    if context.scheduler_config.id != expected_scheduler_config_id {
        return Err(DatabaseError::StaleReviewContext(format!(
            "active scheduler config is {}, expected {}",
            context.scheduler_config.id, expected_scheduler_config_id
        )));
    }
    Ok(())
}

fn validate_next_review_cache(
    current: &ReviewCardCache,
    review: &ReviewFact,
    next: &ReviewCardCache,
) -> Result<()> {
    if next.last_review_at != Some(review.reviewed_at) {
        return Err(DatabaseError::InvalidInput(
            "nextCache.lastReviewAt must equal review.reviewedAt".into(),
        ));
    }
    let expected_reps = current
        .reps
        .checked_add(1)
        .ok_or_else(|| DatabaseError::InvalidInput("review reps overflow".into()))?;
    if next.reps != expected_reps {
        return Err(DatabaseError::InvalidInput(format!(
            "nextCache.reps is {}, expected {}",
            next.reps, expected_reps
        )));
    }
    let lapse_increment = i64::from(current.state == ReviewCardState::Review && review.grade == 1);
    let expected_lapses = current
        .lapses
        .checked_add(lapse_increment)
        .ok_or_else(|| DatabaseError::InvalidInput("review lapses overflow".into()))?;
    if next.lapses != expected_lapses {
        return Err(DatabaseError::InvalidInput(format!(
            "nextCache.lapses is {}, expected {}",
            next.lapses, expected_lapses
        )));
    }
    match next.state {
        ReviewCardState::Learning | ReviewCardState::Relearning => {
            if next.due_at.is_some_and(|due| due <= review.reviewed_at) {
                return Err(DatabaseError::InvalidInput(
                    "an intraday dueAt must be after reviewedAt".into(),
                ));
            }
        }
        ReviewCardState::Review => {
            if next
                .due_study_day
                .is_some_and(|due| due <= review.study_day)
            {
                return Err(DatabaseError::InvalidInput(
                    "a review dueStudyDay must be after the review studyDay".into(),
                ));
            }
        }
        ReviewCardState::New => unreachable!("validated record input cannot be NEW"),
    }
    Ok(())
}

fn validate_undo_cache(cache: &ReviewCardCache, remaining: &[PersistedReviewFact]) -> Result<()> {
    if remaining.is_empty() {
        if cache != &ReviewCardCache::new() {
            return Err(DatabaseError::InvalidInput(
                "undoing the first grade must restore the exact NEW cache".into(),
            ));
        }
        return Ok(());
    }
    if cache.state == ReviewCardState::New {
        return Err(DatabaseError::InvalidInput(
            "a card with remaining reviews cannot have a NEW cache".into(),
        ));
    }
    let remaining_count = i64::try_from(remaining.len())
        .map_err(|_| DatabaseError::InvalidInput("review count overflow".into()))?;
    if cache.reps != remaining_count {
        return Err(DatabaseError::InvalidInput(format!(
            "undo cache has {} reps, expected {}",
            cache.reps, remaining_count
        )));
    }
    let previous_reviewed_at = remaining.last().map(|event| event.review.reviewed_at);
    if cache.last_review_at != previous_reviewed_at {
        return Err(DatabaseError::InvalidInput(
            "undo cache does not end at the last remaining review".into(),
        ));
    }
    Ok(())
}

fn validate_review_fact(review: &ReviewFact) -> Result<()> {
    if !(1..=4).contains(&review.grade) {
        return Err(DatabaseError::InvalidInput(
            "review.grade must be between 1 and 4".into(),
        ));
    }
    validate_non_negative_safe(review.reviewed_at, "review.reviewedAt")?;
    validate_safe_integer(review.study_day, "review.studyDay")?;
    if review.timezone_id.trim().is_empty() {
        return Err(DatabaseError::InvalidInput(
            "review.timezoneId must not be empty".into(),
        ));
    }
    if !(-840..=840).contains(&review.utc_offset_minutes) {
        return Err(DatabaseError::InvalidInput(
            "review.utcOffsetMinutes must be between -840 and 840".into(),
        ));
    }
    Ok(())
}

fn validate_cache(cache: &ReviewCardCache) -> std::result::Result<(), String> {
    validate_non_negative_safe_value(cache.reps, "cache.reps")?;
    validate_non_negative_safe_value(cache.lapses, "cache.lapses")?;
    if cache.lapses > cache.reps {
        return Err("cache.lapses cannot exceed cache.reps".into());
    }
    if let Some(due_at) = cache.due_at {
        validate_non_negative_safe_value(due_at, "cache.dueAt")?;
    }
    if let Some(due_study_day) = cache.due_study_day {
        validate_safe_integer_value(due_study_day, "cache.dueStudyDay")?;
    }
    if let Some(last_review_at) = cache.last_review_at {
        validate_non_negative_safe_value(last_review_at, "cache.lastReviewAt")?;
    }
    if let Some(state) = cache.scheduler_state.as_ref() {
        if !state.stability.is_finite() || state.stability <= 0.0 {
            return Err("cache.schedulerState.stability must be finite and positive".into());
        }
        if !state.difficulty.is_finite() || !(1.0..=10.0).contains(&state.difficulty) {
            return Err("cache.schedulerState.difficulty must be between 1 and 10".into());
        }
        validate_non_negative_safe_value(
            state.scheduled_days,
            "cache.schedulerState.scheduledDays",
        )?;
        validate_non_negative_safe_value(
            state.learning_steps,
            "cache.schedulerState.learningSteps",
        )?;
    }

    let structurally_valid = match cache.state {
        ReviewCardState::New => {
            cache.due_at.is_none()
                && cache.due_study_day.is_none()
                && cache.last_review_at.is_none()
                && cache.reps == 0
                && cache.lapses == 0
                && cache.scheduler_state.is_none()
        }
        ReviewCardState::Learning | ReviewCardState::Relearning => {
            cache.due_at.is_some()
                && cache.due_study_day.is_none()
                && cache.last_review_at.is_some()
                && cache.reps > 0
                && cache.scheduler_state.is_some()
        }
        ReviewCardState::Review => {
            cache.due_at.is_none()
                && cache.due_study_day.is_some()
                && cache.last_review_at.is_some()
                && cache.reps > 0
                && cache.scheduler_state.is_some()
        }
    };
    if !structurally_valid {
        return Err(format!(
            "cache fields do not match its {} state",
            state_name(cache.state)
        ));
    }
    Ok(())
}

fn validate_scheduler_log(log: &SchedulerLogV1) -> Result<()> {
    if let Some(value) = log.due_at_before {
        validate_non_negative_safe(value, "schedulerLog.dueAtBefore")?;
    }
    if let Some(value) = log.due_study_day_before {
        validate_safe_integer(value, "schedulerLog.dueStudyDayBefore")?;
    }
    for (name, value) in [
        ("schedulerLog.stabilityBefore", log.stability_before),
        ("schedulerLog.difficultyBefore", log.difficulty_before),
    ] {
        if value.is_some_and(|number| !number.is_finite()) {
            return Err(DatabaseError::InvalidInput(format!(
                "{name} must be finite"
            )));
        }
    }
    if let Some(value) = log.scheduled_days_before {
        validate_non_negative_safe(value, "schedulerLog.scheduledDaysBefore")?;
    }
    if let Some(value) = log.learning_steps_before {
        validate_non_negative_safe(value, "schedulerLog.learningStepsBefore")?;
    }
    Ok(())
}

fn validate_materialized_history(
    cache: &ReviewCardCache,
    history: &[PersistedReviewFact],
) -> Result<()> {
    let history_count = i64::try_from(history.len())
        .map_err(|_| DatabaseError::CorruptReviewData("review count overflow".into()))?;
    if cache.reps != history_count {
        return Err(DatabaseError::CorruptReviewData(format!(
            "cache has {} reps but {} non-revoked reviews",
            cache.reps, history_count
        )));
    }
    let last_review_at = history.last().map(|event| event.review.reviewed_at);
    if cache.last_review_at != last_review_at {
        return Err(DatabaseError::CorruptReviewData(
            "cache lastReviewAt does not match review history".into(),
        ));
    }
    Ok(())
}

fn scheduler_log_for(cache: &ReviewCardCache) -> SchedulerLogV1 {
    SchedulerLogV1 {
        state_before: cache.state,
        due_at_before: cache.due_at,
        due_study_day_before: cache.due_study_day,
        stability_before: cache.scheduler_state.as_ref().map(|state| state.stability),
        difficulty_before: cache.scheduler_state.as_ref().map(|state| state.difficulty),
        scheduled_days_before: cache
            .scheduler_state
            .as_ref()
            .map(|state| state.scheduled_days),
        learning_steps_before: cache
            .scheduler_state
            .as_ref()
            .map(|state| state.learning_steps),
    }
}

fn review_event_matches(
    event: &StoredEvent,
    input: &RecordGradeInput,
    scheduler_log_json: &str,
) -> bool {
    event.event_schema_version == EVENT_SCHEMA_VERSION
        && event.event_type == "REVIEW"
        && event.review_card_id == input.review_card_id
        && event.reviewed_at == Some(input.review.reviewed_at)
        && event.study_day == Some(input.review.study_day)
        && event.timezone_id.as_deref() == Some(input.review.timezone_id.as_str())
        && event.utc_offset_minutes == Some(input.review.utc_offset_minutes)
        && event.grade == Some(input.review.grade)
        && event.scheduler_config_id == input.expected_scheduler_config_id
        && event.scheduler_log_json.as_deref() == Some(scheduler_log_json)
        && event.target_event_id.is_none()
}

fn revoke_event_matches(event: &StoredEvent, input: &UndoLastGradeInput) -> bool {
    event.event_schema_version == EVENT_SCHEMA_VERSION
        && event.event_type == "REVOKE"
        && event.review_card_id == input.review_card_id
        && event.scheduler_config_id == input.expected_scheduler_config_id
        && event.target_event_id.as_deref() == Some(input.target_event_id.as_str())
        && event.reviewed_at.is_none()
        && event.study_day.is_none()
        && event.timezone_id.is_none()
        && event.utc_offset_minutes.is_none()
        && event.grade.is_none()
        && event.scheduler_log_json.is_none()
}

fn parse_status(value: &str) -> Result<ReviewCardStatus> {
    match value {
        "ACTIVE" => Ok(ReviewCardStatus::Active),
        "SUSPENDED" => Ok(ReviewCardStatus::Suspended),
        other => Err(DatabaseError::CorruptReviewData(format!(
            "unknown review card status {other}"
        ))),
    }
}

fn parse_state(value: &str) -> Result<ReviewCardState> {
    match value {
        "NEW" => Ok(ReviewCardState::New),
        "LEARNING" => Ok(ReviewCardState::Learning),
        "REVIEW" => Ok(ReviewCardState::Review),
        "RELEARNING" => Ok(ReviewCardState::Relearning),
        other => Err(DatabaseError::CorruptReviewData(format!(
            "unknown review card state {other}"
        ))),
    }
}

fn state_name(state: ReviewCardState) -> &'static str {
    match state {
        ReviewCardState::New => "NEW",
        ReviewCardState::Learning => "LEARNING",
        ReviewCardState::Review => "REVIEW",
        ReviewCardState::Relearning => "RELEARNING",
    }
}

fn validate_uuid_v7(value: &str, name: &str) -> Result<()> {
    let uuid = Uuid::parse_str(value)
        .map_err(|_| DatabaseError::InvalidInput(format!("{name} must be a UUIDv7")))?;
    if uuid.get_version_num() != 7 || uuid.to_string() != value {
        return Err(DatabaseError::InvalidInput(format!(
            "{name} must be a canonical lowercase UUIDv7"
        )));
    }
    Ok(())
}

fn validate_non_negative_safe(value: i64, name: &str) -> Result<()> {
    validate_non_negative_safe_value(value, name).map_err(DatabaseError::InvalidInput)
}

fn validate_safe_integer(value: i64, name: &str) -> Result<()> {
    validate_safe_integer_value(value, name).map_err(DatabaseError::InvalidInput)
}

fn validate_non_negative_safe_value(value: i64, name: &str) -> std::result::Result<(), String> {
    if !(0..=MAX_JSON_SAFE_INTEGER).contains(&value) {
        return Err(format!("{name} must be a non-negative JSON-safe integer"));
    }
    Ok(())
}

fn validate_safe_integer_value(value: i64, name: &str) -> std::result::Result<(), String> {
    if !(-MAX_JSON_SAFE_INTEGER..=MAX_JSON_SAFE_INTEGER).contains(&value) {
        return Err(format!("{name} must be a JSON-safe integer"));
    }
    Ok(())
}

fn next_sequence(current: i64) -> Result<i64> {
    current
        .checked_add(1)
        .filter(|value| *value <= MAX_JSON_SAFE_INTEGER)
        .ok_or_else(|| DatabaseError::InvalidInput("card sequence overflow".into()))
}

fn next_updated_at(current: i64, now: i64) -> Result<i64> {
    let incremented = current
        .checked_add(1)
        .ok_or_else(|| DatabaseError::InvalidInput("review card timestamp overflow".into()))?;
    Ok(now.max(incremented))
}

fn now_millis() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DatabaseError::InvalidSystemTime)?;
    i64::try_from(duration.as_millis()).map_err(|_| DatabaseError::InvalidSystemTime)
}
