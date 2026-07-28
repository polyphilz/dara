use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::{
    credentials::{CredentialError, CredentialStore, MacOsKeychainCredentialStore, R2Credentials},
    domain::{BackupErrorCode, InstallationId, RelationalBackupPhase},
    installation::InstallationIdentityStore,
    litestream::{
        configure_credentials_environment, CommandLitestreamControl, LitestreamConfig,
        LitestreamError, LitestreamRuntimePaths, LitestreamTxid, SyncResult, SystemCommandExecutor,
        VerifiedLitestreamBinary,
    },
    object_store::{ObjectStore, R2ObjectStore},
    remote_authority::{map_credential_error, validate_backup_authority},
};
use crate::database::LocalCheckpointSync;
use crate::database::{now_millis, DatabaseClient, OffsiteBackupConfig};

const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
const CONFIG_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const REMOTE_STATUS_INTERVAL: Duration = Duration::from_secs(30);
const RESTART_BASE_DELAY: Duration = Duration::from_secs(1);
const RESTART_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const CONTROL_REMOTE_TIMEOUT_SECONDS: u64 = 30;
// Leave time inside the fence budget to kill, reap, and report a timed-out command.
const CHECKPOINT_LOCAL_COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
const CHECKPOINT_LOCAL_SYNC_TIMEOUT: Duration = Duration::from_secs(5);
// Litestream's protocol timeout is 30 seconds; keep one second to kill and reap
// before the checkpoint handle's outer deadline.
const CHECKPOINT_REMOTE_COMMAND_TIMEOUT: Duration = Duration::from_secs(34);
const CHECKPOINT_REMOTE_SYNC_TIMEOUT: Duration = Duration::from_secs(35);
const DAEMON_STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(35);
const STALE_KILL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PID_RECORD_FORMAT_VERSION: u32 = 1;
const MAX_PID_RECORD_BYTES: u64 = 16 * 1024;
const LITESTREAM_LOG_FILE_NAME: &str = "litestream.log";
const LITESTREAM_LOG_ARCHIVE_NAME: &str = "litestream.1.log";
const LITESTREAM_LOG_MAX_BYTES: u64 = 1024 * 1024;
const LOG_COPY_BUFFER_BYTES: usize = 8 * 1024;
const REDACTED: &[u8] = b"[REDACTED]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RelationalBackupStatus {
    pub(crate) phase: RelationalBackupPhase,
    pub(crate) latest_local_txid: Option<LitestreamTxid>,
    pub(crate) latest_remote_txid: Option<LitestreamTxid>,
    pub(crate) last_remote_confirmed_at: Option<i64>,
    pub(crate) restart_count: u32,
    pub(crate) last_error_code: Option<BackupErrorCode>,
}

impl Default for RelationalBackupStatus {
    fn default() -> Self {
        Self {
            phase: RelationalBackupPhase::Off,
            latest_local_txid: None,
            latest_remote_txid: None,
            last_remote_confirmed_at: None,
            restart_count: 0,
            last_error_code: None,
        }
    }
}

enum SupervisorSignal {
    ReloadConfiguration,
    ConnectivityRestored,
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct LitestreamCheckpointHandle {
    shutdown: Arc<AtomicBool>,
    status: Arc<Mutex<RelationalBackupStatus>>,
    control: Arc<Mutex<Option<Arc<dyn CheckpointControl>>>>,
}

impl LitestreamCheckpointHandle {
    pub(crate) fn status(&self) -> RelationalBackupStatus {
        lock_status(&self.status).clone()
    }

    pub(crate) fn sync_remote(&self) -> Result<SyncResult, BackupErrorCode> {
        self.request_sync(false, CHECKPOINT_REMOTE_SYNC_TIMEOUT)
    }

    fn request_sync(
        &self,
        local_only: bool,
        timeout: Duration,
    ) -> Result<SyncResult, BackupErrorCode> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(BackupErrorCode::WorkerUnavailable);
        }
        let control = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or(BackupErrorCode::LitestreamUnavailable)?;
        let expected_control = Arc::clone(&control);
        let (reply, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name(if local_only {
                "dara-litestream-checkpoint-local".into()
            } else {
                "dara-litestream-checkpoint-remote".into()
            })
            .spawn(move || {
                let result = if local_only {
                    control.sync_local()
                } else {
                    control.sync_remote()
                }
                .map_err(|failure| failure.code);
                let _ = reply.send(result);
            })
            .map_err(|_| BackupErrorCode::WorkerUnavailable)?;
        let result = receiver
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout if local_only => BackupErrorCode::FenceTimeout,
                mpsc::RecvTimeoutError::Timeout => BackupErrorCode::NetworkTimeout,
                mpsc::RecvTimeoutError::Disconnected => BackupErrorCode::WorkerUnavailable,
            })??;
        let control_is_current = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &expected_control));
        if control_is_current {
            let mut status = lock_status(&self.status);
            status.latest_local_txid = Some(result.txid);
            if !local_only {
                status.phase = RelationalBackupPhase::Running;
                status.latest_remote_txid = result.replica_txid;
                status.last_remote_confirmed_at = now_millis().ok();
                status.last_error_code = None;
            }
        }
        Ok(result)
    }
}

impl LocalCheckpointSync for LitestreamCheckpointHandle {
    fn sync_local(&self) -> Result<LitestreamTxid, BackupErrorCode> {
        self.request_sync(true, CHECKPOINT_LOCAL_SYNC_TIMEOUT)
            .map(|sync| sync.txid)
    }
}

pub(crate) struct LitestreamRuntimeService {
    sender: mpsc::Sender<SupervisorSignal>,
    shutdown: Arc<AtomicBool>,
    status: Arc<Mutex<RelationalBackupStatus>>,
    checkpoint_control: Arc<Mutex<Option<Arc<dyn CheckpointControl>>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl LitestreamRuntimeService {
    pub(crate) fn start(
        database: DatabaseClient,
        data_root: PathBuf,
        database_path: PathBuf,
        resource_dir: PathBuf,
    ) -> Self {
        Self::start_with_parts(
            database,
            Arc::new(SystemRuntimeFactory {
                data_root,
                database_path,
                resource_dir,
                credentials: MacOsKeychainCredentialStore,
            }),
            WorkerSchedule::production(),
        )
    }

    fn start_with_parts(
        database: DatabaseClient,
        factory: Arc<dyn RuntimeFactory>,
        schedule: WorkerSchedule,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(RelationalBackupStatus::default()));
        let checkpoint_control = Arc::new(Mutex::new(None));
        let worker_shutdown = Arc::clone(&shutdown);
        let worker_status = Arc::clone(&status);
        let worker_checkpoint_control = Arc::clone(&checkpoint_control);
        let spawned = thread::Builder::new()
            .name("dara-litestream-supervisor".into())
            .spawn(move || {
                supervisor_worker(
                    database,
                    factory,
                    receiver,
                    worker_shutdown,
                    worker_status,
                    worker_checkpoint_control,
                    schedule,
                );
            });
        let worker = match spawned {
            Ok(worker) => Some(worker),
            Err(_) => {
                *lock_status(&status) = RelationalBackupStatus {
                    phase: RelationalBackupPhase::Unavailable,
                    last_error_code: Some(BackupErrorCode::WorkerUnavailable),
                    ..RelationalBackupStatus::default()
                };
                None
            }
        };
        let service = Self {
            sender,
            shutdown,
            status,
            checkpoint_control,
            worker: Mutex::new(worker),
        };
        service.reload_configuration();
        service
    }

