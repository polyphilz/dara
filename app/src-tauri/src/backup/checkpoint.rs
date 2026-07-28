use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use super::{
    credentials::{CredentialStore, MacOsKeychainCredentialStore, R2Credentials},
    domain::{
        BackupErrorCode, CheckpointBackupPhase, CheckpointId, CheckpointManifestInput,
        CheckpointManifestV1, ContentSha256, MediaBackupPhase, PublishedCheckpointEvidence,
        R2Keyspace, R2ObjectKey, UtcTimestamp, OBJECT_FORMAT_VERSION,
    },
    installation::InstallationIdentityStore,
    litestream::{
        configure_credentials_environment, parse_restore_plan_json, LitestreamConfig,
        LitestreamRuntimePaths, LitestreamTxid, ReplicaKind, SyncResult, VerifiedLitestreamBinary,
    },
    litestream_runtime::{LitestreamCheckpointHandle, RelationalBackupStatus},
    media_reconciliation::{MediaBackupHandle, MediaBackupStatus},
    object_store::{
        ObjectContentType, ObjectStore, PutCondition, PutObjectOutcome, PutObjectRequest,
        R2ObjectStore,
    },
    remote_authority::{
        map_credential_error, map_store_error, validate_backup_authority_with_deadline,
    },
};
use crate::database::{
    now_millis, CheckpointMediaReference, DatabaseClient, DatabaseError, LocalCheckpointSync,
    OffsiteBackupConfig, PrepareOffsiteCheckpointInput, PreparedOffsiteCheckpoint,
    PublishedOffsiteCheckpoint,
};

const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(5);
const AUTOMATIC_DEBOUNCE: Duration = Duration::from_secs(60);
const AUTOMATIC_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const AUTOMATIC_RETRY_DELAY: Duration = Duration::from_secs(30);
const REMOTE_EVIDENCE_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MEDIA_WAIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MEDIA_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CHECKPOINT_SHUTDOWN_BUDGET: Duration = Duration::from_secs(45);
const CHECKPOINT_COORDINATOR_JOIN_BUDGET: Duration = Duration::from_secs(46);
const EXACT_RESTORE_TIMEOUT: Duration = Duration::from_secs(35);
const EXACT_RESTORE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_RESTORE_PLAN_BYTES: usize = 16 * 1024 * 1024;
const CHECKPOINT_MEDIA_RACE_RETRIES: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointBackupStatus {
    pub(crate) phase: CheckpointBackupPhase,
    pub(crate) in_progress_checkpoint_id: Option<CheckpointId>,
    pub(crate) last_complete_checkpoint_id: Option<CheckpointId>,
    pub(crate) last_complete_at: Option<i64>,
    pub(crate) last_error_code: Option<BackupErrorCode>,
}

impl Default for CheckpointBackupStatus {
    fn default() -> Self {
        Self {
            phase: CheckpointBackupPhase::Off,
            in_progress_checkpoint_id: None,
            last_complete_checkpoint_id: None,
            last_complete_at: None,
            last_error_code: None,
        }
    }
}

enum CoordinatorSignal {
    WorkAvailable,
    BackupNow {
        reply: mpsc::SyncSender<Result<CheckpointId, BackupErrorCode>>,
    },
    Shutdown,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RemoteEvidenceState {
    Valid,
    Invalid,
}

trait CheckpointRuntime: Send + Sync {
    fn status(&self) -> RelationalBackupStatus;
    fn sync_local(&self, timeout: Duration) -> Result<LitestreamTxid, BackupErrorCode>;
    fn sync_remote(&self, timeout: Duration) -> Result<SyncResult, BackupErrorCode>;
}

impl CheckpointRuntime for LitestreamCheckpointHandle {
    fn status(&self) -> RelationalBackupStatus {
        LitestreamCheckpointHandle::status(self)
    }

    fn sync_local(&self, timeout: Duration) -> Result<LitestreamTxid, BackupErrorCode> {
        LitestreamCheckpointHandle::sync_local_with_timeout(self, timeout)
    }

    fn sync_remote(&self, timeout: Duration) -> Result<SyncResult, BackupErrorCode> {
        LitestreamCheckpointHandle::sync_remote_with_timeout(self, timeout)
    }
}

struct RuntimeLocalSync {
    runtime: Arc<dyn CheckpointRuntime>,
    deadline: Instant,
}

impl LocalCheckpointSync for RuntimeLocalSync {
    fn sync_local(&self) -> Result<LitestreamTxid, BackupErrorCode> {
        self.runtime
            .sync_local(remaining_checkpoint_time(self.deadline)?)
    }
}

trait CheckpointMediaWorker: Send + Sync {
    fn wake(&self);
    fn status(&self) -> MediaBackupStatus;
}

impl CheckpointMediaWorker for MediaBackupHandle {
    fn wake(&self) {
        MediaBackupHandle::wake(self);
    }

    fn status(&self) -> MediaBackupStatus {
        MediaBackupHandle::status(self)
    }
}

pub(crate) struct CheckpointCoordinator {
    sender: mpsc::Sender<CoordinatorSignal>,
    shutdown: Arc<AtomicBool>,
    status: Arc<Mutex<CheckpointBackupStatus>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl CheckpointCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        database: DatabaseClient,
        media: MediaBackupHandle,
        litestream: LitestreamCheckpointHandle,
        data_root: PathBuf,
        database_path: PathBuf,
        resource_dir: PathBuf,
        dara_version: String,
    ) -> Self {
        Self::start_with_parts(
            database,
            Arc::new(media),
            Arc::new(litestream),
            Arc::new(SystemCheckpointTargetFactory {
                data_root,
                database_path,
                resource_dir,
                credentials: MacOsKeychainCredentialStore,
            }),
            dara_version,
            CoordinatorSchedule::production(),
        )
    }

    fn start_with_parts(
        database: DatabaseClient,
        media: Arc<dyn CheckpointMediaWorker>,
        litestream: Arc<dyn CheckpointRuntime>,
        factory: Arc<dyn CheckpointTargetFactory>,
        dara_version: String,
        schedule: CoordinatorSchedule,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(CheckpointBackupStatus::default()));
        let worker_shutdown = Arc::clone(&shutdown);
        let worker_status = Arc::clone(&status);
        let spawned = thread::Builder::new()
            .name("dara-offsite-checkpoint".into())
            .spawn(move || {
                checkpoint_worker(
                    database,
                    media,
                    litestream,
                    factory,
                    dara_version,
                    receiver,
                    worker_shutdown,
                    worker_status,
                    schedule,
                );
            });
        let worker = match spawned {
            Ok(worker) => Some(worker),
            Err(_) => {
                *lock_status(&status) = CheckpointBackupStatus {
                    phase: CheckpointBackupPhase::Unavailable,
                    last_error_code: Some(BackupErrorCode::WorkerUnavailable),
                    ..CheckpointBackupStatus::default()
                };
                None
            }
        };
        let coordinator = Self {
            sender,
            shutdown,
            status,
            worker: Mutex::new(worker),
        };
        coordinator.wake();
        coordinator
    }

    pub(crate) fn status(&self) -> CheckpointBackupStatus {
        lock_status(&self.status).clone()
    }

    pub(crate) fn wake(&self) {
        self.signal(CoordinatorSignal::WorkAvailable);
    }

    pub(crate) fn backup_now(&self) -> Result<CheckpointId, BackupErrorCode> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(BackupErrorCode::WorkerUnavailable);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(CoordinatorSignal::BackupNow { reply })
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        receiver
            .recv()
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?
    }

    pub(crate) fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.sender.send(CoordinatorSignal::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let (done, completed) = mpsc::sync_channel(1);
            match thread::Builder::new()
                .name("dara-offsite-checkpoint-reaper".into())
                .spawn(move || {
                    if worker.join().is_err() {
                        log::error!("off-site checkpoint coordinator panicked during shutdown");
                    }
                    let _ = done.send(());
                }) {
                Ok(_) => {
                    if completed
                        .recv_timeout(CHECKPOINT_COORDINATOR_JOIN_BUDGET)
                        .is_err()
                    {
                        log::warn!(
                            "off-site checkpoint coordinator exceeded the shutdown deadline"
                        );
                    }
                }
                Err(error) => {
                    log::error!("could not start off-site checkpoint reaper: {error}");
                }
            }
        }
    }

    fn signal(&self, signal: CoordinatorSignal) {
        if self.shutdown.load(Ordering::Acquire) {
            return;
        }
        if self.sender.send(signal).is_err() {
            set_failure_status(&self.status, BackupErrorCode::WorkerUnavailable);
        }
    }
}

