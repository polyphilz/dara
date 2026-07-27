use std::{fmt, str::FromStr};

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};
use uuid::Uuid;

pub(crate) const OBJECT_FORMAT_VERSION: u32 = 1;
pub(crate) const MANIFEST_FORMAT_VERSION: u32 = 1;
const MAX_PREFIX_BYTES: usize = 512;
const MAX_OBJECT_KEY_BYTES: usize = 1_024;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_DARA_VERSION_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum BackupProvider {
    R2,
}

impl BackupProvider {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::R2 => "R2",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, BackupDomainError> {
        match value {
            "R2" => Ok(Self::R2),
            _ => Err(BackupDomainError::InvalidStoredValue("provider")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum R2Jurisdiction {
    Default,
    Eu,
    Fedramp,
}

impl R2Jurisdiction {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Default => "DEFAULT",
            Self::Eu => "EU",
            Self::Fedramp => "FEDRAMP",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, BackupDomainError> {
        match value {
            "DEFAULT" => Ok(Self::Default),
            "EU" => Ok(Self::Eu),
            "FEDRAMP" => Ok(Self::Fedramp),
            _ => Err(BackupDomainError::InvalidStoredValue("jurisdiction")),
        }
    }

    fn endpoint_suffix(self) -> &'static str {
        match self {
            Self::Default => "r2.cloudflarestorage.com",
            Self::Eu => "eu.r2.cloudflarestorage.com",
            Self::Fedramp => "fedramp.r2.cloudflarestorage.com",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OffsiteMediaState {
    Pending,
    RetryWait,
    Verified,
    Blocked,
}

impl OffsiteMediaState {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::RetryWait => "RETRY_WAIT",
            Self::Verified => "VERIFIED",
            Self::Blocked => "BLOCKED",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, BackupDomainError> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "RETRY_WAIT" => Ok(Self::RetryWait),
            "VERIFIED" => Ok(Self::Verified),
            "BLOCKED" => Ok(Self::Blocked),
            _ => Err(BackupDomainError::InvalidStoredValue("offsite media state")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum MediaBackupPhase {
    Off,
    WaitingForCredentials,
    Idle,
    Uploading,
    RetryWait,
    Blocked,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RelationalBackupPhase {
    Off,
    WaitingForCredentials,
    Starting,
    Running,
    Degraded,
    Blocked,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CheckpointBackupPhase {
    Off,
    WaitingForMedia,
    Fencing,
    WaitingForReplica,
    Validating,
    Publishing,
    Idle,
    Degraded,
    Blocked,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CheckpointPhase {
    Prepared,
    Fenced,
    Replicated,
    Published,
    Failed,
}

impl CheckpointPhase {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::Prepared => "PREPARED",
            Self::Fenced => "FENCED",
            Self::Replicated => "REPLICATED",
            Self::Published => "PUBLISHED",
            Self::Failed => "FAILED",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, BackupDomainError> {
        match value {
            "PREPARED" => Ok(Self::Prepared),
            "FENCED" => Ok(Self::Fenced),
            "REPLICATED" => Ok(Self::Replicated),
            "PUBLISHED" => Ok(Self::Published),
            "FAILED" => Ok(Self::Failed),
            _ => Err(BackupDomainError::InvalidStoredValue("checkpoint phase")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupErrorCode {
    NetworkOffline,
    NetworkTimeout,
    RateLimited,
    ServiceUnavailable,
    KeychainCredentialMissing,
    KeychainUnavailable,
    InvalidTarget,
    AuthenticationRejected,
    AuthorizationRejected,
    PrefixIdentityMismatch,
    OwnerMismatch,
    ImmutableObjectConflict,
    LocalMediaMissing,
    LocalMediaTooLarge,
    LocalMediaHashMismatch,
    WorkerUnavailable,
    LitestreamUnavailable,
    LitestreamFailed,
    FenceTimeout,
    ReplicaBehind,
    ExactTxidUnavailable,
    MalformedManifest,
    RemoteMediaMissing,
    RemoteMediaCorrupt,
    RestoreValidationFailed,
}

impl BackupErrorCode {
    pub(crate) const fn as_db_str(self) -> &'static str {
        match self {
            Self::NetworkOffline => "NETWORK_OFFLINE",
            Self::NetworkTimeout => "NETWORK_TIMEOUT",
            Self::RateLimited => "RATE_LIMITED",
            Self::ServiceUnavailable => "SERVICE_UNAVAILABLE",
            Self::KeychainCredentialMissing => "KEYCHAIN_CREDENTIAL_MISSING",
            Self::KeychainUnavailable => "KEYCHAIN_UNAVAILABLE",
            Self::InvalidTarget => "INVALID_TARGET",
            Self::AuthenticationRejected => "AUTHENTICATION_REJECTED",
            Self::AuthorizationRejected => "AUTHORIZATION_REJECTED",
            Self::PrefixIdentityMismatch => "PREFIX_IDENTITY_MISMATCH",
            Self::OwnerMismatch => "OWNER_MISMATCH",
            Self::ImmutableObjectConflict => "IMMUTABLE_OBJECT_CONFLICT",
            Self::LocalMediaMissing => "LOCAL_MEDIA_MISSING",
            Self::LocalMediaTooLarge => "LOCAL_MEDIA_TOO_LARGE",
            Self::LocalMediaHashMismatch => "LOCAL_MEDIA_HASH_MISMATCH",
            Self::WorkerUnavailable => "WORKER_UNAVAILABLE",
            Self::LitestreamUnavailable => "LITESTREAM_UNAVAILABLE",
            Self::LitestreamFailed => "LITESTREAM_FAILED",
            Self::FenceTimeout => "FENCE_TIMEOUT",
            Self::ReplicaBehind => "REPLICA_BEHIND",
            Self::ExactTxidUnavailable => "EXACT_TXID_UNAVAILABLE",
            Self::MalformedManifest => "MALFORMED_MANIFEST",
            Self::RemoteMediaMissing => "REMOTE_MEDIA_MISSING",
            Self::RemoteMediaCorrupt => "REMOTE_MEDIA_CORRUPT",
            Self::RestoreValidationFailed => "RESTORE_VALIDATION_FAILED",
        }
    }

    pub(crate) fn from_db(value: &str) -> Result<Self, BackupDomainError> {
        match value {
            "NETWORK_OFFLINE" => Ok(Self::NetworkOffline),
            "NETWORK_TIMEOUT" => Ok(Self::NetworkTimeout),
            "RATE_LIMITED" => Ok(Self::RateLimited),
            "SERVICE_UNAVAILABLE" => Ok(Self::ServiceUnavailable),
            "KEYCHAIN_CREDENTIAL_MISSING" => Ok(Self::KeychainCredentialMissing),
            "KEYCHAIN_UNAVAILABLE" => Ok(Self::KeychainUnavailable),
            "INVALID_TARGET" => Ok(Self::InvalidTarget),
            "AUTHENTICATION_REJECTED" => Ok(Self::AuthenticationRejected),
            "AUTHORIZATION_REJECTED" => Ok(Self::AuthorizationRejected),
            "PREFIX_IDENTITY_MISMATCH" => Ok(Self::PrefixIdentityMismatch),
            "OWNER_MISMATCH" => Ok(Self::OwnerMismatch),
            "IMMUTABLE_OBJECT_CONFLICT" => Ok(Self::ImmutableObjectConflict),
            "LOCAL_MEDIA_MISSING" => Ok(Self::LocalMediaMissing),
            "LOCAL_MEDIA_TOO_LARGE" => Ok(Self::LocalMediaTooLarge),
            "LOCAL_MEDIA_HASH_MISMATCH" => Ok(Self::LocalMediaHashMismatch),
            "WORKER_UNAVAILABLE" => Ok(Self::WorkerUnavailable),
            "LITESTREAM_UNAVAILABLE" => Ok(Self::LitestreamUnavailable),
            "LITESTREAM_FAILED" => Ok(Self::LitestreamFailed),
            "FENCE_TIMEOUT" => Ok(Self::FenceTimeout),
            "REPLICA_BEHIND" => Ok(Self::ReplicaBehind),
            "EXACT_TXID_UNAVAILABLE" => Ok(Self::ExactTxidUnavailable),
            "MALFORMED_MANIFEST" => Ok(Self::MalformedManifest),
            "REMOTE_MEDIA_MISSING" => Ok(Self::RemoteMediaMissing),
            "REMOTE_MEDIA_CORRUPT" => Ok(Self::RemoteMediaCorrupt),
            "RESTORE_VALIDATION_FAILED" => Ok(Self::RestoreValidationFailed),
            _ => Err(BackupDomainError::InvalidStoredValue("backup error code")),
        }
    }

    pub(crate) const fn blocks_all_media(self) -> bool {
        matches!(
            self,
            Self::KeychainCredentialMissing
                | Self::KeychainUnavailable
                | Self::InvalidTarget
                | Self::AuthenticationRejected
                | Self::AuthorizationRejected
                | Self::PrefixIdentityMismatch
                | Self::OwnerMismatch
                | Self::WorkerUnavailable
        )
    }
}

macro_rules! uuid_v7_id {
    ($name:ident, $field:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(String);

        impl $name {
            pub(crate) fn new() -> Self {
                Self(Uuid::now_v7().to_string())
            }

            pub(crate) fn parse(value: impl Into<String>) -> Result<Self, BackupDomainError> {
                let value = value.into();
                let parsed =
                    Uuid::parse_str(&value).map_err(|_| BackupDomainError::InvalidField($field))?;
                if parsed.get_version_num() != 7 || parsed.to_string() != value {
                    return Err(BackupDomainError::InvalidField($field));
                }
                Ok(Self(value))
            }

            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = BackupDomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(de::Error::custom)
            }
        }
    };
}

uuid_v7_id!(BackupSetId, "backupSetId");
uuid_v7_id!(ReplicaEpochId, "replicaEpochId");
uuid_v7_id!(CheckpointId, "checkpointId");
uuid_v7_id!(InstallationId, "installationId");
uuid_v7_id!(ProbeRunId, "probeRunId");

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct R2AccountId(String);

impl R2AccountId {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, BackupDomainError> {
        let value = value.into();
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BackupDomainError::InvalidField("accountId"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct R2BucketName(String);

impl R2BucketName {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, BackupDomainError> {
        let value = value.into();
        let valid_character =
            |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
        if !(3..=63).contains(&value.len())
            || !value.bytes().all(valid_character)
            || !value
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            || !value
                .as_bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(BackupDomainError::InvalidField("bucket"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct R2Prefix(String);

impl R2Prefix {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, BackupDomainError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty()
            || bytes.len() > MAX_PREFIX_BYTES
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains("..")
            || value.contains('\\')
            || value.contains('?')
            || value.contains('#')
            || value.chars().any(char::is_control)
            || value.split('/').any(|segment| {
                segment.is_empty()
                    || segment == "."
                    || !segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(BackupDomainError::InvalidField("prefix"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct R2Target {
    pub(crate) account_id: R2AccountId,
    pub(crate) jurisdiction: R2Jurisdiction,
    pub(crate) bucket: R2BucketName,
    pub(crate) prefix: R2Prefix,
}

impl R2Target {
    pub(crate) fn endpoint(&self) -> String {
        format!(
            "https://{}.{}",
            self.account_id.as_str(),
            self.jurisdiction.endpoint_suffix()
        )
    }

    pub(crate) fn keyspace(&self) -> R2Keyspace {
        R2Keyspace {
            prefix: self.prefix.clone(),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) struct ContentSha256([u8; 32]);

impl ContentSha256 {
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) fn from_slice(bytes: &[u8]) -> Result<Self, BackupDomainError> {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| BackupDomainError::InvalidField("sha256"))?;
        Ok(Self(bytes))
    }

    pub(crate) fn parse_hex(value: &str) -> Result<Self, BackupDomainError> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(BackupDomainError::InvalidField("sha256"));
        }
        let mut bytes = [0_u8; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write;
            let _ = write!(output, "{byte:02x}");
        }
        output
    }
}

impl fmt::Debug for ContentSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ContentSha256")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for ContentSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl Serialize for ContentSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ContentSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse_hex(&value).map_err(de::Error::custom)
    }
}

fn hex_nibble(byte: u8) -> Result<u8, BackupDomainError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(BackupDomainError::InvalidField("sha256")),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct R2ObjectKey(String);

impl R2ObjectKey {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct R2ListPrefix(String);

impl R2ListPrefix {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LitestreamReplicaPath(String);

impl LitestreamReplicaPath {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct R2Keyspace {
    prefix: R2Prefix,
}

impl R2Keyspace {
    pub(crate) fn root_prefix(&self) -> R2ListPrefix {
        R2ListPrefix(format!("{}/", self.prefix.as_str()))
    }

    pub(crate) fn identity(&self) -> R2ObjectKey {
        self.fixed_key("identity/v1.json")
    }

    pub(crate) fn owner(&self) -> R2ObjectKey {
        self.fixed_key("owner/v1.json")
    }

    pub(crate) fn media(&self, sha256: ContentSha256) -> R2ObjectKey {
        let hex = sha256.to_hex();
        self.fixed_key(&format!("media/v1/sha256/{}/{}.webp", &hex[..2], hex))
    }

    pub(crate) fn litestream(&self, epoch: &ReplicaEpochId) -> LitestreamReplicaPath {
        LitestreamReplicaPath(
            self.fixed_key(&format!("litestream/v1/{}/dara.sqlite3", epoch.as_str()))
                .0,
        )
    }

    pub(crate) fn checkpoint(
        &self,
        epoch: &ReplicaEpochId,
        checkpoint: &CheckpointId,
        created_at: &UtcTimestamp,
    ) -> Result<R2ObjectKey, BackupDomainError> {
        let timestamp = created_at.basic_utc()?;
        Ok(self.fixed_key(&format!(
            "checkpoints/v1/{}/{timestamp}-{}.json",
            epoch.as_str(),
            checkpoint.as_str()
        )))
    }

    pub(crate) fn probe_prefix(&self, run_id: &ProbeRunId) -> R2ListPrefix {
        R2ListPrefix(format!(
            "{}/probes/{}/",
            self.prefix.as_str(),
            run_id.as_str()
        ))
    }

    pub(crate) fn probe_object(&self, run_id: &ProbeRunId) -> R2ObjectKey {
        self.fixed_key(&format!("probes/{}/object.bin", run_id.as_str()))
    }

    pub(crate) fn probe_litestream(&self, run_id: &ProbeRunId) -> LitestreamReplicaPath {
        LitestreamReplicaPath(
            self.fixed_key(&format!(
                "probes/{}/litestream/dara.sqlite3",
                run_id.as_str()
            ))
            .0,
        )
    }

    pub(crate) fn validate_returned_key(
        &self,
        value: impl Into<String>,
    ) -> Result<R2ObjectKey, BackupDomainError> {
        let value = value.into();
        let root = format!("{}/", self.prefix.as_str());
        if !value.starts_with(&root) {
            return Err(BackupDomainError::KeyOutsidePrefix);
        }
        validate_object_key(&value)?;
        Ok(R2ObjectKey(value))
    }

    fn fixed_key(&self, suffix: &str) -> R2ObjectKey {
        let value = format!("{}/{suffix}", self.prefix.as_str());
        debug_assert!(validate_object_key(&value).is_ok());
        R2ObjectKey(value)
    }
}

fn validate_object_key(value: &str) -> Result<(), BackupDomainError> {
    if value.is_empty()
        || value.len() > MAX_OBJECT_KEY_BYTES
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || value.contains("..")
        || value.contains('\\')
        || value.contains('?')
        || value.contains('#')
        || value.chars().any(char::is_control)
    {
        return Err(BackupDomainError::InvalidObjectKey);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UtcTimestamp(String);

impl UtcTimestamp {
    pub(crate) fn now() -> Result<Self, BackupDomainError> {
        let value = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| BackupDomainError::InvalidField("createdAt"))?;
        Self::parse(value)
    }

    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, BackupDomainError> {
        let value = value.into();
        let parsed = OffsetDateTime::parse(&value, &Rfc3339)
            .map_err(|_| BackupDomainError::InvalidField("createdAt"))?;
        if parsed.offset() != UtcOffset::UTC || !value.ends_with('Z') {
            return Err(BackupDomainError::InvalidField("createdAt"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn basic_utc(&self) -> Result<String, BackupDomainError> {
        let value = OffsetDateTime::parse(&self.0, &Rfc3339)
            .map_err(|_| BackupDomainError::InvalidField("createdAt"))?;
        Ok(format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            value.year(),
            u8::from(value.month()),
            value.day(),
            value.hour(),
            value.minute(),
            value.second()
        ))
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct IdentityManifestV1 {
    format_version: u32,
    backup_set_id: BackupSetId,
    object_format_version: u32,
    original_installation_id: InstallationId,
}

impl IdentityManifestV1 {
    pub(crate) fn new(
        backup_set_id: BackupSetId,
        original_installation_id: InstallationId,
    ) -> Self {
        Self {
            format_version: MANIFEST_FORMAT_VERSION,
            backup_set_id,
            object_format_version: OBJECT_FORMAT_VERSION,
            original_installation_id,
        }
    }

    pub(crate) fn backup_set_id(&self) -> &BackupSetId {
        &self.backup_set_id
    }

    pub(crate) fn to_json(&self) -> Result<Vec<u8>, BackupDomainError> {
        encode_manifest(self)
    }

    pub(crate) fn from_json(bytes: &[u8]) -> Result<Self, BackupDomainError> {
        let manifest: Self = decode_manifest(bytes)?;
        if manifest.format_version != MANIFEST_FORMAT_VERSION
            || manifest.object_format_version != OBJECT_FORMAT_VERSION
        {
            return Err(BackupDomainError::UnsupportedManifestVersion);
        }
        Ok(manifest)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnerManifestV1 {
    format_version: u32,
    backup_set_id: BackupSetId,
    installation_id: InstallationId,
    replica_epoch_id: ReplicaEpochId,
    updated_at: UtcTimestamp,
}

impl OwnerManifestV1 {
    pub(crate) fn new(
        backup_set_id: BackupSetId,
        installation_id: InstallationId,
        replica_epoch_id: ReplicaEpochId,
        updated_at: UtcTimestamp,
    ) -> Self {
        Self {
            format_version: MANIFEST_FORMAT_VERSION,
            backup_set_id,
            installation_id,
            replica_epoch_id,
            updated_at,
        }
    }

    pub(crate) fn to_json(&self) -> Result<Vec<u8>, BackupDomainError> {
        encode_manifest(self)
    }

    pub(crate) fn from_json(bytes: &[u8]) -> Result<Self, BackupDomainError> {
        let manifest: Self = decode_manifest(bytes)?;
        if manifest.format_version != MANIFEST_FORMAT_VERSION {
            return Err(BackupDomainError::UnsupportedManifestVersion);
        }
        Ok(manifest)
    }

    pub(crate) fn matches(
        &self,
        backup_set_id: &BackupSetId,
        installation_id: &InstallationId,
        replica_epoch_id: &ReplicaEpochId,
    ) -> bool {
        &self.backup_set_id == backup_set_id
            && &self.installation_id == installation_id
            && &self.replica_epoch_id == replica_epoch_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckpointMainManifestV1 {
    migration_head: u32,
    litestream_path: String,
    txid: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckpointMediaManifestV1 {
    migration_head: u32,
    object_format_version: u32,
    referenced_hash_count: u64,
    referenced_total_bytes: u64,
    referenced_hash_set_sha256: ContentSha256,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CheckpointManifestV1 {
    format_version: u32,
    backup_set_id: BackupSetId,
    replica_epoch_id: ReplicaEpochId,
    checkpoint_id: CheckpointId,
    created_at: UtcTimestamp,
    dara_version: String,
    content_revision: u64,
    main: CheckpointMainManifestV1,
    media: CheckpointMediaManifestV1,
}

pub(crate) struct CheckpointManifestInput {
    pub(crate) backup_set_id: BackupSetId,
    pub(crate) replica_epoch_id: ReplicaEpochId,
    pub(crate) checkpoint_id: CheckpointId,
    pub(crate) created_at: UtcTimestamp,
    pub(crate) dara_version: String,
    pub(crate) content_revision: u64,
    pub(crate) main_migration_head: u32,
    pub(crate) litestream_path: LitestreamReplicaPath,
    pub(crate) txid: String,
    pub(crate) media_migration_head: u32,
    pub(crate) referenced_hash_count: u64,
    pub(crate) referenced_total_bytes: u64,
    pub(crate) referenced_hash_set_sha256: ContentSha256,
}

impl CheckpointManifestV1 {
    pub(crate) fn new(input: CheckpointManifestInput) -> Result<Self, BackupDomainError> {
        if input.dara_version.is_empty()
            || input.dara_version.len() > MAX_DARA_VERSION_BYTES
            || input.dara_version.chars().any(char::is_control)
            || input.main_migration_head == 0
            || input.media_migration_head == 0
            || !is_canonical_txid(&input.txid)
        {
            return Err(BackupDomainError::InvalidManifest);
        }
        Ok(Self {
            format_version: MANIFEST_FORMAT_VERSION,
            backup_set_id: input.backup_set_id,
            replica_epoch_id: input.replica_epoch_id,
            checkpoint_id: input.checkpoint_id,
            created_at: input.created_at,
            dara_version: input.dara_version,
            content_revision: input.content_revision,
            main: CheckpointMainManifestV1 {
                migration_head: input.main_migration_head,
                litestream_path: input.litestream_path.0,
                txid: input.txid,
            },
            media: CheckpointMediaManifestV1 {
                migration_head: input.media_migration_head,
                object_format_version: OBJECT_FORMAT_VERSION,
                referenced_hash_count: input.referenced_hash_count,
                referenced_total_bytes: input.referenced_total_bytes,
                referenced_hash_set_sha256: input.referenced_hash_set_sha256,
            },
        })
    }

    pub(crate) fn to_json(&self) -> Result<Vec<u8>, BackupDomainError> {
        encode_manifest(self)
    }

    pub(crate) fn from_json(
        bytes: &[u8],
        keyspace: &R2Keyspace,
    ) -> Result<Self, BackupDomainError> {
        let manifest: Self = decode_manifest(bytes)?;
        if manifest.format_version != MANIFEST_FORMAT_VERSION
            || manifest.media.object_format_version != OBJECT_FORMAT_VERSION
            || manifest.dara_version.is_empty()
            || manifest.dara_version.len() > MAX_DARA_VERSION_BYTES
            || manifest.dara_version.chars().any(char::is_control)
            || manifest.main.migration_head == 0
            || manifest.media.migration_head == 0
            || !is_canonical_txid(&manifest.main.txid)
        {
            return Err(BackupDomainError::InvalidManifest);
        }
        let expected_path = keyspace.litestream(&manifest.replica_epoch_id);
        if manifest.main.litestream_path != expected_path.as_str() {
            return Err(BackupDomainError::KeyOutsidePrefix);
        }
        Ok(manifest)
    }

    pub(crate) fn object_key(
        &self,
        keyspace: &R2Keyspace,
    ) -> Result<R2ObjectKey, BackupDomainError> {
        keyspace.checkpoint(
            &self.replica_epoch_id,
            &self.checkpoint_id,
            &self.created_at,
        )
    }

    pub(crate) fn matches_published_evidence(
        &self,
        checkpoint_id: &CheckpointId,
        backup_set_id: &BackupSetId,
        replica_epoch_id: &ReplicaEpochId,
        content_revision: u64,
        litestream_txid: &str,
    ) -> bool {
        self.checkpoint_id == *checkpoint_id
            && self.backup_set_id == *backup_set_id
            && self.replica_epoch_id == *replica_epoch_id
            && self.content_revision == content_revision
            && self.main.txid == litestream_txid
    }
}

fn is_canonical_txid(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn encode_manifest<T: Serialize>(manifest: &T) -> Result<Vec<u8>, BackupDomainError> {
    let bytes = serde_json::to_vec(manifest).map_err(BackupDomainError::ManifestJson)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(BackupDomainError::ManifestTooLarge);
    }
    Ok(bytes)
}

fn decode_manifest<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, BackupDomainError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(BackupDomainError::ManifestTooLarge);
    }
    serde_json::from_slice(bytes).map_err(BackupDomainError::ManifestJson)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BackupDomainError {
    #[error("invalid off-site backup field: {0}")]
    InvalidField(&'static str),
    #[error("invalid stored off-site backup value: {0}")]
    InvalidStoredValue(&'static str),
    #[error("R2 object key is invalid")]
    InvalidObjectKey,
    #[error("R2 object key is outside the configured prefix")]
    KeyOutsidePrefix,
    #[error("off-site backup manifest is too large")]
    ManifestTooLarge,
    #[error("off-site backup manifest uses an unsupported version")]
    UnsupportedManifestVersion,
    #[error("off-site backup manifest is invalid")]
    InvalidManifest,
    #[error("off-site backup manifest JSON is invalid")]
    ManifestJson(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> R2Target {
        R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-local").expect("bucket"),
            prefix: R2Prefix::parse("dara/primary").expect("prefix"),
        }
    }

    #[test]
    fn validates_r2_target_and_derives_only_cloudflare_endpoints() {
        let target = target();
        assert_eq!(
            target.endpoint(),
            "https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com"
        );
        for invalid in [
            "",
            "/dara/primary",
            "dara//primary",
            "dara/../other",
            "dara\\primary",
            "dara/primary/",
            "dara/primary?query",
        ] {
            assert!(R2Prefix::parse(invalid).is_err(), "{invalid}");
        }
        for invalid in ["Dara", "ab", "-dara", "dara_", "dara-"] {
            assert!(R2BucketName::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn every_key_is_confined_to_the_validated_prefix() {
        let keyspace = target().keyspace();
        let epoch = ReplicaEpochId::new();
        let run = ProbeRunId::new();
        let hash = ContentSha256::from_bytes([0xab; 32]);
        for value in [
            keyspace.identity().0,
            keyspace.owner().0,
            keyspace.media(hash).0,
            keyspace.litestream(&epoch).0,
            keyspace.probe_object(&run).0,
            keyspace.probe_litestream(&run).0,
        ] {
            assert!(value.starts_with("dara/primary/"), "{value}");
            keyspace.validate_returned_key(value).expect("valid key");
        }
        assert!(matches!(
            keyspace.validate_returned_key("other/prefix/object"),
            Err(BackupDomainError::KeyOutsidePrefix)
        ));
    }

    #[test]
    fn strict_versioned_manifests_round_trip_and_reject_unknown_fields() {
        let backup_set_id = BackupSetId::new();
        let identity = IdentityManifestV1::new(backup_set_id.clone(), InstallationId::new());
        let json = identity.to_json().expect("identity JSON");
        assert_eq!(
            IdentityManifestV1::from_json(&json)
                .expect("identity")
                .backup_set_id(),
            &backup_set_id
        );
        assert!(IdentityManifestV1::from_json(
            br#"{"formatVersion":1,"backupSetId":"01980c8e-6c00-7000-8000-000000000001","objectFormatVersion":1,"originalInstallationId":"01980c8e-6c00-7000-8000-000000000002","extra":true}"#
        )
        .is_err());
    }

    #[test]
    fn checkpoint_manifest_requires_the_epoch_specific_litestream_path() {
        let target = target();
        let keyspace = target.keyspace();
        let epoch = ReplicaEpochId::new();
        let checkpoint = CheckpointManifestV1::new(CheckpointManifestInput {
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: epoch.clone(),
            checkpoint_id: CheckpointId::new(),
            created_at: UtcTimestamp::parse("2026-07-27T16:28:33Z").expect("timestamp"),
            dara_version: "0.1.0".into(),
            content_revision: 42,
            main_migration_head: 8,
            litestream_path: keyspace.litestream(&epoch),
            txid: "0000000000000042".into(),
            media_migration_head: 2,
            referenced_hash_count: 1,
            referenced_total_bytes: 12,
            referenced_hash_set_sha256: ContentSha256::from_bytes([0xcd; 32]),
        })
        .expect("checkpoint");
        let json = checkpoint.to_json().expect("checkpoint JSON");
        CheckpointManifestV1::from_json(&json, &keyspace).expect("checkpoint round trip");

        let other = R2Target {
            prefix: R2Prefix::parse("dara/other").expect("other prefix"),
            ..target
        };
        assert!(matches!(
            CheckpointManifestV1::from_json(&json, &other.keyspace()),
            Err(BackupDomainError::KeyOutsidePrefix)
        ));
    }
}
