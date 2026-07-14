pub mod commands;
mod connection;
mod domain;
mod embedding_index;
mod error;
mod migrations;
mod paths;
mod queue;
#[allow(dead_code)]
pub mod snapshot;
mod validation;
mod writer;

use std::{
    fs,
    sync::{
        mpsc::{self, Receiver},
        Mutex,
    },
    thread::{self, JoinHandle},
};

use rusqlite::Connection;

pub use connection::register_sqlite_vec;
pub use domain::{
    CreateBasicCardInput, RecordGradeInput, ReviewContext, ReviewMutationResult, UndoLastGradeInput,
};
pub use error::{DatabaseError, Result};
pub use paths::DatabasePaths;
pub use queue::{ReviewQueueSelection, SelectNextReviewCardInput};
pub use writer::DatabaseClient;
use writer::WriterMessage;

use connection::{DatabaseKind, FileState};

#[derive(Clone, Copy, Debug)]
pub struct InitializationOptions {
    pub launch_snapshot: bool,
}

impl Default for InitializationOptions {
    fn default() -> Self {
        Self {
            launch_snapshot: true,
        }
    }
}

pub struct Database {
    paths: DatabasePaths,
    client: DatabaseClient,
    writer_thread: Mutex<Option<JoinHandle<()>>>,
    snapshot_thread: Mutex<Option<JoinHandle<Result<snapshot::CreatedSnapshot>>>>,
}

impl Database {
    pub fn paths(&self) -> &DatabasePaths {
        &self.paths
    }

    pub fn client(&self) -> DatabaseClient {
        self.client.clone()
    }

    #[cfg(test)]
    fn create_basic_card(&self, input: CreateBasicCardInput) -> Result<ReviewContext> {
        self.client.create_basic_card(input)
    }

    #[cfg(test)]
    fn load_review_context(&self, review_card_id: String) -> Result<ReviewContext> {
        self.client.load_review_context(review_card_id)
    }

    #[cfg(test)]
    fn record_grade(&self, input: RecordGradeInput) -> Result<ReviewMutationResult> {
        self.client.record_grade(input)
    }

    #[cfg(test)]
    fn undo_last_grade(&self, input: UndoLastGradeInput) -> Result<ReviewMutationResult> {
        self.client.undo_last_grade(input)
    }

    #[cfg(test)]
    fn select_next_review_card(
        &self,
        input: SelectNextReviewCardInput,
    ) -> Result<ReviewQueueSelection> {
        self.client.select_next_review_card(input)
    }

    #[cfg(test)]
    pub fn wait_for_launch_snapshot(&self) -> Result<Option<snapshot::CreatedSnapshot>> {
        let thread = self
            .snapshot_thread
            .lock()
            .map_err(|_| DatabaseError::SnapshotThread("snapshot lock was poisoned".into()))?
            .take();
        match thread {
            Some(thread) => thread
                .join()
                .map_err(|_| DatabaseError::SnapshotThread("snapshot worker panicked".into()))?
                .map(Some),
            None => Ok(None),
        }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        if let Ok(snapshot) = self.snapshot_thread.get_mut() {
            if let Some(thread) = snapshot.take() {
                match thread.join() {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        log::error!("launch snapshot failed: {error}");
                    }
                    Err(error) => {
                        log::error!("launch snapshot worker panicked during shutdown: {error:?}");
                    }
                }
            }
        }
        let _ = self.client.shutdown();
        if let Ok(writer) = self.writer_thread.get_mut() {
            if let Some(thread) = writer.take() {
                if let Err(error) = thread.join() {
                    log::error!("database writer panicked during shutdown: {error:?}");
                }
            }
        }
    }
}

pub fn initialize(
    paths: DatabasePaths,
    application_version: &str,
    options: InitializationOptions,
) -> Result<Database> {
    register_sqlite_vec()?;
    fs::create_dir_all(paths.root())?;

    let main_state = connection::inspect_file(&paths.main)?;
    let media_state = connection::inspect_file(&paths.media)?;
    if main_state != media_state {
        return Err(DatabaseError::IncompletePair {
            main_state: main_state.label(),
            media_state: media_state.label(),
        });
    }

    let mut main = connection::open_writer(&paths.main, DatabaseKind::Main, main_state)?;
    let mut media = connection::open_writer(&paths.media, DatabaseKind::Media, media_state)?;
    let main_status = migrations::inspect_main(&mut main)?;
    let media_status = migrations::inspect_media(&mut media)?;

    if main_state == FileState::Existing && (main_status.pending || media_status.pending) {
        snapshot::create_snapshot_pair(&paths, application_version)?;
    }

    migrations::run_media(&mut media)?;
    migrations::run_main(&mut main)?;
    validation::validate_migrated_pair(&mut main, &mut media, &paths.main, &paths.media)?;

    let (writer, writer_rx) = mpsc::channel();
    let client = DatabaseClient::new(writer);
    let writer_paths = paths.clone();
    let writer_thread = thread::Builder::new()
        .name("dara-database-writer".into())
        .spawn(move || writer_loop(main, media, writer_paths, writer_rx))?;

    let snapshot_thread = if options.launch_snapshot {
        let snapshot_paths = paths.clone();
        let version = application_version.to_owned();
        Some(
            thread::Builder::new()
                .name("dara-launch-snapshot".into())
                .spawn(move || {
                    let snapshot = snapshot::create_snapshot_pair(&snapshot_paths, &version)?;
                    log::info!(
                        "created launch snapshot {} at {}",
                        snapshot.manifest_path.display(),
                        snapshot.manifest.created_at
                    );
                    snapshot::prune_snapshots(&snapshot_paths.backups)?;
                    Ok(snapshot)
                })?,
        )
    } else {
        None
    };

    Ok(Database {
        paths,
        client,
        writer_thread: Mutex::new(Some(writer_thread)),
        snapshot_thread: Mutex::new(snapshot_thread),
    })
}

fn writer_loop(
    mut main: Connection,
    media: Connection,
    _paths: DatabasePaths,
    receiver: Receiver<WriterMessage>,
) {
    for message in receiver {
        match message {
            WriterMessage::CreateBasicCard { input, reply } => {
                let _ = reply.send(domain::create_basic_card(&mut main, input));
            }
            WriterMessage::LoadReviewContext {
                review_card_id,
                reply,
            } => {
                let _ = reply.send(domain::load_review_context(&main, &review_card_id));
            }
            WriterMessage::RecordGrade { input, reply } => {
                let _ = reply.send(domain::record_grade(&mut main, input));
            }
            WriterMessage::UndoLastGrade { input, reply } => {
                let _ = reply.send(domain::undo_last_grade(&mut main, input));
            }
            WriterMessage::SelectNextReviewCard { input, reply } => {
                let _ = reply.send(queue::select_next_review_card(&mut main, input));
            }
            WriterMessage::Shutdown => break,
        }
    }
    if let Err(error) = checkpoint_pair(&main, &media) {
        log::error!("database checkpoint failed during shutdown: {error}");
    }
}

fn checkpoint_pair(main: &Connection, media: &Connection) -> Result<()> {
    main.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    media.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    Ok(())
}

#[cfg(test)]
mod domain_tests;
#[cfg(test)]
mod queue_tests;
#[cfg(test)]
mod tests;
