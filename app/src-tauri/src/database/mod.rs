mod connection;
mod embedding_index;
mod error;
mod migrations;
mod paths;
#[allow(dead_code)]
pub mod snapshot;
mod validation;

use std::{
    fs,
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
    thread::{self, JoinHandle},
};

use rusqlite::Connection;

pub use connection::register_sqlite_vec;
pub use error::{DatabaseError, Result};
pub use paths::DatabasePaths;

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
    writer: Sender<()>,
    writer_thread: Mutex<Option<JoinHandle<()>>>,
    snapshot_thread: Mutex<Option<JoinHandle<Result<snapshot::CreatedSnapshot>>>>,
}

impl Database {
    pub fn paths(&self) -> &DatabasePaths {
        &self.paths
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
        let _ = self.writer.send(());
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
        writer,
        writer_thread: Mutex::new(Some(writer_thread)),
        snapshot_thread: Mutex::new(snapshot_thread),
    })
}

fn writer_loop(main: Connection, media: Connection, _paths: DatabasePaths, receiver: Receiver<()>) {
    let _ = receiver.recv();
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
mod tests;
