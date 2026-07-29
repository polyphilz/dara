use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use super::{
    credentials::{CredentialStore, MacOsKeychainCredentialStore},
    domain::{
        BackupErrorCode, BackupSetId, ContentSha256, MediaBackupPhase, R2Keyspace,
        OBJECT_FORMAT_VERSION,
    },
    object_store::{
        GetObjectResult, ObjectContentType, ObjectMetadata, ObjectStore, ObjectStoreErrorCode,
        PutCondition, PutObjectOutcome, PutObjectRequest, R2ObjectStore, MAX_OBJECT_BYTES,
    },
    remote_authority::{map_credential_error, map_store_error, validate_backup_identity},
};
use crate::database::{
    now_millis, DatabaseClient, OffsiteBackupConfig, OffsiteMediaAttemptOutcome,
    OffsiteMediaCandidate, OffsiteMediaSummary, RecordOffsiteMediaAttemptInput,
};

const WORK_POLL_INTERVAL: Duration = Duration::from_secs(30);
const FULL_RECONCILIATION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MAX_OBJECTS_PER_PASS: usize = 64;
const BASE_RETRY_DELAY: Duration = Duration::from_secs(30);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaBackupStatus {
    pub(crate) phase: MediaBackupPhase,
    pub(crate) pending_count: u64,
    pub(crate) pending_bytes: u64,
    pub(crate) retry_wait_count: u64,
    pub(crate) verified_count: u64,
    pub(crate) verified_bytes: u64,
    pub(crate) blocked_count: u64,
    pub(crate) last_error_code: Option<BackupErrorCode>,
}

