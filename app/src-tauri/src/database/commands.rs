use serde::Serialize;
use tauri::State;

use super::{
    CreateBasicCardInput, Database, DatabaseError, RecordGradeInput, ReviewContext,
    ReviewMutationResult, UndoLastGradeInput,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    code: &'static str,
    message: String,
}

impl From<DatabaseError> for CommandError {
    fn from(error: DatabaseError) -> Self {
        let code = match &error {
            DatabaseError::InvalidInput(_) => "invalidInput",
            DatabaseError::NotFound { .. } => "notFound",
            DatabaseError::StaleReviewContext(_) => "staleReviewContext",
            DatabaseError::IdempotencyConflict { .. } => "idempotencyConflict",
            DatabaseError::WriterUnavailable => "databaseUnavailable",
            DatabaseError::CorruptReviewData(_) => "corruptReviewData",
            DatabaseError::UnsupportedSchedulerConfig(_) => "unsupportedSchedulerConfig",
            _ => "databaseError",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

type CommandResult<T> = std::result::Result<T, CommandError>;

#[tauri::command]
pub async fn create_basic_card(
    database: State<'_, Database>,
    input: CreateBasicCardInput,
) -> CommandResult<ReviewContext> {
    let client = database.client();
    run_writer(move || client.create_basic_card(input)).await
}

#[tauri::command]
pub async fn load_review_context(
    database: State<'_, Database>,
    review_card_id: String,
) -> CommandResult<ReviewContext> {
    let client = database.client();
    run_writer(move || client.load_review_context(review_card_id)).await
}

#[tauri::command]
pub async fn record_grade(
    database: State<'_, Database>,
    input: RecordGradeInput,
) -> CommandResult<ReviewMutationResult> {
    let client = database.client();
    run_writer(move || client.record_grade(input)).await
}

#[tauri::command]
pub async fn undo_last_grade(
    database: State<'_, Database>,
    input: UndoLastGradeInput,
) -> CommandResult<ReviewMutationResult> {
    let client = database.client();
    run_writer(move || client.undo_last_grade(input)).await
}

async fn run_writer<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> super::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError {
            code: "databaseUnavailable",
            message: format!("database command worker failed: {error}"),
        })?
        .map_err(Into::into)
}
