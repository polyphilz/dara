use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::credentials::R2Credentials;

const EMBEDDED_MANIFEST: &str = include_str!("../../resources/sidecars/litestream-v1.json");
const EMBEDDED_DISTRIBUTION_SIGNING_POLICY: &str = include_str!("../../distribution-signing.json");
const DEVELOPMENT_BINARY_OVERRIDE_ENV: &str = "DARA_LITESTREAM_PATH";
const ACCESS_KEY_ID_ENV: &str = "DARA_LITESTREAM_R2_ACCESS_KEY_ID";
const SECRET_ACCESS_KEY_ENV: &str = "DARA_LITESTREAM_R2_SECRET_ACCESS_KEY";
const REQUIRED_L0_RETENTION: &str = "720h";
const MAX_CONTROL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_RESTORE_PLAN_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_RESTORE_FILES: usize = 100_000;
const MAX_MACOS_UNIX_SOCKET_PATH_BYTES: usize = 103;
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) fn configure_credentials_environment(
    command: &mut Command,
    credentials: &R2Credentials,
) {
    command
        .env_clear()
        .env(ACCESS_KEY_ID_ENV, credentials.access_key_id());
    command.env(SECRET_ACCESS_KEY_ENV, credentials.secret_access_key());
}

#[derive(Debug, Error)]
pub(crate) enum LitestreamError {
    #[error("the Litestream source manifest is invalid")]
    InvalidEmbeddedManifest(#[source] serde_json::Error),
    #[error("the Dara distribution signing policy is invalid")]
    InvalidEmbeddedDistributionSigningPolicy(#[source] serde_json::Error),
    #[error("the Dara distribution signing policy does not authorize Litestream")]
    UnsafeDistributionSigningPolicy,
    #[error("the bundled Litestream release manifest is unavailable")]
    MissingReleaseManifest(#[source] std::io::Error),
    #[error("the bundled Litestream release manifest does not match the application pin")]
    ReleaseManifestMismatch,
    #[error("the Litestream executable is unavailable: {0}")]
    MissingBinary(PathBuf),
    #[error("the Litestream executable is not a regular file")]
    BinaryNotRegular,
    #[error("the Litestream executable is not executable")]
    BinaryNotExecutable,
    #[error("the Litestream executable size does not match the application pin")]
    BinarySizeMismatch,
    #[error("the Litestream executable checksum does not match the application pin")]
    BinaryChecksumMismatch,
    #[error("the Litestream manifest does not preserve exact TXIDs for 30 days")]
    UnsafeL0Retention,
    #[error("the Litestream runtime path is not valid UTF-8")]
    NonUtf8RuntimePath,
    #[error("the Litestream control socket path is too long for macOS")]
    ControlSocketPathTooLong,
    #[error("invalid Litestream configuration field: {0}")]
    InvalidConfigField(&'static str),
    #[error("could not prepare the private Litestream runtime directory")]
    PrepareRuntime(#[source] std::io::Error),
    #[error("could not write the private Litestream configuration")]
    WriteConfig(#[source] std::io::Error),
    #[error("Litestream command execution failed")]
    Execute(#[source] std::io::Error),
    #[error("Litestream command failed with exit code {exit_code:?}")]
    CommandFailed { exit_code: Option<i32> },
    #[error("Litestream returned an oversized control response")]
    OversizedControlResponse,
    #[error("Litestream returned invalid JSON")]
    InvalidJson(#[source] serde_json::Error),
    #[error("Litestream returned a malformed transaction ID")]
    InvalidTxid,
    #[error("Litestream sync did not return the expected remote transaction ID")]
    InvalidSyncContract,
    #[error("Litestream sync returned a different database path")]
    UnexpectedDatabasePath,
    #[error("Litestream returned too many restore files")]
    RestorePlanTooLarge,
    #[error("Litestream commands require an absolute database path")]
    RelativeDatabasePath,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceManifest {
    component: String,
    binary: BinaryManifest,
    resource_destinations: ResourceDestinations,
    verification: ProtocolVerification,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinaryManifest {
    bundle_path: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceDestinations {
    release_manifest: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolVerification {
    exact_txid_fence_passed: bool,
    ordinary_compaction_exact_restore_passed: bool,
    default_l0_expiry_interior_txid_failure_observed: bool,
    required_l0_retention: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DistributionSigningPolicy {
    application: DistributionApplicationSigning,
    sidecars: DistributionSidecarSigningPolicies,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DistributionApplicationSigning {
    signing_identity: String,
    team_identifier: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DistributionSidecarSigningPolicies {
    litestream: DistributionSidecarSigning,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DistributionSidecarSigning {
    component: String,
    identifier: String,
    bundle_path: String,
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedLitestreamBinary {
    path: PathBuf,
}

impl VerifiedLitestreamBinary {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn resolve(resource_dir: &Path) -> Result<Self, LitestreamError> {
        let manifest = embedded_manifest()?;
        validate_protocol_manifest(&manifest)?;

        #[cfg(debug_assertions)]
        let development_override =
            std::env::var_os(DEVELOPMENT_BINARY_OVERRIDE_ENV).map(PathBuf::from);
        #[cfg(not(debug_assertions))]
        let development_override: Option<PathBuf> = None;

        let path = if let Some(path) = development_override {
            path
        } else {
            verify_release_manifest(resource_dir, &manifest)?;
            resource_dir.join(&manifest.binary.bundle_path)
        };
        verify_binary(&path, &manifest)?;
        Ok(Self { path })
    }

    #[cfg(test)]
    pub(crate) fn resolve_staged_for_test(path: &Path) -> Result<Self, LitestreamError> {
        let manifest = embedded_manifest()?;
        validate_protocol_manifest(&manifest)?;
        verify_binary(path, &manifest)?;
        Ok(Self {
            path: path.to_owned(),
        })
    }
}

fn embedded_manifest() -> Result<SourceManifest, LitestreamError> {
    serde_json::from_str(EMBEDDED_MANIFEST).map_err(LitestreamError::InvalidEmbeddedManifest)
}

fn validate_protocol_manifest(manifest: &SourceManifest) -> Result<(), LitestreamError> {
    if manifest.component != "litestream"
        || !manifest.verification.exact_txid_fence_passed
        || !manifest
            .verification
            .ordinary_compaction_exact_restore_passed
        || !manifest
            .verification
            .default_l0_expiry_interior_txid_failure_observed
        || manifest.verification.required_l0_retention != REQUIRED_L0_RETENTION
    {
        return Err(LitestreamError::UnsafeL0Retention);
    }
    Ok(())
}

fn verify_release_manifest(
    resource_dir: &Path,
    manifest: &SourceManifest,
) -> Result<(), LitestreamError> {
    let release_path = resource_dir.join(&manifest.resource_destinations.release_manifest);
    let release =
        fs::read_to_string(release_path).map_err(LitestreamError::MissingReleaseManifest)?;
    let embedded_value: serde_json::Value = serde_json::from_str(EMBEDDED_MANIFEST)
        .map_err(LitestreamError::InvalidEmbeddedManifest)?;
    let release_value: serde_json::Value =
        serde_json::from_str(&release).map_err(LitestreamError::InvalidEmbeddedManifest)?;
    if release_value != embedded_value {
        return Err(LitestreamError::ReleaseManifestMismatch);
    }
    Ok(())
}

fn verify_binary(path: &Path, manifest: &SourceManifest) -> Result<(), LitestreamError> {
    let symlink_metadata =
        fs::symlink_metadata(path).map_err(|_| LitestreamError::MissingBinary(path.to_owned()))?;
    if symlink_metadata.file_type().is_symlink() || !symlink_metadata.is_file() {
        return Err(LitestreamError::BinaryNotRegular);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if symlink_metadata.permissions().mode() & 0o111 == 0 {
            return Err(LitestreamError::BinaryNotExecutable);
        }
    }

    let size_matches = symlink_metadata.len() == manifest.binary.size;
    let checksum_matches = size_matches && sha256_file(path)? == manifest.binary.sha256;
    if size_matches && checksum_matches {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let signing_policy = embedded_distribution_signing_policy()?;
        validate_distribution_signing_policy(&signing_policy, manifest)?;
        if verify_distribution_signature(path, &signing_policy) {
            return Ok(());
        }
    }

    if !size_matches {
        Err(LitestreamError::BinarySizeMismatch)
    } else {
        Err(LitestreamError::BinaryChecksumMismatch)
    }
}

fn sha256_file(path: &Path) -> Result<String, LitestreamError> {
    let mut file = File::open(path).map_err(|_| LitestreamError::MissingBinary(path.to_owned()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| LitestreamError::BinaryChecksumMismatch)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(target_os = "macos")]
fn embedded_distribution_signing_policy() -> Result<DistributionSigningPolicy, LitestreamError> {
    serde_json::from_str(EMBEDDED_DISTRIBUTION_SIGNING_POLICY)
        .map_err(LitestreamError::InvalidEmbeddedDistributionSigningPolicy)
}

#[cfg(target_os = "macos")]
fn validate_distribution_signing_policy(
    policy: &DistributionSigningPolicy,
    manifest: &SourceManifest,
) -> Result<(), LitestreamError> {
    let sidecar = &policy.sidecars.litestream;
    if !policy
        .application
        .signing_identity
        .starts_with("Developer ID Application: ")
        || policy.application.team_identifier.len() != 10
        || !policy
            .application
            .team_identifier
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || sidecar.component != manifest.component
        || sidecar.bundle_path != manifest.binary.bundle_path
        || !is_bundle_identifier(&sidecar.identifier)
    {
        return Err(LitestreamError::UnsafeDistributionSigningPolicy);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_distribution_signature(path: &Path, policy: &DistributionSigningPolicy) -> bool {
    let requirement = distribution_code_requirement(policy);
    Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", &format!("-R={requirement}")])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn distribution_code_requirement(policy: &DistributionSigningPolicy) -> String {
    let application = &policy.application;
    let sidecar = &policy.sidecars.litestream;
    format!(
        "identifier {} and anchor apple generic and certificate leaf[subject.OU] = {} and certificate leaf[subject.CN] = {}",
        requirement_string_literal(&sidecar.identifier),
        requirement_string_literal(&application.team_identifier),
        requirement_string_literal(&application.signing_identity),
    )
}

#[cfg(target_os = "macos")]
fn requirement_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn is_bundle_identifier(value: &str) -> bool {
    value.split('.').count() >= 2
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LitestreamRuntimePaths {
    directory: PathBuf,
    config: PathBuf,
    socket: PathBuf,
    pid: PathBuf,
}

impl LitestreamRuntimePaths {
    pub(crate) fn new(data_root: &Path) -> Result<Self, LitestreamError> {
        let directory = data_root.join("run").join("backup");
        let paths = Self {
            config: directory.join("ls.yml"),
            socket: directory.join("ls.sock"),
            pid: directory.join("ls.pid.json"),
            directory,
        };
        ensure_socket_path_fits(&paths.socket)?;
        Ok(paths)
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn config(&self) -> &Path {
        &self.config
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }

    pub(crate) fn pid(&self) -> &Path {
        &self.pid
    }

    pub(crate) fn prepare(&self) -> Result<(), LitestreamError> {
        fs::create_dir_all(&self.directory).map_err(LitestreamError::PrepareRuntime)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))
                .map_err(LitestreamError::PrepareRuntime)?;
        }
        Ok(())
    }

    pub(crate) fn write_config(&self, config: &str) -> Result<(), LitestreamError> {
        self.prepare()?;
        let temporary = self.directory.join("ls.yml.tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(LitestreamError::WriteConfig)?;
        file.write_all(config.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(LitestreamError::WriteConfig)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
                .map_err(LitestreamError::WriteConfig)?;
        }
        fs::rename(&temporary, &self.config).map_err(LitestreamError::WriteConfig)?;
        Ok(())
    }
}

#[cfg(unix)]
fn ensure_socket_path_fits(path: &Path) -> Result<(), LitestreamError> {
    use std::os::unix::ffi::OsStrExt;
    if path.as_os_str().as_bytes().len() > MAX_MACOS_UNIX_SOCKET_PATH_BYTES {
        return Err(LitestreamError::ControlSocketPathTooLong);
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_socket_path_fits(_path: &Path) -> Result<(), LitestreamError> {
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct LitestreamConfig<'a> {
    pub(crate) database_path: &'a Path,
    pub(crate) runtime: &'a LitestreamRuntimePaths,
    pub(crate) bucket: &'a str,
    pub(crate) replica_path: &'a str,
    pub(crate) endpoint: &'a str,
}

impl LitestreamConfig<'_> {
    pub(crate) fn render(&self) -> Result<String, LitestreamError> {
        if !self.database_path.is_absolute() || !self.runtime.socket().is_absolute() {
            return Err(LitestreamError::RelativeDatabasePath);
        }
        let database_path = path_scalar("database_path", self.database_path)?;
        let socket_path = path_scalar("socket_path", self.runtime.socket())?;
        let bucket = scalar("bucket", self.bucket)?;
        let replica_path = scalar("replica_path", self.replica_path)?;
        let endpoint = scalar("endpoint", self.endpoint)?;
        if !self.endpoint.starts_with("https://") {
            return Err(LitestreamError::InvalidConfigField("endpoint"));
        }

        Ok(format!(
            r#"logging:
  level: info
  type: json
  stderr: true

socket:
  enabled: true
  path: {socket_path}
  permissions: 0600

sync-interval: 5s
verify-compaction: true
auto-recover: false
l0-retention: 720h
l0-retention-check-interval: 1m
shutdown-sync-timeout: 30s
shutdown-sync-interval: 500ms

snapshot:
  interval: 6h
  retention: 720h

validation:
  interval: 6h

dbs:
  - path: {database_path}
    monitor-interval: 1s
    checkpoint-interval: 1m
    replica:
      type: s3
      bucket: {bucket}
      path: {replica_path}
      endpoint: {endpoint}
      region: auto
      access-key-id: ${{{ACCESS_KEY_ID_ENV}}}
      secret-access-key: ${{{SECRET_ACCESS_KEY_ENV}}}
      force-path-style: false
      sync-interval: 5s
"#
        ))
    }
}

fn path_scalar(name: &'static str, value: &Path) -> Result<String, LitestreamError> {
    let value = value.to_str().ok_or(LitestreamError::NonUtf8RuntimePath)?;
    scalar(name, value)
}

fn scalar(name: &'static str, value: &str) -> Result<String, LitestreamError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(LitestreamError::InvalidConfigField(name));
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct LitestreamTxid(u64);

impl LitestreamTxid {
    pub(crate) fn from_local(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for LitestreamTxid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:016x}", self.0)
    }
}

impl FromStr for LitestreamTxid {
    type Err = LitestreamError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 16
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(LitestreamError::InvalidTxid);
        }
        u64::from_str_radix(value, 16)
            .map(Self)
            .map_err(|_| LitestreamError::InvalidTxid)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SyncResult {
    pub(crate) database_path: PathBuf,
    pub(crate) txid: LitestreamTxid,
    pub(crate) replica_txid: Option<LitestreamTxid>,
    pub(crate) duration_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncWire {
    db_path: String,
    txid: u64,
    #[serde(default)]
    replica_txid: Option<u64>,
    duration_ms: u64,
}

pub(crate) fn parse_sync_json(
    bytes: &[u8],
    expect_remote: bool,
) -> Result<SyncResult, LitestreamError> {
    ensure_bounded_output(bytes, MAX_CONTROL_OUTPUT_BYTES)?;
    let wire: SyncWire = serde_json::from_slice(bytes).map_err(LitestreamError::InvalidJson)?;
    if expect_remote {
        if wire.replica_txid != Some(wire.txid) {
            return Err(LitestreamError::InvalidSyncContract);
        }
    } else if wire.replica_txid.is_some() {
        return Err(LitestreamError::InvalidSyncContract);
    }
    Ok(SyncResult {
        database_path: PathBuf::from(wire.db_path),
        txid: LitestreamTxid::from_local(wire.txid),
        replica_txid: wire.replica_txid.map(LitestreamTxid::from_local),
        duration_ms: wire.duration_ms,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ReplicaKind {
    S3,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoreFile {
    pub(crate) level: u8,
    pub(crate) name: String,
    pub(crate) min_txid: LitestreamTxid,
    pub(crate) max_txid: LitestreamTxid,
    pub(crate) size: u64,
    pub(crate) timestamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestorePlan {
    pub(crate) source: String,
    pub(crate) target_path: PathBuf,
    pub(crate) replica: ReplicaKind,
    pub(crate) min_txid: LitestreamTxid,
    pub(crate) max_txid: LitestreamTxid,
    pub(crate) files: Vec<RestoreFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestorePlanWire {
    source: String,
    target_path: String,
    replica: ReplicaKind,
    min_txid: String,
    max_txid: String,
    files: Vec<RestoreFileWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreFileWire {
    level: u8,
    name: String,
    min_txid: String,
    max_txid: String,
    size: u64,
    timestamp: String,
}

pub(crate) fn parse_restore_plan_json(bytes: &[u8]) -> Result<RestorePlan, LitestreamError> {
    ensure_bounded_output(bytes, MAX_RESTORE_PLAN_OUTPUT_BYTES)?;
    let wire: RestorePlanWire =
        serde_json::from_slice(bytes).map_err(LitestreamError::InvalidJson)?;
    if wire.files.len() > MAX_RESTORE_FILES {
        return Err(LitestreamError::RestorePlanTooLarge);
    }
    let files = wire
        .files
        .into_iter()
        .map(|file| {
            Ok(RestoreFile {
                level: file.level,
                name: file.name,
                min_txid: file.min_txid.parse()?,
                max_txid: file.max_txid.parse()?,
                size: file.size,
                timestamp: file.timestamp,
            })
        })
        .collect::<Result<Vec<_>, LitestreamError>>()?;
    Ok(RestorePlan {
        source: wire.source,
        target_path: PathBuf::from(wire.target_path),
        replica: wire.replica,
        min_txid: wire.min_txid.parse()?,
        max_txid: wire.max_txid.parse()?,
        files,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum IntegrityCheck {
    Full,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RestoreResult {
    pub(crate) database_path: PathBuf,
    pub(crate) replica: ReplicaKind,
    pub(crate) txid: LitestreamTxid,
    pub(crate) duration_ms: u64,
    pub(crate) integrity_check: IntegrityCheck,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreResultWire {
    db_path: String,
    replica: ReplicaKind,
    txid: String,
    duration_ms: u64,
    integrity_check: IntegrityCheck,
}

pub(crate) fn parse_restore_result_json(bytes: &[u8]) -> Result<RestoreResult, LitestreamError> {
    ensure_bounded_output(bytes, MAX_CONTROL_OUTPUT_BYTES)?;
    let wire: RestoreResultWire =
        serde_json::from_slice(bytes).map_err(LitestreamError::InvalidJson)?;
    Ok(RestoreResult {
        database_path: PathBuf::from(wire.db_path),
        replica: wire.replica,
        txid: wire.txid.parse()?,
        duration_ms: wire.duration_ms,
        integrity_check: wire.integrity_check,
    })
}

fn ensure_bounded_output(bytes: &[u8], limit: usize) -> Result<(), LitestreamError> {
    if bytes.len() > limit {
        return Err(LitestreamError::OversizedControlResponse);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandSpec {
    pub(crate) program: PathBuf,
    pub(crate) arguments: Vec<OsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandResult {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
}

pub(crate) trait CommandExecutor: Send + Sync {
    fn execute(&self, spec: &CommandSpec) -> Result<CommandResult, std::io::Error>;

    fn execute_with_timeout(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
    ) -> Result<CommandResult, std::io::Error>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemCommandExecutor;

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, spec: &CommandSpec) -> Result<CommandResult, std::io::Error> {
        let mut child = Command::new(&spec.program)
            .args(&spec.arguments)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::other("Litestream control stdout pipe is unavailable")
        })?;
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(MAX_CONTROL_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CONTROL_OUTPUT_BYTES {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other(
                "Litestream control output exceeded its safety bound",
            ));
        }
        let status = child.wait()?;
        Ok(CommandResult {
            exit_code: status.code(),
            stdout: bytes,
        })
    }

    fn execute_with_timeout(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
    ) -> Result<CommandResult, std::io::Error> {
        let mut child = Command::new(&spec.program)
            .args(&spec.arguments)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other(
                "Litestream control stdout pipe is unavailable",
            ));
        };
        let reader = match thread::Builder::new()
            .name("dara-litestream-control-output".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                stdout
                    .by_ref()
                    .take(MAX_CONTROL_OUTPUT_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > MAX_CONTROL_OUTPUT_BYTES {
                    return Err(std::io::Error::other(
                        "Litestream control output exceeded its safety bound",
                    ));
                }
                Ok(bytes)
            }) {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(error);
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Litestream control command timed out",
                ));
            }
            thread::sleep(COMMAND_POLL_INTERVAL);
        };
        let bytes = reader
            .join()
            .map_err(|_| std::io::Error::other("Litestream control output reader panicked"))??;
        Ok(CommandResult {
            exit_code: status.code(),
            stdout: bytes,
        })
    }
}

pub(crate) trait LitestreamControl: Send + Sync {
    fn sync_local(&self, database_path: &Path) -> Result<SyncResult, LitestreamError>;
    fn sync_remote(&self, database_path: &Path) -> Result<SyncResult, LitestreamError>;
}

#[derive(Clone, Debug)]
pub(crate) struct CommandLitestreamControl<E> {
    binary: PathBuf,
    socket: PathBuf,
    remote_timeout_seconds: u64,
    executor: E,
}

impl<E> CommandLitestreamControl<E> {
    pub(crate) fn new(
        binary: PathBuf,
        socket: PathBuf,
        remote_timeout_seconds: u64,
        executor: E,
    ) -> Self {
        Self {
            binary,
            socket,
            remote_timeout_seconds,
            executor,
        }
    }
}

impl<E: CommandExecutor> CommandLitestreamControl<E> {
    fn sync(
        &self,
        database_path: &Path,
        wait: bool,
        execution_timeout: Option<Duration>,
    ) -> Result<SyncResult, LitestreamError> {
        if !database_path.is_absolute() {
            return Err(LitestreamError::RelativeDatabasePath);
        }
        let mut arguments = vec![OsString::from("sync")];
        if wait {
            arguments.extend([
                OsString::from("-wait"),
                OsString::from("-timeout"),
                OsString::from(self.remote_timeout_seconds.to_string()),
            ]);
        }
        arguments.extend([
            OsString::from("-json"),
            OsString::from("-socket"),
            self.socket.as_os_str().to_owned(),
            database_path.as_os_str().to_owned(),
        ]);
        let spec = CommandSpec {
            program: self.binary.clone(),
            arguments,
        };
        let result = match execution_timeout {
            Some(timeout) => self.executor.execute_with_timeout(&spec, timeout),
            None => self.executor.execute(&spec),
        }
        .map_err(LitestreamError::Execute)?;
        if result.exit_code != Some(0) {
            return Err(LitestreamError::CommandFailed {
                exit_code: result.exit_code,
            });
        }
        let sync = parse_sync_json(&result.stdout, wait)?;
        if sync.database_path != database_path {
            return Err(LitestreamError::UnexpectedDatabasePath);
        }
        Ok(sync)
    }

    pub(crate) fn sync_local_with_timeout(
        &self,
        database_path: &Path,
        timeout: Duration,
    ) -> Result<SyncResult, LitestreamError> {
        self.sync(database_path, false, Some(timeout))
    }

    pub(crate) fn sync_remote_with_timeout(
        &self,
        database_path: &Path,
        timeout: Duration,
    ) -> Result<SyncResult, LitestreamError> {
        self.sync(database_path, true, Some(timeout))
    }
}

impl<E: CommandExecutor> LitestreamControl for CommandLitestreamControl<E> {
    fn sync_local(&self, database_path: &Path) -> Result<SyncResult, LitestreamError> {
        self.sync(database_path, false, None)
    }

    fn sync_remote(&self, database_path: &Path) -> Result<SyncResult, LitestreamError> {
        self.sync(database_path, true, None)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::fs::PermissionsExt,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Clone, Debug)]
    struct FakeExecutor {
        calls: Arc<Mutex<Vec<CommandSpec>>>,
        result: CommandResult,
    }

    impl CommandExecutor for FakeExecutor {
        fn execute(&self, spec: &CommandSpec) -> Result<CommandResult, std::io::Error> {
            self.calls.lock().expect("fake calls").push(spec.clone());
            Ok(self.result.clone())
        }

        fn execute_with_timeout(
            &self,
            spec: &CommandSpec,
            _timeout: Duration,
        ) -> Result<CommandResult, std::io::Error> {
            self.execute(spec)
        }
    }

    #[test]
    fn txids_normalize_to_canonical_lowercase_hex() {
        let txid = LitestreamTxid::from_local(66);
        assert_eq!(txid.to_string(), "0000000000000042");
        assert_eq!(
            "0000000000000042".parse::<LitestreamTxid>().expect("txid"),
            txid
        );
        for malformed in [
            "42",
            "000000000000004G",
            "000000000000004A",
            "00000000000000000",
        ] {
            assert!(malformed.parse::<LitestreamTxid>().is_err());
        }
    }

    #[test]
    fn parses_pinned_sync_json_contracts() {
        let local = parse_sync_json(
            br#"{"db_path":"/tmp/dara.sqlite3","txid":2,"duration_ms":8}"#,
            false,
        )
        .expect("local sync");
        assert_eq!(local.txid.to_string(), "0000000000000002");
        assert_eq!(local.replica_txid, None);

        let remote = parse_sync_json(
            br#"{"db_path":"/tmp/dara.sqlite3","txid":3,"replica_txid":3,"duration_ms":244}"#,
            true,
        )
        .expect("remote sync");
        assert_eq!(
            remote.replica_txid.expect("replica txid"),
            LitestreamTxid::from_local(3)
        );
        assert!(parse_sync_json(
            br#"{"db_path":"/tmp/dara.sqlite3","txid":3,"duration_ms":244}"#,
            true
        )
        .is_err());
        assert!(matches!(
            parse_sync_json(&vec![b' '; MAX_CONTROL_OUTPUT_BYTES + 1], false),
            Err(LitestreamError::OversizedControlResponse)
        ));
    }

    #[test]
    fn parses_pinned_restore_json_contracts() {
        let plan = parse_restore_plan_json(
            br#"{
              "source":"/tmp/dara.sqlite3",
              "target_path":"/tmp/restore.sqlite3",
              "replica":"s3",
              "min_txid":"0000000000000001",
              "max_txid":"0000000000000002",
              "files":[{
                "level":0,
                "name":"0000000000000002-0000000000000002.ltx",
                "min_txid":"0000000000000002",
                "max_txid":"0000000000000002",
                "size":339,
                "timestamp":"2026-07-27T16:28:33Z"
              }]
            }"#,
        )
        .expect("restore plan");
        assert_eq!(plan.max_txid.to_string(), "0000000000000002");
        assert_eq!(plan.files.len(), 1);

        let result = parse_restore_result_json(
            br#"{
              "db_path":"/tmp/restore.sqlite3",
              "replica":"s3",
              "txid":"0000000000000002",
              "duration_ms":1051,
              "integrity_check":"full"
            }"#,
        )
        .expect("restore result");
        assert_eq!(result.txid, plan.max_txid);
        assert_eq!(result.integrity_check, IntegrityCheck::Full);
    }

    #[test]
    fn rendered_config_has_fixed_safe_protocol_settings_and_no_secret() {
        let directory = tempfile::tempdir().expect("data root");
        let runtime = LitestreamRuntimePaths::new(directory.path()).expect("runtime paths");
        let config = LitestreamConfig {
            database_path: &directory.path().join("dara.sqlite3"),
            runtime: &runtime,
            bucket: "dara-local",
            replica_path: "dara/primary/litestream/v1/epoch/dara.sqlite3",
            endpoint: "https://account.r2.cloudflarestorage.com",
        }
        .render()
        .expect("config");
        for required in [
            "l0-retention: 720h",
            "l0-retention-check-interval: 1m",
            "auto-recover: false",
            "verify-compaction: true",
            "permissions: 0600",
            "snapshot:\n  interval: 6h\n  retention: 720h",
            "${DARA_LITESTREAM_R2_ACCESS_KEY_ID}",
            "${DARA_LITESTREAM_R2_SECRET_ACCESS_KEY}",
        ] {
            assert!(config.contains(required), "missing {required}");
        }
        assert!(!config.contains("secret-value"));
        assert!(!config.contains("access-key-value"));
    }

    #[test]
    fn runtime_files_are_private_and_socket_paths_are_bounded() {
        let directory = tempfile::tempdir().expect("data root");
        let runtime = LitestreamRuntimePaths::new(directory.path()).expect("runtime paths");
        runtime.write_config("test: true\n").expect("write config");
        assert_eq!(
            fs::metadata(runtime.directory())
                .expect("runtime metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(runtime.config())
                .expect("config metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let too_long = directory.path().join("x".repeat(200));
        assert!(matches!(
            LitestreamRuntimePaths::new(&too_long),
            Err(LitestreamError::ControlSocketPathTooLong)
        ));
    }

    #[test]
    fn control_commands_use_only_the_private_socket_and_absolute_database_path() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            result: CommandResult {
                exit_code: Some(0),
                stdout: br#"{"db_path":"/tmp/dara.sqlite3","txid":2,"duration_ms":8}"#.to_vec(),
            },
        };
        let control = CommandLitestreamControl::new(
            PathBuf::from("/app/bin/litestream"),
            PathBuf::from("/private/runtime/ls.sock"),
            60,
            executor,
        );
        control
            .sync_local(Path::new("/tmp/dara.sqlite3"))
            .expect("local sync");
        let calls = calls.lock().expect("calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].arguments,
            [
                "sync",
                "-json",
                "-socket",
                "/private/runtime/ls.sock",
                "/tmp/dara.sqlite3",
            ]
            .map(OsString::from)
        );
        assert!(control.sync_local(Path::new("relative.sqlite3")).is_err());

        let mismatched = CommandLitestreamControl::new(
            PathBuf::from("/app/bin/litestream"),
            PathBuf::from("/private/runtime/ls.sock"),
            60,
            FakeExecutor {
                calls: Arc::new(Mutex::new(Vec::new())),
                result: CommandResult {
                    exit_code: Some(0),
                    stdout: br#"{"db_path":"/tmp/other.sqlite3","txid":2,"duration_ms":8}"#
                        .to_vec(),
                },
            },
        );
        assert!(matches!(
            mismatched.sync_local(Path::new("/tmp/dara.sqlite3")),
            Err(LitestreamError::UnexpectedDatabasePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn timed_control_commands_are_killed_and_reaped() {
        let directory = tempfile::tempdir().expect("control runtime");
        let binary = directory.path().join("blocking-litestream");
        let pid_path = directory.path().join("control.pid");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\necho $$ > '{}'\nwhile :; do :; done\n",
                pid_path.display()
            ),
        )
        .expect("blocking control script");
        let mut permissions = fs::metadata(&binary)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&binary, permissions).expect("script permissions");
        let control = CommandLitestreamControl::new(
            binary,
            directory.path().join("litestream.sock"),
            60,
            SystemCommandExecutor,
        );
        let database_path = directory.path().join("dara.sqlite3");

        for remote in [false, true] {
            let error = if remote {
                control.sync_remote_with_timeout(&database_path, Duration::from_secs(1))
            } else {
                control.sync_local_with_timeout(&database_path, Duration::from_secs(1))
            }
            .expect_err("blocking command must time out");
            assert!(matches!(
                error,
                LitestreamError::Execute(ref source)
                    if source.kind() == std::io::ErrorKind::TimedOut
            ));

            let pid = fs::read_to_string(&pid_path)
                .expect("control pid")
                .trim()
                .parse::<i32>()
                .expect("numeric control pid");
            let result = unsafe { libc::kill(pid, 0) };
            assert_eq!(result, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
    }

    #[test]
    fn embedded_manifest_records_the_compaction_discovery() {
        let manifest = embedded_manifest().expect("embedded manifest");
        validate_protocol_manifest(&manifest).expect("safe protocol manifest");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn distribution_policy_authorizes_only_the_pinned_litestream_identity() {
        let manifest = embedded_manifest().expect("embedded manifest");
        let policy = embedded_distribution_signing_policy().expect("distribution signing policy");
        validate_distribution_signing_policy(&policy, &manifest)
            .expect("safe distribution signing policy");
        assert_eq!(
            distribution_code_requirement(&policy),
            "identifier \"com.silo77.dara.sidecar.litestream\" and anchor apple generic and certificate leaf[subject.OU] = \"PMZH6ULML8\" and certificate leaf[subject.CN] = \"Developer ID Application: SILO77 LLC (PMZH6ULML8)\"",
        );
    }
}