impl Drop for CheckpointCoordinator {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone, Copy)]
struct CoordinatorSchedule {
    poll_interval: Duration,
    debounce: Duration,
    maximum_delay: Duration,
    retry_delay: Duration,
    remote_evidence_interval: Duration,
    media_wait_timeout: Duration,
}

impl CoordinatorSchedule {
    const fn production() -> Self {
        Self {
            poll_interval: SCHEDULER_POLL_INTERVAL,
            debounce: AUTOMATIC_DEBOUNCE,
            maximum_delay: AUTOMATIC_MAX_DELAY,
            retry_delay: AUTOMATIC_RETRY_DELAY,
            remote_evidence_interval: REMOTE_EVIDENCE_INTERVAL,
            media_wait_timeout: MEDIA_WAIT_TIMEOUT,
        }
    }
}

struct DirtyWindow {
    revision: u64,
    first_seen: Instant,
    last_change: Instant,
}

impl DirtyWindow {
    fn observe(current: &mut Option<Self>, revision: u64, now: Instant) {
        match current.as_mut() {
            Some(window) if window.revision != revision => {
                window.revision = revision;
                window.last_change = now;
            }
            None => {
                *current = Some(Self {
                    revision,
                    first_seen: now,
                    last_change: now,
                });
            }
            Some(_) => {}
        }
    }

    fn is_due(&self, now: Instant, debounce: Duration, maximum_delay: Duration) -> bool {
        now.duration_since(self.last_change) >= debounce
            || now.duration_since(self.first_seen) >= maximum_delay
    }
}

#[allow(clippy::too_many_arguments)]
fn checkpoint_worker(
    database: DatabaseClient,
    media: Arc<dyn CheckpointMediaWorker>,
    litestream: Arc<dyn CheckpointRuntime>,
    factory: Arc<dyn CheckpointTargetFactory>,
    dara_version: String,
    receiver: mpsc::Receiver<CoordinatorSignal>,
    shutdown: Arc<AtomicBool>,
    status: Arc<Mutex<CheckpointBackupStatus>>,
    schedule: CoordinatorSchedule,
) {
    if database
        .fail_incomplete_offsite_checkpoints(BackupErrorCode::WorkerUnavailable)
        .is_err()
    {
        set_failure_status(&status, BackupErrorCode::WorkerUnavailable);
    }
    let mut dirty: Option<DirtyWindow> = None;
    let mut next_automatic_attempt = Instant::now();
    let mut next_remote_evidence_check = Instant::now();
    let mut remote_evidence = RemoteEvidenceState::Valid;

    loop {
        let signal = match receiver.recv_timeout(schedule.poll_interval) {
            Ok(signal) => Some(signal),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match signal {
            Some(CoordinatorSignal::Shutdown) => {
                let final_database = database.clone();
                let final_media = Arc::clone(&media);
                let final_litestream = Arc::clone(&litestream);
                let final_factory = Arc::clone(&factory);
                let final_version = dara_version.clone();
                let final_status = Arc::clone(&status);
                let final_evidence = remote_evidence;
                let (done, completed) = mpsc::sync_channel(1);
                match thread::Builder::new()
                    .name("dara-offsite-checkpoint-final".into())
                    .spawn(move || {
                        attempt_final_checkpoint(
                            &final_database,
                            final_media.as_ref(),
                            final_litestream,
                            final_factory.as_ref(),
                            &final_version,
                            &final_status,
                            final_evidence,
                        );
                        let _ = done.send(());
                    }) {
                    Ok(_) => {
                        if completed.recv_timeout(CHECKPOINT_SHUTDOWN_BUDGET).is_err() {
                            log::warn!("final off-site checkpoint exceeded the shutdown deadline");
                        }
                    }
                    Err(error) => {
                        log::warn!("could not start final off-site checkpoint worker: {error}");
                    }
                }
                break;
            }
            Some(CoordinatorSignal::BackupNow { reply }) => {
                let result = create_checkpoint(
                    &database,
                    media.as_ref(),
                    Arc::clone(&litestream),
                    factory.as_ref(),
                    &dara_version,
                    &status,
                    true,
                    Instant::now() + schedule.media_wait_timeout,
                );
                if result.is_ok() {
                    dirty = None;
                    remote_evidence = RemoteEvidenceState::Valid;
                    next_remote_evidence_check = Instant::now() + schedule.remote_evidence_interval;
                }
                let _ = reply.send(result);
                continue;
            }
            Some(CoordinatorSignal::WorkAvailable) | None => {}
        }
        if shutdown.load(Ordering::Acquire) {
            continue;
        }

        let config = match database.load_offsite_backup_config() {
            Ok(config) => config,
            Err(_) => {
                set_failure_status(&status, BackupErrorCode::WorkerUnavailable);
                continue;
            }
        };
        let Some(config) = config.filter(|config| config.enabled) else {
            dirty = None;
            remote_evidence = RemoteEvidenceState::Valid;
            *lock_status(&status) = CheckpointBackupStatus::default();
            continue;
        };
        let state = match database.load_offsite_checkpoint_schedule_state() {
            Ok(state) => state,
            Err(_) => {
                set_failure_status(&status, BackupErrorCode::WorkerUnavailable);
                continue;
            }
        };
        refresh_last_complete(&status, state.last_published.as_ref());

        let current_checkpoint = state.last_published.as_ref().is_some_and(|published| {
            published_covers_config(published, &config, state.content_revision)
        });
        if current_checkpoint && remote_evidence == RemoteEvidenceState::Valid {
            dirty = None;
            if Instant::now() >= next_remote_evidence_check {
                let evidence = validate_last_published_evidence(
                    &database,
                    media.as_ref(),
                    factory.as_ref(),
                    &config,
                    state
                        .last_published
                        .as_ref()
                        .expect("published checked above"),
                );
                match evidence {
                    Ok(()) => {
                        lock_status(&status).phase = CheckpointBackupPhase::Idle;
                    }
                    Err(error) => {
                        set_failure_status(&status, error);
                        remote_evidence = RemoteEvidenceState::Invalid;
                        dirty = Some(DirtyWindow {
                            revision: state.content_revision,
                            first_seen: Instant::now(),
                            last_change: Instant::now(),
                        });
                    }
                }
                next_remote_evidence_check = Instant::now() + schedule.remote_evidence_interval;
            } else {
                lock_status(&status).phase = CheckpointBackupPhase::Idle;
            }
            continue;
        }

        let now = Instant::now();
        DirtyWindow::observe(&mut dirty, state.content_revision, now);
        let window = dirty.as_ref().expect("dirty window initialized");
        let due = window.is_due(now, schedule.debounce, schedule.maximum_delay);
        if due && now >= next_automatic_attempt {
            let result = create_checkpoint(
                &database,
                media.as_ref(),
                Arc::clone(&litestream),
                factory.as_ref(),
                &dara_version,
                &status,
                false,
                now + schedule.poll_interval,
            );
            if result.is_ok() {
                dirty = None;
                remote_evidence = RemoteEvidenceState::Valid;
                next_remote_evidence_check = now + schedule.remote_evidence_interval;
            } else {
                next_automatic_attempt = now + schedule.retry_delay;
            }
        }
    }
}

fn attempt_final_checkpoint(
    database: &DatabaseClient,
    media: &dyn CheckpointMediaWorker,
    litestream: Arc<dyn CheckpointRuntime>,
    factory: &dyn CheckpointTargetFactory,
    dara_version: &str,
    status: &Mutex<CheckpointBackupStatus>,
    remote_evidence: RemoteEvidenceState,
) {
    let state = match database.load_offsite_checkpoint_schedule_state() {
        Ok(state) => state,
        Err(_) => return,
    };
    let config = match database.load_offsite_backup_config() {
        Ok(Some(config)) if config.enabled => config,
        _ => return,
    };
    if remote_evidence == RemoteEvidenceState::Valid
        && state.last_published.as_ref().is_some_and(|published| {
            published_covers_config(published, &config, state.content_revision)
        })
    {
        return;
    }
    let deadline = Instant::now() + CHECKPOINT_SHUTDOWN_BUDGET;
    if let Err(error) = create_checkpoint(
        database,
        media,
        litestream,
        factory,
        dara_version,
        status,
        false,
        deadline,
    ) {
        log::warn!("final off-site checkpoint did not complete: {error:?}");
    }
}

#[allow(clippy::too_many_arguments)]
fn create_checkpoint(
    database: &DatabaseClient,
    media: &dyn CheckpointMediaWorker,
    litestream: Arc<dyn CheckpointRuntime>,
    factory: &dyn CheckpointTargetFactory,
    dara_version: &str,
    status: &Mutex<CheckpointBackupStatus>,
    force_media_retry: bool,
    deadline: Instant,
) -> Result<CheckpointId, BackupErrorCode> {
    let config = database
        .load_offsite_backup_config()
        .map_err(map_database_error)?
        .filter(|config| config.enabled)
        .ok_or(BackupErrorCode::InvalidTarget)?;
    ensure_litestream_healthy(&litestream.status())?;
    let target = factory.open(&config, deadline)?;
    set_phase(status, CheckpointBackupPhase::WaitingForMedia, None);

    for race_attempt in 0..CHECKPOINT_MEDIA_RACE_RETRIES {
        wait_for_media(database, media, &config, force_media_retry, deadline)?;
        let checkpoint_id = CheckpointId::new();
        let created_at = UtcTimestamp::now().map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        let created_at_millis = now_millis().map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        set_phase(
            status,
            CheckpointBackupPhase::Fencing,
            Some(checkpoint_id.clone()),
        );
        let prepared = database.prepare_offsite_checkpoint(
            PrepareOffsiteCheckpointInput {
                checkpoint_id: checkpoint_id.clone(),
                backup_set_id: config.backup_set_id.clone(),
                replica_epoch_id: config.replica_epoch_id.clone(),
                created_at: created_at_millis,
                dara_version: dara_version.to_owned(),
            },
            Arc::new(RuntimeLocalSync {
                runtime: Arc::clone(&litestream),
                deadline,
            }),
        );
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(DatabaseError::OffsiteCheckpointMediaIncomplete)
                if race_attempt + 1 < CHECKPOINT_MEDIA_RACE_RETRIES =>
            {
                continue;
            }
            Err(error) => {
                let code = map_database_error(error);
                let _ = database.mark_offsite_checkpoint_failed(checkpoint_id.clone(), code);
                set_failure_status(status, code);
                return Err(code);
            }
        };
        let result = finish_checkpoint(
            database,
            litestream.as_ref(),
            target.as_ref(),
            &config,
            &created_at,
            &prepared,
            status,
            deadline,
        );
        match result {
            Ok(()) => {
                let mut current = lock_status(status);
                current.phase = CheckpointBackupPhase::Idle;
                current.in_progress_checkpoint_id = None;
                current.last_complete_checkpoint_id = Some(checkpoint_id.clone());
                current.last_complete_at = Some(created_at_millis);
                current.last_error_code = None;
                return Ok(checkpoint_id);
            }
            Err(error) => {
                let _ = database.mark_offsite_checkpoint_failed(checkpoint_id.clone(), error);
                set_failure_status(status, error);
                return Err(error);
            }
        }
    }
    set_failure_status(status, BackupErrorCode::RemoteMediaMissing);
    Err(BackupErrorCode::RemoteMediaMissing)
}

