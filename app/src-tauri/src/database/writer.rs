use std::sync::mpsc::{self, Sender, SyncSender};

use super::{
    CanonicalImage, CardContentDraft, CardContentListItem, DatabaseError, DeleteCardContentInput,
    HomeStats, ImageRecord, LoadHomeStatsInput, MediaPayload, OcrJob, OcrQueueRecovery,
    RecordGradeInput, Result, ReviewContext, ReviewMutationResult, ReviewQueueSelection,
    SearchCardContentInput, SelectNextReviewCardInput, SetCardContentSuspendedInput,
    UndoLastGradeInput, UpdateCardContentInput,
};

pub(super) enum WriterMessage {
    CreateCardContent {
        input: CardContentDraft,
        reply: SyncSender<Result<ReviewContext>>,
    },
    UpdateCardContent {
        input: UpdateCardContentInput,
        reply: SyncSender<Result<CardContentListItem>>,
    },
    SearchCardContent {
        input: SearchCardContentInput,
        reply: SyncSender<Result<Vec<CardContentListItem>>>,
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
    IngestImage {
        image: CanonicalImage,
        reply: SyncSender<Result<ImageRecord>>,
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
    Shutdown,
}

#[derive(Clone)]
pub struct DatabaseClient {
    sender: Sender<WriterMessage>,
}

impl DatabaseClient {
    pub(super) fn new(sender: Sender<WriterMessage>) -> Self {
        Self { sender }
    }

    pub fn create_card_content(&self, input: CardContentDraft) -> Result<ReviewContext> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::CreateCardContent { input, reply })
            .map_err(|_| DatabaseError::WriterUnavailable)?;
        receiver
            .recv()
            .map_err(|_| DatabaseError::WriterUnavailable)?
    }

    pub fn update_card_content(
        &self,
        input: UpdateCardContentInput,
    ) -> Result<CardContentListItem> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::UpdateCardContent { input, reply })
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

    pub fn ingest_image(&self, image: CanonicalImage) -> Result<ImageRecord> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::IngestImage { image, reply })
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

    pub(super) fn shutdown(&self) -> Result<()> {
        self.sender
            .send(WriterMessage::Shutdown)
            .map_err(|_| DatabaseError::WriterUnavailable)
    }
}
