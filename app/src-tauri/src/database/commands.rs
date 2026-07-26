use serde::Serialize;
use tauri::State;

use crate::search::{SearchCardContentResult, SearchService, SemanticSearchStatus};

use super::{
    CardContentDraft, CardContentListItem, Database, DatabaseError, DeleteCardContentInput,
    HomeStats, InstallSchedulerReplayInput, LoadHomeStatsInput, MediaMaintenanceReport,
    PrepareDesiredRetentionReplayInput, RecordGradeInput, ReviewContext, ReviewMutationResult,
    ReviewQueueSelection, SchedulerReplayInstallReport, SchedulerReplaySnapshot,
    SearchCardContentInput, SearchMaintenanceOperation, SearchMaintenanceReport,
    SelectNextReviewCardInput, SetCardContentSuspendedInput, UndoLastGradeInput,
    UpdateCardContentInput, MEDIA_ORPHAN_GRACE_MILLIS,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub(crate) code: CommandErrorCode,
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CommandErrorCode {
    InvalidInput,
    NotFound,
    StaleReviewContext,
    StaleSchedulerReplay,
    StaleSettings,
    StaleCardContent,
    IdempotencyConflict,
    DatabaseUnavailable,
    CorruptReviewData,
    UnsupportedSchedulerConfig,
    DatabaseError,
}

impl From<DatabaseError> for CommandError {
    fn from(error: DatabaseError) -> Self {
        let code = match &error {
            DatabaseError::InvalidInput(_) => CommandErrorCode::InvalidInput,
            DatabaseError::NotFound { .. } => CommandErrorCode::NotFound,
            DatabaseError::StaleReviewContext(_) => CommandErrorCode::StaleReviewContext,
            DatabaseError::StaleSchedulerReplay(_) => CommandErrorCode::StaleSchedulerReplay,
            DatabaseError::StaleSettings(_) => CommandErrorCode::StaleSettings,
            DatabaseError::StaleCardContent(_) => CommandErrorCode::StaleCardContent,
            DatabaseError::IdempotencyConflict { .. } => CommandErrorCode::IdempotencyConflict,
            DatabaseError::WriterUnavailable => CommandErrorCode::DatabaseUnavailable,
            DatabaseError::CorruptReviewData(_) => CommandErrorCode::CorruptReviewData,
            DatabaseError::UnsupportedSchedulerConfig(_) => {
                CommandErrorCode::UnsupportedSchedulerConfig
            }
            _ => CommandErrorCode::DatabaseError,
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

impl CommandError {
    pub(crate) fn with_context(mut self, context: String) -> Self {
        self.message = format!("{}: {context}", self.message);
        self
    }

    pub(crate) fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            code: CommandErrorCode::InvalidInput,
            message: message.into(),
        }
    }
}

pub(crate) type CommandResult<T> = std::result::Result<T, CommandError>;

#[tauri::command]
pub async fn create_card_content(
    database: State<'_, Database>,
    input: CardContentDraft,
    media_lease_id: String,
) -> CommandResult<ReviewContext> {
    let client = database.client();
    run_writer(move || client.create_card_content(input, media_lease_id)).await
}

#[tauri::command]
pub async fn update_card_content(
    database: State<'_, Database>,
    input: UpdateCardContentInput,
    media_lease_id: String,
) -> CommandResult<CardContentListItem> {
    let client = database.client();
    run_writer(move || client.update_card_content(input, media_lease_id)).await
}

#[tauri::command]
pub async fn load_card_content(
    database: State<'_, Database>,
    card_content_id: String,
) -> CommandResult<CardContentListItem> {
    let client = database.client();
    run_writer(move || client.load_card_content(card_content_id)).await
}

#[tauri::command]
pub async fn renew_media_lease(
    database: State<'_, Database>,
    lease_id: String,
) -> CommandResult<u64> {
    let client = database.client();
    let now = super::now_millis().map_err(CommandError::from)?;
    run_writer(move || client.renew_media_lease(lease_id, now)).await
}

#[tauri::command]
pub async fn maintain_media(
    database: State<'_, Database>,
) -> CommandResult<MediaMaintenanceReport> {
    let client = database.client();
    let now = super::now_millis().map_err(CommandError::from)?;
    run_writer(move || client.maintain_media(now, MEDIA_ORPHAN_GRACE_MILLIS)).await
}

#[tauri::command]
pub async fn search_card_content(
    search: State<'_, SearchService>,
    input: SearchCardContentInput,
) -> CommandResult<SearchCardContentResult> {
    let search = search.inner().clone();
    run_writer(move || search.search(input)).await
}

#[tauri::command]
pub fn search_status(search: State<'_, SearchService>) -> SemanticSearchStatus {
    search.status()
}

#[tauri::command]
pub async fn maintain_search(
    database: State<'_, Database>,
    operation: SearchMaintenanceOperation,
) -> CommandResult<SearchMaintenanceReport> {
    let client = database.client();
    run_writer(move || client.maintain_search(operation)).await
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

#[tauri::command]
pub async fn load_home_stats(
    database: State<'_, Database>,
    input: LoadHomeStatsInput,
) -> CommandResult<HomeStats> {
    let client = database.client();
    run_writer(move || client.load_home_stats(input)).await
}

#[tauri::command]
pub async fn load_scheduler_replay_snapshot(
    database: State<'_, Database>,
) -> CommandResult<SchedulerReplaySnapshot> {
    let client = database.client();
    run_writer(move || client.load_scheduler_replay_snapshot()).await
}

#[tauri::command]
pub async fn prepare_desired_retention_replay(
    database: State<'_, Database>,
    input: PrepareDesiredRetentionReplayInput,
) -> CommandResult<SchedulerReplaySnapshot> {
    let client = database.client();
    run_writer(move || client.prepare_desired_retention_replay(input)).await
}

#[tauri::command]
pub async fn install_scheduler_replay(
    database: State<'_, Database>,
    input: InstallSchedulerReplayInput,
) -> CommandResult<SchedulerReplayInstallReport> {
    let client = database.client();
    run_writer(move || client.install_scheduler_replay(input)).await
}

pub(crate) async fn run_writer<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> super::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| CommandError {
            code: CommandErrorCode::DatabaseUnavailable,
            message: format!("database command worker failed: {error}"),
        })?
        .map_err(Into::into)
}