#[allow(clippy::too_many_arguments)]
fn finish_checkpoint(
    database: &DatabaseClient,
    litestream: &dyn CheckpointRuntime,
    target: &dyn CheckpointTarget,
    config: &OffsiteBackupConfig,
    created_at: &UtcTimestamp,
    prepared: &PreparedOffsiteCheckpoint,
    status: &Mutex<CheckpointBackupStatus>,
    deadline: Instant,
) -> Result<(), BackupErrorCode> {
    remaining_checkpoint_time(deadline)?;
    database
        .mark_offsite_checkpoint_fenced(prepared.checkpoint_id.clone(), prepared.litestream_txid)
        .map_err(map_database_error)?;
    set_phase(
        status,
        CheckpointBackupPhase::WaitingForReplica,
        Some(prepared.checkpoint_id.clone()),
    );
    let remote = litestream.sync_remote(remaining_checkpoint_time(deadline)?)?;
    if remote
        .replica_txid
        .is_none_or(|replica_txid| replica_txid < prepared.litestream_txid)
    {
        return Err(BackupErrorCode::ReplicaBehind);
    }
    remaining_checkpoint_time(deadline)?;
    database
        .mark_offsite_checkpoint_replicated(prepared.checkpoint_id.clone())
        .map_err(map_database_error)?;

    remaining_checkpoint_time(deadline)?;
    ensure_active_config(database, config)?;
    set_phase(
        status,
        CheckpointBackupPhase::Validating,
        Some(prepared.checkpoint_id.clone()),
    );
    target.validate_exact_txid(
        prepared.litestream_txid,
        remaining_checkpoint_time(deadline)?.min(EXACT_RESTORE_TIMEOUT),
    )?;
    target.validate_media(&prepared.referenced_media, deadline)?;
    remaining_checkpoint_time(deadline)?;
    ensure_active_config(database, config)?;

    let keyspace = config.target.keyspace();
    let manifest = CheckpointManifestV1::new(CheckpointManifestInput {
        backup_set_id: prepared.backup_set_id.clone(),
        replica_epoch_id: prepared.replica_epoch_id.clone(),
        checkpoint_id: prepared.checkpoint_id.clone(),
        created_at: created_at.clone(),
        dara_version: prepared.dara_version.clone(),
        content_revision: prepared.content_revision,
        main_migration_head: prepared.main_migration_head,
        litestream_path: keyspace.litestream(&prepared.replica_epoch_id),
        txid: prepared.litestream_txid.to_string(),
        media_migration_head: prepared.media_migration_head,
        referenced_hash_count: prepared.referenced_media.len() as u64,
        referenced_total_bytes: prepared.referenced_total_bytes,
        referenced_hash_set_sha256: prepared.referenced_hash_set_sha256,
    })
    .map_err(|_| BackupErrorCode::MalformedManifest)?;
    let manifest_key = keyspace
        .checkpoint(
            &prepared.replica_epoch_id,
            &prepared.checkpoint_id,
            created_at,
        )
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    set_phase(
        status,
        CheckpointBackupPhase::Publishing,
        Some(prepared.checkpoint_id.clone()),
    );
    target.publish_and_verify_manifest(&manifest_key, &manifest, deadline)?;
    remaining_checkpoint_time(deadline)?;
    database
        .mark_offsite_checkpoint_published(
            prepared.checkpoint_id.clone(),
            manifest_key.as_str().to_owned(),
        )
        .map_err(map_database_error)
}

fn wait_for_media(
    database: &DatabaseClient,
    media: &dyn CheckpointMediaWorker,
    config: &OffsiteBackupConfig,
    force_retry: bool,
    deadline: Instant,
) -> Result<(), BackupErrorCode> {
    let now = now_millis().map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let report = database
        .reconcile_offsite_media(now)
        .map_err(map_database_error)?;
    if report.missing_local_blobs > 0 {
        return Err(BackupErrorCode::LocalMediaMissing);
    }
    if force_retry {
        database
            .release_all_offsite_media_retries(config.backup_set_id.clone(), now)
            .map_err(map_database_error)?;
    }
    media.wake();

    loop {
        ensure_active_config(database, config)?;
        let summary = database
            .load_referenced_offsite_media_summary(config.backup_set_id.clone())
            .map_err(map_database_error)?;
        if summary.blocked_count > 0 {
            return Err(summary
                .last_error_code
                .unwrap_or(BackupErrorCode::RemoteMediaCorrupt));
        }
        if summary.pending_count == 0 && summary.retry_wait_count == 0 {
            return Ok(());
        }
        let media_status = media.status();
        if matches!(
            media_status.phase,
            MediaBackupPhase::Unavailable | MediaBackupPhase::Blocked
        ) {
            return Err(media_status
                .last_error_code
                .unwrap_or(BackupErrorCode::WorkerUnavailable));
        }
        let remaining = remaining_checkpoint_time(deadline)?;
        media.wake();
        thread::sleep(MEDIA_POLL_INTERVAL.min(remaining));
    }
}

fn remaining_checkpoint_time(deadline: Instant) -> Result<Duration, BackupErrorCode> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(BackupErrorCode::NetworkTimeout)
}

fn ensure_active_config(
    database: &DatabaseClient,
    expected: &OffsiteBackupConfig,
) -> Result<(), BackupErrorCode> {
    let current = database
        .load_offsite_backup_config()
        .map_err(map_database_error)?;
    if current.as_ref() != Some(expected) || !expected.enabled {
        return Err(BackupErrorCode::InvalidTarget);
    }
    Ok(())
}

