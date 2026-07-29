use std::sync::{
    mpsc::{self, Sender, SyncSender},
    Arc,
};

use super::embedding_index::{
    EmbeddingIndexProgress, InstallEmbeddingDisposition, PendingEmbeddingDocument,
    SearchMaintenanceOperation, SearchMaintenanceReport,
};
use super::snapshot::CreatedSnapshot;
use crate::backup::domain::{BackupSetId, ContentSha256};

use super::LocalCheckpointSync;
use super::{
    AdoptLegacyZoomInput, CanonicalImage, CardContentDraft, CardContentListItem,
    DatabaseDiagnosticsSnapshot, DatabaseError, DeleteCardContentInput, HomeStats, ImageRecord,
    InstallSchedulerReplayInput, LoadHomeStatsInput, MediaMaintenanceReport, MediaPayload, OcrJob,
    OcrQueueRecovery, OffsiteBackupConfig, OffsiteBackupTakeoverReason,
    OffsiteCheckpointScheduleState, OffsiteMediaCandidate, OffsiteMediaReconciliationReport,
    OffsiteMediaSummary, PrepareDesiredRetentionReplayInput, PrepareOffsiteCheckpointInput,
    PreparedOffsiteCheckpoint, RecordGradeInput, RecordOffsiteMediaAttemptInput, Result,
    ReviewContext, ReviewMutationResult, ReviewQueueSelection, SaveOffsiteBackupConfigInput,
    SchedulerReplayInstallReport, SchedulerReplaySnapshot, SearchCardContentInput,
    SelectNextReviewCardInput, SetAppearanceInput, SetCardContentSuspendedInput,
    SetKeyboardBindingsInput, SetZoomPercentInput, StoredSettings, UndoLastGradeInput,
    UpdateCardContentInput,
};
use crate::backup::domain::{BackupErrorCode, CheckpointId};
use crate::backup::litestream::LitestreamTxid;

