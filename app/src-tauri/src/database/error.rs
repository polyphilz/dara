use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("database I/O failed: {0}")]
    Io(#[from] io::Error),

    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration operation failed: {0}")]
    Migration(#[from] refinery::Error),

    #[error("snapshot JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sqlite-vec auto-extension registration failed with SQLite code {0}")]
    VecRegistration(i32),

    #[error("database pair is incomplete: main is {main_state}, media is {media_state}")]
    IncompletePair {
        main_state: &'static str,
        media_state: &'static str,
    },

    #[error(
        "{kind} database at {path} has application_id {actual:#010x}; expected {expected:#010x}"
    )]
    WrongApplicationId {
        kind: &'static str,
        path: PathBuf,
        expected: i32,
        actual: i32,
    },

    #[error("{kind} database migration history is incompatible: {reason}")]
    IncompatibleMigrationHistory { kind: &'static str, reason: String },

    #[error("{kind} database validation failed: {reason}")]
    Validation { kind: &'static str, reason: String },

    #[error("snapshot manifest is invalid: {0}")]
    InvalidSnapshot(String),

    #[cfg(test)]
    #[error("background snapshot thread failed: {0}")]
    SnapshotThread(String),

    #[error("system time is before the Unix epoch")]
    InvalidSystemTime,

    #[error("database writer is unavailable")]
    WriterUnavailable,

    #[error("invalid database command: {0}")]
    InvalidInput(String),

    #[error("{entity} {id} was not found")]
    NotFound { entity: &'static str, id: String },

    #[error("review context is stale: {0}")]
    StaleReviewContext(String),

    #[error("event {event_id} was already used for a different request")]
    IdempotencyConflict { event_id: String },

    #[error("stored review data is invalid: {0}")]
    CorruptReviewData(String),

    #[error("active scheduler config is unsupported: {0}")]
    UnsupportedSchedulerConfig(String),
}

pub type Result<T> = std::result::Result<T, DatabaseError>;
