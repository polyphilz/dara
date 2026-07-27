pub mod commands;
mod connection;
mod diagnostics;
mod domain;
pub(crate) mod embedding_index;
mod error;
mod media;
pub(crate) mod migrations;
mod offsite_backup;
mod offsite_media;
mod paths;
mod queue;
#[cfg(test)]
mod release_acceptance;
mod settings;
#[allow(dead_code)]
pub mod snapshot;
mod stats;
mod validation;
mod writer;

use std::{
    fs,
    path::Path,
    sync::{
        mpsc::{self, Receiver},
        Mutex,
    },
    thread::{self, JoinHandle},
};

use rusqlite::Connection;

pub use connection::register_sqlite_vec;
pub use diagnostics::DatabaseDiagnosticsSnapshot;
pub use domain::{
    CardContentDraft, CardContentListItem, DeleteCardContentInput, InstallSchedulerReplayInput,
    PrepareDesiredRetentionReplayInput, RecordGradeInput, ReviewContext, ReviewMutationResult,
    SchedulerReplayInstallReport, SchedulerReplaySnapshot, SearchCardContentInput,
    SetCardContentSuspendedInput, UndoLastGradeInput, UpdateCardContentInput,
};
pub use embedding_index::{SearchMaintenanceOperation, SearchMaintenanceReport};
pub use error::{DatabaseError, Result};
pub(crate) use media::now_millis;
pub use media::{
    CanonicalImage, ImageOcrStatus, ImageRecord, MediaMaintenanceReport, MediaPayload, OcrJob,
    OcrQueueRecovery, MEDIA_ORPHAN_GRACE_MILLIS,
};
pub(crate) use offsite_backup::{OffsiteBackupConfig, SaveOffsiteBackupConfigInput};
pub(crate) use offsite_media::{
    OffsiteMediaAttemptOutcome, OffsiteMediaCandidate, OffsiteMediaReconciliationReport,
    OffsiteMediaSummary, RecordOffsiteMediaAttemptInput,
};
pub use paths::DatabasePaths;
pub use queue::{ReviewQueueSelection, SelectNextReviewCardInput};
pub(crate) use settings::validate_complete_bindings;
pub use settings::{
    AdoptLegacyZoomInput, DaraCommand, KeyboardBinding, SetAppearanceInput,
    SetKeyboardBindingsInput, SetZoomPercentInput, StoredSettings, DEFAULT_HOME_ACCELERATOR,
    DEFAULT_QUICK_ADD_ACCELERATOR,
};
pub use stats::{HomeStats, LoadHomeStatsInput};
pub use writer::DatabaseClient;
use writer::WriterMessage;

use connection::{DatabaseKind, FileState};

pub(crate) fn open_media_read_only(path: &Path) -> Result<rusqlite::Connection> {
    connection::open_read_only(path, DatabaseKind::Media)
}