pub(super) enum WriterMessage {
    CreateCardContent {
        input: CardContentDraft,
        media_lease_id: String,
        reply: SyncSender<Result<ReviewContext>>,
    },
    UpdateCardContent {
        input: UpdateCardContentInput,
        media_lease_id: String,
        reply: SyncSender<Result<CardContentListItem>>,
    },
    LoadCardContent {
        card_content_id: String,
        reply: SyncSender<Result<CardContentListItem>>,
    },
    SearchCardContent {
        input: SearchCardContentInput,
        reply: SyncSender<Result<Vec<CardContentListItem>>>,
    },
    HybridSearchCardContent {
        input: SearchCardContentInput,
        query_embedding: Vec<f32>,
        reply: SyncSender<Result<Vec<CardContentListItem>>>,
    },
    LoadEmbeddingReconciliationBatch {
        limit: i64,
        reply: SyncSender<Result<Vec<PendingEmbeddingDocument>>>,
    },
    InstallTextEmbedding {
        document: PendingEmbeddingDocument,
        embedding: Vec<f32>,
        reply: SyncSender<Result<InstallEmbeddingDisposition>>,
    },
    LoadEmbeddingIndexProgress {
        reply: SyncSender<Result<EmbeddingIndexProgress>>,
    },
    LoadDatabaseDiagnostics {
        reply: SyncSender<Result<DatabaseDiagnosticsSnapshot>>,
    },
    ActivateEmbeddingIndexIfComplete {
        reply: SyncSender<Result<bool>>,
    },
    MaintainSearch {
        operation: SearchMaintenanceOperation,
        reply: SyncSender<Result<SearchMaintenanceReport>>,
    },
    SetCardContentSuspended {
        input: SetCardContentSuspendedInput,
        reply: SyncSender<Result<CardContentListItem>>,
    },
    DeleteCardContent {
        input: DeleteCardContentInput,
        reply: SyncSender<Result<()>>,
    },
    LoadReviewContext {
        review_card_id: String,
        reply: SyncSender<Result<ReviewContext>>,
    },
    RecordGrade {
        input: RecordGradeInput,
        reply: SyncSender<Result<ReviewMutationResult>>,
    },
    UndoLastGrade {
        input: UndoLastGradeInput,
        reply: SyncSender<Result<ReviewMutationResult>>,
    },
    SelectNextReviewCard {
        input: SelectNextReviewCardInput,
        reply: SyncSender<Result<ReviewQueueSelection>>,
    },
    LoadHomeStats {
        input: LoadHomeStatsInput,
        reply: SyncSender<Result<HomeStats>>,
    },
    LoadSchedulerReplaySnapshot {
        reply: SyncSender<Result<SchedulerReplaySnapshot>>,
    },
    PrepareDesiredRetentionReplay {
        input: PrepareDesiredRetentionReplayInput,
        reply: SyncSender<Result<SchedulerReplaySnapshot>>,
    },
    InstallSchedulerReplay {
        input: InstallSchedulerReplayInput,
        reply: SyncSender<Result<SchedulerReplayInstallReport>>,
    },
    LoadSettings {
        reply: SyncSender<Result<StoredSettings>>,
    },
    SetAppearance {
        input: SetAppearanceInput,
        reply: SyncSender<Result<StoredSettings>>,
    },
    SetZoomPercent {
        input: SetZoomPercentInput,
        reply: SyncSender<Result<StoredSettings>>,
    },
    AdoptLegacyZoom {
        input: AdoptLegacyZoomInput,
        reply: SyncSender<Result<StoredSettings>>,
    },
    SetKeyboardBindings {
        input: SetKeyboardBindingsInput,
        reply: SyncSender<Result<StoredSettings>>,
    },
    LoadOffsiteBackupConfig {
        reply: SyncSender<Result<Option<OffsiteBackupConfig>>>,
    },
    LoadOffsiteBackupRuntimeConfig {
        reply: SyncSender<Result<Option<OffsiteBackupConfig>>>,
    },
    LoadOffsiteBackupTakeoverReason {
        reply: SyncSender<Result<Option<OffsiteBackupTakeoverReason>>>,
    },
    LoadPendingOffsiteCredentialCleanup {
        reply: SyncSender<Result<Vec<BackupSetId>>>,
    },
    SaveOffsiteBackupConfig {
        input: SaveOffsiteBackupConfigInput,
        reply: SyncSender<Result<OffsiteBackupConfig>>,
    },
    SetOffsiteBackupTakeoverReason {
        backup_set_id: BackupSetId,
        reason: Option<OffsiteBackupTakeoverReason>,
        reply: SyncSender<Result<()>>,
    },
    CompleteOffsiteCredentialCleanup {
        backup_set_id: BackupSetId,
        reply: SyncSender<Result<()>>,
    },
    ReconcileOffsiteMedia {
        now: i64,
        reply: SyncSender<Result<OffsiteMediaReconciliationReport>>,
    },
    LoadNextOffsiteMedia {
        backup_set_id: BackupSetId,
        now: i64,
        reply: SyncSender<Result<Option<OffsiteMediaCandidate>>>,
    },
    RecordOffsiteMediaAttempt {
        input: RecordOffsiteMediaAttemptInput,
        reply: SyncSender<Result<()>>,
    },
    LoadOffsiteMediaSummary {
        backup_set_id: BackupSetId,
        reply: SyncSender<Result<OffsiteMediaSummary>>,
    },
    LoadReferencedOffsiteMediaSummary {
        backup_set_id: BackupSetId,
        reply: SyncSender<Result<OffsiteMediaSummary>>,
    },
    ReleaseOffsiteMediaRetries {
        backup_set_id: BackupSetId,
        now: i64,
        reply: SyncSender<Result<u64>>,
    },
    ReleaseAllOffsiteMediaRetries {
        backup_set_id: BackupSetId,
        now: i64,
        reply: SyncSender<Result<u64>>,
    },
    RequeueOffsiteMediaCredentialFailures {
        backup_set_id: BackupSetId,
        now: i64,
        reply: SyncSender<Result<u64>>,
    },
    RequeueOffsiteMediaEvidence {
        backup_set_id: BackupSetId,
        sha256s: Vec<ContentSha256>,
        error_code: BackupErrorCode,
        now: i64,
        reply: SyncSender<Result<u64>>,
    },
    PrepareOffsiteCheckpoint {
        input: PrepareOffsiteCheckpointInput,
        local_sync: Arc<dyn LocalCheckpointSync>,
        reply: SyncSender<Result<PreparedOffsiteCheckpoint>>,
    },
    MarkOffsiteCheckpointFenced {
        checkpoint_id: CheckpointId,
        txid: LitestreamTxid,
        reply: SyncSender<Result<()>>,
    },
    MarkOffsiteCheckpointReplicated {
        checkpoint_id: CheckpointId,
        reply: SyncSender<Result<()>>,
    },
    MarkOffsiteCheckpointPublished {
        checkpoint_id: CheckpointId,
        manifest_object_key: String,
        reply: SyncSender<Result<()>>,
    },
    MarkOffsiteCheckpointFailed {
        checkpoint_id: CheckpointId,
        error_code: BackupErrorCode,
        reply: SyncSender<Result<()>>,
    },
    FailIncompleteOffsiteCheckpoints {
        error_code: BackupErrorCode,
        reply: SyncSender<Result<u64>>,
    },
    LoadOffsiteCheckpointScheduleState {
        reply: SyncSender<Result<OffsiteCheckpointScheduleState>>,
    },
    IngestImage {
        image: CanonicalImage,
        lease_id: String,
        reply: SyncSender<Result<ImageRecord>>,
    },
    RenewMediaLease {
        lease_id: String,
        now: i64,
        reply: SyncSender<Result<u64>>,
    },
    MaintainMedia {
        now: i64,
        grace_millis: i64,
        reply: SyncSender<Result<MediaMaintenanceReport>>,
    },
    LoadMediaPayload {
        image_id: String,
        reply: SyncSender<Result<MediaPayload>>,
    },
    ClaimNextOcrJob {
        now: i64,
        reply: SyncSender<Result<Option<OcrJob>>>,
    },
    CompleteImageOcr {
        image_id: String,
        expected_attempt_count: u32,
        result: std::result::Result<String, String>,
        now: i64,
        reply: SyncSender<Result<()>>,
    },
    RecoverInterruptedOcrJobs {
        stale_started_at_or_before: i64,
        now: i64,
        reply: SyncSender<Result<OcrQueueRecovery>>,
    },
    CreateSnapshotPair {
        application_version: String,
        reply: SyncSender<Result<CreatedSnapshot>>,
    },
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WriterContentEffect {
    ReadOnly,
    RecoverableMutation,
    BackupMutation,
    Lifecycle,
}

impl WriterMessage {
    pub(super) fn content_effect(&self) -> WriterContentEffect {
        match self {
            Self::CreateCardContent { .. }
            | Self::UpdateCardContent { .. }
            | Self::InstallTextEmbedding { .. }
            | Self::ActivateEmbeddingIndexIfComplete { .. }
            | Self::MaintainSearch { .. }
            | Self::SetCardContentSuspended { .. }
            | Self::DeleteCardContent { .. }
            | Self::RecordGrade { .. }
            | Self::UndoLastGrade { .. }
            | Self::InstallSchedulerReplay { .. }
            | Self::SetAppearance { .. }
            | Self::SetZoomPercent { .. }
            | Self::AdoptLegacyZoom { .. }
            | Self::SetKeyboardBindings { .. }
            | Self::IngestImage { .. }
            | Self::RenewMediaLease { .. }
            | Self::MaintainMedia { .. }
            | Self::ClaimNextOcrJob { .. }
            | Self::CompleteImageOcr { .. }
            | Self::RecoverInterruptedOcrJobs { .. } => WriterContentEffect::RecoverableMutation,
            Self::SaveOffsiteBackupConfig { .. }
            | Self::SetOffsiteBackupTakeoverReason { .. }
            | Self::CompleteOffsiteCredentialCleanup { .. }
            | Self::ReconcileOffsiteMedia { .. }
            | Self::RecordOffsiteMediaAttempt { .. }
            | Self::ReleaseOffsiteMediaRetries { .. }
            | Self::ReleaseAllOffsiteMediaRetries { .. }
            | Self::RequeueOffsiteMediaCredentialFailures { .. }
            | Self::RequeueOffsiteMediaEvidence { .. }
            | Self::PrepareOffsiteCheckpoint { .. }
            | Self::MarkOffsiteCheckpointFenced { .. }
            | Self::MarkOffsiteCheckpointReplicated { .. }
            | Self::MarkOffsiteCheckpointPublished { .. }
            | Self::MarkOffsiteCheckpointFailed { .. }
            | Self::FailIncompleteOffsiteCheckpoints { .. } => WriterContentEffect::BackupMutation,
            Self::LoadCardContent { .. }
            | Self::SearchCardContent { .. }
            | Self::HybridSearchCardContent { .. }
            | Self::LoadEmbeddingReconciliationBatch { .. }
            | Self::LoadEmbeddingIndexProgress { .. }
            | Self::LoadDatabaseDiagnostics { .. }
            | Self::LoadReviewContext { .. }
            | Self::SelectNextReviewCard { .. }
            | Self::LoadHomeStats { .. }
            | Self::LoadSchedulerReplaySnapshot { .. }
            | Self::PrepareDesiredRetentionReplay { .. }
            | Self::LoadSettings { .. }
            | Self::LoadOffsiteBackupConfig { .. }
            | Self::LoadOffsiteBackupRuntimeConfig { .. }
            | Self::LoadOffsiteBackupTakeoverReason { .. }
            | Self::LoadPendingOffsiteCredentialCleanup { .. }
            | Self::LoadNextOffsiteMedia { .. }
            | Self::LoadOffsiteMediaSummary { .. }
            | Self::LoadReferencedOffsiteMediaSummary { .. }
            | Self::LoadMediaPayload { .. }
            | Self::LoadOffsiteCheckpointScheduleState { .. } => WriterContentEffect::ReadOnly,
            Self::CreateSnapshotPair { .. } | Self::Shutdown => WriterContentEffect::Lifecycle,
        }
    }
}

#[derive(Clone)]
pub struct DatabaseClient {
    sender: Sender<WriterMessage>,
}

impl DatabaseClient {
    pub(super) fn new(sender: Sender<WriterMessage>) -> Self {
        Self { sender }
    }