    pub(crate) fn status(&self) -> RelationalBackupStatus {
        lock_status(&self.status).clone()
    }

    pub(crate) fn checkpoint_handle(&self) -> LitestreamCheckpointHandle {
        LitestreamCheckpointHandle {
            shutdown: Arc::clone(&self.shutdown),
            status: Arc::clone(&self.status),
            control: Arc::clone(&self.checkpoint_control),
        }
    }

    pub(crate) fn reload_configuration(&self) {
        self.signal(SupervisorSignal::ReloadConfiguration);
    }

    pub(crate) fn connectivity_restored(&self) {
        self.signal(SupervisorSignal::ConnectivityRestored);
    }

    pub(crate) fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.sender.send(SupervisorSignal::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            if worker.join().is_err() {
                log::error!("Litestream supervisor panicked during shutdown");
            }
        }
    }

    fn signal(&self, signal: SupervisorSignal) {
        if self.shutdown.load(Ordering::Acquire) {
            return;
        }
        if self.sender.send(signal).is_err() {
            let mut status = lock_status(&self.status);
            status.phase = RelationalBackupPhase::Unavailable;
            status.last_error_code = Some(BackupErrorCode::WorkerUnavailable);
        }
    }
}

impl Drop for LitestreamRuntimeService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone, Copy)]
struct WorkerSchedule {
    supervisor_poll_interval: Duration,
    config_refresh_interval: Duration,
    remote_status_interval: Duration,
    restart_policy: RestartPolicy,
}