impl Default for MediaBackupStatus {
    fn default() -> Self {
        Self {
            phase: MediaBackupPhase::Off,
            pending_count: 0,
            pending_bytes: 0,
            retry_wait_count: 0,
            verified_count: 0,
            verified_bytes: 0,
            blocked_count: 0,
            last_error_code: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerSignal {
    WorkAvailable,
    ReloadConfiguration,
    ConnectivityRestored,
    Shutdown,
}

pub(crate) struct MediaBackupCoordinator {
    sender: mpsc::Sender<WorkerSignal>,
    cancellation: Arc<WorkCancellation>,
    status: Arc<Mutex<MediaBackupStatus>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
pub(crate) struct MediaBackupHandle {
    sender: mpsc::Sender<WorkerSignal>,
    cancellation: Arc<WorkCancellation>,
    status: Arc<Mutex<MediaBackupStatus>>,
}

impl MediaBackupHandle {
    pub(crate) fn wake(&self) {
        if self.cancellation.shutdown.load(Ordering::Acquire) {
            return;
        }
        let _ = self.sender.send(WorkerSignal::WorkAvailable);
    }

    pub(crate) fn status(&self) -> MediaBackupStatus {
        lock_status(&self.status).clone()
    }
}

impl MediaBackupCoordinator {
    pub(crate) fn start(client: DatabaseClient, media_path: PathBuf) -> Self {
        Self::start_with_parts(
            client,
            Arc::new(SqliteMediaBlobSource::new(media_path)),
            Arc::new(KeychainR2ObjectStoreFactory {
                credentials: MacOsKeychainCredentialStore,
            }),
            WorkerSchedule::production(),
        )
    }

    fn start_with_parts(
        client: DatabaseClient,
        source: Arc<dyn MediaBlobSource>,
        factory: Arc<dyn ObjectStoreFactory>,
        schedule: WorkerSchedule,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let cancellation = Arc::new(WorkCancellation::default());
        let status = Arc::new(Mutex::new(MediaBackupStatus::default()));
        let worker_cancellation = Arc::clone(&cancellation);
        let worker_status = Arc::clone(&status);
        let spawned = thread::Builder::new()
            .name("dara-offsite-media".into())
            .spawn(move || {
                media_worker(
                    client,
                    source,
                    factory,
                    receiver,
                    worker_cancellation,
                    worker_status,
                    schedule,
                );
            });
        let thread = match spawned {
            Ok(thread) => Some(thread),
            Err(_) => {
                *lock_status(&status) = MediaBackupStatus {
                    phase: MediaBackupPhase::Unavailable,
                    last_error_code: Some(BackupErrorCode::WorkerUnavailable),
                    ..MediaBackupStatus::default()
                };
                None
            }
        };
        let coordinator = Self {
            sender,
            cancellation,
            status,
            thread: Mutex::new(thread),
        };
        coordinator.wake();
        coordinator
    }

    pub(crate) fn wake(&self) {
        self.signal(WorkerSignal::WorkAvailable);
    }

    pub(crate) fn checkpoint_handle(&self) -> MediaBackupHandle {
        MediaBackupHandle {
            sender: self.sender.clone(),
            cancellation: Arc::clone(&self.cancellation),
            status: Arc::clone(&self.status),
        }
    }

    pub(crate) fn reload_configuration(&self) {
        self.cancellation.reconfigure.store(true, Ordering::Release);
        self.signal(WorkerSignal::ReloadConfiguration);
    }

    pub(crate) fn connectivity_restored(&self) {
        self.signal(WorkerSignal::ConnectivityRestored);
    }

    pub(crate) fn status(&self) -> MediaBackupStatus {
        lock_status(&self.status).clone()
    }

    pub(crate) fn shutdown(&self) {
        if !self.request_shutdown() {
            return;
        }
        if let Some(worker) = self.take_worker() {
            reap_worker_in_background(worker);
        }
    }

    fn request_shutdown(&self) -> bool {
        if self.cancellation.shutdown.swap(true, Ordering::SeqCst) {
            return false;
        }
        let _ = self.sender.send(WorkerSignal::Shutdown);
        true
    }

    fn take_worker(&self) -> Option<JoinHandle<()>> {
        self.thread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    #[cfg(test)]
    fn shutdown_and_wait(&self) {
        self.request_shutdown();
        if let Some(worker) = self.take_worker() {
            join_worker(worker);
        }
    }

    fn signal(&self, signal: WorkerSignal) {
        if self.cancellation.shutdown.load(Ordering::Acquire) {
            return;
        }
        match self.sender.send(signal) {
            Ok(()) => {}
            Err(mpsc::SendError(_)) => {
                set_unavailable(&self.status, BackupErrorCode::WorkerUnavailable);
            }
        }
    }
}

fn reap_worker_in_background(worker: JoinHandle<()>) {
    if let Err(error) = thread::Builder::new()
        .name("dara-offsite-media-reaper".into())
        .spawn(move || join_worker(worker))
    {
        log::error!("could not start off-site media worker reaper: {error}");
    }
}

fn join_worker(worker: JoinHandle<()>) {
    if worker.join().is_err() {
        log::error!("off-site media worker panicked during shutdown");
    }
}

#[derive(Default)]
struct WorkCancellation {
    shutdown: AtomicBool,
    reconfigure: AtomicBool,
}

impl WorkCancellation {
    fn cancelled(&self) -> bool {
        self.shutdown.load(Ordering::Acquire) || self.reconfigure.load(Ordering::Acquire)
    }
}

impl Drop for MediaBackupCoordinator {
    fn drop(&mut self) {
        self.shutdown();
    }
}

trait ObjectStoreFactory: Send + Sync {
    fn open(&self, config: &OffsiteBackupConfig) -> Result<Arc<dyn ObjectStore>, BackupErrorCode>;
}

struct KeychainR2ObjectStoreFactory<C> {
    credentials: C,
}

impl<C: CredentialStore> ObjectStoreFactory for KeychainR2ObjectStoreFactory<C> {
    fn open(&self, config: &OffsiteBackupConfig) -> Result<Arc<dyn ObjectStore>, BackupErrorCode> {
        let credentials = self
            .credentials
            .load(&config.backup_set_id)
            .map_err(map_credential_error)?;
        let store = R2ObjectStore::new(config.target.clone(), &credentials)
            .map_err(|_| BackupErrorCode::InvalidTarget)?;
        let store: Arc<dyn ObjectStore> = Arc::new(store);
        validate_backup_identity(store.as_ref(), config)?;
        Ok(store)
    }
}

trait MediaBlobSource: Send + Sync {
    fn load(&self, sha256: ContentSha256) -> Result<Option<Vec<u8>>, BackupErrorCode>;
}

struct SqliteMediaBlobSource {
    path: PathBuf,
}

impl SqliteMediaBlobSource {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl MediaBlobSource for SqliteMediaBlobSource {
    fn load(&self, sha256: ContentSha256) -> Result<Option<Vec<u8>>, BackupErrorCode> {
        let connection = crate::database::open_media_read_only(&self.path)
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        let byte_length = connection
            .query_row(
                "SELECT length(bytes) FROM media_blob WHERE sha256 = ?1",
                [sha256.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        let Some(byte_length) = byte_length else {
            return Ok(None);
        };
        let byte_length =
            usize::try_from(byte_length).map_err(|_| BackupErrorCode::LocalMediaTooLarge)?;
        if byte_length == 0 || byte_length > MAX_OBJECT_BYTES {
            return Err(BackupErrorCode::LocalMediaTooLarge);
        }
        let bytes = connection
            .query_row(
                "SELECT bytes FROM media_blob WHERE sha256 = ?1",
                [sha256.as_bytes().as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        match bytes {
            Some(bytes) if bytes.len() == byte_length => Ok(Some(bytes)),
            Some(_) => Err(BackupErrorCode::LocalMediaHashMismatch),
            None => Ok(None),
        }
    }
}

struct ActiveTarget {
    revision: i64,
    backup_set_id: BackupSetId,
    keyspace: R2Keyspace,
    store: Arc<dyn ObjectStore>,
}

#[derive(Clone, Copy)]
struct WorkerSchedule {
    poll_interval: Duration,
    full_reconciliation_interval: Duration,
    max_objects_per_pass: usize,
    retry_policy: RetryPolicy,
}

impl WorkerSchedule {
    const fn production() -> Self {
        Self {
            poll_interval: WORK_POLL_INTERVAL,
            full_reconciliation_interval: FULL_RECONCILIATION_INTERVAL,
            max_objects_per_pass: MAX_OBJECTS_PER_PASS,
            retry_policy: RetryPolicy {
                base: BASE_RETRY_DELAY,
                maximum: MAX_RETRY_DELAY,
            },
        }
    }
}

fn media_worker(
    client: DatabaseClient,
    source: Arc<dyn MediaBlobSource>,
    factory: Arc<dyn ObjectStoreFactory>,
    receiver: mpsc::Receiver<WorkerSignal>,
    cancellation: Arc<WorkCancellation>,
    status: Arc<Mutex<MediaBackupStatus>>,
    schedule: WorkerSchedule,
) {
    let mut active: Option<ActiveTarget> = None;
    let mut attempted_revision: Option<i64> = None;
    let mut next_full_reconciliation = Instant::now();
    let mut missing_local_blobs = 0;
    loop {
        if cancellation.shutdown.load(Ordering::Acquire) {
            break;
        }
        let signal = match receiver.recv_timeout(schedule.poll_interval) {
            Ok(signal) => Some(signal),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if matches!(signal, Some(WorkerSignal::Shutdown)) {
            break;
        }
        let reload = matches!(signal, Some(WorkerSignal::ReloadConfiguration));
        let connectivity_restored = matches!(signal, Some(WorkerSignal::ConnectivityRestored));
        if reload {
            cancellation.reconfigure.store(false, Ordering::Release);
            active = None;
            attempted_revision = None;
            missing_local_blobs = 0;
        }

        let config = match client.load_offsite_backup_runtime_config() {
            Ok(config) => config,
            Err(_) => {
                set_unavailable(&status, BackupErrorCode::WorkerUnavailable);
                continue;
            }
        };
        let Some(config) = config else {
            active = None;
            attempted_revision = None;
            missing_local_blobs = 0;
            *lock_status(&status) = MediaBackupStatus::default();
            continue;
        };

        let active_matches = active
            .as_ref()
            .is_some_and(|target| target.revision == config.revision);
        if !active_matches {
            active = None;
            if attempted_revision == Some(config.revision) {
                continue;
            }
            attempted_revision = Some(config.revision);
            match factory.open(&config) {
                Ok(store) => {
                    active = Some(ActiveTarget {
                        revision: config.revision,
                        backup_set_id: config.backup_set_id.clone(),
                        keyspace: config.target.keyspace(),
                        store,
                    });
                    if reload {
                        if let Ok(now) = now_millis() {
                            let _ = client.requeue_offsite_media_credential_failures(
                                config.backup_set_id.clone(),
                                now,
                            );
                        }
                    }
                    next_full_reconciliation = Instant::now();
                }
                Err(error_code) => {
                    if should_retry_target_initialization(error_code) {
                        attempted_revision = None;
                    }
                    set_target_unavailable(&status, error_code);
                    continue;
                }
            }
        }
        let Some(active) = active.as_ref() else {
            continue;
        };

        if connectivity_restored {
            if let Ok(now) = now_millis() {
                let _ = client.release_offsite_media_retries(active.backup_set_id.clone(), now);
            }
        }
        if Instant::now() >= next_full_reconciliation {
            let now = match now_millis() {
                Ok(now) => now,
                Err(_) => {
                    set_unavailable(&status, BackupErrorCode::WorkerUnavailable);
                    continue;
                }
            };
            match client.reconcile_offsite_media(now) {
                Ok(report) => {
                    missing_local_blobs = report.missing_local_blobs;
                }
                Err(_) => {
                    set_unavailable(&status, BackupErrorCode::WorkerUnavailable);
                    continue;
                }
            }
            next_full_reconciliation = Instant::now() + schedule.full_reconciliation_interval;
        }

        let pass = process_media_pass(
            &client,
            source.as_ref(),
            active,
            &cancellation,
            schedule,
            &status,
        );
        match pass {
            PassDisposition::Shutdown => break,
            PassDisposition::Reconfigured => continue,
            PassDisposition::Complete => {}
        }
        refresh_status(&client, &active.backup_set_id, missing_local_blobs, &status);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PassDisposition {
    Complete,
    Reconfigured,
    Shutdown,
}

fn process_media_pass(
    client: &DatabaseClient,
    source: &dyn MediaBlobSource,
    active: &ActiveTarget,
    cancellation: &WorkCancellation,
    schedule: WorkerSchedule,
    status: &Mutex<MediaBackupStatus>,
) -> PassDisposition {
    for _ in 0..schedule.max_objects_per_pass {
        if cancellation.shutdown.load(Ordering::Acquire) {
            return PassDisposition::Shutdown;
        }
        if cancellation.reconfigure.load(Ordering::Acquire) {
            return PassDisposition::Reconfigured;
        }
        let now = match now_millis() {
            Ok(now) => now,
            Err(_) => {
                set_unavailable(status, BackupErrorCode::WorkerUnavailable);
                return PassDisposition::Complete;
            }
        };
        let candidate = match client.load_next_offsite_media(active.backup_set_id.clone(), now) {
            Ok(Some(candidate)) => candidate,
            Ok(None) => return PassDisposition::Complete,
            Err(_) => {
                set_unavailable(status, BackupErrorCode::WorkerUnavailable);
                return PassDisposition::Complete;
            }
        };
        lock_status(status).phase = MediaBackupPhase::Uploading;
        let outcome = reconcile_media_object(
            active.store.as_ref(),
            &active.keyspace,
            &candidate,
            source,
            cancellation,
        );
        let attempted_at = match now_millis() {
            Ok(now) => now,
            Err(_) => {
                set_unavailable(status, BackupErrorCode::WorkerUnavailable);
                return PassDisposition::Complete;
            }
        };
        let (database_outcome, stop_pass) = match outcome {
            MediaAttempt::Verified => (OffsiteMediaAttemptOutcome::Verified, false),
            MediaAttempt::Retry(error_code) => {
                let next_attempt_at = schedule.retry_policy.next_attempt_at(
                    attempted_at,
                    candidate.attempt_count.saturating_add(1),
                    candidate.sha256,
                );
                (
                    OffsiteMediaAttemptOutcome::RetryWait {
                        error_code,
                        next_attempt_at,
                    },
                    true,
                )
            }
            MediaAttempt::Blocked(error_code) => (
                OffsiteMediaAttemptOutcome::Blocked { error_code },
                error_code.blocks_all_media(),
            ),
            MediaAttempt::Cancelled => {
                return if cancellation.shutdown.load(Ordering::Acquire) {
                    PassDisposition::Shutdown
                } else {
                    PassDisposition::Reconfigured
                };
            }
        };
        if client
            .record_offsite_media_attempt(RecordOffsiteMediaAttemptInput {
                backup_set_id: candidate.backup_set_id,
                sha256: candidate.sha256,
                expected_attempt_count: candidate.attempt_count,
                attempted_at,
                outcome: database_outcome,
            })
            .is_err()
        {
            set_unavailable(status, BackupErrorCode::WorkerUnavailable);
            return PassDisposition::Complete;
        }
        if stop_pass {
            return PassDisposition::Complete;
        }
    }
    PassDisposition::Complete
}

fn refresh_status(
    client: &DatabaseClient,
    backup_set_id: &BackupSetId,
    missing_local_blobs: u64,
    status: &Mutex<MediaBackupStatus>,
) {
    let summary = match client.load_offsite_media_summary(backup_set_id.clone()) {
        Ok(summary) => summary,
        Err(_) => {
            set_unavailable(status, BackupErrorCode::WorkerUnavailable);
            return;
        }
    };
    *lock_status(status) = status_from_summary(summary, missing_local_blobs);
}

fn status_from_summary(
    summary: OffsiteMediaSummary,
    missing_local_blobs: u64,
) -> MediaBackupStatus {
    let (phase, last_error_code) = if missing_local_blobs > 0 {
        (
            MediaBackupPhase::Blocked,
            Some(BackupErrorCode::LocalMediaMissing),
        )
    } else if summary.blocked_count > 0 {
        (MediaBackupPhase::Blocked, summary.last_error_code)
    } else if summary.retry_wait_count > 0 {
        (MediaBackupPhase::RetryWait, summary.last_error_code)
    } else if summary.pending_count > 0 {
        (MediaBackupPhase::Uploading, summary.last_error_code)
    } else {
        (MediaBackupPhase::Idle, None)
    };
    MediaBackupStatus {
        phase,
        pending_count: summary.pending_count,
        pending_bytes: summary.pending_bytes,
        retry_wait_count: summary.retry_wait_count,
        verified_count: summary.verified_count,
        verified_bytes: summary.verified_bytes,
        blocked_count: summary.blocked_count.saturating_add(missing_local_blobs),
        last_error_code,
    }
}

fn set_target_unavailable(status: &Mutex<MediaBackupStatus>, error_code: BackupErrorCode) {
    let phase = if is_retryable(error_code) {
        MediaBackupPhase::RetryWait
    } else {
        match error_code {
            BackupErrorCode::KeychainCredentialMissing => MediaBackupPhase::WaitingForCredentials,
            BackupErrorCode::KeychainUnavailable | BackupErrorCode::WorkerUnavailable => {
                MediaBackupPhase::Unavailable
            }
            _ => MediaBackupPhase::Blocked,
        }
    };
    *lock_status(status) = MediaBackupStatus {
        phase,
        last_error_code: Some(error_code),
        ..MediaBackupStatus::default()
    };
}

fn set_unavailable(status: &Mutex<MediaBackupStatus>, error_code: BackupErrorCode) {
    *lock_status(status) = MediaBackupStatus {
        phase: MediaBackupPhase::Unavailable,
        last_error_code: Some(error_code),
        ..MediaBackupStatus::default()
    };
}

fn lock_status(status: &Mutex<MediaBackupStatus>) -> std::sync::MutexGuard<'_, MediaBackupStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    base: Duration,
    maximum: Duration,
}

impl RetryPolicy {
    fn next_attempt_at(self, attempted_at: i64, attempt_count: u32, sha256: ContentSha256) -> i64 {
        let exponent = attempt_count.saturating_sub(1).min(20);
        let multiplier = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
        let delay = self.base.saturating_mul(multiplier);
        let delay_millis = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
        let maximum_millis = i64::try_from(self.maximum.as_millis()).unwrap_or(i64::MAX);
        let jitter_window = delay_millis / 4;
        let mut seed_bytes = [0_u8; 8];
        seed_bytes.copy_from_slice(&sha256.as_bytes()[..8]);
        let seed = u64::from_be_bytes(seed_bytes) ^ u64::from(attempt_count);
        let jitter = if jitter_window == 0 {
            0
        } else {
            i64::try_from(seed % (jitter_window as u64 + 1)).unwrap_or(0)
        };
        attempted_at.saturating_add(delay_millis.saturating_add(jitter).min(maximum_millis))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MediaAttempt {
    Verified,
    Retry(BackupErrorCode),
    Blocked(BackupErrorCode),
    Cancelled,
}

fn reconcile_media_object(
    store: &dyn ObjectStore,
    keyspace: &R2Keyspace,
    candidate: &OffsiteMediaCandidate,
    source: &dyn MediaBlobSource,
    cancellation: &WorkCancellation,
) -> MediaAttempt {
    if cancellation.cancelled() {
        return MediaAttempt::Cancelled;
    }
    if candidate.byte_length == 0 || candidate.byte_length > MAX_OBJECT_BYTES as u64 {
        return MediaAttempt::Blocked(BackupErrorCode::LocalMediaTooLarge);
    }
    let bytes = match source.load(candidate.sha256) {
        Ok(Some(bytes)) => Some(bytes),
        Ok(None) => None,
        Err(BackupErrorCode::WorkerUnavailable) => {
            return MediaAttempt::Retry(BackupErrorCode::WorkerUnavailable);
        }
        Err(error_code) => return MediaAttempt::Blocked(error_code),
    };
    if let Some(bytes) = bytes.as_ref() {
        if bytes.len() as u64 != candidate.byte_length
            || ContentSha256::from_bytes(Sha256::digest(bytes).into()) != candidate.sha256
        {
            return MediaAttempt::Blocked(BackupErrorCode::LocalMediaHashMismatch);
        }
    }
    if cancellation.cancelled() {
        return MediaAttempt::Cancelled;
    }

    let key = keyspace.media(candidate.sha256);
    let head = match store.head(&key) {
        Ok(head) => head,
        Err(error) => return classify_store_error(error.code),
    };
    if let Some(metadata) = head {
        if let Err(outcome) = validate_remote_metadata(&metadata, candidate) {
            return outcome;
        }
    } else {
        let Some(bytes) = bytes else {
            return MediaAttempt::Blocked(BackupErrorCode::LocalMediaMissing);
        };
        if cancellation.cancelled() {
            return MediaAttempt::Cancelled;
        }
        match store.put(PutObjectRequest {
            key: key.clone(),
            bytes,
            content_type: ObjectContentType::Webp,
            dara_sha256: Some(candidate.sha256),
            condition: PutCondition::IfAbsent,
        }) {
            Ok(PutObjectOutcome::Stored) => {}
            Ok(PutObjectOutcome::ConditionNotMet) => {
                let raced = match store.head(&key) {
                    Ok(Some(metadata)) => metadata,
                    Ok(None) => {
                        return MediaAttempt::Retry(BackupErrorCode::RemoteMediaMissing);
                    }
                    Err(error) => return classify_store_error(error.code),
                };
                if let Err(outcome) = validate_remote_metadata(&raced, candidate) {
                    return outcome;
                }
            }
            Err(error) => return classify_store_error(error.code),
        }
    }

    if cancellation.cancelled() {
        return MediaAttempt::Cancelled;
    }
    let downloaded = match store.get(&key) {
        Ok(downloaded) => downloaded,
        Err(error) => return classify_store_error(error.code),
    };
    verify_downloaded(downloaded, candidate)
}

fn validate_remote_metadata(
    metadata: &ObjectMetadata,
    candidate: &OffsiteMediaCandidate,
) -> Result<(), MediaAttempt> {
    if metadata.byte_length != candidate.byte_length
        || metadata
            .content_type
            .is_some_and(|content_type| content_type != ObjectContentType::Webp)
        || metadata
            .dara_sha256
            .is_some_and(|sha256| sha256 != candidate.sha256)
        || metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
    {
        return Err(MediaAttempt::Blocked(
            BackupErrorCode::ImmutableObjectConflict,
        ));
    }
    Ok(())
}

fn verify_downloaded(
    downloaded: GetObjectResult,
    candidate: &OffsiteMediaCandidate,
) -> MediaAttempt {
    if let Err(outcome) = validate_remote_metadata(&downloaded.metadata, candidate) {
        return outcome;
    }
    if downloaded.bytes.len() as u64 != candidate.byte_length
        || ContentSha256::from_bytes(Sha256::digest(&downloaded.bytes).into()) != candidate.sha256
    {
        return MediaAttempt::Blocked(BackupErrorCode::RemoteMediaCorrupt);
    }
    MediaAttempt::Verified
}

fn classify_store_error(error: ObjectStoreErrorCode) -> MediaAttempt {
    let error = map_store_error(error);
    if is_retryable(error) {
        MediaAttempt::Retry(error)
    } else {
        MediaAttempt::Blocked(error)
    }
}

fn is_retryable(error: BackupErrorCode) -> bool {
    matches!(
        error,
        BackupErrorCode::NetworkOffline
            | BackupErrorCode::NetworkTimeout
            | BackupErrorCode::RateLimited
            | BackupErrorCode::ServiceUnavailable
            | BackupErrorCode::RemoteMediaMissing
            | BackupErrorCode::WorkerUnavailable
    )
}

fn should_retry_target_initialization(error: BackupErrorCode) -> bool {
    error == BackupErrorCode::KeychainUnavailable || is_retryable(error)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::atomic::AtomicUsize};

    use super::*;
    use crate::backup::{
        credentials::R2Credentials,
        domain::{
            BackupProvider, IdentityManifestV1, InstallationId, ProbeRunId, R2AccountId,
            R2BucketName, R2Jurisdiction, R2Prefix, R2Target, ReplicaEpochId,
        },
        object_store::{
            fake::{FakeObjectStore, ObjectOperation},
            ObjectStoreErrorCode, R2ObjectStore,
        },
    };
    use crate::database::{
        initialize, CanonicalImage, DatabasePaths, InitializationOptions,
        SaveOffsiteBackupConfigInput,
    };

    struct FakeMediaBlobSource {
        bytes: Option<Vec<u8>>,
    }

    struct FakeObjectStoreFactory {
        store: Arc<FakeObjectStore>,
        opens: Arc<AtomicUsize>,
        failures: Mutex<VecDeque<BackupErrorCode>>,
    }

    impl FakeObjectStoreFactory {
        fn available(store: Arc<FakeObjectStore>, opens: Arc<AtomicUsize>) -> Self {
            Self {
                store,
                opens,
                failures: Mutex::new(VecDeque::new()),
            }
        }

        fn failing_once(
            store: Arc<FakeObjectStore>,
            opens: Arc<AtomicUsize>,
            error: BackupErrorCode,
        ) -> Self {
            Self {
                store,
                opens,
                failures: Mutex::new(VecDeque::from([error])),
            }
        }
    }

    impl ObjectStoreFactory for FakeObjectStoreFactory {
        fn open(
            &self,
            _config: &OffsiteBackupConfig,
        ) -> Result<Arc<dyn ObjectStore>, BackupErrorCode> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self
                .failures
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front()
            {
                return Err(error);
            }
            Ok(self.store.clone())
        }
    }

    impl MediaBlobSource for FakeMediaBlobSource {
        fn load(&self, _sha256: ContentSha256) -> Result<Option<Vec<u8>>, BackupErrorCode> {
            Ok(self.bytes.clone())
        }
    }

    fn target() -> R2Target {
        R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/media-tests").expect("prefix"),
        }
    }

    fn config(backup_set_id: BackupSetId, target: R2Target) -> OffsiteBackupConfig {
        OffsiteBackupConfig {
            revision: 1,
            backup_set_id,
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            provider: BackupProvider::R2,
            target,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn candidate(bytes: &[u8]) -> OffsiteMediaCandidate {
        OffsiteMediaCandidate {
            backup_set_id: BackupSetId::new(),
            sha256: ContentSha256::from_bytes(Sha256::digest(bytes).into()),
            byte_length: bytes.len() as u64,
            attempt_count: 0,
        }
    }

    #[test]
    fn missing_remote_media_is_conditionally_uploaded_then_download_verified() {
        let bytes = b"canonical-webp".to_vec();
        let candidate = candidate(&bytes);
        let store = FakeObjectStore::default();
        let result = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource { bytes: Some(bytes) },
            &WorkCancellation::default(),
        );
        assert_eq!(result, MediaAttempt::Verified);
        let repeated = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource {
                bytes: Some(b"canonical-webp".to_vec()),
            },
            &WorkCancellation::default(),
        );
        assert_eq!(repeated, MediaAttempt::Verified);
        assert_eq!(
            store.operations(),
            [
                ObjectOperation::Head,
                ObjectOperation::Put,
                ObjectOperation::Get,
                ObjectOperation::Head,
                ObjectOperation::Get,
            ]
        );
    }

    #[test]
    fn immutable_remote_conflict_is_blocked_without_overwrite() {
        let expected = b"canonical-webp".to_vec();
        let candidate = candidate(&expected);
        let store = FakeObjectStore::default();
        let key = target().keyspace().media(candidate.sha256);
        store
            .put(PutObjectRequest {
                key,
                bytes: b"different".to_vec(),
                content_type: ObjectContentType::Webp,
                dara_sha256: Some(ContentSha256::from_bytes(
                    Sha256::digest(b"different").into(),
                )),
                condition: PutCondition::IfAbsent,
            })
            .expect("seed conflicting object");
        let operations_before = store.operations().len();

        let result = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource {
                bytes: Some(expected),
            },
            &WorkCancellation::default(),
        );

        assert_eq!(
            result,
            MediaAttempt::Blocked(BackupErrorCode::ImmutableObjectConflict)
        );
        assert_eq!(
            &store.operations()[operations_before..],
            &[ObjectOperation::Head]
        );
    }

    #[test]
    fn missing_remote_format_version_is_an_immutable_conflict() {
        let bytes = b"canonical-webp".to_vec();
        let candidate = candidate(&bytes);
        let store = FakeObjectStore::default();
        let key = target().keyspace().media(candidate.sha256);
        store
            .put(PutObjectRequest {
                key: key.clone(),
                bytes: bytes.clone(),
                content_type: ObjectContentType::Webp,
                dara_sha256: Some(candidate.sha256),
                condition: PutCondition::IfAbsent,
            })
            .expect("seed remote object");
        let mut metadata = store.head(&key).expect("head").expect("remote metadata");
        metadata.object_format_version = None;

        let result = verify_downloaded(GetObjectResult { metadata, bytes }, &candidate);

        assert_eq!(
            result,
            MediaAttempt::Blocked(BackupErrorCode::ImmutableObjectConflict)
        );
    }

    #[test]
    fn local_hash_mismatch_never_touches_the_network() {
        let candidate = candidate(b"expected");
        let store = FakeObjectStore::default();
        let result = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource {
                bytes: Some(b"corrupt".to_vec()),
            },
            &WorkCancellation::default(),
        );
        assert_eq!(
            result,
            MediaAttempt::Blocked(BackupErrorCode::LocalMediaHashMismatch)
        );
        assert!(store.operations().is_empty());
    }

    #[test]
    fn transient_object_store_failures_are_retryable() {
        let bytes = b"canonical-webp".to_vec();
        let candidate = candidate(&bytes);
        let store = FakeObjectStore::default();
        store.fail_next(ObjectOperation::Head, ObjectStoreErrorCode::RateLimited);
        let result = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource { bytes: Some(bytes) },
            &WorkCancellation::default(),
        );
        assert_eq!(result, MediaAttempt::Retry(BackupErrorCode::RateLimited));
    }

    #[test]
    fn uploaded_media_can_be_verified_after_the_local_blob_is_reaped() {
        let bytes = b"canonical-webp".to_vec();
        let candidate = candidate(&bytes);
        let store = FakeObjectStore::default();
        store.fail_next(ObjectOperation::Get, ObjectStoreErrorCode::Network);

        let initial = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource { bytes: Some(bytes) },
            &WorkCancellation::default(),
        );
        assert_eq!(
            initial,
            MediaAttempt::Retry(BackupErrorCode::NetworkOffline)
        );

        let retry = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource { bytes: None },
            &WorkCancellation::default(),
        );
        assert_eq!(retry, MediaAttempt::Verified);
        assert_eq!(
            store.operations(),
            [
                ObjectOperation::Head,
                ObjectOperation::Put,
                ObjectOperation::Get,
                ObjectOperation::Head,
                ObjectOperation::Get,
            ]
        );
    }

    #[test]
    fn media_upload_requires_the_expected_remote_backup_identity() {
        let target = target();
        let backup_set_id = BackupSetId::new();
        let config = config(backup_set_id.clone(), target.clone());
        let empty = FakeObjectStore::default();
        assert_eq!(
            validate_backup_identity(&empty, &config),
            Err(BackupErrorCode::PrefixIdentityMismatch)
        );

        let matching = FakeObjectStore::default();
        let identity = IdentityManifestV1::new(backup_set_id, InstallationId::new())
            .to_json()
            .expect("identity JSON");
        matching
            .put(PutObjectRequest {
                key: target.keyspace().identity(),
                bytes: identity,
                content_type: ObjectContentType::Json,
                dara_sha256: None,
                condition: PutCondition::IfAbsent,
            })
            .expect("matching identity");
        validate_backup_identity(&matching, &config).expect("matching prefix identity");

        let mismatched = FakeObjectStore::default();
        let other_identity = IdentityManifestV1::new(BackupSetId::new(), InstallationId::new())
            .to_json()
            .expect("other identity JSON");
        mismatched
            .put(PutObjectRequest {
                key: target.keyspace().identity(),
                bytes: other_identity,
                content_type: ObjectContentType::Json,
                dara_sha256: None,
                condition: PutCondition::IfAbsent,
            })
            .expect("mismatched identity");
        assert_eq!(
            validate_backup_identity(&mismatched, &config),
            Err(BackupErrorCode::PrefixIdentityMismatch)
        );
    }

    #[test]
    fn disable_or_reconfiguration_cancels_before_network_work() {
        let bytes = b"canonical-webp".to_vec();
        let candidate = candidate(&bytes);
        let store = FakeObjectStore::default();
        let cancellation = WorkCancellation::default();
        cancellation.reconfigure.store(true, Ordering::Release);
        let result = reconcile_media_object(
            &store,
            &target().keyspace(),
            &candidate,
            &FakeMediaBlobSource { bytes: Some(bytes) },
            &cancellation,
        );
        assert_eq!(result, MediaAttempt::Cancelled);
        assert!(store.operations().is_empty());
    }

    #[test]
    fn retry_policy_is_exponential_jittered_and_capped() {
        let policy = RetryPolicy {
            base: Duration::from_secs(10),
            maximum: Duration::from_secs(60),
        };
        let hash = ContentSha256::from_bytes([0x42; 32]);
        let first = policy.next_attempt_at(1_000, 1, hash) - 1_000;
        let second = policy.next_attempt_at(1_000, 2, hash) - 1_000;
        let capped = policy.next_attempt_at(1_000, 20, hash) - 1_000;
        assert!((10_000..=12_500).contains(&first));
        assert!((20_000..=25_000).contains(&second));
        assert_eq!(capped, 60_000);
    }

    #[test]
    fn background_worker_recovers_from_offline_and_wakes_for_new_media() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path().join("data"));
        let database = initialize(
            paths.clone(),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database");
        let client = database.client();
        let target = target();
        let backup_set_id = BackupSetId::new();
        client
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: 0,
                backup_set_id: backup_set_id.clone(),
                replica_epoch_id: crate::backup::domain::ReplicaEpochId::new(),
                enabled: true,
                target,
            })
            .expect("backup config");
        client
            .ingest_image(
                CanonicalImage {
                    bytes: b"captured-while-offline".to_vec(),
                    natural_width: 10,
                    natural_height: 10,
                },
                "01980c8e-6c00-7000-8000-000000000903".into(),
            )
            .expect("offline image");

        let store = Arc::new(FakeObjectStore::default());
        let factory_opens = Arc::new(AtomicUsize::new(0));
        store.fail_next(ObjectOperation::Head, ObjectStoreErrorCode::Network);
        let coordinator = MediaBackupCoordinator::start_with_parts(
            client.clone(),
            Arc::new(SqliteMediaBlobSource::new(paths.media.clone())),
            Arc::new(FakeObjectStoreFactory::available(
                store.clone(),
                factory_opens.clone(),
            )),
            WorkerSchedule {
                poll_interval: Duration::from_millis(5),
                full_reconciliation_interval: Duration::from_millis(25),
                max_objects_per_pass: 8,
                retry_policy: RetryPolicy {
                    base: Duration::from_millis(5),
                    maximum: Duration::from_millis(20),
                },
            },
        );
        wait_until(Duration::from_secs(2), || {
            coordinator.status().verified_count == 1
        });
        assert_eq!(coordinator.status().phase, MediaBackupPhase::Idle);

        client
            .ingest_image(
                CanonicalImage {
                    bytes: b"captured-after-reconnect".to_vec(),
                    natural_width: 11,
                    natural_height: 11,
                },
                "01980c8e-6c00-7000-8000-000000000904".into(),
            )
            .expect("connected image");
        coordinator.wake();
        wait_until(Duration::from_secs(2), || {
            coordinator.status().verified_count == 2
        });
        coordinator.shutdown_and_wait();

        let summary = client
            .load_offsite_media_summary(backup_set_id)
            .expect("final summary");
        assert_eq!(summary.verified_count, 2);
        assert_eq!(summary.pending_count, 0);
        assert!(
            store
                .operations()
                .iter()
                .filter(|operation| **operation == ObjectOperation::Put)
                .count()
                >= 2
        );
        assert_eq!(factory_opens.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn background_worker_retries_after_keychain_becomes_available() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path().join("data"));
        let database = initialize(
            paths.clone(),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database");
        let client = database.client();
        let backup_set_id = BackupSetId::new();
        client
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: 0,
                backup_set_id: backup_set_id.clone(),
                replica_epoch_id: ReplicaEpochId::new(),
                enabled: true,
                target: target(),
            })
            .expect("backup config");
        client
            .ingest_image(
                CanonicalImage {
                    bytes: b"captured-during-keychain-outage".to_vec(),
                    natural_width: 10,
                    natural_height: 10,
                },
                "01980c8e-6c00-7000-8000-000000000906".into(),
            )
            .expect("queued image");

        let store = Arc::new(FakeObjectStore::default());
        let factory_opens = Arc::new(AtomicUsize::new(0));
        let coordinator = MediaBackupCoordinator::start_with_parts(
            client.clone(),
            Arc::new(SqliteMediaBlobSource::new(paths.media)),
            Arc::new(FakeObjectStoreFactory::failing_once(
                store,
                factory_opens.clone(),
                BackupErrorCode::KeychainUnavailable,
            )),
            WorkerSchedule {
                poll_interval: Duration::from_millis(5),
                full_reconciliation_interval: Duration::from_millis(25),
                max_objects_per_pass: 8,
                retry_policy: RetryPolicy {
                    base: Duration::from_millis(5),
                    maximum: Duration::from_millis(20),
                },
            },
        );
        wait_until(Duration::from_secs(2), || {
            coordinator.status().verified_count == 1
        });
        coordinator.shutdown_and_wait();

        assert_eq!(factory_opens.load(Ordering::SeqCst), 2);
        let summary = client
            .load_offsite_media_summary(backup_set_id)
            .expect("final summary");
        assert_eq!(summary.verified_count, 1);
    }

    #[test]
    fn disabled_backup_accumulates_work_without_opening_keychain_or_r2() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path().join("data"));
        let database = initialize(
            paths.clone(),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database");
        let client = database.client();
        let backup_set_id = BackupSetId::new();
        client
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: 0,
                backup_set_id: backup_set_id.clone(),
                replica_epoch_id: crate::backup::domain::ReplicaEpochId::new(),
                enabled: false,
                target: target(),
            })
            .expect("disabled backup config");
        client
            .ingest_image(
                CanonicalImage {
                    bytes: b"queued-while-disabled".to_vec(),
                    natural_width: 10,
                    natural_height: 10,
                },
                "01980c8e-6c00-7000-8000-000000000905".into(),
            )
            .expect("queued image");

