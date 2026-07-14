use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{domain, DatabaseError, Result, ReviewContext};

const MAX_JSON_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const NORMAL_LANE_LENGTH: i64 = 4;
const NEW_LANE_SLOT: i64 = 3;
const REVIEW_TIE_SEED: &str = "dara-review-order-v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectNextReviewCardInput {
    pub now: i64,
    pub study_day: i64,
    /// Slots 0–2 prefer a due review; slot 3 prefers a new card.
    pub normal_lane_cursor: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewQueueLane {
    Intraday,
    Review,
    New,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewQueueSelection {
    Card {
        lane: ReviewQueueLane,
        #[serde(rename = "nextNormalLaneCursor")]
        next_normal_lane_cursor: i64,
        context: Box<ReviewContext>,
    },
    CaughtUp {
        #[serde(rename = "nextDueAt")]
        next_due_at: Option<i64>,
        #[serde(rename = "nextNormalLaneCursor")]
        next_normal_lane_cursor: i64,
    },
}

pub(super) fn select_next_review_card(
    connection: &mut Connection,
    input: SelectNextReviewCardInput,
) -> Result<ReviewQueueSelection> {
    validate_input(&input)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let selection = select_from_snapshot(&transaction, &input)?;
    transaction.commit()?;
    Ok(selection)
}

fn select_from_snapshot(
    connection: &Connection,
    input: &SelectNextReviewCardInput,
) -> Result<ReviewQueueSelection> {
    if let Some(review_card_id) = select_intraday(connection, input.now)? {
        return selected_card(
            connection,
            review_card_id,
            ReviewQueueLane::Intraday,
            input.normal_lane_cursor,
        );
    }

    let preferred_lane = if input.normal_lane_cursor == NEW_LANE_SLOT {
        ReviewQueueLane::New
    } else {
        ReviewQueueLane::Review
    };
    let fallback_lane = match preferred_lane {
        ReviewQueueLane::Review => ReviewQueueLane::New,
        ReviewQueueLane::New => ReviewQueueLane::Review,
        ReviewQueueLane::Intraday => unreachable!("intraday is not a normal lane"),
    };

    for lane in [preferred_lane, fallback_lane] {
        let review_card_id = match lane {
            ReviewQueueLane::Review => select_review(connection, input.study_day)?,
            ReviewQueueLane::New => select_new(connection)?,
            ReviewQueueLane::Intraday => unreachable!("intraday is selected separately"),
        };
        if let Some(review_card_id) = review_card_id {
            return selected_card(
                connection,
                review_card_id,
                lane,
                (input.normal_lane_cursor + 1) % NORMAL_LANE_LENGTH,
            );
        }
    }

    Ok(ReviewQueueSelection::CaughtUp {
        next_due_at: select_next_intraday_due(connection, input.now)?,
        next_normal_lane_cursor: input.normal_lane_cursor,
    })
}

fn selected_card(
    connection: &Connection,
    review_card_id: String,
    lane: ReviewQueueLane,
    next_normal_lane_cursor: i64,
) -> Result<ReviewQueueSelection> {
    Ok(ReviewQueueSelection::Card {
        lane,
        next_normal_lane_cursor,
        context: Box::new(domain::load_review_context(connection, &review_card_id)?),
    })
}

fn select_intraday(connection: &Connection, now: i64) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT id
             FROM review_card
             WHERE deleted_at IS NULL
               AND status = 'ACTIVE'
               AND state IN ('LEARNING', 'RELEARNING')
               AND due_at <= ?1
             ORDER BY due_at, id
             LIMIT 1",
            [now],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn select_review(connection: &Connection, study_day: i64) -> Result<Option<String>> {
    let oldest_due_day: Option<i64> = connection.query_row(
        "SELECT min(due_study_day)
         FROM review_card
         WHERE deleted_at IS NULL
           AND status = 'ACTIVE'
           AND state = 'REVIEW'
           AND due_study_day <= ?1",
        [study_day],
        |row| row.get(0),
    )?;
    let Some(oldest_due_day) = oldest_due_day else {
        return Ok(None);
    };

    let mut statement = connection.prepare(
        "SELECT id
         FROM review_card
         WHERE deleted_at IS NULL
           AND status = 'ACTIVE'
           AND state = 'REVIEW'
           AND due_study_day = ?1",
    )?;
    let candidates = statement.query_map([oldest_due_day], |row| row.get::<_, String>(0))?;
    let mut selected: Option<([u8; 32], String)> = None;
    for review_card_id in candidates {
        let review_card_id = review_card_id?;
        let candidate = (review_tie_key(study_day, &review_card_id), review_card_id);
        if selected.as_ref().is_none_or(|current| &candidate < current) {
            selected = Some(candidate);
        }
    }
    Ok(selected.map(|(_, review_card_id)| review_card_id))
}

fn select_new(connection: &Connection) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT id
             FROM review_card
             WHERE deleted_at IS NULL
               AND status = 'ACTIVE'
               AND state = 'NEW'
             ORDER BY created_at, id
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn select_next_intraday_due(connection: &Connection, now: i64) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT min(due_at)
             FROM review_card
             WHERE deleted_at IS NULL
               AND status = 'ACTIVE'
               AND state IN ('LEARNING', 'RELEARNING')
               AND due_at > ?1",
            [now],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn review_tie_key(study_day: i64, review_card_id: &str) -> [u8; 32] {
    Sha256::digest(format!("{REVIEW_TIE_SEED}:{study_day}:{review_card_id}")).into()
}

fn validate_input(input: &SelectNextReviewCardInput) -> Result<()> {
    if !(0..=MAX_JSON_SAFE_INTEGER).contains(&input.now) {
        return Err(DatabaseError::InvalidInput(
            "now must be a non-negative JSON-safe integer".into(),
        ));
    }
    if !(-MAX_JSON_SAFE_INTEGER..=MAX_JSON_SAFE_INTEGER).contains(&input.study_day) {
        return Err(DatabaseError::InvalidInput(
            "studyDay must be a JSON-safe integer".into(),
        ));
    }
    if !(0..NORMAL_LANE_LENGTH).contains(&input.normal_lane_cursor) {
        return Err(DatabaseError::InvalidInput(
            "normalLaneCursor must be between 0 and 3".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::review_tie_key;

    #[test]
    fn review_tie_key_changes_with_the_study_day() {
        let card_id = "01980c8e-6c00-7000-8000-000000000101";
        assert_ne!(
            review_tie_key(20_000, card_id),
            review_tie_key(20_001, card_id)
        );
        assert_eq!(
            review_tie_key(20_000, card_id),
            review_tie_key(20_000, card_id)
        );
    }
}
