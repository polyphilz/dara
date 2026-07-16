use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use pulldown_cmark::{
    Event as MarkdownEvent, Options as MarkdownOptions, Parser as MarkdownParser,
};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension, Row,
    Transaction, TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::media;
use super::{DatabaseError, Result};

const EVENT_SCHEMA_VERSION: i64 = 1;
const SCHEDULER_STATE_SCHEMA_VERSION: i64 = 1;
const MAX_JSON_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const SUPPORTED_SCHEDULER_ALGORITHM_VERSION: i64 = 6;
const SUPPORTED_SCHEDULER_LIBRARY_VERSION: &str = "5.4.1";
const SUPPORTED_SCHEDULER_CONFIG_SCHEMA_VERSION: i64 = 1;
// An explicit, non-token field boundary keeps the aggregate deterministic.
pub(super) const SEARCH_FIELD_SEPARATOR: &str = "\n\u{1e}\n";
const BASIC_VARIANT_KEY: &str = "basic";
const CLOZE_VARIANT_PREFIX: &str = "cloze:";
const OCCLUSION_VARIANT_PREFIX: &str = "layer:";
const CARD_CONTENT_LIST_SELECT: &str = "
    WITH lifecycle AS (
        SELECT
            card_content_id,
            min(status) AS review_status,
            count(DISTINCT status) AS review_status_count,
            max(updated_at) AS lifecycle_updated_at
        FROM review_card
        WHERE deleted_at IS NULL
        GROUP BY card_content_id
    )
    SELECT
        content.id, content.created_at, content.updated_at, content.type,
        content.front_md, content.back_md, content.source,
        lifecycle.review_status, lifecycle.review_status_count,
        lifecycle.lifecycle_updated_at
    FROM card_content AS content
    JOIN lifecycle ON lifecycle.card_content_id = content.id";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum CardContentDraft {
    Basic {
        #[serde(rename = "frontMd")]
        front_md: String,
        #[serde(rename = "backMd")]
        back_md: String,
        source: Option<String>,
    },
    Cloze {
        #[serde(rename = "frontMd")]
        front_md: String,
        #[serde(rename = "backMd")]
        back_md: String,
        source: Option<String>,
        #[serde(rename = "variantKeys")]
        variant_keys: Vec<String>,
        #[serde(rename = "searchMd")]
        search_md: String,
    },
    Occlusion {
        #[serde(rename = "frontMd")]
        front_md: String,
        #[serde(rename = "backMd")]
        back_md: String,
        source: Option<String>,
        occlusion: OcclusionDefinitionDraft,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OcclusionDefinitionDraft {
    pub(super) id: String,
    pub(super) source_image_id: String,
    pub(super) mode: OcclusionMode,
    pub(super) layers: Vec<OcclusionMaskLayerDraft>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OcclusionMaskLayerDraft {
    pub(super) id: String,
    pub(super) label: Option<String>,
    pub(super) masks: Vec<OcclusionMaskDraft>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OcclusionMaskDraft {
    pub(super) id: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) color: OcclusionMaskColor,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OcclusionMode {
    HideOneGuessOne,
    HideAllGuessOne,
}

impl OcclusionMode {
    const fn as_db_str(self) -> &'static str {
        match self {
            Self::HideOneGuessOne => "HIDE_ONE_GUESS_ONE",
            Self::HideAllGuessOne => "HIDE_ALL_GUESS_ONE",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::HideOneGuessOne.as_db_str() {
            Ok(Self::HideOneGuessOne)
        } else if value == Self::HideAllGuessOne.as_db_str() {
            Ok(Self::HideAllGuessOne)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown image occlusion mode {value}"
            )))
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OcclusionMaskColor {
    White,
    Black,
}

impl OcclusionMaskColor {
    const fn as_db_str(self) -> &'static str {
        match self {
            Self::White => "WHITE",
            Self::Black => "BLACK",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::White.as_db_str() {
            Ok(Self::White)
        } else if value == Self::Black.as_db_str() {
            Ok(Self::Black)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown image occlusion mask color {value}"
            )))
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateCardContentInput {
    pub id: String,
    pub expected_updated_at: i64,
    pub content: CardContentDraft,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CardContent {
    Basic {
        id: String,
        #[serde(rename = "createdAt")]
        created_at: i64,
        #[serde(rename = "updatedAt")]
        updated_at: i64,
        #[serde(rename = "frontMd")]
        front_md: String,
        #[serde(rename = "backMd")]
        back_md: String,
        source: Option<String>,
    },
    Cloze {
        id: String,
        #[serde(rename = "createdAt")]
        created_at: i64,
        #[serde(rename = "updatedAt")]
        updated_at: i64,
        #[serde(rename = "frontMd")]
        front_md: String,
        #[serde(rename = "backMd")]
        back_md: String,
        source: Option<String>,
    },
    Occlusion {
        id: String,
        #[serde(rename = "createdAt")]
        created_at: i64,
        #[serde(rename = "updatedAt")]
        updated_at: i64,
        #[serde(rename = "frontMd")]
        front_md: String,
        #[serde(rename = "backMd")]
        back_md: String,
        source: Option<String>,
        occlusion: OcclusionDefinition,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionDefinition {
    pub(super) id: String,
    pub(super) source_image: media::ImageRecord,
    pub(super) mode: OcclusionMode,
    pub(super) layers: Vec<OcclusionMaskLayer>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionMaskLayer {
    pub(super) id: String,
    pub(super) label: Option<String>,
    pub(super) masks: Vec<OcclusionMask>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcclusionMask {
    pub(super) id: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) color: OcclusionMaskColor,
}

impl CardContent {
    pub fn id(&self) -> &str {
        match self {
            Self::Basic { id, .. } | Self::Cloze { id, .. } | Self::Occlusion { id, .. } => id,
        }
    }

    pub fn updated_at(&self) -> i64 {
        match self {
            Self::Basic { updated_at, .. }
            | Self::Cloze { updated_at, .. }
            | Self::Occlusion { updated_at, .. } => *updated_at,
        }
    }

    fn content_type(&self) -> CardContentType {
        match self {
            Self::Basic { .. } => CardContentType::Basic,
            Self::Cloze { .. } => CardContentType::Cloze,
            Self::Occlusion { .. } => CardContentType::Occlusion,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CardContentReviewStatus {
    Active,
    Suspended,
    Mixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CardContentType {
    Basic,
    Cloze,
    Occlusion,
}

impl CardContentType {
    pub(super) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Basic => "BASIC",
            Self::Cloze => "CLOZE",
            Self::Occlusion => "OCCLUSION",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::Basic.as_db_str() {
            Ok(Self::Basic)
        } else if value == Self::Cloze.as_db_str() {
            Ok(Self::Cloze)
        } else if value == Self::Occlusion.as_db_str() {
            Ok(Self::Occlusion)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unsupported active card content type {value}"
            )))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardContentListItem {
    pub card_content: CardContent,
    pub review_cards: Vec<ReviewCardListItem>,
    pub review_status: CardContentReviewStatus,
    pub lifecycle_updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCardListItem {
    pub id: String,
    pub status: ReviewCardStatus,
    pub variant_key: String,
    pub state: ReviewCardState,
    pub due_at: Option<i64>,
    pub due_study_day: Option<i64>,
    pub last_review_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchCardContentInput {
    pub query: String,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetCardContentSuspendedInput {
    pub card_content_id: String,
    pub expected_lifecycle_updated_at: i64,
    pub suspended: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteCardContentInput {
    pub card_content_id: String,
    pub expected_updated_at: i64,
    pub expected_lifecycle_updated_at: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewCardStatus {
    Active,
    Suspended,
}

impl ReviewCardStatus {
    pub(super) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Suspended => "SUSPENDED",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::Active.as_db_str() {
            Ok(Self::Active)
        } else if value == Self::Suspended.as_db_str() {
            Ok(Self::Suspended)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown review card status {value}"
            )))
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewCardState {
    New,
    Learning,
    Review,
    Relearning,
}

impl ReviewCardState {
    pub(super) const fn as_db_str(self) -> &'static str {
        match self {
            Self::New => "NEW",
            Self::Learning => "LEARNING",
            Self::Review => "REVIEW",
            Self::Relearning => "RELEARNING",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::New.as_db_str() {
            Ok(Self::New)
        } else if value == Self::Learning.as_db_str() {
            Ok(Self::Learning)
        } else if value == Self::Review.as_db_str() {
            Ok(Self::Review)
        } else if value == Self::Relearning.as_db_str() {
            Ok(Self::Relearning)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown review card state {value}"
            )))
        }
    }
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
    pub algorithm: SchedulerAlgorithm,
    pub algorithm_version: i64,
    pub scheduler_library: SchedulerLibrary,
    pub library_version: String,
    pub config_schema_version: i64,
    pub config: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchedulerAlgorithm {
    Fsrs,
}

impl SchedulerAlgorithm {
    const fn as_db_str(self) -> &'static str {
        match self {
            Self::Fsrs => "FSRS",
        }
    }

    fn from_db(value: &str) -> Option<Self> {
        if value == Self::Fsrs.as_db_str() {
            Some(Self::Fsrs)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum SchedulerLibrary {
    #[serde(rename = "ts-fsrs")]
    TsFsrs,
}

impl SchedulerLibrary {
    const fn as_db_str(self) -> &'static str {
        match self {
            Self::TsFsrs => "ts-fsrs",
        }
    }

    fn from_db(value: &str) -> Option<Self> {
        if value == Self::TsFsrs.as_db_str() {
            Some(Self::TsFsrs)
        } else {
            None
        }
    }
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
    pub card_content: CardContent,
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
    pub expected_card_content_updated_at: i64,
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
    content_created_at: i64,
    content_updated_at: i64,
    content_type: CardContentType,
    front_md: String,
    back_md: String,
    source: Option<String>,
    card_id: String,
    status: ReviewCardStatus,
    variant_key: String,
    updated_at: i64,
    state: ReviewCardState,
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
    event_type: ReviewEventType,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReviewEventType {
    Review,
    Revoke,
}

impl ReviewEventType {
    pub(super) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Review => "REVIEW",
            Self::Revoke => "REVOKE",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::Review.as_db_str() {
            Ok(Self::Review)
        } else if value == Self::Revoke.as_db_str() {
            Ok(Self::Revoke)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown review event type {value}"
            )))
        }
    }
}

pub(super) fn create_card_content(
    connection: &mut Connection,
    input: CardContentDraft,
) -> Result<ReviewContext> {
    validate_card_content(&input)?;
    let now = now_millis()?;
    let content_id = Uuid::now_v7().to_string();
    let fields = draft_fields(&input);
    let image_references = media::parse_card_image_references(fields.front_md, fields.back_md)?;
    let occlusion_source_image_id = fields.occlusion.map(|value| value.source_image_id.as_str());

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    media::validate_active_image_references(&transaction, &image_references)?;
    if let Some(image_id) = occlusion_source_image_id {
        media::load_active_image_record(&transaction, image_id)?;
    }
    let search_body = search_body(
        &transaction,
        fields.search_md,
        fields.back_md,
        fields.source,
        &image_references,
        occlusion_source_image_id,
    )?;
    if let Some(occlusion) = fields.occlusion {
        insert_occlusion_definition(&transaction, &content_id, occlusion, now)?;
    }
    let content_hash = Sha256::digest(search_body.as_bytes());
    transaction.execute(
        "INSERT INTO card_content (
            id, created_at, updated_at, deleted_at, type, front_md, back_md, source
         ) VALUES (?1, ?2, ?2, NULL, ?3, ?4, ?5, ?6)",
        params![
            content_id,
            now,
            fields.content_type.as_db_str(),
            fields.front_md,
            fields.back_md,
            fields.source
        ],
    )?;
    let mut first_review_card_id = None;
    for variant_key in &fields.variant_keys {
        let review_card_id = insert_new_review_card(&transaction, &content_id, variant_key, now)?;
        first_review_card_id.get_or_insert(review_card_id);
    }
    let first_review_card_id = first_review_card_id
        .ok_or_else(|| DatabaseError::InvalidInput("card has no review variants".into()))?;
    media::sync_card_content_images(&transaction, &content_id, &image_references)?;
    transaction.execute(
        "INSERT INTO search_document (card_content_id, body, content_hash, updated_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![content_id, search_body, content_hash.as_slice(), now],
    )?;
    transaction.commit()?;

    load_review_context(connection, &first_review_card_id)
}

pub(super) fn update_card_content(
    connection: &mut Connection,
    input: UpdateCardContentInput,
) -> Result<CardContentListItem> {
    validate_uuid_v7(&input.id, "id")?;
    validate_non_negative_safe(input.expected_updated_at, "expectedUpdatedAt")?;
    validate_card_content(&input.content)?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = load_card_content(&transaction, &input.id)?;
    if current.updated_at() != input.expected_updated_at {
        return Err(DatabaseError::StaleCardContent(format!(
            "card content timestamp is {}, expected {}",
            current.updated_at(),
            input.expected_updated_at
        )));
    }
    let fields = draft_fields(&input.content);
    if current.content_type() != fields.content_type {
        return Err(DatabaseError::InvalidInput(
            "card type cannot be changed after creation".into(),
        ));
    }
    let image_references = media::parse_card_image_references(fields.front_md, fields.back_md)?;
    media::validate_active_image_references(&transaction, &image_references)?;
    let occlusion_source_image_id = fields.occlusion.map(|value| value.source_image_id.as_str());
    if let Some(image_id) = occlusion_source_image_id {
        media::load_active_image_record(&transaction, image_id)?;
    }

    let now = now_millis()?;
    let updated_at = next_updated_at(current.updated_at(), now)?;
    let changed = transaction.execute(
        "UPDATE card_content
         SET updated_at = ?1, type = ?2, front_md = ?3, back_md = ?4, source = ?5
         WHERE id = ?6 AND updated_at = ?7 AND deleted_at IS NULL",
        params![
            updated_at,
            fields.content_type.as_db_str(),
            fields.front_md,
            fields.back_md,
            fields.source,
            input.id,
            input.expected_updated_at,
        ],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleCardContent(
            "card content changed before the edit was saved".into(),
        ));
    }
    rebuild_search_document(
        &transaction,
        &input.id,
        fields.search_md,
        fields.back_md,
        fields.source,
        &image_references,
        occlusion_source_image_id,
        updated_at,
    )?;
    media::sync_card_content_images(&transaction, &input.id, &image_references)?;
    if let Some(occlusion) = fields.occlusion {
        reconcile_occlusion_definition(&transaction, &input.id, occlusion, updated_at)?;
    }
    reconcile_review_card_variants(&transaction, &input.id, fields.variant_keys, updated_at)?;
    transaction.commit()?;
    load_card_content_list_item(connection, &input.id)
}

pub(super) fn search_card_content(
    connection: &mut Connection,
    input: SearchCardContentInput,
) -> Result<Vec<CardContentListItem>> {
    if !(1..=100).contains(&input.limit) {
        return Err(DatabaseError::InvalidInput(
            "limit must be between 1 and 100".into(),
        ));
    }
    validate_non_negative_safe(input.offset, "offset")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let terms = literal_search_terms(&input.query);
    let mut stored_results = Vec::new();
    if !terms.is_empty() && terms.iter().all(|term| term.chars().count() >= 3) {
        let fts_query = literal_trigram_query(&terms);
        let mut statement = transaction.prepare(&format!(
            "{CARD_CONTENT_LIST_SELECT}
             JOIN search_document AS document ON document.card_content_id = content.id
             JOIN search_document_fts ON search_document_fts.rowid = document.rowid
             WHERE content.deleted_at IS NULL
               AND search_document_fts MATCH ?1
             ORDER BY bm25(search_document_fts), content.updated_at DESC, content.id
             LIMIT ?2 OFFSET ?3"
        ))?;
        let rows = statement.query_map(
            params![fts_query, input.limit, input.offset],
            card_content_list_row,
        )?;
        for row in rows {
            stored_results.push(row?);
        }
    } else if !terms.is_empty() {
        // FTS5's trigram tokenizer cannot produce a token for a one- or
        // two-character term. Keep those early keystrokes useful with a
        // literal scan; normal queries remain index-backed.
        let predicates = terms
            .iter()
            .enumerate()
            .map(|(index, _)| format!("instr(lower(document.body), lower(?{})) > 0", index + 1))
            .collect::<Vec<_>>()
            .join(" AND ");
        let limit_parameter = terms.len() + 1;
        let offset_parameter = terms.len() + 2;
        let mut parameters = terms.into_iter().map(SqlValue::Text).collect::<Vec<_>>();
        parameters.push(SqlValue::Integer(input.limit));
        parameters.push(SqlValue::Integer(input.offset));
        let mut statement = transaction.prepare(&format!(
            "{CARD_CONTENT_LIST_SELECT}
             JOIN search_document AS document ON document.card_content_id = content.id
             WHERE content.deleted_at IS NULL
               AND {predicates}
             ORDER BY content.updated_at DESC, content.id
             LIMIT ?{limit_parameter} OFFSET ?{offset_parameter}"
        ))?;
        let rows = statement.query_map(params_from_iter(parameters), card_content_list_row)?;
        for row in rows {
            stored_results.push(row?);
        }
    } else if input.query.trim().is_empty() {
        let mut statement = transaction.prepare(&format!(
            "{CARD_CONTENT_LIST_SELECT}
             WHERE content.deleted_at IS NULL
             ORDER BY content.updated_at DESC, content.id
             LIMIT ?1 OFFSET ?2"
        ))?;
        let rows =
            statement.query_map(params![input.limit, input.offset], card_content_list_row)?;
        for row in rows {
            stored_results.push(row?);
        }
    }
    drop(transaction);
    stored_results
        .into_iter()
        .map(|item| hydrate_card_content_list_item(connection, item))
        .collect()
}

pub(super) fn set_card_content_suspended(
    connection: &mut Connection,
    input: SetCardContentSuspendedInput,
) -> Result<CardContentListItem> {
    validate_uuid_v7(&input.card_content_id, "cardContentId")?;
    validate_non_negative_safe(
        input.expected_lifecycle_updated_at,
        "expectedLifecycleUpdatedAt",
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = load_card_content_list_item(&transaction, &input.card_content_id)?;
    if current.lifecycle_updated_at != input.expected_lifecycle_updated_at {
        return Err(DatabaseError::StaleCardContent(format!(
            "card lifecycle timestamp is {}, expected {}",
            current.lifecycle_updated_at, input.expected_lifecycle_updated_at
        )));
    }
    let now = now_millis()?;
    let updated_at = next_updated_at(current.lifecycle_updated_at, now)?;
    let (status, suspended_at) = if input.suspended {
        (ReviewCardStatus::Suspended, Some(now))
    } else {
        (ReviewCardStatus::Active, None)
    };
    let changed = transaction.execute(
        "UPDATE review_card
         SET status = ?1, suspended_at = ?2, updated_at = ?3
         WHERE card_content_id = ?4 AND deleted_at IS NULL",
        params![
            status.as_db_str(),
            suspended_at,
            updated_at,
            input.card_content_id
        ],
    )?;
    if changed == 0 {
        return Err(DatabaseError::NotFound {
            entity: "active review cards for content",
            id: input.card_content_id,
        });
    }
    transaction.commit()?;
    load_card_content_list_item(connection, current.card_content.id())
}

pub(super) fn delete_card_content(
    connection: &mut Connection,
    input: DeleteCardContentInput,
) -> Result<()> {
    validate_uuid_v7(&input.card_content_id, "cardContentId")?;
    validate_non_negative_safe(input.expected_updated_at, "expectedUpdatedAt")?;
    validate_non_negative_safe(
        input.expected_lifecycle_updated_at,
        "expectedLifecycleUpdatedAt",
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = load_card_content_list_item(&transaction, &input.card_content_id)?;
    if current.card_content.updated_at() != input.expected_updated_at
        || current.lifecycle_updated_at != input.expected_lifecycle_updated_at
    {
        return Err(DatabaseError::StaleCardContent(
            "card content changed before it could be deleted".into(),
        ));
    }

    let now = now_millis()?;
    let content_updated_at = next_updated_at(current.card_content.updated_at(), now)?;
    let lifecycle_updated_at = next_updated_at(current.lifecycle_updated_at, now)?;
    transaction.execute(
        "UPDATE card_content
         SET updated_at = ?1, deleted_at = ?2
         WHERE id = ?3 AND updated_at = ?4 AND deleted_at IS NULL",
        params![
            content_updated_at,
            now,
            input.card_content_id,
            input.expected_updated_at
        ],
    )?;
    transaction.execute(
        "UPDATE review_card
         SET updated_at = ?1, deleted_at = ?2
         WHERE card_content_id = ?3 AND deleted_at IS NULL",
        params![lifecycle_updated_at, now, input.card_content_id],
    )?;
    transaction.execute(
        "DELETE FROM card_content_image WHERE card_content_id = ?1",
        [&input.card_content_id],
    )?;
    transaction.execute(
        "UPDATE card_occlusion_mask
         SET updated_at = ?1, deleted_at = ?2
         WHERE card_occlusion_mask_layer_id IN (
             SELECT layer.id
             FROM card_occlusion_mask_layer AS layer
             JOIN card_occlusion_content AS occlusion
               ON occlusion.id = layer.card_occlusion_content_id
             WHERE occlusion.card_content_id = ?3
               AND layer.deleted_at IS NULL
               AND occlusion.deleted_at IS NULL
         ) AND deleted_at IS NULL",
        params![content_updated_at, now, input.card_content_id],
    )?;
    transaction.execute(
        "UPDATE card_occlusion_mask_layer
         SET updated_at = ?1, deleted_at = ?2
         WHERE card_occlusion_content_id IN (
             SELECT id FROM card_occlusion_content
             WHERE card_content_id = ?3 AND deleted_at IS NULL
         ) AND deleted_at IS NULL",
        params![content_updated_at, now, input.card_content_id],
    )?;
    transaction.execute(
        "UPDATE card_occlusion_content
         SET updated_at = ?1, deleted_at = ?2
         WHERE card_content_id = ?3 AND deleted_at IS NULL",
        params![content_updated_at, now, input.card_content_id],
    )?;
    // Search documents are derived rather than tombstoned. No embedding rows are
    // produced before the complete-search milestone, so vector invalidation remains deferred.
    transaction.execute(
        "DELETE FROM search_document WHERE card_content_id = ?1",
        [&input.card_content_id],
    )?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn load_review_context(
    connection: &Connection,
    review_card_id: &str,
) -> Result<ReviewContext> {
    validate_uuid_v7(review_card_id, "reviewCardId")?;
    let stored = load_stored_card(connection, review_card_id)?;
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
        card_content: card_content_from_fields(
            connection,
            stored.content_id,
            stored.content_created_at,
            stored.content_updated_at,
            stored.content_type,
            stored.front_md,
            stored.back_md,
            stored.source,
        )?,
        review_card: ReviewCardSummary {
            id: stored.card_id,
            status: stored.status,
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
        Some(input.expected_card_content_updated_at),
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
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL
         )",
        params![
            input.event_id,
            now,
            EVENT_SCHEMA_VERSION,
            ReviewEventType::Review.as_db_str(),
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
        None,
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
            ?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, NULL, ?7, NULL, ?8
         )",
        params![
            input.event_id,
            now,
            EVENT_SCHEMA_VERSION,
            ReviewEventType::Revoke.as_db_str(),
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

fn validate_card_content(input: &CardContentDraft) -> Result<()> {
    match input {
        CardContentDraft::Basic {
            front_md, back_md, ..
        } => {
            if markdown_plain_text(front_md).trim().is_empty()
                && !media::contains_image_reference(front_md)?
            {
                return Err(DatabaseError::InvalidInput(
                    "frontMd must contain visible content".into(),
                ));
            }
            if markdown_plain_text(back_md).trim().is_empty()
                && !media::contains_image_reference(back_md)?
            {
                return Err(DatabaseError::InvalidInput(
                    "backMd must contain visible content".into(),
                ));
            }
        }
        CardContentDraft::Cloze {
            front_md,
            variant_keys,
            search_md,
            ..
        } => {
            if front_md.trim().is_empty() {
                return Err(DatabaseError::InvalidInput(
                    "frontMd must contain cloze content".into(),
                ));
            }
            if markdown_plain_text(search_md).trim().is_empty() {
                return Err(DatabaseError::InvalidInput(
                    "searchMd must contain the revealed cloze text".into(),
                ));
            }
            validate_cloze_variant_keys(variant_keys)?;
        }
        CardContentDraft::Occlusion { occlusion, .. } => {
            validate_occlusion_definition(occlusion)?;
        }
    }
    Ok(())
}

fn validate_occlusion_definition(definition: &OcclusionDefinitionDraft) -> Result<()> {
    validate_uuid_v7(&definition.id, "occlusion.id")?;
    validate_uuid_v7(&definition.source_image_id, "occlusion.sourceImageId")?;
    if definition.layers.is_empty() {
        return Err(DatabaseError::InvalidInput(
            "an image occlusion card must have at least one layer".into(),
        ));
    }
    let mut ids = HashSet::new();
    if !ids.insert(definition.id.as_str()) {
        unreachable!("a fresh ID set accepts the definition ID");
    }
    for layer in &definition.layers {
        validate_uuid_v7(&layer.id, "occlusion layer ID")?;
        if !ids.insert(layer.id.as_str()) {
            return Err(DatabaseError::InvalidInput(
                "image occlusion IDs must be unique".into(),
            ));
        }
        if layer.masks.is_empty() {
            return Err(DatabaseError::InvalidInput(format!(
                "image occlusion layer {} has no masks",
                layer.id
            )));
        }
        if layer.label.as_ref().is_some_and(|label| label.len() > 500) {
            return Err(DatabaseError::InvalidInput(
                "image occlusion layer labels must be at most 500 characters".into(),
            ));
        }
        for mask in &layer.masks {
            validate_uuid_v7(&mask.id, "occlusion mask ID")?;
            if !ids.insert(mask.id.as_str()) {
                return Err(DatabaseError::InvalidInput(
                    "image occlusion IDs must be unique".into(),
                ));
            }
            if !mask.x.is_finite()
                || !mask.y.is_finite()
                || !mask.width.is_finite()
                || !mask.height.is_finite()
                || !(0.0..1.0).contains(&mask.x)
                || !(0.0..1.0).contains(&mask.y)
                || !(0.0..=1.0).contains(&mask.width)
                || !(0.0..=1.0).contains(&mask.height)
                || mask.width == 0.0
                || mask.height == 0.0
                || mask.x + mask.width > 1.0
                || mask.y + mask.height > 1.0
            {
                return Err(DatabaseError::InvalidInput(format!(
                    "image occlusion mask {} has invalid normalized geometry",
                    mask.id
                )));
            }
            let coordinate_scale = 10_000.0;
            if [mask.x, mask.y, mask.width, mask.height]
                .iter()
                .any(|value| {
                    let scaled = value * coordinate_scale;
                    (scaled - scaled.round()).abs() > 1e-8
                })
            {
                return Err(DatabaseError::InvalidInput(format!(
                    "image occlusion mask {} coordinates require at most four decimal places",
                    mask.id
                )));
            }
        }
    }
    Ok(())
}

struct DraftFields<'a> {
    content_type: CardContentType,
    front_md: &'a str,
    back_md: &'a str,
    source: Option<&'a str>,
    search_md: &'a str,
    variant_keys: Vec<String>,
    occlusion: Option<&'a OcclusionDefinitionDraft>,
}

fn draft_fields(input: &CardContentDraft) -> DraftFields<'_> {
    match input {
        CardContentDraft::Basic {
            front_md,
            back_md,
            source,
        } => DraftFields {
            content_type: CardContentType::Basic,
            front_md,
            back_md,
            source: source.as_deref(),
            search_md: front_md,
            variant_keys: vec![BASIC_VARIANT_KEY.into()],
            occlusion: None,
        },
        CardContentDraft::Cloze {
            front_md,
            back_md,
            source,
            variant_keys,
            search_md,
        } => DraftFields {
            content_type: CardContentType::Cloze,
            front_md,
            back_md,
            source: source.as_deref(),
            search_md,
            variant_keys: variant_keys.clone(),
            occlusion: None,
        },
        CardContentDraft::Occlusion {
            front_md,
            back_md,
            source,
            occlusion,
        } => DraftFields {
            content_type: CardContentType::Occlusion,
            front_md,
            back_md,
            source: source.as_deref(),
            search_md: front_md,
            variant_keys: occlusion
                .layers
                .iter()
                .map(|layer| format!("{OCCLUSION_VARIANT_PREFIX}{}", layer.id))
                .collect(),
            occlusion: Some(occlusion),
        },
    }
}

fn validate_cloze_variant_keys(variant_keys: &[String]) -> Result<()> {
    if variant_keys.is_empty() {
        return Err(DatabaseError::InvalidInput(
            "a cloze card must have at least one variant key".into(),
        ));
    }
    let mut unique = HashSet::with_capacity(variant_keys.len());
    for variant_key in variant_keys {
        let Some(index) = variant_key.strip_prefix(CLOZE_VARIANT_PREFIX) else {
            return Err(DatabaseError::InvalidInput(format!(
                "invalid cloze variant key {variant_key}"
            )));
        };
        if index.is_empty()
            || index.starts_with('0')
            || !index.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(DatabaseError::InvalidInput(format!(
                "invalid cloze variant key {variant_key}"
            )));
        }
        if !unique.insert(variant_key) {
            return Err(DatabaseError::InvalidInput(format!(
                "duplicate cloze variant key {variant_key}"
            )));
        }
    }
    Ok(())
}

fn insert_occlusion_definition(
    transaction: &Transaction<'_>,
    card_content_id: &str,
    definition: &OcclusionDefinitionDraft,
    now: i64,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO card_occlusion_content (
            id, created_at, updated_at, deleted_at, card_content_id,
            source_image_id, mode
         ) VALUES (?1, ?2, ?2, NULL, ?3, ?4, ?5)",
        params![
            definition.id,
            now,
            card_content_id,
            definition.source_image_id,
            definition.mode.as_db_str(),
        ],
    )?;
    for (sort_order, layer) in definition.layers.iter().enumerate() {
        insert_occlusion_layer(transaction, &definition.id, layer, sort_order, now)?;
    }
    Ok(())
}

fn insert_occlusion_layer(
    transaction: &Transaction<'_>,
    definition_id: &str,
    layer: &OcclusionMaskLayerDraft,
    sort_order: usize,
    now: i64,
) -> Result<()> {
    let sort_order = i64::try_from(sort_order)
        .map_err(|_| DatabaseError::InvalidInput("too many image occlusion layers".into()))?;
    transaction.execute(
        "INSERT INTO card_occlusion_mask_layer (
            id, created_at, updated_at, deleted_at, card_occlusion_content_id,
            label, sort_order
         ) VALUES (?1, ?2, ?2, NULL, ?3, ?4, ?5)",
        params![
            layer.id,
            now,
            definition_id,
            normalized_label(&layer.label),
            sort_order
        ],
    )?;
    for mask in &layer.masks {
        insert_occlusion_mask(transaction, &layer.id, mask, now)?;
    }
    Ok(())
}

fn insert_occlusion_mask(
    transaction: &Transaction<'_>,
    layer_id: &str,
    mask: &OcclusionMaskDraft,
    now: i64,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO card_occlusion_mask (
            id, created_at, updated_at, deleted_at, card_occlusion_mask_layer_id,
            x, y, width, height, color
         ) VALUES (?1, ?2, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            mask.id,
            now,
            layer_id,
            mask.x,
            mask.y,
            mask.width,
            mask.height,
            mask.color.as_db_str(),
        ],
    )?;
    Ok(())
}

fn reconcile_occlusion_definition(
    transaction: &Transaction<'_>,
    card_content_id: &str,
    desired: &OcclusionDefinitionDraft,
    updated_at: i64,
) -> Result<()> {
    let active_definition_id = transaction
        .query_row(
            "SELECT id
             FROM card_occlusion_content
             WHERE card_content_id = ?1 AND deleted_at IS NULL",
            [card_content_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            DatabaseError::CorruptReviewData(format!(
                "active occlusion card content {card_content_id} has no definition"
            ))
        })?;
    if active_definition_id != desired.id {
        return Err(DatabaseError::InvalidInput(
            "an existing image occlusion definition cannot change identity".into(),
        ));
    }
    transaction.execute(
        "UPDATE card_occlusion_content
         SET updated_at = ?1, source_image_id = ?2, mode = ?3
         WHERE id = ?4 AND deleted_at IS NULL",
        params![
            updated_at,
            desired.source_image_id,
            desired.mode.as_db_str(),
            desired.id,
        ],
    )?;

    let active_layer_ids = active_occlusion_child_ids(
        transaction,
        "card_occlusion_mask_layer",
        "card_occlusion_content_id",
        &desired.id,
    )?;
    // Move current orders out of the active unique-index range before applying
    // the desired order, so swaps never fail halfway through the transaction.
    transaction.execute(
        "UPDATE card_occlusion_mask_layer
         SET sort_order = sort_order + 1000000
         WHERE card_occlusion_content_id = ?1 AND deleted_at IS NULL",
        [&desired.id],
    )?;

    let desired_layer_ids = desired
        .layers
        .iter()
        .map(|layer| layer.id.as_str())
        .collect::<HashSet<_>>();
    for (sort_order, layer) in desired.layers.iter().enumerate() {
        if active_layer_ids.contains(&layer.id) {
            let sort_order = i64::try_from(sort_order).map_err(|_| {
                DatabaseError::InvalidInput("too many image occlusion layers".into())
            })?;
            transaction.execute(
                "UPDATE card_occlusion_mask_layer
                 SET updated_at = ?1, label = ?2, sort_order = ?3
                 WHERE id = ?4 AND card_occlusion_content_id = ?5
                   AND deleted_at IS NULL",
                params![
                    updated_at,
                    normalized_label(&layer.label),
                    sort_order,
                    layer.id,
                    desired.id,
                ],
            )?;
            reconcile_occlusion_masks(transaction, layer, updated_at)?;
        } else {
            reject_reused_occlusion_id(transaction, "card_occlusion_mask_layer", &layer.id)?;
            insert_occlusion_layer(transaction, &desired.id, layer, sort_order, updated_at)?;
        }
    }
    for layer_id in active_layer_ids {
        if desired_layer_ids.contains(layer_id.as_str()) {
            continue;
        }
        transaction.execute(
            "UPDATE card_occlusion_mask
             SET updated_at = ?1, deleted_at = ?1
             WHERE card_occlusion_mask_layer_id = ?2 AND deleted_at IS NULL",
            params![updated_at, layer_id],
        )?;
        transaction.execute(
            "UPDATE card_occlusion_mask_layer
             SET updated_at = ?1, deleted_at = ?1
             WHERE id = ?2 AND deleted_at IS NULL",
            params![updated_at, layer_id],
        )?;
    }
    Ok(())
}

fn reconcile_occlusion_masks(
    transaction: &Transaction<'_>,
    desired_layer: &OcclusionMaskLayerDraft,
    updated_at: i64,
) -> Result<()> {
    let active_mask_ids = active_occlusion_child_ids(
        transaction,
        "card_occlusion_mask",
        "card_occlusion_mask_layer_id",
        &desired_layer.id,
    )?;
    let desired_mask_ids = desired_layer
        .masks
        .iter()
        .map(|mask| mask.id.as_str())
        .collect::<HashSet<_>>();
    for mask in &desired_layer.masks {
        if active_mask_ids.contains(&mask.id) {
            transaction.execute(
                "UPDATE card_occlusion_mask
                 SET updated_at = ?1, x = ?2, y = ?3, width = ?4, height = ?5,
                     color = ?6
                 WHERE id = ?7 AND card_occlusion_mask_layer_id = ?8
                   AND deleted_at IS NULL",
                params![
                    updated_at,
                    mask.x,
                    mask.y,
                    mask.width,
                    mask.height,
                    mask.color.as_db_str(),
                    mask.id,
                    desired_layer.id,
                ],
            )?;
        } else {
            reject_reused_occlusion_id(transaction, "card_occlusion_mask", &mask.id)?;
            insert_occlusion_mask(transaction, &desired_layer.id, mask, updated_at)?;
        }
    }
    for mask_id in active_mask_ids {
        if desired_mask_ids.contains(mask_id.as_str()) {
            continue;
        }
        transaction.execute(
            "UPDATE card_occlusion_mask
             SET updated_at = ?1, deleted_at = ?1
             WHERE id = ?2 AND deleted_at IS NULL",
            params![updated_at, mask_id],
        )?;
    }
    Ok(())
}

fn active_occlusion_child_ids(
    transaction: &Transaction<'_>,
    table: &str,
    parent_column: &str,
    parent_id: &str,
) -> Result<HashSet<String>> {
    let mut statement = transaction.prepare(&format!(
        "SELECT id FROM {table} WHERE {parent_column} = ?1 AND deleted_at IS NULL"
    ))?;
    let ids = statement
        .query_map([parent_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(ids)
}

fn reject_reused_occlusion_id(transaction: &Transaction<'_>, table: &str, id: &str) -> Result<()> {
    let exists = transaction
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE id = ?1"),
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        return Err(DatabaseError::InvalidInput(
            "deleted image occlusion IDs cannot be reused".into(),
        ));
    }
    Ok(())
}

fn normalized_label(label: &Option<String>) -> Option<&str> {
    label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn insert_new_review_card(
    transaction: &Transaction<'_>,
    card_content_id: &str,
    variant_key: &str,
    now: i64,
) -> Result<String> {
    let review_card_id = Uuid::now_v7().to_string();
    transaction.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status,
            suspended_at, variant_key, state, due_at, due_study_day,
            last_review_at, reps, lapses, scheduler_config_id,
            scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, ?2, ?2, NULL, ?3, ?4, NULL, ?5, ?6, NULL, NULL,
            NULL, 0, 0, NULL, NULL, NULL
         )",
        params![
            review_card_id,
            now,
            card_content_id,
            ReviewCardStatus::Active.as_db_str(),
            variant_key,
            ReviewCardState::New.as_db_str()
        ],
    )?;
    Ok(review_card_id)
}

fn reconcile_review_card_variants(
    transaction: &Transaction<'_>,
    card_content_id: &str,
    desired_variant_keys: Vec<String>,
    updated_at: i64,
) -> Result<()> {
    let mut statement = transaction.prepare(
        "SELECT variant_key
         FROM review_card
         WHERE card_content_id = ?1 AND deleted_at IS NULL",
    )?;
    let active_variant_keys = statement
        .query_map([card_content_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    drop(statement);

    let desired = desired_variant_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for variant_key in &desired_variant_keys {
        if !active_variant_keys.contains(variant_key) {
            insert_new_review_card(transaction, card_content_id, variant_key, updated_at)?;
        }
    }
    for variant_key in active_variant_keys {
        if !desired.contains(variant_key.as_str()) {
            let changed = transaction.execute(
                "UPDATE review_card
                 SET updated_at = ?1, deleted_at = ?1
                 WHERE card_content_id = ?2 AND variant_key = ?3 AND deleted_at IS NULL",
                params![updated_at, card_content_id, variant_key],
            )?;
            if changed != 1 {
                return Err(DatabaseError::StaleCardContent(
                    "review-card variants changed before the edit was saved".into(),
                ));
            }
        }
    }
    Ok(())
}

fn search_body(
    transaction: &Transaction<'_>,
    front_md: &str,
    back_md: &str,
    source: Option<&str>,
    image_references: &[media::ImageReference],
    occlusion_source_image_id: Option<&str>,
) -> Result<String> {
    let front = markdown_plain_text(front_md);
    let back = markdown_plain_text(back_md);
    let mut fields = vec![front, back];
    if let Some(source) = source.map(str::trim).filter(|source| !source.is_empty()) {
        fields.push(source.to_owned());
    }
    let authored_body = fields.join(SEARCH_FIELD_SEPARATOR);
    let mut ocr_texts = media::referenced_ocr_texts(transaction, image_references)?;
    if let Some(image_id) = occlusion_source_image_id {
        let ocr_text = media::active_image_ocr_text(transaction, image_id)?;
        if !ocr_text.trim().is_empty() {
            ocr_texts.push(ocr_text);
        }
    }
    Ok(media::search_body_with_ocr(&authored_body, &ocr_texts))
}

fn markdown_plain_text(markdown: &str) -> String {
    let options = MarkdownOptions::ENABLE_STRIKETHROUGH
        | MarkdownOptions::ENABLE_TABLES
        | MarkdownOptions::ENABLE_TASKLISTS
        | MarkdownOptions::ENABLE_FOOTNOTES
        | MarkdownOptions::ENABLE_MATH;
    let mut output = String::new();
    for event in MarkdownParser::new_ext(markdown, options) {
        match event {
            MarkdownEvent::Text(text) => {
                if media::parse_image_reference_token(&text)
                    .ok()
                    .flatten()
                    .is_some()
                {
                    continue;
                }
                push_plain_text(&mut output, &text);
            }
            MarkdownEvent::Code(text)
            | MarkdownEvent::Html(text)
            | MarkdownEvent::InlineHtml(text)
            | MarkdownEvent::InlineMath(text)
            | MarkdownEvent::DisplayMath(text) => {
                push_plain_text(&mut output, &text);
            }
            MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak | MarkdownEvent::Rule => {
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
            MarkdownEvent::TaskListMarker(checked) => {
                output.push_str(if checked { " checked " } else { " unchecked " });
            }
            MarkdownEvent::Start(_)
            | MarkdownEvent::End(_)
            | MarkdownEvent::FootnoteReference(_) => {}
        }
    }
    output.trim().to_owned()
}

fn push_plain_text(output: &mut String, text: &str) {
    if !output.is_empty()
        && !output.chars().last().is_some_and(char::is_whitespace)
        && !text.chars().next().is_some_and(char::is_whitespace)
    {
        output.push(' ');
    }
    output.push_str(text);
}

fn rebuild_search_document(
    transaction: &Transaction<'_>,
    card_content_id: &str,
    front_md: &str,
    back_md: &str,
    source: Option<&str>,
    image_references: &[media::ImageReference],
    occlusion_source_image_id: Option<&str>,
    updated_at: i64,
) -> Result<()> {
    let body = search_body(
        transaction,
        front_md,
        back_md,
        source,
        image_references,
        occlusion_source_image_id,
    )?;
    let content_hash = Sha256::digest(body.as_bytes());
    let changed = transaction.execute(
        "UPDATE search_document
         SET body = ?1, content_hash = ?2, updated_at = ?3
         WHERE card_content_id = ?4",
        params![body, content_hash.as_slice(), updated_at, card_content_id],
    )?;
    if changed != 1 {
        return Err(DatabaseError::CorruptReviewData(format!(
            "active card content {card_content_id} has no search document"
        )));
    }
    Ok(())
}

fn literal_search_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn literal_trigram_query(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn load_card_content(connection: &Connection, card_content_id: &str) -> Result<CardContent> {
    let fields = connection
        .query_row(
            "SELECT id, created_at, updated_at, type, front_md, back_md, source
             FROM card_content
             WHERE id = ?1 AND deleted_at IS NULL",
            [card_content_id],
            card_content_fields_row,
        )
        .optional()?
        .ok_or_else(|| DatabaseError::NotFound {
            entity: "card content",
            id: card_content_id.to_owned(),
        })?;
    hydrate_card_content(connection, fields)
}

fn load_card_content_list_item(
    connection: &Connection,
    card_content_id: &str,
) -> Result<CardContentListItem> {
    let stored = connection
        .query_row(
            &format!(
                "{CARD_CONTENT_LIST_SELECT}
                 WHERE content.id = ?1 AND content.deleted_at IS NULL"
            ),
            [card_content_id],
            card_content_list_row,
        )
        .optional()?
        .ok_or_else(|| DatabaseError::NotFound {
            entity: "card content",
            id: card_content_id.to_owned(),
        })?;
    hydrate_card_content_list_item(connection, stored)
}

struct StoredCardContentFields {
    id: String,
    created_at: i64,
    updated_at: i64,
    content_type: CardContentType,
    front_md: String,
    back_md: String,
    source: Option<String>,
}

struct StoredCardContentListItem {
    fields: StoredCardContentFields,
    review_status: CardContentReviewStatus,
    lifecycle_updated_at: i64,
}

fn card_content_fields_row(row: &Row<'_>) -> rusqlite::Result<StoredCardContentFields> {
    Ok(StoredCardContentFields {
        id: row.get(0)?,
        created_at: row.get(1)?,
        updated_at: row.get(2)?,
        content_type: enum_column(row, 3, CardContentType::from_db)?,
        front_md: row.get(4)?,
        back_md: row.get(5)?,
        source: row.get(6)?,
    })
}

fn card_content_list_row(row: &Row<'_>) -> rusqlite::Result<StoredCardContentListItem> {
    let fields = card_content_fields_row(row)?;
    let review_card_status = enum_column(row, 7, ReviewCardStatus::from_db)?;
    let review_status = match (review_card_status, row.get::<_, i64>(8)?) {
        (ReviewCardStatus::Active, 1) => CardContentReviewStatus::Active,
        (ReviewCardStatus::Suspended, 1) => CardContentReviewStatus::Suspended,
        (_, 2) => CardContentReviewStatus::Mixed,
        (_, count) => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::other(format!(
                    "card content has {count} distinct review statuses"
                ))),
            ));
        }
    };
    Ok(StoredCardContentListItem {
        fields,
        review_status,
        lifecycle_updated_at: row.get(9)?,
    })
}

fn hydrate_card_content_list_item(
    connection: &Connection,
    stored: StoredCardContentListItem,
) -> Result<CardContentListItem> {
    let review_cards = load_review_card_list_items(connection, &stored.fields.id)?;
    Ok(CardContentListItem {
        card_content: hydrate_card_content(connection, stored.fields)?,
        review_cards,
        review_status: stored.review_status,
        lifecycle_updated_at: stored.lifecycle_updated_at,
    })
}

fn load_review_card_list_items(
    connection: &Connection,
    card_content_id: &str,
) -> Result<Vec<ReviewCardListItem>> {
    let mut statement = connection.prepare(
        "SELECT id, status, variant_key, state, due_at, due_study_day,
                last_review_at
         FROM review_card
         WHERE card_content_id = ?1 AND deleted_at IS NULL
         ORDER BY created_at, id",
    )?;
    let review_cards = statement
        .query_map([card_content_id], |row| {
            Ok(ReviewCardListItem {
                id: row.get(0)?,
                status: enum_column(row, 1, ReviewCardStatus::from_db)?,
                variant_key: row.get(2)?,
                state: enum_column(row, 3, ReviewCardState::from_db)?,
                due_at: row.get(4)?,
                due_study_day: row.get(5)?,
                last_review_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(review_cards)
}

fn hydrate_card_content(
    connection: &Connection,
    fields: StoredCardContentFields,
) -> Result<CardContent> {
    card_content_from_fields(
        connection,
        fields.id,
        fields.created_at,
        fields.updated_at,
        fields.content_type,
        fields.front_md,
        fields.back_md,
        fields.source,
    )
}

fn card_content_from_fields(
    connection: &Connection,
    id: String,
    created_at: i64,
    updated_at: i64,
    content_type: CardContentType,
    front_md: String,
    back_md: String,
    source: Option<String>,
) -> Result<CardContent> {
    Ok(match content_type {
        CardContentType::Basic => CardContent::Basic {
            id,
            created_at,
            updated_at,
            front_md,
            back_md,
            source,
        },
        CardContentType::Cloze => CardContent::Cloze {
            id,
            created_at,
            updated_at,
            front_md,
            back_md,
            source,
        },
        CardContentType::Occlusion => CardContent::Occlusion {
            occlusion: load_occlusion_definition(connection, &id)?,
            id,
            created_at,
            updated_at,
            front_md,
            back_md,
            source,
        },
    })
}

fn load_occlusion_definition(
    connection: &Connection,
    card_content_id: &str,
) -> Result<OcclusionDefinition> {
    let (id, source_image_id, mode) = connection
        .query_row(
            "SELECT id, source_image_id, mode
             FROM card_occlusion_content
             WHERE card_content_id = ?1 AND deleted_at IS NULL",
            [card_content_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            DatabaseError::CorruptReviewData(format!(
                "active occlusion card content {card_content_id} has no definition"
            ))
        })?;
    let source_image = media::load_active_image_record(connection, &source_image_id)?;
    let mode = OcclusionMode::from_db(&mode)?;
    let mut layer_statement = connection.prepare(
        "SELECT id, label
         FROM card_occlusion_mask_layer
         WHERE card_occlusion_content_id = ?1 AND deleted_at IS NULL
         ORDER BY sort_order, id",
    )?;
    let stored_layers = layer_statement
        .query_map([&id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(layer_statement);
    if stored_layers.is_empty() {
        return Err(DatabaseError::CorruptReviewData(format!(
            "active image occlusion definition {id} has no layers"
        )));
    }
    let mut layers = Vec::with_capacity(stored_layers.len());
    for (layer_id, label) in stored_layers {
        let mut mask_statement = connection.prepare(
            "SELECT id, x, y, width, height, color
             FROM card_occlusion_mask
             WHERE card_occlusion_mask_layer_id = ?1 AND deleted_at IS NULL
             ORDER BY created_at, id",
        )?;
        let masks = mask_statement
            .query_map([&layer_id], |row| {
                let color = row.get::<_, String>(5)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    color,
                ))
            })?
            .map(|row| {
                let (id, x, y, width, height, color) = row?;
                Ok(OcclusionMask {
                    id,
                    x,
                    y,
                    width,
                    height,
                    color: OcclusionMaskColor::from_db(&color)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if masks.is_empty() {
            return Err(DatabaseError::CorruptReviewData(format!(
                "active image occlusion layer {layer_id} has no masks"
            )));
        }
        layers.push(OcclusionMaskLayer {
            id: layer_id,
            label,
            masks,
        });
    }
    Ok(OcclusionDefinition {
        id,
        source_image,
        mode,
        layers,
    })
}

fn load_stored_card(connection: &Connection, review_card_id: &str) -> Result<StoredCard> {
    connection
        .query_row(
            "SELECT
                content.id, content.created_at, content.updated_at, content.type,
                content.front_md, content.back_md, content.source,
                card.id, card.status, card.variant_key, card.updated_at,
                card.state, card.due_at, card.due_study_day, card.last_review_at,
                card.reps, card.lapses, card.scheduler_config_id,
                card.scheduler_state_schema_version, card.scheduler_state_json
             FROM review_card AS card
             JOIN card_content AS content ON content.id = card.card_content_id
             WHERE card.id = ?1
               AND card.deleted_at IS NULL
               AND content.deleted_at IS NULL",
            [review_card_id],
            |row| {
                Ok(StoredCard {
                    content_id: row.get(0)?,
                    content_created_at: row.get(1)?,
                    content_updated_at: row.get(2)?,
                    content_type: enum_column(row, 3, CardContentType::from_db)?,
                    front_md: row.get(4)?,
                    back_md: row.get(5)?,
                    source: row.get(6)?,
                    card_id: row.get(7)?,
                    status: enum_column(row, 8, ReviewCardStatus::from_db)?,
                    variant_key: row.get(9)?,
                    updated_at: row.get(10)?,
                    state: enum_column(row, 11, ReviewCardState::from_db)?,
                    due_at: row.get(12)?,
                    due_study_day: row.get(13)?,
                    last_review_at: row.get(14)?,
                    reps: row.get(15)?,
                    lapses: row.get(16)?,
                    scheduler_config_id: row.get(17)?,
                    scheduler_state_schema_version: row.get(18)?,
                    scheduler_state_json: row.get(19)?,
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
    let algorithm = SchedulerAlgorithm::from_db(&tuple.1);
    let scheduler_library = SchedulerLibrary::from_db(&tuple.3);
    if algorithm.is_none()
        || tuple.2 != SUPPORTED_SCHEDULER_ALGORITHM_VERSION
        || scheduler_library.is_none()
        || tuple.4 != SUPPORTED_SCHEDULER_LIBRARY_VERSION
        || tuple.5 != SUPPORTED_SCHEDULER_CONFIG_SCHEMA_VERSION
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
        algorithm: algorithm.expect("validated scheduler algorithm"),
        algorithm_version: tuple.2,
        scheduler_library: scheduler_library.expect("validated scheduler library"),
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
           AND event.event_type = ?2
           AND NOT EXISTS (
               SELECT 1
               FROM review_event AS revoke
               WHERE revoke.event_type = ?3
                 AND revoke.target_event_id = event.id
           )
         ORDER BY event.card_sequence",
    )?;
    let rows = statement.query_map(
        params![
            review_card_id,
            ReviewEventType::Review.as_db_str(),
            ReviewEventType::Revoke.as_db_str()
        ],
        |row| {
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
        },
    )?;

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
                    event_type: enum_column(row, 1, ReviewEventType::from_db)?,
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
        state: stored.state,
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
            cache.state.as_db_str(),
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
    validate_non_negative_safe(
        input.expected_card_content_updated_at,
        "expectedCardContentUpdatedAt",
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
    expected_content_updated_at: Option<i64>,
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
    if let Some(expected_content_updated_at) = expected_content_updated_at {
        if context.card_content.updated_at() != expected_content_updated_at {
            return Err(DatabaseError::StaleReviewContext(format!(
                "card content timestamp is {}, expected {}",
                context.card_content.updated_at(),
                expected_content_updated_at
            )));
        }
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
            cache.state.as_db_str()
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
        && event.event_type == ReviewEventType::Review
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
        && event.event_type == ReviewEventType::Revoke
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

fn enum_column<T>(
    row: &Row<'_>,
    index: usize,
    parse: fn(&str) -> Result<T>,
) -> rusqlite::Result<T> {
    let value = row.get::<_, String>(index)?;
    parse(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(error.to_string())),
        )
    })
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
