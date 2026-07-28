use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use super::{
    credentials::R2Credentials,
    domain::{ProbeRunId, R2Keyspace, R2Target},
    litestream::{
        configure_credentials_environment, parse_restore_result_json, CommandLitestreamControl,
        LitestreamConfig, LitestreamControl, LitestreamRuntimePaths, LitestreamTxid,
        SystemCommandExecutor, VerifiedLitestreamBinary,
    },
    object_store::{
        ContinuationToken, ObjectContentType, ObjectStore, ObjectStoreErrorCode, PutCondition,
        PutObjectOutcome, PutObjectRequest,
    },
};

const PROBE_SOCKET_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(35);
const PROBE_RESTORE_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_CLEANUP_PAGES: usize = 100;
const MAX_RESTORE_OUTPUT_BYTES: usize = 64 * 1024;
const PROBE_RUNTIME_DIRECTORY_NAME_BYTES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProbeStage {
    ObjectPut,
    ObjectHead,
    ObjectGet,
    ObjectList,
    LitestreamRoundTrip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProbeCleanupStatus {
    Complete,
    Failed(ObjectStoreErrorCode),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConnectionProbeReport {
    pub(crate) run_id: ProbeRunId,
    pub(crate) object_store_verified: bool,
    pub(crate) litestream_verified: bool,
    pub(crate) cleanup: ProbeCleanupStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelationalProbeErrorCode {
    Prepare,
    Start,
    SocketUnavailable,
    Sync,
    Shutdown,
    Restore,
    Validate,
}

pub(crate) trait RelationalProbe: Send + Sync {
    fn verify(&self, run_id: &ProbeRunId) -> Result<(), RelationalProbeErrorCode>;
}

pub(crate) fn verify_connection(
    object_store: &dyn ObjectStore,
    relational_probe: &dyn RelationalProbe,
    keyspace: &R2Keyspace,
) -> Result<ConnectionProbeReport, ConnectionProbeError> {
    verify_connection_with_progress(object_store, relational_probe, keyspace, |_| {})
}

pub(crate) fn verify_connection_with_progress(
    object_store: &dyn ObjectStore,
    relational_probe: &dyn RelationalProbe,
    keyspace: &R2Keyspace,
    mut on_stage: impl FnMut(ProbeStage),
) -> Result<ConnectionProbeReport, ConnectionProbeError> {
    let run_id = ProbeRunId::new();
    let verification = verify_connection_inner(
        object_store,
        relational_probe,
        keyspace,
        &run_id,
        &mut on_stage,
    );
    let cleanup = cleanup_probe_prefix(object_store, keyspace, &run_id);
    match verification {
        Ok(()) => Ok(ConnectionProbeReport {
            run_id,
            object_store_verified: true,
            litestream_verified: true,
            cleanup,
        }),
        Err(failure) => Err(ConnectionProbeError {
            stage: failure.stage,
            object_store_code: failure.object_store_code,
            relational_code: failure.relational_code,
            cleanup,
        }),
    }
}

fn verify_connection_inner(
    object_store: &dyn ObjectStore,
    relational_probe: &dyn RelationalProbe,
    keyspace: &R2Keyspace,
    run_id: &ProbeRunId,
    on_stage: &mut dyn FnMut(ProbeStage),
) -> Result<(), ProbeFailure> {
    on_stage(ProbeStage::ObjectPut);
    let key = keyspace.probe_object(run_id);
    let payload = format!("dara-r2-probe-v1:{}", run_id.as_str()).into_bytes();
    let payload_sha256 = super::domain::ContentSha256::from_bytes(Sha256::digest(&payload).into());
    let outcome = object_store
        .put(PutObjectRequest {
            key: key.clone(),
            bytes: payload.clone(),
            content_type: ObjectContentType::Binary,
            dara_sha256: Some(payload_sha256),
            condition: PutCondition::IfAbsent,
        })
        .map_err(|error| ProbeFailure::object(ProbeStage::ObjectPut, error.code))?;
    if outcome != PutObjectOutcome::Stored {
        return Err(ProbeFailure::object(
            ProbeStage::ObjectPut,
            ObjectStoreErrorCode::Conflict,
        ));
    }

    on_stage(ProbeStage::ObjectHead);
    let head = object_store
        .head(&key)
        .map_err(|error| ProbeFailure::object(ProbeStage::ObjectHead, error.code))?
        .ok_or_else(|| {
            ProbeFailure::object(ProbeStage::ObjectHead, ObjectStoreErrorCode::NotFound)
        })?;
    if head.byte_length != payload.len() as u64
        || head.content_type != Some(ObjectContentType::Binary)
        || head.dara_sha256 != Some(payload_sha256)
        || head.object_format_version != Some(super::domain::OBJECT_FORMAT_VERSION)
    {
        return Err(ProbeFailure::object(
            ProbeStage::ObjectHead,
            ObjectStoreErrorCode::InvalidResponse,
        ));
    }

    on_stage(ProbeStage::ObjectGet);
    let get = object_store
        .get(&key)
        .map_err(|error| ProbeFailure::object(ProbeStage::ObjectGet, error.code))?;
    if get.bytes != payload
        || super::domain::ContentSha256::from_bytes(Sha256::digest(&get.bytes).into())
            != payload_sha256
    {
        return Err(ProbeFailure::object(
            ProbeStage::ObjectGet,
            ObjectStoreErrorCode::InvalidResponse,
        ));
    }

    on_stage(ProbeStage::ObjectList);
    let listed = object_store
        .list(&keyspace.probe_prefix(run_id), None)
        .map_err(|error| ProbeFailure::object(ProbeStage::ObjectList, error.code))?;
    if !listed.objects.iter().any(|object| object.key == key) {
        return Err(ProbeFailure::object(
            ProbeStage::ObjectList,
            ObjectStoreErrorCode::InvalidResponse,
        ));
    }

    on_stage(ProbeStage::LitestreamRoundTrip);
    relational_probe
        .verify(run_id)
        .map_err(ProbeFailure::relational)?;
    Ok(())
}

fn cleanup_probe_prefix(
    object_store: &dyn ObjectStore,
    keyspace: &R2Keyspace,
    run_id: &ProbeRunId,
) -> ProbeCleanupStatus {
    let prefix = keyspace.probe_prefix(run_id);
    let mut continuation: Option<ContinuationToken> = None;
    for _ in 0..MAX_CLEANUP_PAGES {
        let page = match object_store.list(&prefix, continuation.as_ref()) {
            Ok(page) => page,
            Err(error) => return ProbeCleanupStatus::Failed(error.code),
        };
        for object in page.objects {
            if let Err(error) = object_store.delete(&object.key) {
                return ProbeCleanupStatus::Failed(error.code);
            }
        }
        let Some(next) = page.next else {
            return ProbeCleanupStatus::Complete;
        };
        continuation = Some(next);
    }
    ProbeCleanupStatus::Failed(ObjectStoreErrorCode::InvalidResponse)
}

struct ProbeFailure {
    stage: ProbeStage,
    object_store_code: Option<ObjectStoreErrorCode>,
    relational_code: Option<RelationalProbeErrorCode>,
}

impl ProbeFailure {
    fn object(stage: ProbeStage, code: ObjectStoreErrorCode) -> Self {
        Self {
            stage,
            object_store_code: Some(code),
            relational_code: None,
        }
    }

    fn relational(code: RelationalProbeErrorCode) -> Self {
        Self {
            stage: ProbeStage::LitestreamRoundTrip,
            object_store_code: None,
            relational_code: Some(code),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("off-site backup connection probe failed during {stage:?}")]
pub(crate) struct ConnectionProbeError {
    pub(crate) stage: ProbeStage,
    pub(crate) object_store_code: Option<ObjectStoreErrorCode>,
    pub(crate) relational_code: Option<RelationalProbeErrorCode>,
    pub(crate) cleanup: ProbeCleanupStatus,
}

pub(crate) struct LitestreamRelationalProbe<'a> {
    binary: &'a VerifiedLitestreamBinary,
    data_root: &'a Path,
    target: &'a R2Target,
    credentials: &'a R2Credentials,
}

impl<'a> LitestreamRelationalProbe<'a> {
    pub(crate) fn new(
        binary: &'a VerifiedLitestreamBinary,
        data_root: &'a Path,
        target: &'a R2Target,
        credentials: &'a R2Credentials,
    ) -> Self {
        Self {
            binary,
            data_root,
            target,
            credentials,
        }
    }
}

impl RelationalProbe for LitestreamRelationalProbe<'_> {
    fn verify(&self, run_id: &ProbeRunId) -> Result<(), RelationalProbeErrorCode> {
        let root = self.prepare_root(run_id)?;
        let result = self.verify_in_root(run_id, &root);
        let _ = fs::remove_dir_all(&root);
        result
    }
}

impl LitestreamRelationalProbe<'_> {
    fn prepare_root(&self, run_id: &ProbeRunId) -> Result<PathBuf, RelationalProbeErrorCode> {
        if !self.data_root.is_absolute() {
            return Err(RelationalProbeErrorCode::Prepare);
        }
        let parent = self.data_root.join("bp");
        fs::create_dir_all(&parent).map_err(|_| RelationalProbeErrorCode::Prepare)?;
        let root = parent.join(probe_runtime_directory_name(run_id));
        fs::create_dir(&root).map_err(|_| RelationalProbeErrorCode::Prepare)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|_| RelationalProbeErrorCode::Prepare)?;
        }
        Ok(root)
    }

    fn verify_in_root(
        &self,
        run_id: &ProbeRunId,
        root: &Path,
    ) -> Result<(), RelationalProbeErrorCode> {
        let database_path = root.join("probe.sqlite3");
        let restore_path = root.join("restore.sqlite3");
        create_probe_database(&database_path)?;
        let runtime =
            LitestreamRuntimePaths::new(root).map_err(|_| RelationalProbeErrorCode::Prepare)?;
        let keyspace = self.target.keyspace();
        let config = LitestreamConfig {
            database_path: &database_path,
            runtime: &runtime,
            bucket: self.target.bucket.as_str(),
            replica_path: keyspace.probe_litestream(run_id).as_str(),
            endpoint: &self.target.endpoint(),
        }
        .render()
        .map_err(|_| RelationalProbeErrorCode::Prepare)?;
        runtime
            .write_config(&config)
            .map_err(|_| RelationalProbeErrorCode::Prepare)?;

        let mut command = Command::new(self.binary.path());
        command
            .args(["replicate", "-config"])
            .arg(runtime.config())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_credentials_environment(&mut command, self.credentials);
        let mut child = command
            .spawn()
            .map_err(|_| RelationalProbeErrorCode::Start)?;
        let probe_result = (|| {
            wait_for_socket(&mut child, runtime.socket())?;
            let control = CommandLitestreamControl::new(
                self.binary.path().to_owned(),
                runtime.socket().to_owned(),
                60,
                SystemCommandExecutor,
            );
            let sync = control
                .sync_remote(&database_path)
                .map_err(|_| RelationalProbeErrorCode::Sync)?;
            terminate_child(&mut child)?;
            restore_probe(
                self.binary.path(),
                runtime.config(),
                &database_path,
                &restore_path,
                sync.txid,
                self.credentials,
            )?;
            validate_probe_database(&restore_path)
        })();
        if child
            .try_wait()
            .map_err(|_| RelationalProbeErrorCode::Shutdown)?
            .is_none()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        probe_result
    }
}