    pub fn create_card_content(
        &self,
        input: CardContentDraft,
        media_lease_id: String,
    ) -> Result<ReviewContext> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::CreateCardContent {
                input,
                media_lease_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn update_card_content(
        &self,
        input: UpdateCardContentInput,
        media_lease_id: String,
    ) -> Result<CardContentListItem> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::UpdateCardContent {
                input,
                media_lease_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_card_content(&self, card_content_id: String) -> Result<CardContentListItem> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadCardContent {
                card_content_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn search_card_content(
        &self,
        input: SearchCardContentInput,
    ) -> Result<Vec<CardContentListItem>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SearchCardContent { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn hybrid_search_card_content(
        &self,
        input: SearchCardContentInput,
        query_embedding: Vec<f32>,
    ) -> Result<Vec<CardContentListItem>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::HybridSearchCardContent {
                input,
                query_embedding,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_embedding_reconciliation_batch(
        &self,
        limit: i64,
    ) -> Result<Vec<PendingEmbeddingDocument>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadEmbeddingReconciliationBatch { limit, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn install_text_embedding(
        &self,
        document: PendingEmbeddingDocument,
        embedding: Vec<f32>,
    ) -> Result<InstallEmbeddingDisposition> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::InstallTextEmbedding {
                document,
                embedding,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_embedding_index_progress(&self) -> Result<EmbeddingIndexProgress> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadEmbeddingIndexProgress { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn activate_embedding_index_if_complete(&self) -> Result<bool> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::ActivateEmbeddingIndexIfComplete { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn maintain_search(
        &self,
        operation: SearchMaintenanceOperation,
    ) -> Result<SearchMaintenanceReport> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::MaintainSearch { operation, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn set_card_content_suspended(
        &self,
        input: SetCardContentSuspendedInput,
    ) -> Result<CardContentListItem> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SetCardContentSuspended { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn delete_card_content(&self, input: DeleteCardContentInput) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::DeleteCardContent { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_review_context(&self, review_card_id: String) -> Result<ReviewContext> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadReviewContext {
                review_card_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn record_grade(&self, input: RecordGradeInput) -> Result<ReviewMutationResult> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::RecordGrade { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn undo_last_grade(&self, input: UndoLastGradeInput) -> Result<ReviewMutationResult> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::UndoLastGrade { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn select_next_review_card(
        &self,
        input: SelectNextReviewCardInput,
    ) -> Result<ReviewQueueSelection> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SelectNextReviewCard { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_home_stats(&self, input: LoadHomeStatsInput) -> Result<HomeStats> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadHomeStats { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_scheduler_replay_snapshot(&self) -> Result<SchedulerReplaySnapshot> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadSchedulerReplaySnapshot { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn prepare_desired_retention_replay(
        &self,
        input: PrepareDesiredRetentionReplayInput,
    ) -> Result<SchedulerReplaySnapshot> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::PrepareDesiredRetentionReplay { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn install_scheduler_replay(
        &self,
        input: InstallSchedulerReplayInput,
    ) -> Result<SchedulerReplayInstallReport> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::InstallSchedulerReplay { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_settings(&self) -> Result<StoredSettings> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadSettings { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_database_diagnostics(&self) -> Result<DatabaseDiagnosticsSnapshot> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadDatabaseDiagnostics { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn set_appearance(&self, input: SetAppearanceInput) -> Result<StoredSettings> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SetAppearance { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn set_zoom_percent(&self, input: SetZoomPercentInput) -> Result<StoredSettings> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SetZoomPercent { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn adopt_legacy_zoom(&self, input: AdoptLegacyZoomInput) -> Result<StoredSettings> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::AdoptLegacyZoom { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn set_keyboard_bindings(&self, input: SetKeyboardBindingsInput) -> Result<StoredSettings> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SetKeyboardBindings { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    #[allow(dead_code)] // This is the persistence seam for the upcoming backup service.
    pub(crate) fn load_offsite_backup_config(&self) -> Result<Option<OffsiteBackupConfig>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadOffsiteBackupConfig { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_offsite_backup_runtime_config(&self) -> Result<Option<OffsiteBackupConfig>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadOffsiteBackupRuntimeConfig { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_offsite_backup_takeover_reason(
        &self,
    ) -> Result<Option<OffsiteBackupTakeoverReason>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadOffsiteBackupTakeoverReason { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_offsite_backup_takeover_availability(&self) -> Result<bool> {
        self.load_offsite_backup_takeover_reason()
            .map(|reason| reason.is_some())
    }

    pub(crate) fn load_pending_offsite_credential_cleanup(&self) -> Result<Vec<BackupSetId>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadPendingOffsiteCredentialCleanup { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    #[allow(dead_code)] // This is the persistence seam for the upcoming Settings commands.
    pub(crate) fn save_offsite_backup_config(
        &self,
        input: SaveOffsiteBackupConfigInput,
    ) -> Result<OffsiteBackupConfig> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SaveOffsiteBackupConfig { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn set_offsite_backup_takeover_reason(
        &self,
        backup_set_id: BackupSetId,
        reason: Option<OffsiteBackupTakeoverReason>,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::SetOffsiteBackupTakeoverReason {
                backup_set_id,
                reason,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn complete_offsite_credential_cleanup(
        &self,
        backup_set_id: BackupSetId,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::CompleteOffsiteCredentialCleanup {
                backup_set_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn reconcile_offsite_media(
        &self,
        now: i64,
    ) -> Result<OffsiteMediaReconciliationReport> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::ReconcileOffsiteMedia { now, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_next_offsite_media(
        &self,
        backup_set_id: BackupSetId,
        now: i64,
    ) -> Result<Option<OffsiteMediaCandidate>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadNextOffsiteMedia {
                backup_set_id,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn record_offsite_media_attempt(
        &self,
        input: RecordOffsiteMediaAttemptInput,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::RecordOffsiteMediaAttempt { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_offsite_media_summary(
        &self,
        backup_set_id: BackupSetId,
    ) -> Result<OffsiteMediaSummary> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadOffsiteMediaSummary {
                backup_set_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_referenced_offsite_media_summary(
        &self,
        backup_set_id: BackupSetId,
    ) -> Result<OffsiteMediaSummary> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadReferencedOffsiteMediaSummary {
                backup_set_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn release_offsite_media_retries(
        &self,
        backup_set_id: BackupSetId,
        now: i64,
    ) -> Result<u64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::ReleaseOffsiteMediaRetries {
                backup_set_id,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn release_all_offsite_media_retries(
        &self,
        backup_set_id: BackupSetId,
        now: i64,
    ) -> Result<u64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::ReleaseAllOffsiteMediaRetries {
                backup_set_id,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn requeue_offsite_media_credential_failures(
        &self,
        backup_set_id: BackupSetId,
        now: i64,
    ) -> Result<u64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::RequeueOffsiteMediaCredentialFailures {
                backup_set_id,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn requeue_offsite_media_evidence(
        &self,
        backup_set_id: BackupSetId,
        sha256s: Vec<ContentSha256>,
        error_code: BackupErrorCode,
        now: i64,
    ) -> Result<u64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::RequeueOffsiteMediaEvidence {
                backup_set_id,
                sha256s,
                error_code,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn prepare_offsite_checkpoint(
        &self,
        input: PrepareOffsiteCheckpointInput,
        local_sync: Arc<dyn LocalCheckpointSync>,
    ) -> Result<PreparedOffsiteCheckpoint> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::PrepareOffsiteCheckpoint {
                input,
                local_sync,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn mark_offsite_checkpoint_fenced(
        &self,
        checkpoint_id: CheckpointId,
        txid: LitestreamTxid,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::MarkOffsiteCheckpointFenced {
                checkpoint_id,
                txid,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn mark_offsite_checkpoint_replicated(
        &self,
        checkpoint_id: CheckpointId,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::MarkOffsiteCheckpointReplicated {
                checkpoint_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn mark_offsite_checkpoint_published(
        &self,
        checkpoint_id: CheckpointId,
        manifest_object_key: String,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::MarkOffsiteCheckpointPublished {
                checkpoint_id,
                manifest_object_key,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn mark_offsite_checkpoint_failed(
        &self,
        checkpoint_id: CheckpointId,
        error_code: BackupErrorCode,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::MarkOffsiteCheckpointFailed {
                checkpoint_id,
                error_code,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn fail_incomplete_offsite_checkpoints(
        &self,
        error_code: BackupErrorCode,
    ) -> Result<u64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::FailIncompleteOffsiteCheckpoints { error_code, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(crate) fn load_offsite_checkpoint_schedule_state(
        &self,
    ) -> Result<OffsiteCheckpointScheduleState> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadOffsiteCheckpointScheduleState { reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn ingest_image(&self, image: CanonicalImage, lease_id: String) -> Result<ImageRecord> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::IngestImage {
                image,
                lease_id,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn renew_media_lease(&self, lease_id: String, now: i64) -> Result<u64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::RenewMediaLease {
                lease_id,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn maintain_media(&self, now: i64, grace_millis: i64) -> Result<MediaMaintenanceReport> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::MaintainMedia {
                now,
                grace_millis,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn load_media_payload(&self, image_id: String) -> Result<MediaPayload> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::LoadMediaPayload { image_id, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn claim_next_ocr_job(&self, now: i64) -> Result<Option<OcrJob>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::ClaimNextOcrJob { now, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn complete_image_ocr(
        &self,
        image_id: String,
        expected_attempt_count: u32,
        result: std::result::Result<String, String>,
        now: i64,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::CompleteImageOcr {
                image_id,
                expected_attempt_count,
                result,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn recover_interrupted_ocr_jobs(
        &self,
        stale_started_at_or_before: i64,
        now: i64,
    ) -> Result<OcrQueueRecovery> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::RecoverInterruptedOcrJobs {
                stale_started_at_or_before,
                now,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(super) fn create_snapshot_pair(
        &self,
        application_version: String,
    ) -> Result<CreatedSnapshot> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::CreateSnapshotPair {
                application_version,
                reply,
            })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub(super) fn shutdown(&self) -> Result<()> {
        self.sender
            .send(WriterMessage::Shutdown)
            .map_err(|_| DatabaseError::WriterUnavailable)
    }
}
