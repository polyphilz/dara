use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::{domain, DatabaseError, Result};

const MAX_JSON_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_ACTIVITY_DAYS: i64 = 732;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadHomeStatsInput {
    pub now: i64,
    pub study_day: i64,
    pub activity_start_study_day: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReviewActivity {
    pub study_day: i64,
    pub count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeQueueCounts {
    pub new: i64,
    pub learning: i64,
    pub review: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeStats {
    pub activity: Vec<DailyReviewActivity>,
    pub reviewed_today: i64,
    pub queue: HomeQueueCounts,
    pub next_learning_due_at: Option<i64>,
}

pub(super) fn load_home_stats(
    connection: &Connection,
    input: LoadHomeStatsInput,
) -> Result<HomeStats> {
    validate_input(&input)?;

    let (queue, next_learning_due_at) = connection.query_row(
        "SELECT
            count(*) FILTER (WHERE state = ?3),
            count(*) FILTER (WHERE state IN (?4, ?5) AND due_at <= ?1),
            count(*) FILTER (WHERE state = ?6 AND due_study_day <= ?2),
            min(due_at) FILTER (WHERE state IN (?4, ?5) AND due_at > ?1)
         FROM review_card
         WHERE deleted_at IS NULL AND status = ?7",
        params![
            input.now,
            input.study_day,
            domain::ReviewCardState::New.as_db_str(),
            domain::ReviewCardState::Learning.as_db_str(),
            domain::ReviewCardState::Relearning.as_db_str(),
            domain::ReviewCardState::Review.as_db_str(),
            domain::ReviewCardStatus::Active.as_db_str(),
        ],
        |row| {
            Ok((
                HomeQueueCounts {
                    new: row.get(0)?,
                    learning: row.get(1)?,
                    review: row.get(2)?,
                },
                row.get(3)?,
            ))
        },
    )?;

    let mut statement = connection.prepare(
        "SELECT event.study_day, count(*)
         FROM review_event AS event
         WHERE event.event_type = ?1
           AND event.study_day BETWEEN ?2 AND ?3
           AND NOT EXISTS (
               SELECT 1
               FROM review_event AS revoke
               WHERE revoke.event_type = ?4
                 AND revoke.target_event_id = event.id
           )
         GROUP BY event.study_day
         ORDER BY event.study_day",
    )?;
    let rows = statement.query_map(
        params![
            domain::ReviewEventType::Review.as_db_str(),
            input.activity_start_study_day,
            input.study_day,
            domain::ReviewEventType::Revoke.as_db_str(),
        ],
        |row| {
            Ok(DailyReviewActivity {
                study_day: row.get(0)?,
                count: row.get(1)?,
            })
        },
    )?;

    let mut activity = Vec::new();
    let mut reviewed_today = 0;
    for row in rows {
        let day = row?;
        if day.study_day == input.study_day {
            reviewed_today = day.count;
        }
        activity.push(day);
    }

    Ok(HomeStats {
        activity,
        reviewed_today,
        queue,
        next_learning_due_at,
    })
}

fn validate_input(input: &LoadHomeStatsInput) -> Result<()> {
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
    if !(-MAX_JSON_SAFE_INTEGER..=MAX_JSON_SAFE_INTEGER).contains(&input.activity_start_study_day) {
        return Err(DatabaseError::InvalidInput(
            "activityStartStudyDay must be a JSON-safe integer".into(),
        ));
    }
    if input.activity_start_study_day > input.study_day {
        return Err(DatabaseError::InvalidInput(
            "activityStartStudyDay cannot be after studyDay".into(),
        ));
    }
    if input.study_day - input.activity_start_study_day >= MAX_ACTIVITY_DAYS {
        return Err(DatabaseError::InvalidInput(format!(
            "activity range cannot exceed {MAX_ACTIVITY_DAYS} days"
        )));
    }
    Ok(())
}
