use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use pulldown_cmark::{CodeBlockKind, Event as MarkdownEvent, Options as MarkdownOptions, Parser};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{DatabaseError, Result};

pub const IMAGE_MIME_TYPE: &str = "image/webp";
pub const SEARCH_OCR_SEPARATOR: &str = "\n\u{1f}\n";
const IMAGE_TOKEN_PREFIX: &str = "{{image:";
const IMAGE_TOKEN_WIDTH_SEPARATOR: &str = ";width=";
const IMAGE_TOKEN_SUFFIX: &str = "}}";
const MIN_DISPLAY_WIDTH_PERCENT: u8 = 10;
const MAX_DISPLAY_WIDTH_PERCENT: u8 = 100;
pub(crate) const MAX_OCR_ATTEMPTS: u32 = 4;
pub(crate) const OCR_RETRY_DELAYS_MILLIS: [i64; 3] = [5 * 60_000, 30 * 60_000, 2 * 60 * 60_000];
const INTERRUPTED_OCR_ERROR: &str = "the previous OCR attempt was interrupted";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImageOcrStatus {
    Pending,
    Ready,
    Failed,
}

impl ImageOcrStatus {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Ready => "READY",
            Self::Failed => "FAILED",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::Pending.as_db_str() {
            Ok(Self::Pending)
        } else if value == Self::Ready.as_db_str() {
            Ok(Self::Ready)
        } else if value == Self::Failed.as_db_str() {
            Ok(Self::Failed)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown image OCR status {value}"
            )))
        }
    }

    pub(crate) const fn needs_worker_wake(self) -> bool {
        matches!(self, Self::Pending)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OcrQueueState {
    Pending,
    Running,
    RetryWait,
    Ready,
    Failed,
}

impl OcrQueueState {
    pub(super) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Running => "RUNNING",
            Self::RetryWait => "RETRY_WAIT",
            Self::Ready => "READY",
            Self::Failed => "FAILED",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        if value == Self::Pending.as_db_str() {
            Ok(Self::Pending)
        } else if value == Self::Running.as_db_str() {
            Ok(Self::Running)
        } else if value == Self::RetryWait.as_db_str() {
            Ok(Self::RetryWait)
        } else if value == Self::Ready.as_db_str() {
            Ok(Self::Ready)
        } else if value == Self::Failed.as_db_str() {
            Ok(Self::Failed)
        } else {
            Err(DatabaseError::CorruptReviewData(format!(
                "unknown OCR queue state {value}"
            )))
        }
    }
}

