use serde::Serialize;
use tauri::State;

use super::{
    CardContentDraft, CardContentListItem, Database, DatabaseError, DeleteCardContentInput,
    RecordGradeInput, ReviewContext, ReviewMutationResult, ReviewQueueSelection,
    SearchCardContentInput, SelectNextReviewCardInput, SetCardContentSuspendedInput,
    UndoLastGradeInput, UpdateCardContentInput,
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
            DatabaseError::StaleCardContent(_) => "staleCardContent",
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
pub async fn create_card_content(
    database: State<'_, Database>,
    input: CardContentDraft,
) -> CommandResult<ReviewContext> {
    let client = database.client();
    run_writer(move || client.create_card_content(input)).await
}

#[tauri::command]
pub async fn update_card_content(
    database: State<'_, Database>,
    input: UpdateCardContentInput,
) -> CommandResult<CardContentListItem> {
    let client = database.client();
    run_writer(move || client.update_card_content(input)).await
}

#[tauri::command]
pub async fn search_card_content(
    database: State<'_, Database>,
    input: SearchCardContentInput,
) -> CommandResult<Vec<CardContentListItem>> {
    let client = database.client();
    run_writer(move || client.search_card_content(input)).await
}

#[tauri::command]
pub async fn set_card_content_suspended(
    database: State<'_, Database>,
    input: SetCardContentSuspendedInput,
) -> CommandResult<CardContentListItem> {
    let client = database.client();
    run_writer(move || client.set_card_content_suspended(input)).await
}

#[tauri::command]
pub async fn delete_card_content(
    database: State<'_, Database>,
    input: DeleteCardContentInput,
) -> CommandResult<()> {
    let client = database.client();
    run_writer(move || client.delete_card_content(input)).await
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

#[tauri::command]
pub async fn select_next_review_card(
    database: State<'_, Database>,
    input: SelectNextReviewCardInput,
) -> CommandResult<ReviewQueueSelection> {
    let client = database.client();
    run_writer(move || client.select_next_review_card(input)).await
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