#[cfg(test)]
const TEST_MEDIA_LEASE_ID: &str = "01980c8e-6c00-7000-8000-000000000901";

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
    fn create_card_content(&self, input: CardContentDraft) -> Result<ReviewContext> {
        self.client
            .create_card_content(input, TEST_MEDIA_LEASE_ID.into())
    }

    #[cfg(test)]
    fn update_card_content(&self, input: UpdateCardContentInput) -> Result<CardContentListItem> {
        self.client
            .update_card_content(input, TEST_MEDIA_LEASE_ID.into())
    }

    #[cfg(test)]
    fn load_card_content(&self, card_content_id: String) -> Result<CardContentListItem> {
        self.client.load_card_content(card_content_id)
    }

    #[cfg(test)]
    fn search_card_content(
        &self,
        input: SearchCardContentInput,
    ) -> Result<Vec<CardContentListItem>> {
        self.client.search_card_content(input)
    }

    #[cfg(test)]
    fn set_card_content_suspended(
        &self,
        input: SetCardContentSuspendedInput,
    ) -> Result<CardContentListItem> {
        self.client.set_card_content_suspended(input)
    }

    #[cfg(test)]
    fn delete_card_content(&self, input: DeleteCardContentInput) -> Result<()> {
        self.client.delete_card_content(input)
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
    fn load_home_stats(&self, input: LoadHomeStatsInput) -> Result<HomeStats> {
        self.client.load_home_stats(input)
    }

    #[cfg(test)]
    fn load_scheduler_replay_snapshot(&self) -> Result<SchedulerReplaySnapshot> {
        self.client.load_scheduler_replay_snapshot()
    }

    #[cfg(test)]
    fn prepare_desired_retention_replay(
        &self,
        input: PrepareDesiredRetentionReplayInput,
    ) -> Result<SchedulerReplaySnapshot> {
        self.client.prepare_desired_retention_replay(input)
    }

    #[cfg(test)]
    fn install_scheduler_replay(
        &self,
        input: InstallSchedulerReplayInput,
    ) -> Result<SchedulerReplayInstallReport> {
        self.client.install_scheduler_replay(input)
    }

    #[cfg(test)]
    fn load_settings(&self) -> Result<StoredSettings> {
        self.client.load_settings()
    }

    #[cfg(test)]
    fn set_appearance(&self, input: SetAppearanceInput) -> Result<StoredSettings> {
        self.client.set_appearance(input)
    }

    #[cfg(test)]
    fn set_zoom_percent(&self, input: SetZoomPercentInput) -> Result<StoredSettings> {
        self.client.set_zoom_percent(input)
    }

    #[cfg(test)]
    fn adopt_legacy_zoom(&self, input: AdoptLegacyZoomInput) -> Result<StoredSettings> {
        self.client.adopt_legacy_zoom(input)
    }

    #[cfg(test)]
    fn set_keyboard_bindings(&self, input: SetKeyboardBindingsInput) -> Result<StoredSettings> {
        self.client.set_keyboard_bindings(input)
    }

    #[cfg(test)]
    fn ingest_image(&self, image: CanonicalImage) -> Result<ImageRecord> {
        self.client.ingest_image(image, TEST_MEDIA_LEASE_ID.into())
    }

    #[cfg(test)]
    fn claim_next_ocr_job(&self, now: i64) -> Result<Option<OcrJob>> {
        self.client.claim_next_ocr_job(now)
    }

    #[cfg(test)]
    fn complete_image_ocr(
        &self,
        job: &OcrJob,
        result: std::result::Result<String, String>,
        now: i64,
    ) -> Result<()> {
        self.client
            .complete_image_ocr(job.image_id.clone(), job.attempt_count, result, now)
    }

    #[cfg(test)]
    fn recover_interrupted_ocr_jobs(
        &self,
        stale_started_at_or_before: i64,
        now: i64,
    ) -> Result<OcrQueueRecovery> {
        self.client
            .recover_interrupted_ocr_jobs(stale_started_at_or_before, now)
    }

    #[cfg(test)]
    fn maintain_media(&self, now: i64, grace_millis: i64) -> Result<MediaMaintenanceReport> {
        self.client.maintain_media(now, grace_millis)
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
        snapshot::create_migration_safety_snapshot_pair(&paths, application_version)?;
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
        let snapshot_client = client.clone();
        Some(
            thread::Builder::new()
                .name("dara-launch-snapshot".into())
                .spawn(move || {
                    let snapshot = snapshot_client.create_snapshot_pair(version)?;
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
    mut media: Connection,
    paths: DatabasePaths,
    receiver: Receiver<WriterMessage>,
) {
    for message in receiver {
        match message {
            WriterMessage::CreateCardContent {
                input,
                media_lease_id,
                reply,
            } => {
                let _ = reply.send(domain::create_card_content(
                    &mut main,
                    input,
                    &media_lease_id,
                ));
            }
            WriterMessage::UpdateCardContent {
                input,
                media_lease_id,
                reply,
            } => {
                let _ = reply.send(domain::update_card_content(
                    &mut main,
                    input,
                    &media_lease_id,
                ));
            }
            WriterMessage::LoadCardContent {
                card_content_id,
                reply,
            } => {
                let _ = reply.send(domain::load_card_content_list_item(&main, &card_content_id));
            }
            WriterMessage::SearchCardContent { input, reply } => {
                let _ = reply.send(domain::search_card_content(&mut main, input, None));
            }
            WriterMessage::HybridSearchCardContent {
                input,
                query_embedding,
                reply,
            } => {
                let _ = reply.send(domain::search_card_content(
                    &mut main,
                    input,
                    Some(query_embedding),
                ));
            }
            WriterMessage::LoadEmbeddingReconciliationBatch { limit, reply } => {
                let result = main
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
                    .map_err(Into::into)
                    .and_then(|transaction| {
                        embedding_index::load_reconciliation_batch(&transaction, limit)
                    });
                let _ = reply.send(result);
            }
            WriterMessage::InstallTextEmbedding {
                document,
                embedding,
                reply,
            } => {
                let result = (|| {
                    let transaction =
                        main.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    let disposition =
                        embedding_index::install_embedding(&transaction, &document, &embedding)?;
                    transaction.commit()?;
                    Ok(disposition)
                })();
                let _ = reply.send(result);
            }
            WriterMessage::LoadEmbeddingIndexProgress { reply } => {
                let result = main
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
                    .map_err(Into::into)
                    .and_then(|transaction| embedding_index::index_progress(&transaction));
                let _ = reply.send(result);
            }
            WriterMessage::LoadDatabaseDiagnostics { reply } => {
                let _ = reply.send(diagnostics::load_database_diagnostics(
                    &mut main, &mut media,
                ));
            }
            WriterMessage::ActivateEmbeddingIndexIfComplete { reply } => {
                let result = (|| {
                    let transaction =
                        main.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    let activated = embedding_index::activate_index_if_complete(&transaction)?;
                    transaction.commit()?;
                    Ok(activated)
                })();
                let _ = reply.send(result);
            }
            WriterMessage::MaintainSearch { operation, reply } => {
                let result = (|| {
                    let transaction =
                        main.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    let report = embedding_index::maintain_search(&transaction, operation)?;
                    transaction.commit()?;
                    Ok(report)
                })();
                let _ = reply.send(result);
            }
            WriterMessage::SetCardContentSuspended { input, reply } => {
                let _ = reply.send(domain::set_card_content_suspended(&mut main, input));
            }
            WriterMessage::DeleteCardContent { input, reply } => {
                let _ = reply.send(domain::delete_card_content(&mut main, input));
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
            WriterMessage::LoadHomeStats { input, reply } => {
                let _ = reply.send(stats::load_home_stats(&main, input));
            }
            WriterMessage::LoadSchedulerReplaySnapshot { reply } => {
                let _ = reply.send(domain::load_scheduler_replay_snapshot(&main));
            }
            WriterMessage::PrepareDesiredRetentionReplay { input, reply } => {
                let _ = reply.send(domain::prepare_desired_retention_replay(&main, input));
            }
            WriterMessage::InstallSchedulerReplay { input, reply } => {
                let _ = reply.send(domain::install_scheduler_replay(&mut main, input));
            }
            WriterMessage::LoadSettings { reply } => {
                let _ = reply.send(settings::load_settings(&main));
            }
            WriterMessage::SetAppearance { input, reply } => {
                let _ = reply.send(settings::set_appearance(&mut main, input));
            }
            WriterMessage::SetZoomPercent { input, reply } => {
                let _ = reply.send(settings::set_zoom_percent(&mut main, input));
            }
            WriterMessage::AdoptLegacyZoom { input, reply } => {
                let _ = reply.send(settings::adopt_legacy_zoom(&mut main, input));
            }
            WriterMessage::SetKeyboardBindings { input, reply } => {
                let _ = reply.send(settings::set_keyboard_bindings(&mut main, input));
            }
            WriterMessage::LoadOffsiteBackupConfig { reply } => {
                let _ = reply.send(offsite_backup::load(&main));
            }
            WriterMessage::SaveOffsiteBackupConfig { input, reply } => {
                let _ = reply.send(offsite_backup::save(&mut main, &media, input));
            }
            WriterMessage::ReconcileOffsiteMedia { now, reply } => {
                let _ = reply.send(offsite_media::reconcile(&mut main, &media, now));
            }
            WriterMessage::LoadNextOffsiteMedia {
                backup_set_id,
                now,
                reply,
            } => {
                let _ = reply.send(offsite_media::load_next(&main, &backup_set_id, now));
            }
            WriterMessage::RecordOffsiteMediaAttempt { input, reply } => {
                let _ = reply.send(offsite_media::record_attempt(&mut main, input));
            }
            WriterMessage::LoadOffsiteMediaSummary {
                backup_set_id,
                reply,
            } => {
                let _ = reply.send(offsite_media::summary(&main, &backup_set_id));
            }
            WriterMessage::ReleaseOffsiteMediaRetries {
                backup_set_id,
                now,
                reply,
            } => {
                let _ = reply.send(offsite_media::release_transient_retries(
                    &mut main,
                    &backup_set_id,
                    now,
                ));
            }
            WriterMessage::RequeueOffsiteMediaCredentialFailures {
                backup_set_id,
                now,
                reply,
            } => {
                let _ = reply.send(offsite_media::requeue_credential_failures(
                    &mut main,
                    &backup_set_id,
                    now,
                ));
            }
            WriterMessage::IngestImage {
                image,
                lease_id,
                reply,
            } => {
                let _ = reply.send(media::ingest_image(&mut main, &mut media, image, &lease_id));
            }
            WriterMessage::RenewMediaLease {
                lease_id,
                now,
                reply,
            } => {
                let _ = reply.send(media::renew_media_lease(&mut main, &lease_id, now));
            }
            WriterMessage::MaintainMedia {
                now,
                grace_millis,
                reply,
            } => {
                let _ = reply.send(media::maintain_media(
                    &mut main,
                    &mut media,
                    now,
                    grace_millis,
                ));
            }
            WriterMessage::LoadMediaPayload { image_id, reply } => {
                let _ = reply.send(media::load_media_payload(&main, &media, &image_id));
            }
            WriterMessage::ClaimNextOcrJob { now, reply } => {
                let _ = reply.send(media::claim_next_ocr_job(&mut main, &media, now));
            }
            WriterMessage::CompleteImageOcr {
                image_id,
                expected_attempt_count,
                result,
                now,
                reply,
            } => {
                let _ = reply.send(media::complete_image_ocr(
                    &mut main,
                    &image_id,
                    expected_attempt_count,
                    result,
                    now,
                ));
            }
            WriterMessage::RecoverInterruptedOcrJobs {
                stale_started_at_or_before,
                now,
                reply,
            } => {
                let _ = reply.send(media::recover_interrupted_ocr_jobs(
                    &mut main,
                    stale_started_at_or_before,
                    now,
                ));
            }
            WriterMessage::CreateSnapshotPair {
                application_version,
                reply,
            } => {
                let _ = reply.send(snapshot::create_snapshot_pair_from_connections(
                    &paths,
                    &application_version,
                    &mut main,
                    &mut media,
                ));
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
mod settings_tests;
#[cfg(test)]
mod tests;