fn ensure_litestream_healthy(status: &RelationalBackupStatus) -> Result<(), BackupErrorCode> {
    if status.phase != super::domain::RelationalBackupPhase::Running {
        return Err(status
            .last_error_code
            .unwrap_or(BackupErrorCode::LitestreamUnavailable));
    }
    Ok(())
}

fn validate_last_published_evidence(
    database: &DatabaseClient,
    media: &dyn CheckpointMediaWorker,
    factory: &dyn CheckpointTargetFactory,
    config: &OffsiteBackupConfig,
    published: &PublishedOffsiteCheckpoint,
) -> Result<(), BackupErrorCode> {
    let target = factory.open(config, Instant::now() + CHECKPOINT_SHUTDOWN_BUDGET)?;
    target.validate_published_manifest(published)?;
    let checked_at = now_millis().map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let sampled_media = rotating_media_evidence_sample(&published.referenced_media, checked_at)?;
    match target.validate_published_media(sampled_media) {
        Err(
            error @ (BackupErrorCode::RemoteMediaMissing | BackupErrorCode::RemoteMediaCorrupt),
        ) => {
            let sha256s = sampled_media
                .iter()
                .map(|reference| reference.sha256)
                .collect();
            database
                .requeue_offsite_media_evidence(
                    published.backup_set_id.clone(),
                    sha256s,
                    error,
                    checked_at,
                )
                .map_err(map_database_error)?;
            media.wake();
            Err(error)
        }
        result => result,
    }
}

fn rotating_media_evidence_sample(
    references: &[CheckpointMediaReference],
    checked_at: i64,
) -> Result<&[CheckpointMediaReference], BackupErrorCode> {
    if references.is_empty() {
        return Ok(&[]);
    }
    let checked_at = u64::try_from(checked_at).map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let interval_millis = u64::try_from(REMOTE_EVIDENCE_INTERVAL.as_millis())
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let reference_count =
        u64::try_from(references.len()).map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    let sample_index = usize::try_from((checked_at / interval_millis) % reference_count)
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
    Ok(&references[sample_index..sample_index + 1])
}

fn refresh_last_complete(
    status: &Mutex<CheckpointBackupStatus>,
    published: Option<&PublishedOffsiteCheckpoint>,
) {
    let mut current = lock_status(status);
    if let Some(published) = published {
        current.last_complete_checkpoint_id = Some(published.checkpoint_id.clone());
        current.last_complete_at = Some(published.created_at);
    }
}

fn published_covers_config(
    published: &PublishedOffsiteCheckpoint,
    config: &OffsiteBackupConfig,
    content_revision: u64,
) -> bool {
    published.backup_set_id == config.backup_set_id
        && published.replica_epoch_id == config.replica_epoch_id
        && published.config_revision == config.revision
        && published.content_revision == content_revision
}

fn set_phase(
    status: &Mutex<CheckpointBackupStatus>,
    phase: CheckpointBackupPhase,
    checkpoint_id: Option<CheckpointId>,
) {
    let mut current = lock_status(status);
    current.phase = phase;
    current.in_progress_checkpoint_id = checkpoint_id;
    current.last_error_code = None;
}

fn set_failure_status(status: &Mutex<CheckpointBackupStatus>, error: BackupErrorCode) {
    let mut current = lock_status(status);
    current.phase = match error {
        BackupErrorCode::KeychainCredentialMissing
        | BackupErrorCode::KeychainUnavailable
        | BackupErrorCode::WorkerUnavailable
        | BackupErrorCode::LitestreamUnavailable => CheckpointBackupPhase::Unavailable,
        BackupErrorCode::NetworkOffline
        | BackupErrorCode::NetworkTimeout
        | BackupErrorCode::RateLimited
        | BackupErrorCode::ServiceUnavailable
        | BackupErrorCode::ReplicaBehind => CheckpointBackupPhase::Degraded,
        _ => CheckpointBackupPhase::Blocked,
    };
    current.in_progress_checkpoint_id = None;
    current.last_error_code = Some(error);
}

fn lock_status(
    status: &Mutex<CheckpointBackupStatus>,
) -> std::sync::MutexGuard<'_, CheckpointBackupStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn map_database_error(error: DatabaseError) -> BackupErrorCode {
    match error {
        DatabaseError::OffsiteCheckpointFence(code) => code,
        DatabaseError::OffsiteCheckpointMediaIncomplete => BackupErrorCode::RemoteMediaMissing,
        DatabaseError::InvalidOffsiteBackupConfig(_)
        | DatabaseError::InvalidOffsiteCheckpoint(_)
        | DatabaseError::StaleOffsiteBackupConfig
        | DatabaseError::StaleOffsiteCheckpoint => BackupErrorCode::InvalidTarget,
        _ => BackupErrorCode::WorkerUnavailable,
    }
}

trait CheckpointTargetFactory: Send + Sync {
    fn open(
        &self,
        config: &OffsiteBackupConfig,
        deadline: Instant,
    ) -> Result<Box<dyn CheckpointTarget>, BackupErrorCode>;
}

trait CheckpointTarget: Send + Sync {
    fn validate_exact_txid(
        &self,
        txid: LitestreamTxid,
        timeout: Duration,
    ) -> Result<(), BackupErrorCode>;
    fn validate_media(
        &self,
        references: &[CheckpointMediaReference],
        deadline: Instant,
    ) -> Result<(), BackupErrorCode>;
    fn publish_and_verify_manifest(
        &self,
        key: &R2ObjectKey,
        manifest: &CheckpointManifestV1,
        deadline: Instant,
    ) -> Result<(), BackupErrorCode>;
    fn validate_published_manifest(
        &self,
        published: &PublishedOffsiteCheckpoint,
    ) -> Result<(), BackupErrorCode>;
    fn validate_published_media(
        &self,
        references: &[CheckpointMediaReference],
    ) -> Result<(), BackupErrorCode>;
}

struct SystemCheckpointTargetFactory<C> {
    data_root: PathBuf,
    database_path: PathBuf,
    resource_dir: PathBuf,
    credentials: C,
}

impl<C: CredentialStore> CheckpointTargetFactory for SystemCheckpointTargetFactory<C> {
    fn open(
        &self,
        config: &OffsiteBackupConfig,
        deadline: Instant,
    ) -> Result<Box<dyn CheckpointTarget>, BackupErrorCode> {
        remaining_checkpoint_time(deadline)?;
        let credentials = self
            .credentials
            .load(&config.backup_set_id)
            .map_err(map_credential_error)?;
        let installation_id = InstallationIdentityStore::new(&self.data_root)
            .and_then(|store| store.load())
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        let store = R2ObjectStore::new(config.target.clone(), &credentials)
            .map_err(|error| map_store_error(error.code))?;
        let store: Arc<dyn ObjectStore> = Arc::new(store);
        validate_backup_authority_with_deadline(
            store.as_ref(),
            config,
            &installation_id,
            deadline,
        )?;

        remaining_checkpoint_time(deadline)?;
        let runtime = LitestreamRuntimePaths::new(&self.data_root)
            .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        let replica_path = config
            .target
            .keyspace()
            .litestream(&config.replica_epoch_id);
        let endpoint = config.target.endpoint();
        let expected_config = LitestreamConfig {
            database_path: &self.database_path,
            runtime: &runtime,
            bucket: config.target.bucket.as_str(),
            replica_path: replica_path.as_str(),
            endpoint: &endpoint,
        }
        .render()
        .map_err(|_| BackupErrorCode::InvalidTarget)?;
        let actual_config =
            fs::read(runtime.config()).map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        if actual_config != expected_config.as_bytes() {
            return Err(BackupErrorCode::LitestreamUnavailable);
        }
        let binary = VerifiedLitestreamBinary::resolve(&self.resource_dir)
            .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
        Ok(Box::new(SystemCheckpointTarget {
            store,
            keyspace: config.target.keyspace(),
            binary: binary.path().to_owned(),
            runtime,
            database_path: self.database_path.clone(),
            credentials,
        }))
    }
}

struct SystemCheckpointTarget {
    store: Arc<dyn ObjectStore>,
    keyspace: R2Keyspace,
    binary: PathBuf,
    runtime: LitestreamRuntimePaths,
    database_path: PathBuf,
    credentials: R2Credentials,
}