fn probe_runtime_directory_name(run_id: &ProbeRunId) -> &str {
    // UUIDv7's leading characters are timestamp bits, while its final group is
    // random. Keep the directory as short as before for the macOS socket limit.
    let value = run_id.as_str();
    &value[value.len() - PROBE_RUNTIME_DIRECTORY_NAME_BYTES..]
}

fn create_probe_database(path: &Path) -> Result<(), RelationalProbeErrorCode> {
    let connection = Connection::open(path).map_err(|_| RelationalProbeErrorCode::Prepare)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             CREATE TABLE probe (id INTEGER PRIMARY KEY, marker TEXT NOT NULL UNIQUE);
             INSERT INTO probe(marker) VALUES ('known-row');",
        )
        .map_err(|_| RelationalProbeErrorCode::Prepare)
}

fn wait_for_socket(child: &mut Child, socket: &Path) -> Result<(), RelationalProbeErrorCode> {
    let deadline = Instant::now() + PROBE_SOCKET_TIMEOUT;
    while Instant::now() < deadline {
        if socket.exists() {
            return Ok(());
        }
        if child
            .try_wait()
            .map_err(|_| RelationalProbeErrorCode::SocketUnavailable)?
            .is_some()
        {
            return Err(RelationalProbeErrorCode::SocketUnavailable);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(RelationalProbeErrorCode::SocketUnavailable)
}

fn terminate_child(child: &mut Child) -> Result<(), RelationalProbeErrorCode> {
    #[cfg(target_os = "macos")]
    {
        let result = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        if result != 0 {
            return Err(RelationalProbeErrorCode::Shutdown);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        child
            .kill()
            .map_err(|_| RelationalProbeErrorCode::Shutdown)?;
    }
    let deadline = Instant::now() + PROBE_SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|_| RelationalProbeErrorCode::Shutdown)?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(RelationalProbeErrorCode::Shutdown)
}

fn restore_probe(
    binary: &Path,
    config: &Path,
    database: &Path,
    output: &Path,
    txid: LitestreamTxid,
    credentials: &R2Credentials,
) -> Result<(), RelationalProbeErrorCode> {
    let txid = txid.to_string();
    let mut command = Command::new(binary);
    command
        .args(["restore", "-config"])
        .arg(config)
        .args(["-txid", &txid, "-json", "-integrity-check", "full", "-o"])
        .arg(output)
        .arg(database)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_credentials_environment(&mut command, credentials);
    let mut child = command
        .spawn()
        .map_err(|_| RelationalProbeErrorCode::Restore)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RelationalProbeErrorCode::Restore)?;
    let (status, bytes) = collect_restore_output(&mut child, stdout, PROBE_RESTORE_TIMEOUT)?;
    if !status.success() {
        return Err(RelationalProbeErrorCode::Restore);
    }
    let restored =
        parse_restore_result_json(&bytes).map_err(|_| RelationalProbeErrorCode::Restore)?;
    if restored.txid.to_string() != txid || restored.database_path != output {
        return Err(RelationalProbeErrorCode::Restore);
    }
    Ok(())
}

fn collect_restore_output(
    child: &mut Child,
    mut stdout: ChildStdout,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>), RelationalProbeErrorCode> {
    let mut reader = Some(thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(MAX_RESTORE_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(bytes)
    }));
    let deadline = Instant::now() + timeout;
    let mut status = None;
    let mut bytes = None;
    loop {
        if bytes.is_none() && reader.as_ref().is_some_and(|reader| reader.is_finished()) {
            let output = match reader
                .take()
                .expect("finished restore output reader")
                .join()
            {
                Ok(Ok(output)) => output,
                Ok(Err(_)) | Err(_) => {
                    stop_restore_process(child, &mut reader);
                    return Err(RelationalProbeErrorCode::Restore);
                }
            };
            if output.len() > MAX_RESTORE_OUTPUT_BYTES {
                stop_restore_process(child, &mut reader);
                return Err(RelationalProbeErrorCode::Restore);
            }
            bytes = Some(output);
        }
        if status.is_none() {
            status = match child.try_wait() {
                Ok(status) => status,
                Err(_) => {
                    stop_restore_process(child, &mut reader);
                    return Err(RelationalProbeErrorCode::Restore);
                }
            };
        }
        if status.is_some() && bytes.is_some() {
            return Ok((
                status.take().expect("completed restore status"),
                bytes.take().expect("completed restore output"),
            ));
        }
        if Instant::now() >= deadline {
            stop_restore_process(child, &mut reader);
            return Err(RelationalProbeErrorCode::Restore);
        }
        thread::sleep(PROBE_PROCESS_POLL_INTERVAL);
    }
}