impl WorkerSchedule {
    const fn production() -> Self {
        Self {
            supervisor_poll_interval: SUPERVISOR_POLL_INTERVAL,
            config_refresh_interval: CONFIG_REFRESH_INTERVAL,
            remote_status_interval: REMOTE_STATUS_INTERVAL,
            restart_policy: RestartPolicy {
                base: RESTART_BASE_DELAY,
                maximum: RESTART_MAX_DELAY,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct RestartPolicy {
    base: Duration,
    maximum: Duration,
}

impl RestartPolicy {
    fn delay(self, failure_count: u32) -> Duration {
        let exponent = failure_count.saturating_sub(1).min(20);
        let multiplier = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
        self.base.saturating_mul(multiplier).min(self.maximum)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeFailure {
    code: BackupErrorCode,
    retryable: bool,
}

impl RuntimeFailure {
    const fn new(code: BackupErrorCode, retryable: bool) -> Self {
        Self { code, retryable }
    }
}

trait RuntimeFactory: Send + Sync {
    fn sweep_stale(&self) -> Result<(), RuntimeFailure> {
        Ok(())
    }

    fn start(
        &self,
        config: &OffsiteBackupConfig,
    ) -> Result<Box<dyn ManagedLitestream>, RuntimeFailure>;
}

trait ManagedLitestream: Send {
    fn has_exited(&mut self) -> Result<bool, RuntimeFailure>;
    fn checkpoint_control(&self) -> Arc<dyn CheckpointControl>;
    fn shutdown(&mut self);
}

trait CheckpointControl: Send + Sync {
    fn sync_local(&self) -> Result<SyncResult, RuntimeFailure>;
    fn sync_remote(&self) -> Result<SyncResult, RuntimeFailure>;
}

fn supervisor_worker(
    database: DatabaseClient,
    factory: Arc<dyn RuntimeFactory>,
    receiver: mpsc::Receiver<SupervisorSignal>,
    shutdown: Arc<AtomicBool>,
    status: Arc<Mutex<RelationalBackupStatus>>,
    checkpoint_control: Arc<Mutex<Option<Arc<dyn CheckpointControl>>>>,
    schedule: WorkerSchedule,
) {
    let mut current_config: Option<OffsiteBackupConfig> = None;
    let mut daemon: Option<Box<dyn ManagedLitestream>> = None;
    let mut blocked_revision: Option<i64> = None;
    let mut restart_count = 0_u32;
    let mut next_start = Instant::now();
    let mut next_config_refresh = Instant::now();
    let mut next_remote_status = Instant::now();
    let mut force_reload = true;
    let mut force_remote_status = false;

    if let Err(failure) = factory.sweep_stale() {
        update_failure_status(&status, failure, restart_count);
        log::warn!(
            "could not safely sweep a stale Litestream runtime: {:?}",
            failure.code
        );
    }

    loop {
        let signal = match receiver.recv_timeout(schedule.supervisor_poll_interval) {
            Ok(signal) => Some(signal),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if shutdown.load(Ordering::Acquire)
            || matches!(signal.as_ref(), Some(SupervisorSignal::Shutdown))
        {
            break;
        }
        match signal {
            Some(SupervisorSignal::ReloadConfiguration) => {
                force_reload = true;
                blocked_revision = None;
                restart_count = 0;
                next_start = Instant::now();
            }
            Some(SupervisorSignal::ConnectivityRestored) => {
                force_remote_status = true;
                next_start = Instant::now();
            }
            Some(SupervisorSignal::Shutdown) | None => {}
        }

        let now = Instant::now();
        if force_reload || now >= next_config_refresh {
            match database.load_offsite_backup_config() {
                Ok(config) => {
                    let enabled = config.filter(|config| config.enabled);
                    let previous_revision = current_config.as_ref().map(|config| config.revision);
                    let next_revision = enabled.as_ref().map(|config| config.revision);
                    if force_reload || previous_revision != next_revision {
                        shutdown_daemon(&mut daemon, &checkpoint_control);
                        current_config = enabled;
                        blocked_revision = None;
                        restart_count = 0;
                        next_start = Instant::now();
                        next_remote_status = Instant::now();
                        force_remote_status = false;
                        *lock_status(&status) = if current_config.is_some() {
                            RelationalBackupStatus {
                                phase: RelationalBackupPhase::Starting,
                                ..RelationalBackupStatus::default()
                            }
                        } else {
                            RelationalBackupStatus::default()
                        };
                    }
                    force_reload = false;
                    next_config_refresh = now + schedule.config_refresh_interval;
                }
                Err(_) => {
                    force_reload = false;
                    update_failure_status(
                        &status,
                        RuntimeFailure::new(BackupErrorCode::WorkerUnavailable, true),
                        restart_count,
                    );
                    next_config_refresh = now + schedule.config_refresh_interval;
                }
            }
        }

        let Some(config) = current_config.as_ref() else {
            continue;
        };

        if let Some(running) = daemon.as_mut() {
            match running.has_exited() {
                Ok(false) => {}
                Ok(true) | Err(_) => {
                    shutdown_daemon(&mut daemon, &checkpoint_control);
                    let failure = RuntimeFailure::new(BackupErrorCode::LitestreamFailed, true);
                    schedule_restart(
                        &status,
                        failure,
                        &mut restart_count,
                        &mut next_start,
                        schedule.restart_policy,
                    );
                    continue;
                }
            }
        }

        if daemon.is_none()
            && blocked_revision != Some(config.revision)
            && Instant::now() >= next_start
        {
            lock_status(&status).phase = RelationalBackupPhase::Starting;
            match factory.start(config) {
                Ok(started) => {
                    *checkpoint_control
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                        Some(started.checkpoint_control());
                    daemon = Some(started);
                    next_remote_status = Instant::now();
                }
                Err(failure) => {
                    if failure.retryable {
                        schedule_restart(
                            &status,
                            failure,
                            &mut restart_count,
                            &mut next_start,
                            schedule.restart_policy,
                        );
                    } else {
                        blocked_revision = Some(config.revision);
                        update_failure_status(&status, failure, restart_count);
                    }
                    continue;
                }
            }
        }

        if daemon.is_some() && (force_remote_status || Instant::now() >= next_remote_status) {
            force_remote_status = false;
            let sync = checkpoint_control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .ok_or_else(|| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, true))
                .and_then(|control| control.sync_remote());
            match sync {
                Ok(sync) => {
                    restart_count = 0;
                    let mut current = lock_status(&status);
                    current.phase = RelationalBackupPhase::Running;
                    current.latest_local_txid = Some(sync.txid);
                    current.latest_remote_txid = sync.replica_txid;
                    current.last_remote_confirmed_at = now_millis().ok();
                    current.restart_count = 0;
                    current.last_error_code = None;
                }
                Err(failure) if failure.retryable => {
                    update_failure_status(&status, failure, restart_count);
                }
                Err(failure) => {
                    shutdown_daemon(&mut daemon, &checkpoint_control);
                    blocked_revision = Some(config.revision);
                    update_failure_status(&status, failure, restart_count);
                }
            }
            next_remote_status = Instant::now() + schedule.remote_status_interval;
        }
    }

    shutdown_daemon(&mut daemon, &checkpoint_control);
}

fn schedule_restart(
    status: &Mutex<RelationalBackupStatus>,
    failure: RuntimeFailure,
    restart_count: &mut u32,
    next_start: &mut Instant,
    policy: RestartPolicy,
) {
    *restart_count = restart_count.saturating_add(1);
    *next_start = Instant::now() + policy.delay(*restart_count);
    update_failure_status(status, failure, *restart_count);
}

fn update_failure_status(
    status: &Mutex<RelationalBackupStatus>,
    failure: RuntimeFailure,
    restart_count: u32,
) {
    let mut current = lock_status(status);
    current.phase = match failure.code {
        BackupErrorCode::KeychainCredentialMissing => RelationalBackupPhase::WaitingForCredentials,
        BackupErrorCode::KeychainUnavailable
        | BackupErrorCode::WorkerUnavailable
        | BackupErrorCode::LitestreamUnavailable => RelationalBackupPhase::Unavailable,
        _ if failure.retryable => RelationalBackupPhase::Degraded,
        _ => RelationalBackupPhase::Blocked,
    };
    current.restart_count = restart_count;
    current.last_error_code = Some(failure.code);
}

fn shutdown_daemon(
    daemon: &mut Option<Box<dyn ManagedLitestream>>,
    checkpoint_control: &Mutex<Option<Arc<dyn CheckpointControl>>>,
) {
    checkpoint_control
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(mut daemon) = daemon.take() {
        daemon.shutdown();
    }
}

fn lock_status(
    status: &Mutex<RelationalBackupStatus>,
) -> std::sync::MutexGuard<'_, RelationalBackupStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn is_transient(code: BackupErrorCode) -> bool {
    matches!(
        code,
        BackupErrorCode::NetworkOffline
            | BackupErrorCode::NetworkTimeout
            | BackupErrorCode::RateLimited
            | BackupErrorCode::ServiceUnavailable
            | BackupErrorCode::RemoteMediaMissing
            | BackupErrorCode::WorkerUnavailable
            | BackupErrorCode::KeychainUnavailable
            | BackupErrorCode::LitestreamFailed
    )
}

fn map_credential_start_error(error: CredentialError) -> RuntimeFailure {
    let code = map_credential_error(error);
    RuntimeFailure::new(
        code,
        matches!(
            code,
            BackupErrorCode::KeychainCredentialMissing | BackupErrorCode::KeychainUnavailable
        ),
    )
}

struct SystemRuntimeFactory<C> {
    data_root: PathBuf,
    database_path: PathBuf,
    resource_dir: PathBuf,
    credentials: C,
}

impl<C: CredentialStore> RuntimeFactory for SystemRuntimeFactory<C> {
    fn sweep_stale(&self) -> Result<(), RuntimeFailure> {
        let runtime =
            LitestreamRuntimePaths::new(&self.data_root).map_err(map_litestream_start_error)?;
        if fs::symlink_metadata(runtime.pid()).is_err()
            && fs::symlink_metadata(runtime.socket()).is_err()
        {
            return Ok(());
        }
        let binary = VerifiedLitestreamBinary::resolve(&self.resource_dir)
            .map_err(map_litestream_start_error)?;
        runtime.prepare().map_err(map_litestream_start_error)?;
        sweep_stale_runtime(&runtime, binary.path(), &self.database_path)
    }

    fn start(
        &self,
        config: &OffsiteBackupConfig,
    ) -> Result<Box<dyn ManagedLitestream>, RuntimeFailure> {
        let binary = VerifiedLitestreamBinary::resolve(&self.resource_dir)
            .map_err(map_litestream_start_error)?;
        let runtime =
            LitestreamRuntimePaths::new(&self.data_root).map_err(map_litestream_start_error)?;
        runtime.prepare().map_err(map_litestream_start_error)?;
        sweep_stale_runtime(&runtime, binary.path(), &self.database_path)?;

        let credentials = self
            .credentials
            .load(&config.backup_set_id)
            .map_err(map_credential_start_error)?;
        let installation_id = InstallationIdentityStore::new(&self.data_root)
            .and_then(|store| store.load_or_create())
            .map_err(|_| RuntimeFailure::new(BackupErrorCode::WorkerUnavailable, true))?;
        let store = R2ObjectStore::new(config.target.clone(), &credentials)
            .map_err(|_| RuntimeFailure::new(BackupErrorCode::InvalidTarget, false))?;
        let store: Arc<dyn ObjectStore> = Arc::new(store);
        validate_backup_authority(store.as_ref(), config, &installation_id)
            .map_err(|code| RuntimeFailure::new(code, is_transient(code)))?;

        let replica_path = config
            .target
            .keyspace()
            .litestream(&config.replica_epoch_id);
        let endpoint = config.target.endpoint();
        let rendered = LitestreamConfig {
            database_path: &self.database_path,
            runtime: &runtime,
            bucket: config.target.bucket.as_str(),
            replica_path: replica_path.as_str(),
            endpoint: &endpoint,
        }
        .render()
        .map_err(map_litestream_start_error)?;
        runtime
            .write_config(&rendered)
            .map_err(map_litestream_start_error)?;
        let config_sha256 = sha256_hex(rendered.as_bytes());
        let daemon = SystemManagedLitestream::launch(
            binary.path().to_owned(),
            runtime,
            self.database_path.clone(),
            self.data_root.clone(),
            config.backup_set_id.as_str().to_owned(),
            config.replica_epoch_id.as_str().to_owned(),
            config_sha256,
            credentials,
            RemoteAuthority {
                store,
                config: config.clone(),
                installation_id,
            },
        )?;
        Ok(Box::new(daemon))
    }
}

fn map_litestream_start_error(error: LitestreamError) -> RuntimeFailure {
    match error {
        LitestreamError::PrepareRuntime(_)
        | LitestreamError::WriteConfig(_)
        | LitestreamError::Execute(_) => {
            RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, true)
        }
        LitestreamError::InvalidConfigField(_) | LitestreamError::RelativeDatabasePath => {
            RuntimeFailure::new(BackupErrorCode::InvalidTarget, false)
        }
        LitestreamError::CommandFailed { .. } => {
            RuntimeFailure::new(BackupErrorCode::LitestreamFailed, true)
        }
        LitestreamError::InvalidJson(_)
        | LitestreamError::InvalidTxid
        | LitestreamError::InvalidSyncContract
        | LitestreamError::UnexpectedDatabasePath
        | LitestreamError::OversizedControlResponse => {
            RuntimeFailure::new(BackupErrorCode::LitestreamFailed, false)
        }
        LitestreamError::InvalidEmbeddedManifest(_)
        | LitestreamError::MissingReleaseManifest(_)
        | LitestreamError::ReleaseManifestMismatch
        | LitestreamError::MissingBinary(_)
        | LitestreamError::BinaryNotRegular
        | LitestreamError::BinaryNotExecutable
        | LitestreamError::BinarySizeMismatch
        | LitestreamError::BinaryChecksumMismatch
        | LitestreamError::UnsafeL0Retention
        | LitestreamError::NonUtf8RuntimePath
        | LitestreamError::ControlSocketPathTooLong
        | LitestreamError::RestorePlanTooLarge => {
            RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, false)
        }
    }
}

fn map_checkpoint_local_sync_error(error: LitestreamError) -> RuntimeFailure {
    if matches!(
        &error,
        LitestreamError::Execute(source) if source.kind() == std::io::ErrorKind::TimedOut
    ) {
        RuntimeFailure::new(BackupErrorCode::FenceTimeout, true)
    } else {
        map_litestream_start_error(error)
    }
}

fn map_checkpoint_remote_sync_error(error: LitestreamError) -> RuntimeFailure {
    if matches!(
        &error,
        LitestreamError::Execute(source) if source.kind() == std::io::ErrorKind::TimedOut
    ) {
        RuntimeFailure::new(BackupErrorCode::NetworkTimeout, true)
    } else {
        map_litestream_start_error(error)
    }
}

struct SystemManagedLitestream {
    child: Option<Child>,
    runtime: LitestreamRuntimePaths,
    checkpoint: Arc<SystemCheckpointControl>,
    log_threads: Vec<JoinHandle<()>>,
}

struct SystemCheckpointControl {
    database_path: PathBuf,
    control: CommandLitestreamControl<SystemCommandExecutor>,
    authority: RemoteAuthority,
}

struct RemoteAuthority {
    store: Arc<dyn ObjectStore>,
    config: OffsiteBackupConfig,
    installation_id: InstallationId,
}

impl RemoteAuthority {
    fn validate(&self) -> Result<(), RuntimeFailure> {
        validate_backup_authority(self.store.as_ref(), &self.config, &self.installation_id)
            .map_err(|code| RuntimeFailure::new(code, is_transient(code)))
    }
}

struct LaunchIdentity {
    backup_set_id: String,
    replica_epoch_id: String,
    config_sha256: String,
}

impl SystemManagedLitestream {
    #[allow(clippy::too_many_arguments)]
    fn launch(
        binary: PathBuf,
        runtime: LitestreamRuntimePaths,
        database_path: PathBuf,
        data_root: PathBuf,
        backup_set_id: String,
        replica_epoch_id: String,
        config_sha256: String,
        credentials: R2Credentials,
        authority: RemoteAuthority,
    ) -> Result<Self, RuntimeFailure> {
        let log_directory = data_root.join("logs");
        let log_writer = Arc::new(Mutex::new(
            BoundedLitestreamLog::open(&log_directory)
                .map_err(|_| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, true))?,
        ));
        let secrets = Arc::new(SecretPatterns::new(&credentials));
        let mut command = Command::new(&binary);
        command
            .arg("replicate")
            .arg("-config")
            .arg(runtime.config())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_credentials_environment(&mut command, &credentials);
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|_| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, true))?;
        let log_threads = match start_log_pumps(&mut child, log_writer, secrets) {
            Ok(threads) => threads,
            Err(_) => {
                terminate_process_group(&mut child, DAEMON_SHUTDOWN_TIMEOUT);
                return Err(RuntimeFailure::new(
                    BackupErrorCode::LitestreamUnavailable,
                    true,
                ));
            }
        };
        let pid = child.id();
        let identity = LaunchIdentity {
            backup_set_id,
            replica_epoch_id,
            config_sha256,
        };
        if write_pid_record(&runtime, pid, &binary, &database_path, &identity).is_err() {
            terminate_process_group(&mut child, DAEMON_SHUTDOWN_TIMEOUT);
            join_log_threads(log_threads);
            return Err(RuntimeFailure::new(
                BackupErrorCode::LitestreamUnavailable,
                true,
            ));
        }
        if let Err(failure) = wait_for_control_socket(&mut child, &runtime) {
            terminate_process_group(&mut child, DAEMON_SHUTDOWN_TIMEOUT);
            remove_pid_record_if_owned(&runtime, pid);
            remove_socket_if_present(&runtime);
            join_log_threads(log_threads);
            return Err(failure);
        }
        let control = CommandLitestreamControl::new(
            binary.clone(),
            runtime.socket().to_owned(),
            CONTROL_REMOTE_TIMEOUT_SECONDS,
            SystemCommandExecutor,
        );
        Ok(Self {
            child: Some(child),
            runtime,
            checkpoint: Arc::new(SystemCheckpointControl {
                database_path,
                control,
                authority,
            }),
            log_threads,
        })
    }