impl CheckpointTarget for SystemCheckpointTarget {
    fn validate_exact_txid(
        &self,
        txid: LitestreamTxid,
        timeout: Duration,
    ) -> Result<(), BackupErrorCode> {
        let output = self.runtime.directory().join(format!(
            ".checkpoint-restore-{}-{}.sqlite3",
            std::process::id(),
            CheckpointId::new().as_str()
        ));
        let plan = exact_restore_dry_run(
            &self.binary,
            self.runtime.config(),
            &self.database_path,
            &output,
            txid,
            &self.credentials,
            timeout,
        )?;
        if plan.target_path != output || plan.replica != ReplicaKind::S3 || plan.max_txid != txid {
            return Err(BackupErrorCode::ExactTxidUnavailable);
        }
        Ok(())
    }

    fn validate_media(
        &self,
        references: &[CheckpointMediaReference],
        deadline: Instant,
    ) -> Result<(), BackupErrorCode> {
        for reference in references {
            let key = self.keyspace.media(reference.sha256);
            let metadata = self
                .store
                .head_with_timeout(&key, remaining_checkpoint_time(deadline)?)
                .map_err(|error| map_store_error(error.code))?
                .ok_or(BackupErrorCode::RemoteMediaMissing)?;
            if metadata.byte_length != reference.byte_length
                || metadata.content_type != Some(ObjectContentType::Webp)
                || metadata.dara_sha256 != Some(reference.sha256)
                || metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
            {
                return Err(BackupErrorCode::RemoteMediaCorrupt);
            }
        }
        Ok(())
    }

    fn publish_and_verify_manifest(
        &self,
        key: &R2ObjectKey,
        manifest: &CheckpointManifestV1,
        deadline: Instant,
    ) -> Result<(), BackupErrorCode> {
        let expected = manifest
            .to_json()
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        let outcome = self
            .store
            .put_with_timeout(
                PutObjectRequest {
                    key: key.clone(),
                    bytes: expected.clone(),
                    content_type: ObjectContentType::Json,
                    dara_sha256: None,
                    condition: PutCondition::IfAbsent,
                },
                remaining_checkpoint_time(deadline)?,
            )
            .map_err(|error| map_store_error(error.code))?;
        let stored = self
            .store
            .get_with_timeout(key, remaining_checkpoint_time(deadline)?)
            .map_err(|error| map_store_error(error.code))?;
        if outcome == PutObjectOutcome::ConditionNotMet && stored.bytes != expected {
            return Err(BackupErrorCode::ImmutableObjectConflict);
        }
        if stored.bytes != expected
            || stored.metadata.byte_length != expected.len() as u64
            || stored.metadata.content_type != Some(ObjectContentType::Json)
            || stored.metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
            || stored.metadata.dara_sha256.is_some()
        {
            return Err(BackupErrorCode::MalformedManifest);
        }
        let parsed = CheckpointManifestV1::from_json(&stored.bytes, &self.keyspace)
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        if parsed != *manifest
            || parsed
                .object_key(&self.keyspace)
                .map_err(|_| BackupErrorCode::MalformedManifest)?
                != *key
        {
            return Err(BackupErrorCode::MalformedManifest);
        }
        Ok(())
    }

    fn validate_published_manifest(
        &self,
        published: &PublishedOffsiteCheckpoint,
    ) -> Result<(), BackupErrorCode> {
        let key = self
            .keyspace
            .validate_returned_key(published.manifest_object_key.clone())
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        let stored = self
            .store
            .get(&key)
            .map_err(|error| map_store_error(error.code))?;
        let manifest = CheckpointManifestV1::from_json(&stored.bytes, &self.keyspace)
            .map_err(|_| BackupErrorCode::MalformedManifest)?;
        if stored.metadata.byte_length != stored.bytes.len() as u64
            || stored.metadata.content_type != Some(ObjectContentType::Json)
            || stored.metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
            || stored.metadata.dara_sha256.is_some()
            || manifest
                .object_key(&self.keyspace)
                .map_err(|_| BackupErrorCode::MalformedManifest)?
                != key
            || !manifest.matches_published_evidence(&PublishedCheckpointEvidence {
                checkpoint_id: &published.checkpoint_id,
                backup_set_id: &published.backup_set_id,
                replica_epoch_id: &published.replica_epoch_id,
                content_revision: published.content_revision,
                dara_version: &published.dara_version,
                main_migration_head: published.main_migration_head,
                media_migration_head: published.media_migration_head,
                referenced_hash_count: published.referenced_hash_count,
                referenced_total_bytes: published.referenced_total_bytes,
                referenced_hash_set_sha256: published.referenced_hash_set_sha256,
                litestream_txid: &published.litestream_txid.to_string(),
            })
        {
            return Err(BackupErrorCode::MalformedManifest);
        }
        Ok(())
    }

    fn validate_published_media(
        &self,
        references: &[CheckpointMediaReference],
    ) -> Result<(), BackupErrorCode> {
        for reference in references {
            let key = self.keyspace.media(reference.sha256);
            let stored = self
                .store
                .get(&key)
                .map_err(|error| map_store_error(error.code))?;
            let actual_sha256 = ContentSha256::from_bytes(Sha256::digest(&stored.bytes).into());
            if stored.metadata.byte_length != reference.byte_length
                || stored.metadata.byte_length != stored.bytes.len() as u64
                || stored.metadata.content_type != Some(ObjectContentType::Webp)
                || stored.metadata.dara_sha256 != Some(reference.sha256)
                || stored.metadata.object_format_version != Some(OBJECT_FORMAT_VERSION)
                || actual_sha256 != reference.sha256
            {
                return Err(BackupErrorCode::RemoteMediaCorrupt);
            }
        }
        Ok(())
    }
}

fn exact_restore_dry_run(
    binary: &Path,
    config: &Path,
    database_path: &Path,
    output_path: &Path,
    txid: LitestreamTxid,
    credentials: &R2Credentials,
    timeout: Duration,
) -> Result<super::litestream::RestorePlan, BackupErrorCode> {
    if timeout.is_zero() {
        return Err(BackupErrorCode::NetworkTimeout);
    }
    if !database_path.is_absolute() || !output_path.is_absolute() {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    let mut command = Command::new(binary);
    command
        .arg("restore")
        .arg("-config")
        .arg(config)
        .arg("-txid")
        .arg(txid.to_string())
        .arg("-dry-run")
        .arg("-json")
        .arg("-o")
        .arg(output_path)
        .arg(database_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_credentials_environment(&mut command, credentials);
    let mut child = command
        .spawn()
        .map_err(|_| BackupErrorCode::LitestreamUnavailable)?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        remove_dry_run_output(output_path)?;
        return Err(BackupErrorCode::RestoreValidationFailed);
    };
    let reader = match thread::Builder::new()
        .name("dara-litestream-restore-plan".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .by_ref()
                .take(MAX_RESTORE_PLAN_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        }) {
        Ok(reader) => reader,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            remove_dry_run_output(output_path)?;
            return Err(BackupErrorCode::WorkerUnavailable);
        }
    };
    let deadline = Instant::now()
        .checked_add(timeout.min(EXACT_RESTORE_TIMEOUT))
        .ok_or(BackupErrorCode::NetworkTimeout)?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(EXACT_RESTORE_POLL_INTERVAL);
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                remove_dry_run_output(output_path)?;
                return Err(BackupErrorCode::NetworkTimeout);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                remove_dry_run_output(output_path)?;
                return Err(BackupErrorCode::LitestreamFailed);
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| BackupErrorCode::WorkerUnavailable)?
        .map_err(|_| BackupErrorCode::RestoreValidationFailed)?;
    remove_dry_run_output(output_path)?;
    if !status.success() {
        return Err(BackupErrorCode::ExactTxidUnavailable);
    }
    if bytes.len() > MAX_RESTORE_PLAN_BYTES {
        return Err(BackupErrorCode::RestoreValidationFailed);
    }
    parse_restore_plan_json(&bytes).map_err(|_| BackupErrorCode::RestoreValidationFailed)
}

