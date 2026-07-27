use std::{
    fmt,
    io::{Cursor, Read},
    time::Duration,
};

use reqwest::{
    blocking::{Body, Client, Response},
    header::{HeaderMap, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MATCH, IF_NONE_MATCH},
    redirect::Policy,
    StatusCode,
};
use rusty_s3::{actions::ListObjectsV2, Bucket, Credentials, S3Action, UrlStyle};

use super::{
    credentials::R2Credentials,
    domain::{
        ContentSha256, R2Keyspace, R2ListPrefix, R2ObjectKey, R2Target, OBJECT_FORMAT_VERSION,
    },
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SIGNED_URL_LIFETIME: Duration = Duration::from_secs(60);
const MAX_OBJECT_BYTES: usize = 32 * 1024 * 1024;
const MAX_LIST_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_LIST_OBJECTS: usize = 1_000;
const MAX_ETAG_BYTES: usize = 256;
const MAX_CONTINUATION_TOKEN_BYTES: usize = 4 * 1024;
const DARA_SHA256_HEADER: &str = "x-amz-meta-dara-sha256";
const DARA_OBJECT_FORMAT_HEADER: &str = "x-amz-meta-dara-object-format";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectContentType {
    Binary,
    Json,
    Webp,
}

impl ObjectContentType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "application/octet-stream",
            Self::Json => "application/json",
            Self::Webp => "image/webp",
        }
    }

    fn parse(value: Option<&HeaderValue>) -> Result<Option<Self>, ObjectStoreError> {
        let Some(value) = value else {
            return Ok(None);
        };
        match value.to_str().map_err(|_| invalid_response())? {
            "application/octet-stream" => Ok(Some(Self::Binary)),
            "application/json" => Ok(Some(Self::Json)),
            "image/webp" => Ok(Some(Self::Webp)),
            _ => Err(invalid_response()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObjectVersion(String);

impl ObjectVersion {
    fn parse(value: impl Into<String>) -> Result<Self, ObjectStoreError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_ETAG_BYTES || value.chars().any(char::is_control) {
            return Err(invalid_response());
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObjectMetadata {
    pub(crate) byte_length: u64,
    pub(crate) version: ObjectVersion,
    pub(crate) content_type: Option<ObjectContentType>,
    pub(crate) dara_sha256: Option<ContentSha256>,
    pub(crate) object_format_version: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GetObjectResult {
    pub(crate) metadata: ObjectMetadata,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PutCondition {
    IfAbsent,
    IfMatch(ObjectVersion),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PutObjectOutcome {
    Stored,
    ConditionNotMet,
}

#[derive(Debug)]
pub(crate) struct PutObjectRequest {
    pub(crate) key: R2ObjectKey,
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: ObjectContentType,
    pub(crate) dara_sha256: Option<ContentSha256>,
    pub(crate) condition: PutCondition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListedObject {
    pub(crate) key: R2ObjectKey,
    pub(crate) byte_length: u64,
    pub(crate) version: ObjectVersion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContinuationToken(String);

impl ContinuationToken {
    fn parse(value: impl Into<String>) -> Result<Self, ObjectStoreError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_CONTINUATION_TOKEN_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(invalid_response());
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListObjectsPage {
    pub(crate) objects: Vec<ListedObject>,
    pub(crate) next: Option<ContinuationToken>,
}

pub(crate) trait ObjectStore: Send + Sync {
    fn head(&self, key: &R2ObjectKey) -> Result<Option<ObjectMetadata>, ObjectStoreError>;
    fn get(&self, key: &R2ObjectKey) -> Result<GetObjectResult, ObjectStoreError>;
    fn put(&self, request: PutObjectRequest) -> Result<PutObjectOutcome, ObjectStoreError>;
    fn list(
        &self,
        prefix: &R2ListPrefix,
        continuation: Option<&ContinuationToken>,
    ) -> Result<ListObjectsPage, ObjectStoreError>;
    fn delete(&self, key: &R2ObjectKey) -> Result<(), ObjectStoreError>;
}

pub(crate) struct R2ObjectStore {
    client: Client,
    bucket: Bucket,
    credentials: Credentials,
    keyspace: R2Keyspace,
}

impl fmt::Debug for R2ObjectStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("R2ObjectStore")
            .field("bucket", &self.bucket.name())
            .field("credentials", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl R2ObjectStore {
    pub(crate) fn new(
        target: R2Target,
        credentials: &R2Credentials,
    ) -> Result<Self, ObjectStoreError> {
        let endpoint = target
            .endpoint()
            .parse()
            .map_err(|_| ObjectStoreError::new(ObjectStoreErrorCode::InvalidConfiguration))?;
        let bucket = Bucket::new(
            endpoint,
            UrlStyle::VirtualHost,
            target.bucket.as_str().to_owned(),
            "auto",
        )
        .map_err(|_| ObjectStoreError::new(ObjectStoreErrorCode::InvalidConfiguration))?;
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .https_only(true)
            .build()
            .map_err(|_| ObjectStoreError::new(ObjectStoreErrorCode::InvalidConfiguration))?;
        let keyspace = target.keyspace();
        Ok(Self {
            client,
            bucket,
            credentials: Credentials::new(
                credentials.access_key_id().to_owned(),
                credentials.secret_access_key().to_owned(),
            ),
            keyspace,
        })
    }

    fn signing_credentials(&self) -> Credentials {
        self.credentials.clone()
    }

    fn validate_key(&self, key: &R2ObjectKey) -> Result<(), ObjectStoreError> {
        self.keyspace
            .validate_returned_key(key.as_str())
            .map(|_| ())
            .map_err(|_| ObjectStoreError::new(ObjectStoreErrorCode::KeyOutsidePrefix))
    }

    fn validate_prefix(&self, prefix: &R2ListPrefix) -> Result<(), ObjectStoreError> {
        let root = self.keyspace.root_prefix();
        if !prefix.as_str().starts_with(root.as_str()) || !prefix.as_str().ends_with('/') {
            return Err(ObjectStoreError::new(
                ObjectStoreErrorCode::KeyOutsidePrefix,
            ));
        }
        Ok(())
    }

    fn send(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<Response, ObjectStoreError> {
        request.send().map_err(|error| {
            if error.is_timeout() {
                ObjectStoreError::new(ObjectStoreErrorCode::Timeout)
            } else {
                ObjectStoreError::new(ObjectStoreErrorCode::Network)
            }
        })
    }
}

impl ObjectStore for R2ObjectStore {
    fn head(&self, key: &R2ObjectKey) -> Result<Option<ObjectMetadata>, ObjectStoreError> {
        self.validate_key(key)?;
        let credentials = self.signing_credentials();
        let action = self.bucket.head_object(Some(&credentials), key.as_str());
        let response = self.send(self.client.head(action.sign(SIGNED_URL_LIFETIME)))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure_success(response.status())?;
        Ok(Some(parse_metadata(response.headers())?))
    }

    fn get(&self, key: &R2ObjectKey) -> Result<GetObjectResult, ObjectStoreError> {
        self.validate_key(key)?;
        let credentials = self.signing_credentials();
        let action = self.bucket.get_object(Some(&credentials), key.as_str());
        let response = self.send(self.client.get(action.sign(SIGNED_URL_LIFETIME)))?;
        ensure_success(response.status())?;
        let metadata = parse_metadata(response.headers())?;
        let bytes = read_bounded(response, MAX_OBJECT_BYTES)?;
        if bytes.len() as u64 != metadata.byte_length {
            return Err(invalid_response());
        }
        Ok(GetObjectResult { metadata, bytes })
    }

    fn put(&self, request: PutObjectRequest) -> Result<PutObjectOutcome, ObjectStoreError> {
        self.validate_key(&request.key)?;
        if request.bytes.len() > MAX_OBJECT_BYTES {
            return Err(ObjectStoreError::new(ObjectStoreErrorCode::ObjectTooLarge));
        }
        let byte_length = request.bytes.len();
        let credentials = self.signing_credentials();
        let mut action = self
            .bucket
            .put_object(Some(&credentials), request.key.as_str());
        action
            .headers_mut()
            .insert("content-type", request.content_type.as_str());
        action
            .headers_mut()
            .insert("content-length", byte_length.to_string());
        action
            .headers_mut()
            .insert(DARA_OBJECT_FORMAT_HEADER, OBJECT_FORMAT_VERSION.to_string());
        let sha256 = request.dara_sha256.map(ContentSha256::to_hex);
        if let Some(sha256) = &sha256 {
            action
                .headers_mut()
                .insert(DARA_SHA256_HEADER, sha256.clone());
        }
        match &request.condition {
            PutCondition::IfAbsent => {
                action.headers_mut().insert("if-none-match", "*");
            }
            PutCondition::IfMatch(version) => {
                action
                    .headers_mut()
                    .insert("if-match", version.as_str().to_owned());
            }
        }
        let url = action.sign(SIGNED_URL_LIFETIME);
        let mut builder = self
            .client
            .put(url)
            .header(CONTENT_TYPE, request.content_type.as_str())
            .header(CONTENT_LENGTH, byte_length)
            .header(DARA_OBJECT_FORMAT_HEADER, OBJECT_FORMAT_VERSION)
            .body(Body::new(Cursor::new(request.bytes)));
        if let Some(sha256) = sha256 {
            builder = builder.header(DARA_SHA256_HEADER, sha256);
        }
        builder = match request.condition {
            PutCondition::IfAbsent => builder.header(IF_NONE_MATCH, "*"),
            PutCondition::IfMatch(version) => builder.header(IF_MATCH, version.as_str()),
        };
        let response = self.send(builder)?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Ok(PutObjectOutcome::ConditionNotMet);
        }
        ensure_success(response.status())?;
        parse_version(response.headers())?;
        Ok(PutObjectOutcome::Stored)
    }

    fn list(
        &self,
        prefix: &R2ListPrefix,
        continuation: Option<&ContinuationToken>,
    ) -> Result<ListObjectsPage, ObjectStoreError> {
        self.validate_prefix(prefix)?;
        let credentials = self.signing_credentials();
        let mut action = self.bucket.list_objects_v2(Some(&credentials));
        action.with_prefix(prefix.as_str().to_owned());
        action.with_max_keys(MAX_LIST_OBJECTS);
        if let Some(continuation) = continuation {
            action.with_continuation_token(continuation.as_str().to_owned());
        }
        let response = self.send(self.client.get(action.sign(SIGNED_URL_LIFETIME)))?;
        ensure_success(response.status())?;
        let bytes = read_bounded(response, MAX_LIST_RESPONSE_BYTES)?;
        let body = std::str::from_utf8(&bytes).map_err(|_| invalid_response())?;
        let parsed = ListObjectsV2::parse_response(body).map_err(|_| invalid_response())?;
        if parsed.contents.len() > MAX_LIST_OBJECTS {
            return Err(invalid_response());
        }
        let objects = parsed
            .contents
            .into_iter()
            .map(|object| {
                if !object.key.starts_with(prefix.as_str()) {
                    return Err(ObjectStoreError::new(
                        ObjectStoreErrorCode::KeyOutsidePrefix,
                    ));
                }
                Ok(ListedObject {
                    key: self
                        .keyspace
                        .validate_returned_key(object.key)
                        .map_err(|_| {
                            ObjectStoreError::new(ObjectStoreErrorCode::KeyOutsidePrefix)
                        })?,
                    byte_length: object.size,
                    version: ObjectVersion::parse(object.etag)?,
                })
            })
            .collect::<Result<Vec<_>, ObjectStoreError>>()?;
        let next = parsed
            .next_continuation_token
            .map(ContinuationToken::parse)
            .transpose()?;
        Ok(ListObjectsPage { objects, next })
    }

    fn delete(&self, key: &R2ObjectKey) -> Result<(), ObjectStoreError> {
        self.validate_key(key)?;
        let credentials = self.signing_credentials();
        let action = self.bucket.delete_object(Some(&credentials), key.as_str());
        let response = self.send(self.client.delete(action.sign(SIGNED_URL_LIFETIME)))?;
        ensure_success(response.status())
    }
}

fn parse_metadata(headers: &HeaderMap) -> Result<ObjectMetadata, ObjectStoreError> {
    let byte_length = headers
        .get(CONTENT_LENGTH)
        .ok_or_else(invalid_response)?
        .to_str()
        .map_err(|_| invalid_response())?
        .parse::<u64>()
        .map_err(|_| invalid_response())?;
    let version = parse_version(headers)?;
    let content_type = ObjectContentType::parse(headers.get(CONTENT_TYPE))?;
    let dara_sha256 = headers
        .get(DARA_SHA256_HEADER)
        .map(|value| {
            let value = value.to_str().map_err(|_| invalid_response())?;
            ContentSha256::parse_hex(value).map_err(|_| invalid_response())
        })
        .transpose()?;
    let object_format_version = headers
        .get(DARA_OBJECT_FORMAT_HEADER)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| invalid_response())?
                .parse::<u32>()
                .map_err(|_| invalid_response())
        })
        .transpose()?;
    Ok(ObjectMetadata {
        byte_length,
        version,
        content_type,
        dara_sha256,
        object_format_version,
    })
}

fn parse_version(headers: &HeaderMap) -> Result<ObjectVersion, ObjectStoreError> {
    let etag = headers
        .get(ETAG)
        .ok_or_else(invalid_response)?
        .to_str()
        .map_err(|_| invalid_response())?;
    ObjectVersion::parse(etag)
}

fn read_bounded(mut response: Response, limit: usize) -> Result<Vec<u8>, ObjectStoreError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ObjectStoreError::new(
            ObjectStoreErrorCode::ResponseTooLarge,
        ));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ObjectStoreError::new(ObjectStoreErrorCode::Network))?;
    if bytes.len() > limit {
        return Err(ObjectStoreError::new(
            ObjectStoreErrorCode::ResponseTooLarge,
        ));
    }
    Ok(bytes)
}

fn ensure_success(status: StatusCode) -> Result<(), ObjectStoreError> {
    if status.is_success() {
        return Ok(());
    }
    let code = match status {
        StatusCode::UNAUTHORIZED => ObjectStoreErrorCode::AuthenticationRejected,
        StatusCode::FORBIDDEN => ObjectStoreErrorCode::AuthorizationRejected,
        StatusCode::NOT_FOUND => ObjectStoreErrorCode::NotFound,
        StatusCode::CONFLICT => ObjectStoreErrorCode::Conflict,
        StatusCode::PRECONDITION_FAILED => ObjectStoreErrorCode::PreconditionFailed,
        StatusCode::TOO_MANY_REQUESTS => ObjectStoreErrorCode::RateLimited,
        status if status.is_server_error() => ObjectStoreErrorCode::ServiceUnavailable,
        _ => ObjectStoreErrorCode::InvalidResponse,
    };
    Err(ObjectStoreError::new(code))
}

fn invalid_response() -> ObjectStoreError {
    ObjectStoreError::new(ObjectStoreErrorCode::InvalidResponse)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectStoreErrorCode {
    InvalidConfiguration,
    KeyOutsidePrefix,
    Network,
    Timeout,
    AuthenticationRejected,
    AuthorizationRejected,
    NotFound,
    Conflict,
    PreconditionFailed,
    RateLimited,
    ServiceUnavailable,
    ObjectTooLarge,
    ResponseTooLarge,
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("R2 object-store operation failed: {code:?}")]
pub(crate) struct ObjectStoreError {
    pub(crate) code: ObjectStoreErrorCode,
}

impl ObjectStoreError {
    pub(crate) const fn new(code: ObjectStoreErrorCode) -> Self {
        Self { code }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::{
        collections::{BTreeMap, VecDeque},
        sync::Mutex,
    };

    use sha2::{Digest, Sha256};

    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum ObjectOperation {
        Head,
        Get,
        Put,
        List,
        Delete,
    }

    #[derive(Clone)]
    struct StoredObject {
        key: R2ObjectKey,
        bytes: Vec<u8>,
        metadata: ObjectMetadata,
    }

    #[derive(Default)]
    struct FakeState {
        objects: BTreeMap<String, StoredObject>,
        failures: VecDeque<(ObjectOperation, ObjectStoreErrorCode)>,
        operations: Vec<ObjectOperation>,
    }

    #[derive(Default)]
    pub(crate) struct FakeObjectStore {
        state: Mutex<FakeState>,
    }

    impl FakeObjectStore {
        pub(crate) fn fail_next(&self, operation: ObjectOperation, code: ObjectStoreErrorCode) {
            self.state
                .lock()
                .expect("fake object store")
                .failures
                .push_back((operation, code));
        }

        pub(crate) fn operations(&self) -> Vec<ObjectOperation> {
            self.state
                .lock()
                .expect("fake object store")
                .operations
                .clone()
        }

        fn begin(
            state: &mut FakeState,
            operation: ObjectOperation,
        ) -> Result<(), ObjectStoreError> {
            state.operations.push(operation);
            if state
                .failures
                .front()
                .is_some_and(|(expected, _)| *expected == operation)
            {
                let (_, code) = state.failures.pop_front().expect("queued failure");
                return Err(ObjectStoreError::new(code));
            }
            Ok(())
        }
    }

    impl ObjectStore for FakeObjectStore {
        fn head(&self, key: &R2ObjectKey) -> Result<Option<ObjectMetadata>, ObjectStoreError> {
            let mut state = self.state.lock().expect("fake object store");
            Self::begin(&mut state, ObjectOperation::Head)?;
            Ok(state
                .objects
                .get(key.as_str())
                .map(|object| object.metadata.clone()))
        }

        fn get(&self, key: &R2ObjectKey) -> Result<GetObjectResult, ObjectStoreError> {
            let mut state = self.state.lock().expect("fake object store");
            Self::begin(&mut state, ObjectOperation::Get)?;
            let object = state
                .objects
                .get(key.as_str())
                .ok_or_else(|| ObjectStoreError::new(ObjectStoreErrorCode::NotFound))?;
            Ok(GetObjectResult {
                metadata: object.metadata.clone(),
                bytes: object.bytes.clone(),
            })
        }

        fn put(&self, request: PutObjectRequest) -> Result<PutObjectOutcome, ObjectStoreError> {
            let mut state = self.state.lock().expect("fake object store");
            Self::begin(&mut state, ObjectOperation::Put)?;
            let current = state.objects.get(request.key.as_str());
            let condition_met = match &request.condition {
                PutCondition::IfAbsent => current.is_none(),
                PutCondition::IfMatch(expected) => {
                    current.is_some_and(|object| object.metadata.version == *expected)
                }
            };
            if !condition_met {
                return Ok(PutObjectOutcome::ConditionNotMet);
            }
            let version = ObjectVersion::parse(format!(
                "\"{}\"",
                ContentSha256::from_bytes(Sha256::digest(&request.bytes).into()).to_hex()
            ))?;
            let byte_length = request.bytes.len() as u64;
            state.objects.insert(
                request.key.as_str().to_owned(),
                StoredObject {
                    key: request.key.clone(),
                    bytes: request.bytes,
                    metadata: ObjectMetadata {
                        byte_length,
                        version,
                        content_type: Some(request.content_type),
                        dara_sha256: request.dara_sha256,
                        object_format_version: Some(OBJECT_FORMAT_VERSION),
                    },
                },
            );
            Ok(PutObjectOutcome::Stored)
        }

        fn list(
            &self,
            prefix: &R2ListPrefix,
            _continuation: Option<&ContinuationToken>,
        ) -> Result<ListObjectsPage, ObjectStoreError> {
            let mut state = self.state.lock().expect("fake object store");
            Self::begin(&mut state, ObjectOperation::List)?;
            let objects = state
                .objects
                .iter()
                .filter(|(key, _)| key.starts_with(prefix.as_str()))
                .map(|(_, object)| ListedObject {
                    key: object.key.clone(),
                    byte_length: object.metadata.byte_length,
                    version: object.metadata.version.clone(),
                })
                .collect();
            Ok(ListObjectsPage {
                objects,
                next: None,
            })
        }

        fn delete(&self, key: &R2ObjectKey) -> Result<(), ObjectStoreError> {
            let mut state = self.state.lock().expect("fake object store");
            Self::begin(&mut state, ObjectOperation::Delete)?;
            state.objects.remove(key.as_str());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fake::{FakeObjectStore, ObjectOperation},
        *,
    };
    use crate::backup::domain::{R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix};

    fn target() -> R2Target {
        R2Target {
            account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef").expect("account ID"),
            jurisdiction: R2Jurisdiction::Default,
            bucket: R2BucketName::parse("dara-test").expect("bucket"),
            prefix: R2Prefix::parse("dara/protocol-test").expect("prefix"),
        }
    }

    #[test]
    fn fake_enforces_conditional_immutable_writes_and_all_operations() {
        let store = FakeObjectStore::default();
        let keyspace = target().keyspace();
        let key = keyspace.identity();
        let request = || PutObjectRequest {
            key: key.clone(),
            bytes: b"payload".to_vec(),
            content_type: ObjectContentType::Binary,
            dara_sha256: None,
            condition: PutCondition::IfAbsent,
        };
        assert_eq!(
            store.put(request()).expect("first put"),
            PutObjectOutcome::Stored
        );
        assert_eq!(
            store.put(request()).expect("second put"),
            PutObjectOutcome::ConditionNotMet
        );
        let head = store.head(&key).expect("head").expect("metadata");
        assert_eq!(head.byte_length, 7);
        assert_eq!(store.get(&key).expect("get").bytes, b"payload");
        assert_eq!(
            store
                .list(&keyspace.root_prefix(), None)
                .expect("list")
                .objects
                .len(),
            1
        );
        store.delete(&key).expect("delete");
        assert!(store.head(&key).expect("head after delete").is_none());
        assert_eq!(
            store.operations(),
            [
                ObjectOperation::Put,
                ObjectOperation::Put,
                ObjectOperation::Head,
                ObjectOperation::Get,
                ObjectOperation::List,
                ObjectOperation::Delete,
                ObjectOperation::Head,
            ]
        );
    }

    #[test]
    fn fake_supports_deterministic_failure_injection() {
        let store = FakeObjectStore::default();
        store.fail_next(ObjectOperation::Head, ObjectStoreErrorCode::RateLimited);
        let error = store
            .head(&target().keyspace().identity())
            .expect_err("injected failure");
        assert_eq!(error.code, ObjectStoreErrorCode::RateLimited);
    }
}