    fn cleanup(&mut self, pid: u32) {
        remove_pid_record_if_owned(&self.runtime, pid);
        remove_socket_if_present(&self.runtime);
        join_log_threads(self.log_threads.drain(..));
    }
}

impl ManagedLitestream for SystemManagedLitestream {
    fn has_exited(&mut self) -> Result<bool, RuntimeFailure> {
        let Some(child) = self.child.as_mut() else {
            return Ok(true);
        };
        match child.try_wait() {
            Ok(None) => Ok(false),
            Ok(Some(_)) => {
                let pid = child.id();
                self.child.take();
                self.cleanup(pid);
                Ok(true)
            }
            Err(_) => Err(RuntimeFailure::new(BackupErrorCode::LitestreamFailed, true)),
        }
    }

    fn checkpoint_control(&self) -> Arc<dyn CheckpointControl> {
        self.checkpoint.clone()
    }

    fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            terminate_process_group(&mut child, DAEMON_SHUTDOWN_TIMEOUT);
            self.cleanup(pid);
        } else {
            join_log_threads(self.log_threads.drain(..));
        }
    }
}

impl CheckpointControl for SystemCheckpointControl {
    fn sync_local(&self) -> Result<SyncResult, RuntimeFailure> {
        self.control
            .sync_local_with_timeout(&self.database_path, CHECKPOINT_LOCAL_COMMAND_TIMEOUT)
            .map_err(map_checkpoint_local_sync_error)
    }

    fn sync_remote(&self) -> Result<SyncResult, RuntimeFailure> {
        self.authority.validate()?;
        self.control
            .sync_remote_with_timeout(&self.database_path, CHECKPOINT_REMOTE_COMMAND_TIMEOUT)
            .map_err(map_checkpoint_remote_sync_error)
    }
}

impl Drop for SystemManagedLitestream {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn wait_for_control_socket(
    child: &mut Child,
    runtime: &LitestreamRuntimePaths,
) -> Result<(), RuntimeFailure> {
    let deadline = Instant::now() + DAEMON_STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => {
                return Err(RuntimeFailure::new(
                    BackupErrorCode::LitestreamFailed,
                    false,
                ));
            }
            Ok(None) => {}
            Err(_) => {
                return Err(RuntimeFailure::new(BackupErrorCode::LitestreamFailed, true));
            }
        }
        match control_socket_is_private(runtime.socket()) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(()) => {
                return Err(RuntimeFailure::new(
                    BackupErrorCode::LitestreamUnavailable,
                    false,
                ));
            }
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    Err(RuntimeFailure::new(BackupErrorCode::LitestreamFailed, true))
}