fn remove_dry_run_output(output_path: &Path) -> Result<(), BackupErrorCode> {
    match fs::remove_file(output_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(BackupErrorCode::RestoreValidationFailed),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::Mutex,
        time::{Duration, Instant},
    };

    use rusqlite::{Connection, OpenFlags};
    use sha2::Digest;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backup::domain::{
            BackupProvider, CheckpointPhase, ContentSha256, R2AccountId, R2BucketName,
            R2Jurisdiction, R2Prefix, R2Target, RelationalBackupPhase, ReplicaEpochId,
        },
        backup::object_store::{
            fake::{FakeObjectStore, ObjectOperation},
            ObjectStoreErrorCode,
        },
        database::{
            initialize, Database, DatabasePaths, InitializationOptions,
            SaveOffsiteBackupConfigInput, SetZoomPercentInput,
        },
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TargetEvent {
        ExactTxid,
        Media,
        Publish,
        ValidatePublished,
        ValidatePublishedMedia,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TargetFailureStage {
        ExactTxid,
        Media,
        Publish,
        ValidatePublished,
        ValidatePublishedMedia,
    }

    #[derive(Clone)]
    struct FakeTarget {
        events: Arc<Mutex<Vec<TargetEvent>>>,
        failure: Option<TargetFailureStage>,
        exact_txid_delay: Option<Duration>,
        manifest: Arc<Mutex<Option<Vec<u8>>>>,
    }

    impl FakeTarget {
        fn new(failure: Option<TargetFailureStage>) -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
                failure,
                exact_txid_delay: None,
                manifest: Arc::new(Mutex::new(None)),
            }
        }

        fn with_exact_txid_delay(mut self, delay: Duration) -> Self {
            self.exact_txid_delay = Some(delay);
            self
        }

        fn events(&self) -> Vec<TargetEvent> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl CheckpointTarget for FakeTarget {
        fn validate_exact_txid(
            &self,
            _txid: LitestreamTxid,
            timeout: Duration,
        ) -> Result<(), BackupErrorCode> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(TargetEvent::ExactTxid);
            if self.exact_txid_delay.is_some_and(|delay| delay > timeout) {
                thread::sleep(timeout);
                return Err(BackupErrorCode::NetworkTimeout);
            }
            if let Some(delay) = self.exact_txid_delay {
                thread::sleep(delay);
            }
            if self.failure == Some(TargetFailureStage::ExactTxid) {
                return Err(BackupErrorCode::ExactTxidUnavailable);
            }
            Ok(())
        }

        fn validate_media(
            &self,
            _references: &[CheckpointMediaReference],
            _deadline: Instant,
        ) -> Result<(), BackupErrorCode> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(TargetEvent::Media);
            if self.failure == Some(TargetFailureStage::Media) {
                return Err(BackupErrorCode::RemoteMediaMissing);
            }
            Ok(())
        }

        fn publish_and_verify_manifest(
            &self,
            _key: &R2ObjectKey,
            manifest: &CheckpointManifestV1,
            _deadline: Instant,
        ) -> Result<(), BackupErrorCode> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(TargetEvent::Publish);
            if self.failure == Some(TargetFailureStage::Publish) {
                return Err(BackupErrorCode::NetworkOffline);
            }
            *self
                .manifest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(
                manifest
                    .to_json()
                    .map_err(|_| BackupErrorCode::MalformedManifest)?,
            );
            Ok(())
        }

        fn validate_published_manifest(
            &self,
            _published: &PublishedOffsiteCheckpoint,
        ) -> Result<(), BackupErrorCode> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(TargetEvent::ValidatePublished);
            if self.failure == Some(TargetFailureStage::ValidatePublished) {
                return Err(BackupErrorCode::MalformedManifest);
            }
            Ok(())
        }

        fn validate_published_media(
            &self,
            _references: &[CheckpointMediaReference],
        ) -> Result<(), BackupErrorCode> {
            self.events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(TargetEvent::ValidatePublishedMedia);
            if self.failure == Some(TargetFailureStage::ValidatePublishedMedia) {
                return Err(BackupErrorCode::RemoteMediaMissing);
            }
            Ok(())
        }
    }

    struct FakeTargetFactory {
        target: FakeTarget,
    }

    impl CheckpointTargetFactory for FakeTargetFactory {
        fn open(
            &self,
            _config: &OffsiteBackupConfig,
            _deadline: Instant,
        ) -> Result<Box<dyn CheckpointTarget>, BackupErrorCode> {
            Ok(Box::new(self.target.clone()))
        }
    }

    struct FakeMediaWorker;

    impl CheckpointMediaWorker for FakeMediaWorker {
        fn wake(&self) {}

        fn status(&self) -> MediaBackupStatus {
            MediaBackupStatus {
                phase: MediaBackupPhase::Idle,
                ..MediaBackupStatus::default()
            }
        }
    }

    struct FakeRuntime {
        local_txid: LitestreamTxid,
        remote_txid: Option<LitestreamTxid>,
        before_remote: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl FakeRuntime {
        fn caught_up() -> Self {
            let txid = LitestreamTxid::from_local(42);
            Self {
                local_txid: txid,
                remote_txid: Some(txid),
                before_remote: Mutex::new(None),
            }
        }
    }

    impl CheckpointRuntime for FakeRuntime {
        fn status(&self) -> RelationalBackupStatus {
            RelationalBackupStatus {
                phase: RelationalBackupPhase::Running,
                latest_local_txid: Some(self.local_txid),
                latest_remote_txid: self.remote_txid,
                last_remote_confirmed_at: Some(1),
                restart_count: 0,
                last_error_code: None,
            }
        }

        fn sync_local(&self, _timeout: Duration) -> Result<LitestreamTxid, BackupErrorCode> {
            Ok(self.local_txid)
        }

        fn sync_remote(&self, _timeout: Duration) -> Result<SyncResult, BackupErrorCode> {
            if let Some(callback) = self
                .before_remote
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                callback();
            }
            Ok(SyncResult {
                database_path: PathBuf::from("/tmp/dara-test.sqlite3"),
                txid: self.local_txid,
                replica_txid: self.remote_txid,
                duration_ms: 1,
            })
        }
    }

    fn enabled_database() -> (TempDir, Database, OffsiteBackupConfig) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = initialize(
            DatabasePaths::new(directory.path().join("data")),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database");
        let config = database
            .client()
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: 0,
                backup_set_id: super::super::domain::BackupSetId::new(),
                replica_epoch_id: ReplicaEpochId::new(),
                enabled: true,
                target: R2Target {
                    account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                        .expect("account ID"),
                    jurisdiction: R2Jurisdiction::Default,
                    bucket: R2BucketName::parse("dara-test").expect("bucket"),
                    prefix: R2Prefix::parse("dara/coordinator-test").expect("prefix"),
                },
            })
            .expect("config");
        assert_eq!(config.provider, BackupProvider::R2);
        (directory, database, config)
    }

    fn run_checkpoint(
        database: &Database,
        runtime: Arc<dyn CheckpointRuntime>,
        target: &FakeTarget,
    ) -> Result<CheckpointId, BackupErrorCode> {
        run_checkpoint_with_deadline(
            database,
            runtime,
            target,
            Instant::now() + Duration::from_secs(1),
        )
    }

    fn run_checkpoint_with_deadline(
        database: &Database,
        runtime: Arc<dyn CheckpointRuntime>,
        target: &FakeTarget,
        deadline: Instant,
    ) -> Result<CheckpointId, BackupErrorCode> {
        let status = Mutex::new(CheckpointBackupStatus::default());
        create_checkpoint(
            &database.client(),
            &FakeMediaWorker,
            runtime,
            &FakeTargetFactory {
                target: target.clone(),
            },
            "test",
            &status,
            true,
            deadline,
        )
    }

    fn checkpoint_phase(database: &Database, checkpoint_id: &CheckpointId) -> CheckpointPhase {
        let connection = Connection::open_with_flags(
            &database.paths().main,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .expect("read-only main");
        let value = connection
            .query_row(
                "SELECT phase FROM offsite_backup_checkpoint WHERE checkpoint_id = ?1",
                [checkpoint_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .expect("checkpoint phase");
        CheckpointPhase::from_db(&value).expect("valid phase")
    }

    fn test_schedule() -> CoordinatorSchedule {
        CoordinatorSchedule {
            poll_interval: Duration::from_millis(5),
            debounce: Duration::from_millis(20),
            maximum_delay: Duration::from_millis(50),
            retry_delay: Duration::from_millis(20),
            remote_evidence_interval: Duration::from_secs(1),
            media_wait_timeout: Duration::from_secs(1),
        }
    }

    fn manifest_fixture(
        keyspace: &R2Keyspace,
        checkpoint_id: CheckpointId,
        replica_epoch_id: ReplicaEpochId,
    ) -> (CheckpointManifestV1, R2ObjectKey) {
        let created_at = UtcTimestamp::parse("2026-07-27T20:00:00Z").expect("timestamp");
        let manifest = CheckpointManifestV1::new(CheckpointManifestInput {
            backup_set_id: super::super::domain::BackupSetId::new(),
            replica_epoch_id: replica_epoch_id.clone(),
            checkpoint_id: checkpoint_id.clone(),
            created_at: created_at.clone(),
            dara_version: "test".into(),
            content_revision: 7,
            main_migration_head: 9,
            litestream_path: keyspace.litestream(&replica_epoch_id),
            txid: LitestreamTxid::from_local(42).to_string(),
            media_migration_head: 2,
            referenced_hash_count: 0,
            referenced_total_bytes: 0,
            referenced_hash_set_sha256: ContentSha256::from_bytes(sha2::Sha256::digest([]).into()),
        })
        .expect("manifest");
        let key = keyspace
            .checkpoint(&replica_epoch_id, &checkpoint_id, &created_at)
            .expect("manifest key");
        (manifest, key)
    }

    #[test]
    fn manifest_is_published_last_and_only_after_exact_txid_and_media_validation() {
        let (_directory, database, _config) = enabled_database();
        let target = FakeTarget::new(None);
        let checkpoint_id = run_checkpoint(&database, Arc::new(FakeRuntime::caught_up()), &target)
            .expect("complete checkpoint");

        assert_eq!(
            target.events(),
            vec![
                TargetEvent::ExactTxid,
                TargetEvent::Media,
                TargetEvent::Publish
            ]
        );
        assert!(target
            .manifest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some());
        assert_eq!(
            checkpoint_phase(&database, &checkpoint_id),
            CheckpointPhase::Published
        );
        assert_eq!(
            database
                .client()
                .load_offsite_checkpoint_schedule_state()
                .expect("schedule")
                .last_published
                .expect("published")
                .checkpoint_id,
            checkpoint_id
        );
    }

    #[test]
    fn scheduler_debounces_quiet_writes_but_caps_continuous_changes() {
        let base = Instant::now();
        let mut dirty = None;
        DirtyWindow::observe(&mut dirty, 1, base);
        let window = dirty.as_ref().expect("dirty");
        assert!(!window.is_due(
            base + Duration::from_secs(59),
            Duration::from_secs(60),
            Duration::from_secs(300),
        ));

        DirtyWindow::observe(&mut dirty, 2, base + Duration::from_secs(59));
        let window = dirty.as_ref().expect("dirty after change");
        assert!(window.is_due(
            base + Duration::from_secs(119),
            Duration::from_secs(60),
            Duration::from_secs(300),
        ));

        DirtyWindow::observe(&mut dirty, 3, base + Duration::from_secs(250));
        let window = dirty.as_ref().expect("continuously dirty");
        assert!(window.is_due(
            base + Duration::from_secs(300),
            Duration::from_secs(60),
            Duration::from_secs(300),
        ));
    }

    #[test]
    fn media_evidence_sampling_is_bounded_and_rotates_with_wall_time() {
        let references = [0x11, 0x22, 0x33].map(|byte| CheckpointMediaReference {
            sha256: ContentSha256::from_bytes([byte; 32]),
            byte_length: 10,
        });
        let interval =
            i64::try_from(REMOTE_EVIDENCE_INTERVAL.as_millis()).expect("interval milliseconds");

        for (slot, expected) in references.iter().enumerate() {
            let checked_at = interval
                .checked_mul(i64::try_from(slot).expect("slot"))
                .expect("sample timestamp");
            assert_eq!(
                rotating_media_evidence_sample(&references, checked_at).expect("sample"),
                std::slice::from_ref(expected)
            );
        }
        assert_eq!(
            rotating_media_evidence_sample(&references, interval * 3).expect("wrapped sample"),
            &references[..1]
        );
        assert!(rotating_media_evidence_sample(&[], 0)
            .expect("empty sample")
            .is_empty());
    }

    #[test]
    fn backend_backup_now_bypasses_debounce_and_automatic_scheduler_stays_idle_after_publish() {
        let (_directory, database, _config) = enabled_database();
        let target = FakeTarget::new(None);
        let service = CheckpointCoordinator::start_with_parts(
            database.client(),
            Arc::new(FakeMediaWorker),
            Arc::new(FakeRuntime::caught_up()),
            Arc::new(FakeTargetFactory {
                target: target.clone(),
            }),
            "test".into(),
            CoordinatorSchedule {
                debounce: Duration::from_secs(60),
                maximum_delay: Duration::from_secs(300),
                ..test_schedule()
            },
        );

        let checkpoint_id = service.backup_now().expect("manual checkpoint");
        assert_eq!(
            checkpoint_phase(&database, &checkpoint_id),
            CheckpointPhase::Published
        );
        let publication_count = target
            .events()
            .into_iter()
            .filter(|event| *event == TargetEvent::Publish)
            .count();
        thread::sleep(Duration::from_millis(80));
        assert_eq!(
            target
                .events()
                .into_iter()
                .filter(|event| *event == TargetEvent::Publish)
                .count(),
            publication_count
        );
        service.shutdown();
    }

    #[test]
    fn automatic_scheduler_publishes_the_first_checkpoint_after_debounce() {
        let (_directory, database, _config) = enabled_database();
        let target = FakeTarget::new(None);
        let service = CheckpointCoordinator::start_with_parts(
            database.client(),
            Arc::new(FakeMediaWorker),
            Arc::new(FakeRuntime::caught_up()),
            Arc::new(FakeTargetFactory {
                target: target.clone(),
            }),
            "test".into(),
            test_schedule(),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && (!target.events().contains(&TargetEvent::Publish)
                || service.status().phase != CheckpointBackupPhase::Idle)
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(target.events().contains(&TargetEvent::Publish));
        assert_eq!(service.status().phase, CheckpointBackupPhase::Idle);
        service.shutdown();
    }

    #[test]
    fn automatic_scheduler_republishes_when_same_revision_remote_evidence_is_invalid() {
        for failure in [
            TargetFailureStage::ValidatePublished,
            TargetFailureStage::ValidatePublishedMedia,
        ] {
            let (_directory, database, _config) = enabled_database();
            let target = FakeTarget::new(Some(failure));
            let service = CheckpointCoordinator::start_with_parts(
                database.client(),
                Arc::new(FakeMediaWorker),
                Arc::new(FakeRuntime::caught_up()),
                Arc::new(FakeTargetFactory {
                    target: target.clone(),
                }),
                "test".into(),
                CoordinatorSchedule {
                    remote_evidence_interval: Duration::from_millis(20),
                    ..test_schedule()
                },
            );
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline
                && target
                    .events()
                    .into_iter()
                    .filter(|event| *event == TargetEvent::Publish)
                    .count()
                    < 2
            {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(
                target
                    .events()
                    .into_iter()
                    .filter(|event| *event == TargetEvent::Publish)
                    .count()
                    >= 2
            );
            service.shutdown();
        }
    }

    #[test]
    fn shutdown_repairs_a_checkpoint_with_known_invalid_remote_evidence() {
        let (_directory, database, _config) = enabled_database();
        let target = FakeTarget::new(Some(TargetFailureStage::ValidatePublished));
        let service = CheckpointCoordinator::start_with_parts(
            database.client(),
            Arc::new(FakeMediaWorker),
            Arc::new(FakeRuntime::caught_up()),
            Arc::new(FakeTargetFactory {
                target: target.clone(),
            }),
            "test".into(),
            CoordinatorSchedule {
                debounce: Duration::from_secs(60),
                maximum_delay: Duration::from_secs(300),
                remote_evidence_interval: Duration::from_millis(20),
                ..test_schedule()
            },
        );
        service.backup_now().expect("initial checkpoint");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && service.status().last_error_code != Some(BackupErrorCode::MalformedManifest)
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            service.status().last_error_code,
            Some(BackupErrorCode::MalformedManifest)
        );
        assert_eq!(
            target
                .events()
                .into_iter()
                .filter(|event| *event == TargetEvent::Publish)
                .count(),
            1
        );

        service.shutdown();

        assert_eq!(
            target
                .events()
                .into_iter()
                .filter(|event| *event == TargetEvent::Publish)
                .count(),
            2
        );
    }

    #[test]
    fn re_enabling_backup_requires_a_checkpoint_even_when_content_did_not_change() {
        let (_directory, database, config) = enabled_database();
        let target = FakeTarget::new(None);
        let service = CheckpointCoordinator::start_with_parts(
            database.client(),
            Arc::new(FakeMediaWorker),
            Arc::new(FakeRuntime::caught_up()),
            Arc::new(FakeTargetFactory {
                target: target.clone(),
            }),
            "test".into(),
            test_schedule(),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && target
                .events()
                .into_iter()
                .filter(|event| *event == TargetEvent::Publish)
                .count()
                < 1
        {
            thread::sleep(Duration::from_millis(5));
        }
        let disabled = database
            .client()
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: config.revision,
                backup_set_id: config.backup_set_id.clone(),
                replica_epoch_id: config.replica_epoch_id.clone(),
                enabled: false,
                target: config.target.clone(),
            })
            .expect("disable");
        database
            .client()
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: disabled.revision,
                backup_set_id: disabled.backup_set_id,
                replica_epoch_id: disabled.replica_epoch_id,
                enabled: true,
                target: disabled.target,
            })
            .expect("re-enable");

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && target
                .events()
                .into_iter()
                .filter(|event| *event == TargetEvent::Publish)
                .count()
                < 2
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            target
                .events()
                .into_iter()
                .filter(|event| *event == TargetEvent::Publish)
                .count(),
            2
        );
        service.shutdown();
    }

    #[test]
    fn authored_write_during_remote_wait_belongs_to_the_next_checkpoint() {
        let (_directory, database, _config) = enabled_database();
        let client = database.client();
        let runtime = FakeRuntime::caught_up();
        *runtime
            .before_remote
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Box::new(move || {
            let settings = client.load_settings().expect("settings");
            client
                .set_zoom_percent(SetZoomPercentInput {
                    expected_revision: settings.revision,
                    zoom_percent: 110,
                })
                .expect("later authored write");
        }));
        let target = FakeTarget::new(None);
        run_checkpoint(&database, Arc::new(runtime), &target).expect("checkpoint");

        let state = database
            .client()
            .load_offsite_checkpoint_schedule_state()
            .expect("schedule");
        let published = state.last_published.expect("published");
        assert!(state.content_revision > published.content_revision);
    }

    #[test]
    fn replica_behind_and_validation_or_publish_failures_never_publish_locally() {
        let cases = [
            (
                Some(TargetFailureStage::ExactTxid),
                Some(LitestreamTxid::from_local(42)),
                BackupErrorCode::ExactTxidUnavailable,
                vec![TargetEvent::ExactTxid],
            ),
            (
                Some(TargetFailureStage::Media),
                Some(LitestreamTxid::from_local(42)),
                BackupErrorCode::RemoteMediaMissing,
                vec![TargetEvent::ExactTxid, TargetEvent::Media],
            ),
            (
                Some(TargetFailureStage::Publish),
                Some(LitestreamTxid::from_local(42)),
                BackupErrorCode::NetworkOffline,
                vec![
                    TargetEvent::ExactTxid,
                    TargetEvent::Media,
                    TargetEvent::Publish,
                ],
            ),
            (
                None,
                Some(LitestreamTxid::from_local(41)),
                BackupErrorCode::ReplicaBehind,
                vec![],
            ),
        ];
        for (failure, replica_txid, expected_error, expected_events) in cases {
            let (_directory, database, _config) = enabled_database();
            let target = FakeTarget::new(failure);
            let runtime = FakeRuntime {
                remote_txid: replica_txid,
                ..FakeRuntime::caught_up()
            };
            let error = run_checkpoint(&database, Arc::new(runtime), &target)
                .expect_err("checkpoint must fail");
            assert_eq!(error, expected_error);
            assert_eq!(target.events(), expected_events);
            assert!(database
                .client()
                .load_offsite_checkpoint_schedule_state()
                .expect("schedule")
                .last_published
                .is_none());
        }
    }

    #[test]
    fn finalization_stops_when_the_shared_checkpoint_deadline_expires() {
        let (_directory, database, _config) = enabled_database();
        let target = FakeTarget::new(None).with_exact_txid_delay(Duration::from_millis(500));
        let started = Instant::now();
        let error = run_checkpoint_with_deadline(
            &database,
            Arc::new(FakeRuntime::caught_up()),
            &target,
            started + Duration::from_millis(100),
        )
        .expect_err("checkpoint must respect its deadline");

        assert_eq!(error, BackupErrorCode::NetworkTimeout);
        assert_eq!(target.events(), vec![TargetEvent::ExactTxid]);
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "finalization outlived the shared deadline"
        );
    }

    #[test]
    fn immutable_manifest_create_is_idempotent_but_rejects_different_existing_bytes() {
        let directory = tempfile::tempdir().expect("runtime");
        let keyspace = R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/manifest-test").expect("prefix"),
        }
        .keyspace();
        let store = Arc::new(FakeObjectStore::default());
        let target = SystemCheckpointTarget {
            store: store.clone(),
            keyspace: keyspace.clone(),
            binary: PathBuf::from("/unused/litestream"),
            runtime: LitestreamRuntimePaths::new(directory.path()).expect("runtime paths"),
            database_path: directory.path().join("dara.sqlite3"),
            credentials: R2Credentials::new(
                "0123456789abcdef0123456789abcdef",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .expect("credentials"),
        };

        let (manifest, key) =
            manifest_fixture(&keyspace, CheckpointId::new(), ReplicaEpochId::new());
        target
            .publish_and_verify_manifest(&key, &manifest, Instant::now() + Duration::from_secs(5))
            .expect("first publication");
        target
            .publish_and_verify_manifest(&key, &manifest, Instant::now() + Duration::from_secs(5))
            .expect("idempotent retry");
        assert_eq!(
            store.operations(),
            vec![
                ObjectOperation::Put,
                ObjectOperation::Get,
                ObjectOperation::Put,
                ObjectOperation::Get,
            ]
        );

        let (conflicting_manifest, conflicting_key) =
            manifest_fixture(&keyspace, CheckpointId::new(), ReplicaEpochId::new());
        store
            .put(PutObjectRequest {
                key: conflicting_key.clone(),
                bytes: b"{}".to_vec(),
                content_type: ObjectContentType::Json,
                dara_sha256: None,
                condition: PutCondition::IfAbsent,
            })
            .expect("seed conflict");
        assert_eq!(
            target.publish_and_verify_manifest(
                &conflicting_key,
                &conflicting_manifest,
                Instant::now() + Duration::from_secs(5),
            ),
            Err(BackupErrorCode::ImmutableObjectConflict)
        );

        let (readback_manifest, readback_key) =
            manifest_fixture(&keyspace, CheckpointId::new(), ReplicaEpochId::new());
        store.fail_next(ObjectOperation::Get, ObjectStoreErrorCode::Network);
        assert_eq!(
            target.publish_and_verify_manifest(
                &readback_key,
                &readback_manifest,
                Instant::now() + Duration::from_secs(5),
            ),
            Err(BackupErrorCode::NetworkOffline)
        );
    }

    #[test]
    fn published_media_evidence_downloads_and_hashes_the_remote_object() {
        let directory = tempfile::tempdir().expect("runtime");
        let keyspace = R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/media-evidence-test").expect("prefix"),
        }
        .keyspace();
        let store = Arc::new(FakeObjectStore::default());
        let target = SystemCheckpointTarget {
            store: store.clone(),
            keyspace: keyspace.clone(),
            binary: PathBuf::from("/unused/litestream"),
            runtime: LitestreamRuntimePaths::new(directory.path()).expect("runtime paths"),
            database_path: directory.path().join("dara.sqlite3"),
            credentials: R2Credentials::new(
                "0123456789abcdef0123456789abcdef",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .expect("credentials"),
        };
        let expected_bytes = b"expected-media";
        let expected_sha256 = ContentSha256::from_bytes(Sha256::digest(expected_bytes).into());
        let reference = CheckpointMediaReference {
            sha256: expected_sha256,
            byte_length: expected_bytes.len() as u64,
        };
        store
            .put(PutObjectRequest {
                key: keyspace.media(expected_sha256),
                bytes: b"corrupt-media!".to_vec(),
                content_type: ObjectContentType::Webp,
                dara_sha256: Some(expected_sha256),
                condition: PutCondition::IfAbsent,
            })
            .expect("seed corrupt object");

        assert_eq!(
            target.validate_published_media(std::slice::from_ref(&reference)),
            Err(BackupErrorCode::RemoteMediaCorrupt)
        );
        store
            .delete(&keyspace.media(expected_sha256))
            .expect("remove object");
        assert_eq!(
            target.validate_published_media(&[reference]),
            Err(BackupErrorCode::RemoteMediaMissing)
        );
    }
}