        let store = Arc::new(FakeObjectStore::default());
        let factory_opens = Arc::new(AtomicUsize::new(0));
        let coordinator = MediaBackupCoordinator::start_with_parts(
            client.clone(),
            Arc::new(SqliteMediaBlobSource::new(paths.media)),
            Arc::new(FakeObjectStoreFactory::available(
                store.clone(),
                factory_opens.clone(),
            )),
            WorkerSchedule {
                poll_interval: Duration::from_millis(5),
                full_reconciliation_interval: Duration::from_millis(10),
                max_objects_per_pass: 8,
                retry_policy: RetryPolicy {
                    base: Duration::from_millis(5),
                    maximum: Duration::from_millis(20),
                },
            },
        );
        thread::sleep(Duration::from_millis(30));
        assert_eq!(coordinator.status().phase, MediaBackupPhase::Off);
        assert_eq!(factory_opens.load(Ordering::SeqCst), 0);
        assert!(store.operations().is_empty());
        assert_eq!(
            client
                .load_offsite_media_summary(backup_set_id)
                .expect("queued summary")
                .pending_count,
            1
        );
        coordinator.shutdown_and_wait();
    }

    #[test]
    fn shutdown_returns_without_waiting_for_the_worker() {
        let (sender, _receiver) = mpsc::channel();
        let cancellation = Arc::new(WorkCancellation::default());
        let status = Arc::new(Mutex::new(MediaBackupStatus::default()));
        let (release_sender, release_receiver) = mpsc::channel();
        let (worker_finished_sender, worker_finished_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            release_receiver.recv().expect("release worker");
            worker_finished_sender.send(()).expect("report worker exit");
        });
        let coordinator = Arc::new(MediaBackupCoordinator {
            sender,
            cancellation,
            status,
            thread: Mutex::new(Some(worker)),
        });
        let shutdown_coordinator = Arc::clone(&coordinator);
        let (shutdown_returned_sender, shutdown_returned_receiver) = mpsc::channel();
        let shutdown_caller = thread::spawn(move || {
            shutdown_coordinator.shutdown();
            shutdown_returned_sender
                .send(())
                .expect("report shutdown return");
        });

        let returned_while_worker_was_blocked = shutdown_returned_receiver
            .recv_timeout(Duration::from_secs(1))
            .is_ok();
        release_sender.send(()).expect("release worker");
        shutdown_caller.join().expect("shutdown caller");
        worker_finished_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("worker exit");

        assert!(
            returned_while_worker_was_blocked,
            "shutdown waited for the worker to exit"
        );
    }

    #[test]
    #[ignore = "requires app/.env.local and the disposable dara-local R2 bucket"]
    fn live_r2_media_upload_is_verified_and_confined_to_a_disposable_prefix() {
        let jurisdiction =
            R2Jurisdiction::from_db(&required_environment("DARA_LITESTREAM_R2_JURISDICTION"))
                .expect("R2 jurisdiction");
        let root_prefix = R2Prefix::parse(required_environment("DARA_LITESTREAM_R2_PREFIX"))
            .expect("root prefix");
        let target = R2Target {
            account_id: R2AccountId::parse(required_environment("DARA_LITESTREAM_R2_ACCOUNT_ID"))
                .expect("R2 account ID"),
            jurisdiction,
            bucket: R2BucketName::parse(required_environment("DARA_LITESTREAM_R2_BUCKET"))
                .expect("R2 bucket"),
            prefix: R2Prefix::parse(format!(
                "{}/media-slice3/{}",
                root_prefix.as_str(),
                ProbeRunId::new().as_str()
            ))
            .expect("disposable prefix"),
        };
        let credentials = R2Credentials::new(
            required_environment("DARA_LITESTREAM_R2_ACCESS_KEY_ID"),
            required_environment("DARA_LITESTREAM_R2_SECRET_ACCESS_KEY"),
        )
        .expect("R2 credentials");
        let store = R2ObjectStore::new(target.clone(), &credentials).expect("R2 client");
        let bytes = b"disposable-canonical-webp".to_vec();
        let candidate = candidate(&bytes);
        let config = config(candidate.backup_set_id.clone(), target.clone());
        let identity =
            IdentityManifestV1::new(candidate.backup_set_id.clone(), InstallationId::new())
                .to_json()
                .expect("identity JSON");
        store
            .put(PutObjectRequest {
                key: target.keyspace().identity(),
                bytes: identity,
                content_type: ObjectContentType::Json,
                dara_sha256: None,
                condition: PutCondition::IfAbsent,
            })
            .expect("create disposable identity");
        validate_backup_identity(&store, &config).expect("validate disposable identity");

        let result = reconcile_media_object(
            &store,
            &target.keyspace(),
            &candidate,
            &FakeMediaBlobSource {
                bytes: Some(bytes.clone()),
            },
            &WorkCancellation::default(),
        );
        assert_eq!(result, MediaAttempt::Verified);
        let repeated = reconcile_media_object(
            &store,
            &target.keyspace(),
            &candidate,
            &FakeMediaBlobSource { bytes: Some(bytes) },
            &WorkCancellation::default(),
        );
        assert_eq!(repeated, MediaAttempt::Verified);

        let page = store
            .list(&target.keyspace().root_prefix(), None)
            .expect("list disposable prefix");
        assert!(page.next.is_none());
        assert_eq!(page.objects.len(), 2);
        for object in page.objects {
            store.delete(&object.key).expect("delete disposable object");
        }
        assert!(store
            .list(&target.keyspace().root_prefix(), None)
            .expect("confirm cleanup")
            .objects
            .is_empty());
    }

    fn required_environment(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
    }

    fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if predicate() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("condition did not become true before timeout");
    }
}