fn stop_restore_process(
    child: &mut Child,
    reader: &mut Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
) {
    let _ = child.kill();
    let _ = child.wait();
    if let Some(reader) = reader.take() {
        let _ = reader.join();
    }
}

fn validate_probe_database(path: &Path) -> Result<(), RelationalProbeErrorCode> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| RelationalProbeErrorCode::Validate)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| RelationalProbeErrorCode::Validate)?;
    let known_rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM probe WHERE marker = ?1",
            params!["known-row"],
            |row| row.get(0),
        )
        .map_err(|_| RelationalProbeErrorCode::Validate)?;
    if integrity != "ok" || known_rows != 1 {
        return Err(RelationalProbeErrorCode::Validate);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{
        credentials::R2Credentials,
        domain::{R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix, R2Target},
        object_store::{
            fake::{FakeObjectStore, ObjectOperation},
            ObjectStoreErrorCode, R2ObjectStore,
        },
    };

    #[derive(Clone, Copy)]
    struct FakeRelationalProbe {
        result: Result<(), RelationalProbeErrorCode>,
    }

    impl RelationalProbe for FakeRelationalProbe {
        fn verify(&self, _run_id: &ProbeRunId) -> Result<(), RelationalProbeErrorCode> {
            self.result
        }
    }

    fn target() -> R2Target {
        R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/probe-test").expect("prefix"),
        }
    }

    #[test]
    fn connection_probe_exercises_object_store_and_relational_round_trip_then_cleans_up() {
        let store = FakeObjectStore::default();
        let report = verify_connection(
            &store,
            &FakeRelationalProbe { result: Ok(()) },
            &target().keyspace(),
        )
        .expect("connection probe");
        assert!(report.object_store_verified);
        assert!(report.litestream_verified);
        assert_eq!(report.cleanup, ProbeCleanupStatus::Complete);
        let operations = store.operations();
        for required in [
            ObjectOperation::Put,
            ObjectOperation::Head,
            ObjectOperation::Get,
            ObjectOperation::List,
            ObjectOperation::Delete,
        ] {
            assert!(operations.contains(&required), "missing {required:?}");
        }
    }

    #[test]
    fn connection_probe_reports_progress_in_protocol_order() {
        let store = FakeObjectStore::default();
        let mut stages = Vec::new();

        verify_connection_with_progress(
            &store,
            &FakeRelationalProbe { result: Ok(()) },
            &target().keyspace(),
            |stage| stages.push(stage),
        )
        .expect("connection probe");

        assert_eq!(
            stages,
            vec![
                ProbeStage::ObjectPut,
                ProbeStage::ObjectHead,
                ProbeStage::ObjectGet,
                ProbeStage::ObjectList,
                ProbeStage::LitestreamRoundTrip,
            ]
        );
    }

    #[test]
    fn primary_failure_is_separate_from_bounded_cleanup_failure() {
        let store = FakeObjectStore::default();
        store.fail_next(
            ObjectOperation::Head,
            ObjectStoreErrorCode::AuthorizationRejected,
        );
        store.fail_next(ObjectOperation::List, ObjectStoreErrorCode::RateLimited);
        let error = verify_connection(
            &store,
            &FakeRelationalProbe { result: Ok(()) },
            &target().keyspace(),
        )
        .expect_err("connection failure");
        assert_eq!(error.stage, ProbeStage::ObjectHead);
        assert_eq!(
            error.object_store_code,
            Some(ObjectStoreErrorCode::AuthorizationRejected)
        );
        assert_eq!(
            error.cleanup,
            ProbeCleanupStatus::Failed(ObjectStoreErrorCode::RateLimited)
        );
    }

    #[test]
    fn relational_failure_is_typed_and_still_cleans_the_exact_run_prefix() {
        let store = FakeObjectStore::default();
        let error = verify_connection(
            &store,
            &FakeRelationalProbe {
                result: Err(RelationalProbeErrorCode::Restore),
            },
            &target().keyspace(),
        )
        .expect_err("relational failure");
        assert_eq!(error.stage, ProbeStage::LitestreamRoundTrip);
        assert_eq!(
            error.relational_code,
            Some(RelationalProbeErrorCode::Restore)
        );
        assert_eq!(error.cleanup, ProbeCleanupStatus::Complete);
    }

    #[test]
    fn probe_runtime_directories_do_not_reuse_the_uuid_v7_timestamp_prefix() {
        let first =
            ProbeRunId::parse("01980c8e-6c00-7000-8000-000000000001").expect("first probe run ID");
        let second =
            ProbeRunId::parse("01980c8e-6c00-7000-8000-000000000002").expect("second probe run ID");
        assert_eq!(&first.as_str()[..8], &second.as_str()[..8]);
        assert_ne!(
            probe_runtime_directory_name(&first),
            probe_runtime_directory_name(&second)
        );
    }

    #[test]
    fn restore_output_timeout_kills_and_reaps_the_child() {
        let mut child = Command::new("/bin/sleep")
            .arg("5")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("stalled restore child");
        let stdout = child.stdout.take().expect("restore stdout");
        let started = Instant::now();
        assert_eq!(
            collect_restore_output(&mut child, stdout, Duration::from_millis(20)),
            Err(RelationalProbeErrorCode::Restore)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(child
            .try_wait()
            .expect("reaped restore child status")
            .is_some());
    }

    #[test]
    #[ignore = "requires app/.env.local and the disposable dara-local R2 bucket"]
    fn live_r2_and_litestream_connection_probe_round_trips_and_cleans_up() {
        let jurisdiction =
            R2Jurisdiction::from_db(&required_environment("DARA_LITESTREAM_R2_JURISDICTION"))
                .expect("R2 jurisdiction");
        let target = R2Target {
            account_id: R2AccountId::parse(required_environment("DARA_LITESTREAM_R2_ACCOUNT_ID"))
                .expect("R2 account ID"),
            jurisdiction,
            bucket: R2BucketName::parse(required_environment("DARA_LITESTREAM_R2_BUCKET"))
                .expect("R2 bucket"),
            prefix: R2Prefix::parse(required_environment("DARA_LITESTREAM_R2_PREFIX"))
                .expect("R2 prefix"),
        };
        let credentials = R2Credentials::new(
            required_environment("DARA_LITESTREAM_R2_ACCESS_KEY_ID"),
            required_environment("DARA_LITESTREAM_R2_SECRET_ACCESS_KEY"),
        )
        .expect("R2 credentials");
        let binary_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("release")
            .join("bin")
            .join("litestream");
        let binary = VerifiedLitestreamBinary::resolve_staged_for_test(&binary_path)
            .expect("verified Litestream");
        let data_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("app directory")
            .join(".data")
            .join("litestream-slice2");
        fs::create_dir_all(&data_root).expect("test data root");
        let store = R2ObjectStore::new(target.clone(), &credentials).expect("R2 client");
        let relational = LitestreamRelationalProbe::new(&binary, &data_root, &target, &credentials);

        let report = verify_connection(&store, &relational, &target.keyspace())
            .expect("R2 and Litestream connection probe");

        assert!(report.object_store_verified);
        assert!(report.litestream_verified);
        assert_eq!(report.cleanup, ProbeCleanupStatus::Complete);
    }

    fn required_environment(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
    }
}
