use std::sync::mpsc::{self, Sender, SyncSender};

use super::{
    CreateBasicCardInput, DatabaseError, RecordGradeInput, Result, ReviewContext,
    ReviewMutationResult, UndoLastGradeInput,
};

pub(super) enum WriterMessage {
    CreateBasicCard {
        input: CreateBasicCardInput,
        reply: SyncSender<Result<ReviewContext>>,
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

    pub fn create_basic_card(&self, input: CreateBasicCardInput) -> Result<ReviewContext> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::CreateBasicCard { input, reply })
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

    pub(super) fn shutdown(&self) -> Result<()> {
        self.sender
            .send(WriterMessage::Shutdown)
            .map_err(|_| DatabaseError::WriterUnavailable)
    }
}