#[derive(Clone, Debug)]
pub struct CanonicalImage {
    pub bytes: Vec<u8>,
    pub natural_width: u32,
    pub natural_height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRecord {
    pub id: String,
    pub mime_type: String,
    pub natural_width: u32,
    pub natural_height: u32,
    pub ocr_status: ImageOcrStatus,
}

#[derive(Clone, Debug)]
pub struct MediaPayload {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub sha256: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct OcrJob {
    pub image_id: String,
    pub bytes: Vec<u8>,
    pub attempt_count: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OcrQueueRecovery {
    pub requeued: u64,
    pub terminally_failed: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageReference {
    pub image_id: String,
    pub display_width_percent: u8,
}

pub(super) fn ingest_image(
    main: &mut Connection,
    media: &mut Connection,
    image: CanonicalImage,
) -> Result<ImageRecord> {
    if image.bytes.is_empty() {
        return Err(DatabaseError::InvalidInput(
            "the canonical image is empty".into(),
        ));
    }
    if image.natural_width == 0 || image.natural_height == 0 {
        return Err(DatabaseError::InvalidInput(
            "the canonical image has invalid dimensions".into(),
        ));
    }

    let sha256 = Sha256::digest(&image.bytes).to_vec();
    let media_transaction = media.transaction_with_behavior(TransactionBehavior::Immediate)?;
    media_transaction.execute(
        "INSERT OR IGNORE INTO media_blob(sha256, bytes) VALUES (?1, ?2)",
        params![&sha256, &image.bytes],
    )?;
    let stored: Vec<u8> = media_transaction.query_row(
        "SELECT bytes FROM media_blob WHERE sha256 = ?1",
        [&sha256],
        |row| row.get(0),
    )?;
    if Sha256::digest(&stored).as_slice() != sha256.as_slice() {
        return Err(DatabaseError::Validation {
            kind: "media",
            reason: "stored image bytes do not match their digest".into(),
        });
    }
    media_transaction.commit()?;

    let transaction = main.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing = transaction
        .query_row(
            "SELECT id, mime_type, natural_width, natural_height, ocr_status, deleted_at
             FROM image
             WHERE sha256 = ?1",
            [&sha256],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .optional()?;

    let record = if let Some((id, mime_type, width, height, status, deleted_at)) = existing {
        if deleted_at.is_some() {
            return Err(DatabaseError::InvalidInput(
                "the pasted image was previously deleted".into(),
            ));
        }
        if mime_type != IMAGE_MIME_TYPE
            || width != image.natural_width
            || height != image.natural_height
        {
            return Err(DatabaseError::Validation {
                kind: "image",
                reason: "deduplicated image metadata does not match its canonical bytes".into(),
            });
        }
        ImageRecord {
            id,
            mime_type,
            natural_width: width,
            natural_height: height,
            ocr_status: ImageOcrStatus::from_db(&status)?,
        }
    } else {
        let id = Uuid::now_v7().to_string();
        let now = now_millis()?;
        transaction.execute(
            "INSERT INTO image (
                id, created_at, updated_at, deleted_at, sha256, mime_type,
                natural_width, natural_height, ocr_text, ocr_status, ocr_error,
                ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at, ocr_started_at
             ) VALUES (
                ?1, ?2, ?2, NULL, ?3, ?4, ?5, ?6, '', ?7, NULL, ?8, 0, ?2, NULL
             )",
            params![
                id,
                now,
                &sha256,
                IMAGE_MIME_TYPE,
                image.natural_width,
                image.natural_height,
                ImageOcrStatus::Pending.as_db_str(),
                OcrQueueState::Pending.as_db_str(),
            ],
        )?;
        ImageRecord {
            id,
            mime_type: IMAGE_MIME_TYPE.into(),
            natural_width: image.natural_width,
            natural_height: image.natural_height,
            ocr_status: ImageOcrStatus::Pending,
        }
    };
    transaction.commit()?;
    Ok(record)
}

pub(super) fn load_media_payload(
    main: &Connection,
    media: &Connection,
    image_id: &str,
) -> Result<MediaPayload> {
    validate_uuid_v7(image_id, "imageId")?;
    let (sha256, mime_type) = main
        .query_row(
            "SELECT sha256, mime_type
             FROM image
             WHERE id = ?1 AND deleted_at IS NULL",
            [image_id],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| DatabaseError::NotFound {
            entity: "image",
            id: image_id.into(),
        })?;
    let bytes = media
        .query_row(
            "SELECT bytes FROM media_blob WHERE sha256 = ?1",
            [&sha256],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or_else(|| DatabaseError::Validation {
            kind: "database pair",
            reason: format!("image {image_id} has no media blob"),
        })?;
    if Sha256::digest(&bytes).as_slice() != sha256.as_slice() {
        return Err(DatabaseError::Validation {
            kind: "database pair",
            reason: format!("image {image_id} has corrupt media bytes"),
        });
    }
    Ok(MediaPayload {
        bytes,
        mime_type,
        sha256,
    })
}

pub(super) fn claim_next_ocr_job(
    main: &mut Connection,
    media: &Connection,
    now: i64,
) -> Result<Option<OcrJob>> {
    validate_non_negative_timestamp(now, "OCR claim time")?;
    let transaction = main.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let candidate = transaction
        .query_row(
            "SELECT id, ocr_attempt_count
             FROM image
             WHERE deleted_at IS NULL
               AND ocr_queue_state IN (?1, ?2)
               AND ocr_next_attempt_at <= ?3
             ORDER BY ocr_next_attempt_at, created_at, id
             LIMIT 1",
            params![
                OcrQueueState::Pending.as_db_str(),
                OcrQueueState::RetryWait.as_db_str(),
                now,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        )
        .optional()?;
    let Some((image_id, previous_attempt_count)) = candidate else {
        transaction.commit()?;
        return Ok(None);
    };
    let attempt_count = previous_attempt_count
        .checked_add(1)
        .ok_or_else(|| DatabaseError::CorruptReviewData("OCR attempt count overflowed".into()))?;
    if attempt_count > MAX_OCR_ATTEMPTS {
        return Err(DatabaseError::CorruptReviewData(format!(
            "eligible image {image_id} exceeded the OCR attempt limit"
        )));
    }
    let changed = transaction.execute(
        "UPDATE image
         SET updated_at = max(updated_at + 1, ?1), ocr_queue_state = ?2,
             ocr_attempt_count = ?3, ocr_next_attempt_at = NULL,
             ocr_started_at = ?1
         WHERE id = ?4 AND deleted_at IS NULL
           AND ocr_queue_state IN (?5, ?6) AND ocr_next_attempt_at <= ?1",
        params![
            now,
            OcrQueueState::Running.as_db_str(),
            attempt_count,
            image_id,
            OcrQueueState::Pending.as_db_str(),
            OcrQueueState::RetryWait.as_db_str(),
        ],
    )?;
    if changed != 1 {
        return Err(DatabaseError::CorruptReviewData(format!(
            "image {image_id} could not be claimed for OCR"
        )));
    }
    let payload = load_media_payload(&transaction, media, &image_id)?;
    transaction.commit()?;
    Ok(Some(OcrJob {
        image_id,
        bytes: payload.bytes,
        attempt_count,
    }))
}

pub(super) fn complete_image_ocr(
    main: &mut Connection,
    image_id: &str,
    expected_attempt_count: u32,
    result: std::result::Result<String, String>,
    now: i64,
) -> Result<()> {
    validate_uuid_v7(image_id, "imageId")?;
    validate_non_negative_timestamp(now, "OCR completion time")?;
    let transaction = main.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = transaction
        .query_row(
            "SELECT ocr_queue_state, ocr_attempt_count
             FROM image WHERE id = ?1 AND deleted_at IS NULL",
            [image_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        )
        .optional()?
        .ok_or_else(|| DatabaseError::NotFound {
            entity: "image",
            id: image_id.into(),
        })?;
    if OcrQueueState::from_db(&current.0)? != OcrQueueState::Running
        || current.1 != expected_attempt_count
    {
        transaction.commit()?;
        return Ok(());
    }

    match result {
        Ok(text) => {
            transaction.execute(
                "UPDATE image
                 SET updated_at = max(updated_at + 1, ?1), ocr_text = ?2,
                     ocr_status = ?3, ocr_queue_state = ?4, ocr_error = NULL,
                     ocr_next_attempt_at = NULL, ocr_started_at = NULL
                 WHERE id = ?5 AND deleted_at IS NULL AND ocr_queue_state = ?6
                   AND ocr_attempt_count = ?7",
                params![
                    now,
                    text.trim(),
                    ImageOcrStatus::Ready.as_db_str(),
                    OcrQueueState::Ready.as_db_str(),
                    image_id,
                    OcrQueueState::Running.as_db_str(),
                    expected_attempt_count,
                ],
            )?;
            rebuild_search_documents_for_image(&transaction, image_id, now)?;
        }
        Err(error) => {
            let error = truncate_error(&error);
            if expected_attempt_count >= MAX_OCR_ATTEMPTS {
                transaction.execute(
                    "UPDATE image
                     SET updated_at = max(updated_at + 1, ?1), ocr_text = '',
                         ocr_status = ?2, ocr_queue_state = ?3, ocr_error = ?4,
                         ocr_next_attempt_at = NULL, ocr_started_at = NULL
                     WHERE id = ?5 AND deleted_at IS NULL AND ocr_queue_state = ?6
                       AND ocr_attempt_count = ?7",
                    params![
                        now,
                        ImageOcrStatus::Failed.as_db_str(),
                        OcrQueueState::Failed.as_db_str(),
                        error,
                        image_id,
                        OcrQueueState::Running.as_db_str(),
                        expected_attempt_count,
                    ],
                )?;
            } else {
                let next_attempt_at = now
                    .checked_add(retry_delay_millis(expected_attempt_count)?)
                    .ok_or(DatabaseError::InvalidSystemTime)?;
                transaction.execute(
                    "UPDATE image
                     SET updated_at = max(updated_at + 1, ?1), ocr_text = '',
                         ocr_queue_state = ?2, ocr_error = ?3,
                         ocr_next_attempt_at = ?4, ocr_started_at = NULL
                     WHERE id = ?5 AND deleted_at IS NULL AND ocr_queue_state = ?6
                       AND ocr_attempt_count = ?7",
                    params![
                        now,
                        OcrQueueState::RetryWait.as_db_str(),
                        error,
                        next_attempt_at,
                        image_id,
                        OcrQueueState::Running.as_db_str(),
                        expected_attempt_count,
                    ],
                )?;
            }
        }
    }
    transaction.commit()?;
    Ok(())
}

pub(super) fn recover_interrupted_ocr_jobs(
    main: &mut Connection,
    stale_started_at_or_before: i64,
    now: i64,
) -> Result<OcrQueueRecovery> {
    validate_non_negative_timestamp(stale_started_at_or_before, "OCR stale cutoff")?;
    validate_non_negative_timestamp(now, "OCR recovery time")?;
    let transaction = main.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let terminally_failed = transaction.execute(
        "UPDATE image
         SET updated_at = max(updated_at + 1, ?1), ocr_status = ?2,
             ocr_queue_state = ?3, ocr_error = ?4,
             ocr_started_at = NULL, ocr_next_attempt_at = NULL
         WHERE deleted_at IS NULL AND ocr_queue_state = ?5
           AND ocr_started_at <= ?6 AND ocr_attempt_count >= ?7",
        params![
            now,
            ImageOcrStatus::Failed.as_db_str(),
            OcrQueueState::Failed.as_db_str(),
            INTERRUPTED_OCR_ERROR,
            OcrQueueState::Running.as_db_str(),
            stale_started_at_or_before,
            MAX_OCR_ATTEMPTS,
        ],
    )?;
    let requeued = transaction.execute(
        "UPDATE image
         SET updated_at = max(updated_at + 1, ?1), ocr_queue_state = ?2,
             ocr_error = ?3, ocr_started_at = NULL, ocr_next_attempt_at = ?1
         WHERE deleted_at IS NULL AND ocr_queue_state = ?4
           AND ocr_started_at <= ?5 AND ocr_attempt_count < ?6",
        params![
            now,
            OcrQueueState::RetryWait.as_db_str(),
            INTERRUPTED_OCR_ERROR,
            OcrQueueState::Running.as_db_str(),
            stale_started_at_or_before,
            MAX_OCR_ATTEMPTS,
        ],
    )?;
    transaction.commit()?;
    Ok(OcrQueueRecovery {
        requeued: requeued as u64,
        terminally_failed: terminally_failed as u64,
    })
}

pub(super) fn parse_card_image_references(
    front_md: &str,
    back_md: &str,
) -> Result<Vec<ImageReference>> {
    let mut unique = HashSet::new();
    let mut references = Vec::new();
    for source in [front_md, back_md] {
        for reference in parse_markdown_image_references(source)? {
            if unique.insert(reference.image_id.clone()) {
                references.push(reference);
            }
        }
    }
    Ok(references)
}

pub(super) fn contains_image_reference(markdown: &str) -> Result<bool> {
    Ok(!parse_markdown_image_references(markdown)?.is_empty())
}

pub(super) fn validate_active_image_references(
    transaction: &Transaction<'_>,
    references: &[ImageReference],
) -> Result<()> {
    let mut statement =
        transaction.prepare("SELECT 1 FROM image WHERE id = ?1 AND deleted_at IS NULL")?;
    for reference in references {
        if statement
            .query_row([&reference.image_id], |_| Ok(()))
            .optional()?
            .is_none()
        {
            return Err(DatabaseError::InvalidInput(format!(
                "image {} is unavailable",
                reference.image_id
            )));
        }
    }
    Ok(())
}

pub(super) fn sync_card_content_images(
    transaction: &Transaction<'_>,
    card_content_id: &str,
    references: &[ImageReference],
) -> Result<()> {
    transaction.execute(
        "DELETE FROM card_content_image WHERE card_content_id = ?1",
        [card_content_id],
    )?;
    let mut statement = transaction
        .prepare("INSERT INTO card_content_image(card_content_id, image_id) VALUES (?1, ?2)")?;
    for reference in references {
        statement.execute(params![card_content_id, reference.image_id])?;
    }
    Ok(())
}

pub(super) fn referenced_ocr_texts(
    transaction: &Transaction<'_>,
    references: &[ImageReference],
) -> Result<Vec<String>> {
    let mut statement = transaction.prepare(
        "SELECT ocr_text FROM image
         WHERE id = ?1 AND deleted_at IS NULL AND ocr_status = ?2",
    )?;
    let mut texts = Vec::new();
    for reference in references {
        if let Some(text) = statement
            .query_row(
                params![reference.image_id, ImageOcrStatus::Ready.as_db_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
        {
            texts.push(text);
        }
    }
    Ok(texts)
}

pub(super) fn search_body_with_ocr(authored_body: &str, ocr_texts: &[String]) -> String {
    if ocr_texts.is_empty() {
        return authored_body.to_owned();
    }
    format!(
        "{authored_body}{SEARCH_OCR_SEPARATOR}{}",
        ocr_texts.join(super::domain::SEARCH_FIELD_SEPARATOR)
    )
}

pub(super) fn parse_image_reference_token(value: &str) -> Result<Option<ImageReference>> {
    let value = value.trim();
    if !value.starts_with(IMAGE_TOKEN_PREFIX) {
        return Ok(None);
    }
    if !value.ends_with(IMAGE_TOKEN_SUFFIX) {
        return Err(invalid_image_token());
    }
    let body = &value[IMAGE_TOKEN_PREFIX.len()..value.len() - IMAGE_TOKEN_SUFFIX.len()];
    let Some((image_id, width)) = body.split_once(IMAGE_TOKEN_WIDTH_SEPARATOR) else {
        return Err(invalid_image_token());
    };
    if image_id.contains(char::is_whitespace) || width.contains(char::is_whitespace) {
        return Err(invalid_image_token());
    }
    validate_uuid_v7(image_id, "image token ID")?;
    let display_width_percent = width
        .strip_suffix('%')
        .ok_or_else(invalid_image_token)?
        .parse::<u8>()
        .map_err(|_| invalid_image_token())?;
    if !(MIN_DISPLAY_WIDTH_PERCENT..=MAX_DISPLAY_WIDTH_PERCENT).contains(&display_width_percent) {
        return Err(invalid_image_token());
    }
    Ok(Some(ImageReference {
        image_id: image_id.into(),
        display_width_percent,
    }))
}

fn parse_markdown_image_references(markdown: &str) -> Result<Vec<ImageReference>> {
    let options = MarkdownOptions::ENABLE_STRIKETHROUGH
        | MarkdownOptions::ENABLE_TABLES
        | MarkdownOptions::ENABLE_TASKLISTS
        | MarkdownOptions::ENABLE_FOOTNOTES
        | MarkdownOptions::ENABLE_MATH;
    let mut references = Vec::new();
    let mut code_block_depth = 0_u32;
    let mut in_paragraph = false;
    let mut paragraph_reference = None;
    let mut paragraph_has_other_content = false;
    for event in Parser::new_ext(markdown, options) {
        match event {
            MarkdownEvent::Start(pulldown_cmark::Tag::CodeBlock(
                CodeBlockKind::Fenced(_) | CodeBlockKind::Indented,
            )) => code_block_depth += 1,
            MarkdownEvent::End(pulldown_cmark::TagEnd::CodeBlock) => {
                code_block_depth = code_block_depth.saturating_sub(1)
            }
            MarkdownEvent::Start(pulldown_cmark::Tag::Paragraph) if code_block_depth == 0 => {
                in_paragraph = true;
                paragraph_reference = None;
                paragraph_has_other_content = false;
            }
            MarkdownEvent::End(pulldown_cmark::TagEnd::Paragraph) if code_block_depth == 0 => {
                if let Some(reference) = paragraph_reference.take() {
                    if paragraph_has_other_content {
                        return Err(invalid_image_token());
                    }
                    references.push(reference);
                }
                in_paragraph = false;
            }
            MarkdownEvent::Text(text) if code_block_depth == 0 => {
                if let Some(reference) = parse_image_reference_token(&text)? {
                    if !in_paragraph || paragraph_reference.is_some() {
                        return Err(invalid_image_token());
                    }
                    paragraph_reference = Some(reference);
                } else if text.contains(IMAGE_TOKEN_PREFIX) {
                    return Err(invalid_image_token());
                } else if in_paragraph && !text.trim().is_empty() {
                    paragraph_has_other_content = true;
                }
            }
            MarkdownEvent::Code(_) if code_block_depth == 0 => {
                if in_paragraph {
                    paragraph_has_other_content = true;
                }
            }
            MarkdownEvent::Start(_) if code_block_depth == 0 && in_paragraph => {
                paragraph_has_other_content = true;
            }
            MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak if in_paragraph => {
                paragraph_has_other_content = true;
            }
            _ => {}
        }
    }
    Ok(references)
}

fn rebuild_search_documents_for_image(
    transaction: &Transaction<'_>,
    image_id: &str,
    now: i64,
) -> Result<()> {
    let mut affected = transaction.prepare(
        "SELECT link.card_content_id
         FROM card_content_image AS link
         JOIN card_content AS content ON content.id = link.card_content_id
         WHERE link.image_id = ?1 AND content.deleted_at IS NULL
         UNION
         SELECT occlusion.card_content_id
         FROM card_occlusion_content AS occlusion
         JOIN card_content AS content ON content.id = occlusion.card_content_id
         WHERE occlusion.source_image_id = ?1
           AND occlusion.deleted_at IS NULL
           AND content.deleted_at IS NULL",
    )?;
    let card_content_ids = affected
        .query_map([image_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(affected);

    for card_content_id in card_content_ids {
        let (front_md, back_md) = transaction.query_row(
            "SELECT front_md, back_md FROM card_content WHERE id = ?1",
            [&card_content_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        let references = parse_card_image_references(&front_md, &back_md)?;
        let ocr_texts = referenced_ocr_texts(transaction, &references)?;
        let (current_body, current_updated_at) = transaction.query_row(
            "SELECT body, updated_at FROM search_document WHERE card_content_id = ?1",
            [&card_content_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let authored_body = current_body
            .split_once(SEARCH_OCR_SEPARATOR)
            .map(|(authored, _)| authored)
            .unwrap_or(current_body.as_str());
        let body = search_body_with_ocr(authored_body, &ocr_texts);
        let content_hash = Sha256::digest(body.as_bytes());
        transaction.execute(
            "UPDATE search_document
             SET body = ?1, content_hash = ?2, updated_at = max(updated_at + 1, ?3)
             WHERE card_content_id = ?4",
            params![
                body,
                content_hash.as_slice(),
                now.max(current_updated_at),
                card_content_id,
            ],
        )?;
    }
    Ok(())
}

fn validate_uuid_v7(value: &str, field: &str) -> Result<()> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| DatabaseError::InvalidInput(format!("{field} must be a UUIDv7")))?;
    if parsed.get_version_num() != 7 || parsed.to_string() != value {
        return Err(DatabaseError::InvalidInput(format!(
            "{field} must be a lowercase canonical UUIDv7"
        )));
    }
    Ok(())
}

fn invalid_image_token() -> DatabaseError {
    DatabaseError::InvalidInput(
        "image references must use {{image:<uuid-v7>;width=<10-100>%}} on their own block".into(),
    )
}

pub(crate) fn now_millis() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DatabaseError::InvalidSystemTime)?;
    i64::try_from(duration.as_millis()).map_err(|_| DatabaseError::InvalidSystemTime)
}

fn validate_non_negative_timestamp(value: i64, field: &str) -> Result<()> {
    if value < 0 {
        return Err(DatabaseError::InvalidInput(format!(
            "{field} must be non-negative"
        )));
    }
    Ok(())
}

fn retry_delay_millis(attempt_count: u32) -> Result<i64> {
    let index = attempt_count
        .checked_sub(1)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| DatabaseError::CorruptReviewData("invalid OCR attempt count".into()))?;
    OCR_RETRY_DELAYS_MILLIS.get(index).copied().ok_or_else(|| {
        DatabaseError::CorruptReviewData(format!("OCR attempt {attempt_count} has no retry delay"))
    })
}

fn truncate_error(error: &str) -> String {
    error.chars().take(1_000).collect()
}