#[cfg(unix)]
fn control_socket_is_private(path: &Path) -> Result<bool, ()> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket() {
                return Err(());
            }
            if metadata.permissions().mode() & 0o777 != 0o600 {
                return Err(());
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(()),
    }
}

#[cfg(not(unix))]
fn control_socket_is_private(path: &Path) -> Result<bool, ()> {
    Ok(path.exists())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LitestreamPidRecord {
    format_version: u32,
    pid: u32,
    executable: String,
    config: String,
    socket: String,
    database: String,
    backup_set_id: String,
    replica_epoch_id: String,
    config_sha256: String,
}

fn write_pid_record(
    runtime: &LitestreamRuntimePaths,
    pid: u32,
    binary: &Path,
    database_path: &Path,
    identity: &LaunchIdentity,
) -> std::io::Result<()> {
    let record = LitestreamPidRecord {
        format_version: PID_RECORD_FORMAT_VERSION,
        pid,
        executable: utf8_path(binary)?,
        config: utf8_path(runtime.config())?,
        socket: utf8_path(runtime.socket())?,
        database: utf8_path(database_path)?,
        backup_set_id: identity.backup_set_id.clone(),
        replica_epoch_id: identity.replica_epoch_id.clone(),
        config_sha256: identity.config_sha256.clone(),
    };
    let bytes = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
    if bytes.len() as u64 > MAX_PID_RECORD_BYTES {
        return Err(std::io::Error::other("Litestream PID record is oversized"));
    }
    write_private_atomic(runtime.pid(), &bytes)
}

fn read_pid_record(
    runtime: &LitestreamRuntimePaths,
) -> std::io::Result<Option<LitestreamPidRecord>> {
    let metadata = match fs::symlink_metadata(runtime.pid()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PID_RECORD_BYTES
    {
        return Err(std::io::Error::other("invalid Litestream PID record"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::other(
                "Litestream PID record is not private",
            ));
        }
    }
    let bytes = fs::read(runtime.pid())?;
    let record: LitestreamPidRecord =
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if record.format_version != PID_RECORD_FORMAT_VERSION {
        return Err(std::io::Error::other("unsupported Litestream PID record"));
    }
    Ok(Some(record))
}

fn sweep_stale_runtime(
    runtime: &LitestreamRuntimePaths,
    expected_binary: &Path,
    expected_database_path: &Path,
) -> Result<(), RuntimeFailure> {
    let record = read_pid_record(runtime)
        .map_err(|_| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, false))?;
    let Some(record) = record else {
        if runtime.socket().exists() {
            return Err(RuntimeFailure::new(
                BackupErrorCode::LitestreamUnavailable,
                false,
            ));
        }
        return Ok(());
    };
    let expected_binary = utf8_path(expected_binary)
        .map_err(|_| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, false))?;
    let expected_database = utf8_path(expected_database_path)
        .map_err(|_| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, false))?;
    if record.executable != expected_binary
        || record.config != runtime.config().to_string_lossy()
        || record.socket != runtime.socket().to_string_lossy()
        || record.database != expected_database
    {
        return Err(RuntimeFailure::new(
            BackupErrorCode::LitestreamUnavailable,
            false,
        ));
    }
    let config = fs::read(runtime.config())
        .map_err(|_| RuntimeFailure::new(BackupErrorCode::LitestreamUnavailable, false))?;
    if sha256_hex(&config) != record.config_sha256 {
        return Err(RuntimeFailure::new(
            BackupErrorCode::LitestreamUnavailable,
            false,
        ));
    }
    if !process_exists(record.pid) {
        remove_pid_record_if_owned(runtime, record.pid);
        remove_socket_if_present(runtime);
        return Ok(());
    }
    if !process_matches_record(&record) {
        return Err(RuntimeFailure::new(
            BackupErrorCode::LitestreamUnavailable,
            false,
        ));
    }
    terminate_stale_process_group(&record);
    if process_exists(record.pid) {
        return Err(RuntimeFailure::new(
            BackupErrorCode::LitestreamUnavailable,
            false,
        ));
    }
    remove_pid_record_if_owned(runtime, record.pid);
    remove_socket_if_present(runtime);
    Ok(())
}

fn process_matches_record(record: &LitestreamPidRecord) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-p", &record.pid.to_string(), "-o", "command="])
        .output()
    else {
        return false;
    };
    let command = String::from_utf8_lossy(&output.stdout);
    output.status.success()
        && process_group_matches(record.pid)
        && command.contains(&record.executable)
        && command.contains("replicate")
        && command.contains("-config")
        && command.contains(&record.config)
}

#[cfg(target_os = "macos")]
fn process_group_matches(pid: u32) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pgid="])
        .output()
    else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            == Ok(pid)
}

#[cfg(not(target_os = "macos"))]
fn process_group_matches(_pid: u32) -> bool {
    true
}

#[cfg(target_os = "macos")]
fn process_exists(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 checks process existence without changing it.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(target_os = "macos"))]
fn process_exists(pid: u32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn terminate_process_group(child: &mut Child, timeout: Duration) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    // SAFETY: launch created a process group whose ID is the child PID.
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    // SAFETY: this is still the child-owned process group.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

#[cfg(not(target_os = "macos"))]
fn terminate_process_group(child: &mut Child, _timeout: Duration) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "macos")]
fn terminate_stale_process_group(record: &LitestreamPidRecord) {
    let Ok(pid) = i32::try_from(record.pid) else {
        return;
    };
    // SAFETY: the caller matched the PID record to the running process.
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + DAEMON_SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if !process_exists(record.pid) {
            return;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    if !process_matches_record(record) {
        return;
    }
    // SAFETY: the verified process group did not exit after SIGTERM.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let deadline = Instant::now() + STALE_KILL_WAIT_TIMEOUT;
    while Instant::now() < deadline {
        if !process_exists(record.pid) {
            return;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

#[cfg(not(target_os = "macos"))]
fn terminate_stale_process_group(_record: &LitestreamPidRecord) {}

fn remove_pid_record_if_owned(runtime: &LitestreamRuntimePaths, expected_pid: u32) {
    let Ok(Some(record)) = read_pid_record(runtime) else {
        return;
    };
    if record.pid == expected_pid {
        let _ = fs::remove_file(runtime.pid());
    }
}

fn remove_socket_if_present(runtime: &LitestreamRuntimePaths) {
    match fs::remove_file(runtime.socket()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    drop(file);
    fs::rename(temporary, path)
}

fn utf8_path(path: &Path) -> std::io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("path is not valid UTF-8"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn start_log_pumps(
    child: &mut Child,
    writer: Arc<Mutex<BoundedLitestreamLog>>,
    secrets: Arc<SecretPatterns>,
) -> std::io::Result<Vec<JoinHandle<()>>> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("Litestream stdout pipe is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("Litestream stderr pipe is unavailable"))?;
    let stdout_writer = Arc::clone(&writer);
    let stdout_secrets = Arc::clone(&secrets);
    let stdout_thread = thread::Builder::new()
        .name("dara-litestream-stdout".into())
        .spawn(move || copy_redacted_log(stdout, stdout_writer, stdout_secrets))?;
    let stderr_thread = match thread::Builder::new()
        .name("dara-litestream-stderr".into())
        .spawn(move || copy_redacted_log(stderr, writer, secrets))
    {
        Ok(thread) => thread,
        Err(error) => {
            terminate_process_group(child, DAEMON_SHUTDOWN_TIMEOUT);
            join_log_threads([stdout_thread]);
            return Err(error);
        }
    };
    Ok(vec![stdout_thread, stderr_thread])
}

fn copy_redacted_log(
    mut source: impl Read,
    writer: Arc<Mutex<BoundedLitestreamLog>>,
    secrets: Arc<SecretPatterns>,
) {
    let mut redactor = StreamRedactor::new(secrets);
    let mut buffer = [0_u8; LOG_COPY_BUFFER_BYTES];
    loop {
        match source.read(&mut buffer) {
            Ok(0) => {
                let _ = redactor.finish(&mut lock_log(&writer));
                return;
            }
            Ok(read) => {
                if redactor
                    .push(&buffer[..read], &mut lock_log(&writer))
                    .is_err()
                {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

fn lock_log(
    writer: &Mutex<BoundedLitestreamLog>,
) -> std::sync::MutexGuard<'_, BoundedLitestreamLog> {
    writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn join_log_threads(threads: impl IntoIterator<Item = JoinHandle<()>>) {
    for thread in threads {
        if thread.join().is_err() {
            log::error!("Litestream log worker panicked");
        }
    }
}

struct SecretPatterns {
    values: Vec<Zeroizing<Vec<u8>>>,
    maximum_length: usize,
}

impl SecretPatterns {
    fn new(credentials: &R2Credentials) -> Self {
        let mut values = vec![
            Zeroizing::new(credentials.access_key_id().as_bytes().to_vec()),
            Zeroizing::new(credentials.secret_access_key().as_bytes().to_vec()),
        ];
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        let maximum_length = values.iter().map(|value| value.len()).max().unwrap_or(1);
        Self {
            values,
            maximum_length,
        }
    }
}

struct StreamRedactor {
    secrets: Arc<SecretPatterns>,
    pending: Vec<u8>,
}

impl StreamRedactor {
    fn new(secrets: Arc<SecretPatterns>) -> Self {
        Self {
            secrets,
            pending: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8], writer: &mut BoundedLitestreamLog) -> std::io::Result<()> {
        self.pending.extend_from_slice(bytes);
        self.flush(false, writer)
    }

    fn finish(&mut self, writer: &mut BoundedLitestreamLog) -> std::io::Result<()> {
        self.flush(true, writer)
    }

    fn flush(
        &mut self,
        final_chunk: bool,
        writer: &mut BoundedLitestreamLog,
    ) -> std::io::Result<()> {
        let safe_end = if final_chunk {
            self.pending.len()
        } else {
            self.pending
                .len()
                .saturating_sub(self.secrets.maximum_length.saturating_sub(1))
        };
        if safe_end == 0 {
            return Ok(());
        }
        let mut output = Vec::with_capacity(safe_end);
        let mut consumed = 0;
        while consumed < safe_end {
            let matched = self
                .secrets
                .values
                .iter()
                .find(|secret| self.pending[consumed..].starts_with(secret.as_slice()));
            if let Some(secret) = matched {
                output.extend_from_slice(REDACTED);
                consumed += secret.len();
            } else {
                output.push(self.pending[consumed]);
                consumed += 1;
            }
        }
        self.pending.drain(..consumed);
        writer.write_bounded(&output)
    }
}

struct BoundedLitestreamLog {
    directory: PathBuf,
    maximum_bytes: u64,
    active: File,
    active_bytes: u64,
}

impl BoundedLitestreamLog {
    fn open(directory: &Path) -> std::io::Result<Self> {
        Self::open_with_limit(directory, LITESTREAM_LOG_MAX_BYTES)
    }

    fn open_with_limit(directory: &Path, maximum_bytes: u64) -> std::io::Result<Self> {
        if maximum_bytes == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Litestream log limit must be positive",
            ));
        }
        fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
        trim_file_to_tail(&directory.join(LITESTREAM_LOG_ARCHIVE_NAME), maximum_bytes)?;
        let active_path = directory.join(LITESTREAM_LOG_FILE_NAME);
        trim_file_to_tail(&active_path, maximum_bytes)?;
        if active_path
            .metadata()
            .is_ok_and(|metadata| metadata.len() >= maximum_bytes)
        {
            rotate_litestream_log(directory)?;
        }
        let active = open_private_append(&active_path)?;
        let active_bytes = active.metadata()?.len();
        Ok(Self {
            directory: directory.to_owned(),
            maximum_bytes,
            active,
            active_bytes,
        })
    }

    fn write_bounded(&mut self, mut bytes: &[u8]) -> std::io::Result<()> {
        if bytes.len() as u64 > self.maximum_bytes {
            let keep = usize::try_from(self.maximum_bytes)
                .map_err(|_| std::io::Error::other("Litestream log limit is too large"))?;
            bytes = &bytes[bytes.len() - keep..];
        }
        if self.active_bytes.saturating_add(bytes.len() as u64) > self.maximum_bytes {
            self.rotate()?;
        }
        self.active.write_all(bytes)?;
        self.active_bytes = self.active_bytes.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.active.flush()?;
        rotate_litestream_log(&self.directory)?;
        self.active = open_private_append(&self.directory.join(LITESTREAM_LOG_FILE_NAME))?;
        self.active_bytes = 0;
        Ok(())
    }
}

fn open_private_append(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn rotate_litestream_log(directory: &Path) -> std::io::Result<()> {
    let active = directory.join(LITESTREAM_LOG_FILE_NAME);
    let archive = directory.join(LITESTREAM_LOG_ARCHIVE_NAME);
    match fs::remove_file(&archive) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::rename(active, archive) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn trim_file_to_tail(path: &Path, maximum_bytes: u64) -> std::io::Result<()> {
    let length = match path.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if length <= maximum_bytes {
        return Ok(());
    }
    let mut source = File::open(path)?;
    source.seek(SeekFrom::Start(length - maximum_bytes))?;
    let capacity = usize::try_from(maximum_bytes)
        .map_err(|_| std::io::Error::other("Litestream log limit is too large"))?;
    let mut tail = Vec::with_capacity(capacity);
    source.read_to_end(&mut tail)?;
    drop(source);
    let mut destination = OpenOptions::new().write(true).truncate(true).open(path)?;
    destination.write_all(&tail)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::atomic::AtomicUsize};

    use tempfile::TempDir;

    use super::*;
    use crate::{
        backup::domain::{
            BackupSetId, R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix, R2Target,
            ReplicaEpochId,
        },
        database::{
            initialize, Database, DatabasePaths, InitializationOptions,
            SaveOffsiteBackupConfigInput,
        },
    };

    const TEST_ACCESS_KEY: &str = "0123456789abcdef0123456789abcdef";
    const TEST_SECRET_KEY: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    enum StartPlan {
        Failure(RuntimeFailure),
        Daemon {
            exited: Arc<AtomicBool>,
            sync: Result<SyncResult, RuntimeFailure>,
        },
    }

    struct FakeRuntimeFactory {
        plans: Mutex<VecDeque<StartPlan>>,
        sweeps: AtomicUsize,
        starts: AtomicUsize,
        shutdowns: Arc<AtomicUsize>,
    }

    impl FakeRuntimeFactory {
        fn new(plans: impl IntoIterator<Item = StartPlan>) -> Arc<Self> {
            Arc::new(Self {
                plans: Mutex::new(plans.into_iter().collect()),
                sweeps: AtomicUsize::new(0),
                starts: AtomicUsize::new(0),
                shutdowns: Arc::new(AtomicUsize::new(0)),
            })
        }

        fn starts(&self) -> usize {
            self.starts.load(Ordering::Acquire)
        }

        fn sweeps(&self) -> usize {
            self.sweeps.load(Ordering::Acquire)
        }

        fn shutdowns(&self) -> usize {
            self.shutdowns.load(Ordering::Acquire)
        }
    }

    impl RuntimeFactory for FakeRuntimeFactory {
        fn sweep_stale(&self) -> Result<(), RuntimeFailure> {
            self.sweeps.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }

        fn start(
            &self,
            _config: &OffsiteBackupConfig,
        ) -> Result<Box<dyn ManagedLitestream>, RuntimeFailure> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            match self
                .plans
                .lock()
                .expect("fake runtime plans")
                .pop_front()
                .unwrap_or(StartPlan::Failure(RuntimeFailure::new(
                    BackupErrorCode::WorkerUnavailable,
                    false,
                ))) {
                StartPlan::Failure(failure) => Err(failure),
                StartPlan::Daemon { exited, sync } => Ok(Box::new(FakeManagedLitestream {
                    exited,
                    checkpoint: Arc::new(FakeCheckpointControl { sync }),
                    shutdowns: Arc::clone(&self.shutdowns),
                    stopped: false,
                })),
            }
        }
    }

    struct FakeManagedLitestream {
        exited: Arc<AtomicBool>,
        checkpoint: Arc<FakeCheckpointControl>,
        shutdowns: Arc<AtomicUsize>,
        stopped: bool,
    }

    struct FakeCheckpointControl {
        sync: Result<SyncResult, RuntimeFailure>,
    }

    impl ManagedLitestream for FakeManagedLitestream {
        fn has_exited(&mut self) -> Result<bool, RuntimeFailure> {
            Ok(self.exited.load(Ordering::Acquire))
        }

        fn checkpoint_control(&self) -> Arc<dyn CheckpointControl> {
            self.checkpoint.clone()
        }

        fn shutdown(&mut self) {
            if !self.stopped {
                self.stopped = true;
                self.shutdowns.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    impl CheckpointControl for FakeCheckpointControl {
        fn sync_local(&self) -> Result<SyncResult, RuntimeFailure> {
            self.sync.clone()
        }

        fn sync_remote(&self) -> Result<SyncResult, RuntimeFailure> {
            self.sync.clone()
        }
    }

    fn test_target() -> R2Target {
        R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/runtime-test").expect("prefix"),
        }
    }

    fn test_database(enabled: bool) -> (TempDir, Database, OffsiteBackupConfig) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = DatabasePaths::new(directory.path().join("data"));
        let database = initialize(
            paths,
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
                backup_set_id: BackupSetId::new(),
                replica_epoch_id: ReplicaEpochId::new(),
                enabled,
                target: test_target(),
            })
            .expect("backup config");
        (directory, database, config)
    }

    fn successful_plan(database_path: &Path) -> (StartPlan, Arc<AtomicBool>) {
        let exited = Arc::new(AtomicBool::new(false));
        (
            StartPlan::Daemon {
                exited: Arc::clone(&exited),
                sync: Ok(SyncResult {
                    database_path: database_path.to_owned(),
                    txid: LitestreamTxid::from_local(7),
                    replica_txid: Some(LitestreamTxid::from_local(7)),
                    duration_ms: 2,
                }),
            },
            exited,
        )
    }

    fn test_schedule() -> WorkerSchedule {
        WorkerSchedule {
            supervisor_poll_interval: Duration::from_millis(2),
            config_refresh_interval: Duration::from_millis(20),
            remote_status_interval: Duration::from_millis(10),
            restart_policy: RestartPolicy {
                base: Duration::from_millis(2),
                maximum: Duration::from_millis(10),
            },
        }
    }

    fn start_test_service(
        database: &Database,
        factory: Arc<dyn RuntimeFactory>,
    ) -> LitestreamRuntimeService {
        LitestreamRuntimeService::start_with_parts(database.client(), factory, test_schedule())
    }

    fn wait_until(timeout: Duration, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(condition(), "condition was not reached before timeout");
    }

    #[test]
    fn disabled_configuration_never_starts_litestream() {
        let (_directory, database, _config) = test_database(false);
        let factory = FakeRuntimeFactory::new([]);
        let service = start_test_service(&database, factory.clone());
        thread::sleep(Duration::from_millis(40));
        assert_eq!(factory.sweeps(), 1);
        assert_eq!(factory.starts(), 0);
        assert_eq!(service.status().phase, RelationalBackupPhase::Off);
        service.shutdown();
    }

    #[test]
    fn enabled_configuration_starts_and_reports_remote_sync() {
        let (_directory, database, _config) = test_database(true);
        let (plan, _exited) = successful_plan(&database.paths().main);
        let factory = FakeRuntimeFactory::new([plan]);
        let service = start_test_service(&database, factory.clone());
        wait_until(Duration::from_secs(2), || {
            service.status().phase == RelationalBackupPhase::Running
        });
        let status = service.status();
        assert_eq!(factory.starts(), 1);
        assert_eq!(
            status.latest_local_txid,
            Some(LitestreamTxid::from_local(7))
        );
        assert_eq!(
            status.latest_remote_txid,
            Some(LitestreamTxid::from_local(7))
        );
        assert!(status.last_remote_confirmed_at.is_some());
        service.shutdown();
        assert_eq!(factory.shutdowns(), 1);
    }

    #[test]
    fn checkpoint_handle_can_sync_without_round_tripping_through_the_supervisor_loop() {
        let (_directory, database, _config) = test_database(true);
        let (plan, _exited) = successful_plan(&database.paths().main);
        let factory = FakeRuntimeFactory::new([plan]);
        let service = start_test_service(&database, factory);
        wait_until(Duration::from_secs(2), || {
            service.status().phase == RelationalBackupPhase::Running
        });

        let handle = service.checkpoint_handle();
        assert_eq!(
            LocalCheckpointSync::sync_local(&handle).expect("local checkpoint sync"),
            LitestreamTxid::from_local(7)
        );
        assert_eq!(
            handle
                .sync_remote()
                .expect("remote checkpoint sync")
                .replica_txid,
            Some(LitestreamTxid::from_local(7))
        );
        service.shutdown();
    }

    #[test]
    fn local_command_timeout_maps_to_a_retryable_fence_timeout() {
        let failure = map_checkpoint_local_sync_error(LitestreamError::Execute(
            std::io::Error::new(std::io::ErrorKind::TimedOut, "test timeout"),
        ));

        assert_eq!(
            failure,
            RuntimeFailure::new(BackupErrorCode::FenceTimeout, true)
        );
    }

    #[test]
    fn remote_command_timeout_maps_to_a_retryable_network_timeout() {
        let failure = map_checkpoint_remote_sync_error(LitestreamError::Execute(
            std::io::Error::new(std::io::ErrorKind::TimedOut, "test timeout"),
        ));

        assert_eq!(
            failure,
            RuntimeFailure::new(BackupErrorCode::NetworkTimeout, true)
        );
    }

    #[test]
    fn retryable_start_failure_recovers_with_bounded_restart() {
        let (_directory, database, _config) = test_database(true);
        let (success, _exited) = successful_plan(&database.paths().main);
        let factory = FakeRuntimeFactory::new([
            StartPlan::Failure(RuntimeFailure::new(BackupErrorCode::NetworkOffline, true)),
            success,
        ]);
        let service = start_test_service(&database, factory.clone());
        wait_until(Duration::from_secs(2), || {
            factory.starts() == 2 && service.status().phase == RelationalBackupPhase::Running
        });
        service.shutdown();
    }

    #[test]
    fn missing_credentials_retry_without_a_configuration_change() {
        assert_eq!(
            map_credential_start_error(CredentialError::Missing),
            RuntimeFailure::new(BackupErrorCode::KeychainCredentialMissing, true)
        );

        let (_directory, database, _config) = test_database(true);
        let (success, _exited) = successful_plan(&database.paths().main);
        let factory = FakeRuntimeFactory::new([
            StartPlan::Failure(map_credential_start_error(CredentialError::Missing)),
            success,
        ]);
        let service = start_test_service(&database, factory.clone());

        wait_until(Duration::from_secs(2), || {
            factory.starts() == 2 && service.status().phase == RelationalBackupPhase::Running
        });

        service.shutdown();
    }

    #[test]
    fn ownership_failure_blocks_without_restart_spam() {
        let (_directory, database, _config) = test_database(true);
        let factory = FakeRuntimeFactory::new([StartPlan::Failure(RuntimeFailure::new(
            BackupErrorCode::OwnerMismatch,
            false,
        ))]);
        let service = start_test_service(&database, factory.clone());
        wait_until(Duration::from_secs(2), || {
            service.status().phase == RelationalBackupPhase::Blocked
        });
        thread::sleep(Duration::from_millis(40));
        assert_eq!(factory.starts(), 1);
        assert_eq!(
            service.status().last_error_code,
            Some(BackupErrorCode::OwnerMismatch)
        );
        service.shutdown();
    }

    #[test]
    fn ownership_loss_during_status_refresh_stops_the_daemon() {
        let (_directory, database, _config) = test_database(true);
        let factory = FakeRuntimeFactory::new([StartPlan::Daemon {
            exited: Arc::new(AtomicBool::new(false)),
            sync: Err(RuntimeFailure::new(BackupErrorCode::OwnerMismatch, false)),
        }]);
        let service = start_test_service(&database, factory.clone());
        wait_until(Duration::from_secs(2), || {
            service.status().phase == RelationalBackupPhase::Blocked
        });
        assert_eq!(factory.starts(), 1);
        assert_eq!(factory.shutdowns(), 1);
        service.shutdown();
    }

    #[test]
    fn unexpected_exit_restarts_and_disable_stops_the_replacement() {
        let (_directory, database, config) = test_database(true);
        let (first, first_exited) = successful_plan(&database.paths().main);
        let (second, _second_exited) = successful_plan(&database.paths().main);
        let factory = FakeRuntimeFactory::new([first, second]);
        let service = start_test_service(&database, factory.clone());
        wait_until(Duration::from_secs(2), || {
            factory.starts() == 1 && service.status().phase == RelationalBackupPhase::Running
        });

        first_exited.store(true, Ordering::Release);
        wait_until(Duration::from_secs(2), || {
            factory.starts() == 2 && service.status().phase == RelationalBackupPhase::Running
        });

        database
            .client()
            .save_offsite_backup_config(SaveOffsiteBackupConfigInput {
                expected_revision: config.revision,
                backup_set_id: config.backup_set_id,
                replica_epoch_id: config.replica_epoch_id,
                enabled: false,
                target: config.target,
            })
            .expect("disable backup");
        service.reload_configuration();
        wait_until(Duration::from_secs(2), || {
            service.status().phase == RelationalBackupPhase::Off
        });
        assert_eq!(factory.shutdowns(), 2);
        service.shutdown();
    }

    #[test]
    fn restart_delay_is_exponential_and_capped() {
        let policy = RestartPolicy {
            base: Duration::from_secs(1),
            maximum: Duration::from_secs(5),
        };
        assert_eq!(policy.delay(1), Duration::from_secs(1));
        assert_eq!(policy.delay(2), Duration::from_secs(2));
        assert_eq!(policy.delay(3), Duration::from_secs(4));
        assert_eq!(policy.delay(4), Duration::from_secs(5));
        assert_eq!(policy.delay(u32::MAX), Duration::from_secs(5));
    }

    #[test]
    fn log_redaction_handles_secrets_split_across_reads() {
        let directory = tempfile::tempdir().expect("log directory");
        let credentials =
            R2Credentials::new(TEST_ACCESS_KEY, TEST_SECRET_KEY).expect("credentials");
        let secrets = Arc::new(SecretPatterns::new(&credentials));
        let mut log =
            BoundedLitestreamLog::open_with_limit(directory.path(), 4_096).expect("bounded log");
        let mut redactor = StreamRedactor::new(secrets);
        let split = TEST_SECRET_KEY.len() / 2;
        redactor
            .push(
                format!(
                    "access={TEST_ACCESS_KEY}; secret={}",
                    &TEST_SECRET_KEY[..split]
                )
                .as_bytes(),
                &mut log,
            )
            .expect("first chunk");
        redactor
            .push(&TEST_SECRET_KEY.as_bytes()[split..], &mut log)
            .expect("second chunk");
        redactor.finish(&mut log).expect("finish redaction");
        log.active.flush().expect("flush log");
        drop(log);

        let contents =
            fs::read_to_string(directory.path().join(LITESTREAM_LOG_FILE_NAME)).expect("read log");
        assert!(!contents.contains(TEST_ACCESS_KEY));
        assert!(!contents.contains(TEST_SECRET_KEY));
        assert_eq!(contents, "access=[REDACTED]; secret=[REDACTED]");
    }

    #[test]
    fn logs_rotate_to_two_bounded_files() {
        let directory = tempfile::tempdir().expect("log directory");
        let mut log =
            BoundedLitestreamLog::open_with_limit(directory.path(), 10).expect("bounded log");
        log.write_bounded(b"12345678").expect("first write");
        log.write_bounded(b"abcdefgh").expect("second write");
        log.write_bounded(b"ABCDEFGHIJKL").expect("oversized write");
        log.active.flush().expect("flush log");
        drop(log);

        let active = fs::read(directory.path().join(LITESTREAM_LOG_FILE_NAME)).expect("active log");
        let archive =
            fs::read(directory.path().join(LITESTREAM_LOG_ARCHIVE_NAME)).expect("archive log");
        assert_eq!(active, b"CDEFGHIJKL");
        assert_eq!(archive, b"abcdefgh");
        assert!(active.len() <= 10);
        assert!(archive.len() <= 10);
    }

    #[cfg(unix)]
    #[test]
    fn stale_nonexistent_owned_process_is_swept() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("data root");
        let runtime = LitestreamRuntimePaths::new(directory.path()).expect("runtime paths");
        runtime
            .write_config("test: true\n")
            .expect("runtime config");
        let binary = directory.path().join("litestream");
        let identity = LaunchIdentity {
            backup_set_id: BackupSetId::new().to_string(),
            replica_epoch_id: ReplicaEpochId::new().to_string(),
            config_sha256: sha256_hex(b"test: true\n"),
        };
        write_pid_record(
            &runtime,
            u32::MAX,
            &binary,
            &directory.path().join("dara.sqlite3"),
            &identity,
        )
        .expect("PID record");
        assert_eq!(
            fs::metadata(runtime.pid())
                .expect("PID metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        assert_eq!(
            sweep_stale_runtime(&runtime, &binary, &directory.path().join("other.sqlite3"),),
            Err(RuntimeFailure::new(
                BackupErrorCode::LitestreamUnavailable,
                false,
            ))
        );
        assert!(runtime.pid().exists());

        sweep_stale_runtime(&runtime, &binary, &directory.path().join("dara.sqlite3"))
            .expect("stale sweep");
        assert!(!runtime.pid().exists());
    }
}
