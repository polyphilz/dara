use std::{
    env,
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Seek, SeekFrom, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, UNIX_EPOCH},
};

use reqwest::{
    blocking::Client,
    header::{CONTENT_RANGE, RANGE},
    StatusCode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::database::{
    embedding_index::{
        self, InstallEmbeddingDisposition, TextEmbeddingIndexManifest,
        EMBEDDING_RECONCILIATION_BATCH_SIZE,
    },
    CardContentListItem, DatabaseClient, DatabaseError, Result as DatabaseResult,
    SearchCardContentInput,
};

const MODEL_OVERRIDE_ENV: &str = "DARA_EMBEDDING_MODEL_PATH";
const SIDECAR_OVERRIDE_ENV: &str = "DARA_LLAMA_SERVER_PATH";
const LLAMA_DEVICE_ENV: &str = "DARA_LLAMA_DEVICE";
const LLAMA_GPU_LAYERS_ENV: &str = "DARA_LLAMA_GPU_LAYERS";
const LIFECYCLE_LOCK_FILE: &str = "semantic-search.lock";
const VERIFICATION_RECEIPT_FILE: &str = "semantic-search-verification.json";
const VERIFICATION_RECEIPT_VERSION: u32 = 1;
const LLAMA_EMBEDDING_NORMALIZATION: &str = "2";
const LLAMA_PARALLEL_SLOTS: &str = "1";
const SIDECAR_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const SIDECAR_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const HEALTH_POLL_DELAY: Duration = Duration::from_millis(100);
const RECONCILIATION_POLL_DELAY: Duration = Duration::from_secs(1);
const EMBEDDING_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const LIFECYCLE_LOCK_WAIT: Duration = Duration::from_secs(30);
const LIFECYCLE_LOCK_POLL_DELAY: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SemanticSearchPhase {
    Downloading,
    Verifying,
    Starting,
    Indexing,
    Ready,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchExecutionMode {
    Browse,
    Lexical,
    Hybrid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchStatus {
    pub phase: SemanticSearchPhase,
    pub downloaded_bytes: u64,
    pub model_bytes: u64,
    pub indexed_documents: i64,
    pub total_documents: i64,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCardContentResult {
    pub items: Vec<CardContentListItem>,
    pub mode: SearchExecutionMode,
    pub semantic_status: SemanticSearchStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("search runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("search runtime HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("search runtime JSON failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("search database operation failed: {0}")]
    Database(#[from] DatabaseError),

    #[error("model artifact is invalid: {0}")]
    InvalidArtifact(String),

    #[error("llama.cpp sidecar is unavailable: {0}")]
    RuntimeUnavailable(String),

    #[error("llama.cpp sidecar failed: {0}")]
    Runtime(String),
}

struct SearchServiceInner {
    database: DatabaseClient,
    manifest: TextEmbeddingIndexManifest,
    model_path: PathBuf,
    model_override: Option<PathBuf>,
    sidecar_path: Option<PathBuf>,
    runtime_settings: LlamaRuntimeSettings,
    data_root: PathBuf,
    http: Client,
    status: Mutex<SemanticSearchStatus>,
    runtime: Mutex<SidecarRuntime>,
    sidecar_startup: Mutex<()>,
    failure_recording: Mutex<()>,
    lifecycle_lock: Mutex<Option<File>>,
    shutdown: AtomicBool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LlamaRuntimeSettings {
    device: Option<String>,
    gpu_layers: String,
    pooling: String,
    embedding_normalization: String,
    parallel_slots: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationFileFingerprint {
    canonical_path: String,
    byte_length: u64,
    modified_at_unix_nanos: u64,
    unix_device: Option<u64>,
    unix_inode: Option<u64>,
    unix_change_time_seconds: Option<i64>,
    unix_change_time_nanoseconds: Option<i64>,
    unix_mode: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationReceipt {
    receipt_version: u32,
    manifest_sha256: String,
    golden_fixtures_sha256: String,
    model: VerificationFileFingerprint,
    sidecar: VerificationFileFingerprint,
    runtime: LlamaRuntimeSettings,
}

#[derive(Clone)]
pub struct SearchService {
    inner: Arc<SearchServiceInner>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl SearchService {
    pub fn start(
        database: DatabaseClient,
        data_root: &Path,
        resource_dir: &Path,
    ) -> Result<Self, SearchError> {
        let lifecycle_lock = acquire_lifecycle_lock(data_root)?;
        let manifest = embedding_index::jina_v1_manifest();
        let model_path = data_root.join("models").join(&manifest.config.model_file);
        let model_override = env::var_os(MODEL_OVERRIDE_ENV).map(PathBuf::from);
        let sidecar_path = resolve_sidecar_path(resource_dir);
        let runtime_settings = LlamaRuntimeSettings {
            device: env::var(LLAMA_DEVICE_ENV).ok(),
            gpu_layers: env::var(LLAMA_GPU_LAYERS_ENV).unwrap_or_else(|_| {
                if cfg!(target_os = "macos") {
                    "all".into()
                } else {
                    "0".into()
                }
            }),
            pooling: manifest.config.pooling.clone(),
            embedding_normalization: LLAMA_EMBEDDING_NORMALIZATION.into(),
            parallel_slots: LLAMA_PARALLEL_SLOTS.into(),
        };
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let inner = Arc::new(SearchServiceInner {
            database,
            manifest: manifest.clone(),
            model_path,
            model_override,
            sidecar_path,
            runtime_settings,
            data_root: data_root.to_owned(),
            http,
            status: Mutex::new(SemanticSearchStatus {
                phase: SemanticSearchPhase::Verifying,
                downloaded_bytes: 0,
                model_bytes: manifest.config.model_file_size,
                indexed_documents: 0,
                total_documents: 0,
                message: Some("Preparing local semantic search".into()),
            }),
            runtime: Mutex::new(SidecarRuntime::default()),
            sidecar_startup: Mutex::new(()),
            failure_recording: Mutex::new(()),
            lifecycle_lock: Mutex::new(Some(lifecycle_lock)),
            shutdown: AtomicBool::new(false),
        });
        let worker_inner = Arc::clone(&inner);
        let worker = thread::Builder::new()
            .name("dara-semantic-search".into())
            .spawn(move || semantic_worker(worker_inner))?;
        Ok(Self {
            inner,
            worker: Arc::new(Mutex::new(Some(worker))),
        })
    }

    pub fn status(&self) -> SemanticSearchStatus {
        lock(&self.inner.status).clone()
    }

    pub fn model_name(&self) -> &str {
        &self.inner.manifest.model_name
    }

    pub fn model_disk_usage_bytes(&self) -> std::io::Result<u64> {
        if let Some(model_override) = self.inner.model_override.as_deref() {
            return file_size_if_present(model_override);
        }
        let partial_path = self.inner.model_path.with_extension("gguf.part");
        Ok(file_size_if_present(&self.inner.model_path)?
            .saturating_add(file_size_if_present(&partial_path)?))
    }

    pub fn search(&self, input: SearchCardContentInput) -> DatabaseResult<SearchCardContentResult> {
        if input.query.trim().is_empty() {
            let items = self.inner.database.search_card_content(input)?;
            return Ok(SearchCardContentResult {
                items,
                mode: SearchExecutionMode::Browse,
                semantic_status: self.status(),
            });
        }

        let semantic_ready = !self.inner.shutdown.load(Ordering::Acquire)
            && self.status().phase == SemanticSearchPhase::Ready;
        if semantic_ready {
            let prompt = format!("{}{}", self.inner.manifest.config.query_prefix, input.query);
            match embed(&self.inner, &prompt) {
                Ok(query_embedding) => {
                    let items = self
                        .inner
                        .database
                        .hybrid_search_card_content(input, query_embedding)?;
                    return Ok(SearchCardContentResult {
                        items,
                        mode: SearchExecutionMode::Hybrid,
                        semantic_status: self.status(),
                    });
                }
                Err(error) => {
                    set_failure(&self.inner, &error);
                    stop_sidecar(&self.inner);
                    if !self.inner.shutdown.load(Ordering::Acquire) {
                        log::error!("semantic query failed; returning lexical results: {error}");
                    }
                }
            }
        }

        let items = self.inner.database.search_card_content(input)?;
        Ok(SearchCardContentResult {
            items,
            mode: SearchExecutionMode::Lexical,
            semantic_status: self.status(),
        })
    }

    pub fn shutdown(&self) {
        {
            let _failure_recording = lock(&self.inner.failure_recording);
            if self.inner.shutdown.swap(true, Ordering::SeqCst) {
                return;
            }
        }
        stop_sidecar(&self.inner);
        drop(lock(&self.inner.sidecar_startup));
        if let Some(worker) = lock(&self.worker).take() {
            if let Err(error) = worker.join() {
                log::error!("semantic-search worker panicked during shutdown: {error:?}");
            }
        }
        release_lifecycle_lock(&self.inner);
    }
}

fn file_size_if_present(path: &Path) -> std::io::Result<u64> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

fn semantic_worker(inner: Arc<SearchServiceInner>) {
    if let Err(error) = prepare_and_verify(&inner) {
        handle_worker_failure(&inner, "initialization", &error);
        return;
    }
    while !inner.shutdown.load(Ordering::Relaxed) {
        if let Err(error) = reconcile_embeddings(&inner) {
            handle_worker_failure(&inner, "embedding reconciliation", &error);
            return;
        }
        reap_idle_sidecar(&inner);
        interruptible_sleep(&inner.shutdown, RECONCILIATION_POLL_DELAY);
    }
}

fn handle_worker_failure(inner: &SearchServiceInner, operation: &str, error: &SearchError) {
    set_failure(inner, error);
    stop_sidecar(inner);
    if inner.shutdown.load(Ordering::Acquire) {
        log::debug!("semantic-search {operation} stopped during shutdown");
    } else {
        log::error!("semantic-search {operation} failed: {error}");
    }
}

fn prepare_and_verify(inner: &SearchServiceInner) -> Result<(), SearchError> {
    let sidecar_path = inner.sidecar_path.as_deref().ok_or_else(|| {
        SearchError::RuntimeUnavailable(format!(
            "llama-server was not found; set {SIDECAR_OVERRIDE_ENV} during development or bundle it with Dara"
        ))
    })?;
    let candidate_artifact_path = inner
        .model_override
        .as_deref()
        .unwrap_or(inner.model_path.as_path());
    if candidate_artifact_path.is_file() {
        match build_verification_receipt(
            &inner.runtime_settings,
            candidate_artifact_path,
            sidecar_path,
        ) {
            Ok(expected) if verification_receipt_matches(&inner.data_root, &expected) => {
                sweep_stale_sidecar(&inner.data_root, sidecar_path, candidate_artifact_path);
                update_status(inner, |status| {
                    status.phase = SemanticSearchPhase::Verifying;
                    status.downloaded_bytes = inner.manifest.config.model_file_size;
                    status.message = Some("Using cached semantic-search verification".into());
                });
                log::info!("semantic-search verification receipt matched");
                return Ok(());
            }
            Ok(_) => {}
            Err(error) => {
                log::warn!("could not inspect semantic-search verification inputs: {error}");
            }
        }
    }

    let artifact_path = if let Some(override_path) = inner.model_override.as_deref() {
        update_status(inner, |status| {
            status.phase = SemanticSearchPhase::Verifying;
            status.message = Some(format!("Verifying {}", override_path.display()));
        });
        verify_artifact(override_path, &inner.manifest)?;
        override_path.to_owned()
    } else {
        prepare_managed_artifact(inner)?
    };
    let receipt_before =
        build_verification_receipt(&inner.runtime_settings, &artifact_path, sidecar_path)
            .inspect_err(|error| {
                log::warn!("could not fingerprint semantic-search verification inputs: {error}");
            })
            .ok();
    update_status(inner, |status| {
        status.phase = SemanticSearchPhase::Starting;
        status.downloaded_bytes = inner.manifest.config.model_file_size;
        status.message = Some("Verifying llama.cpp compatibility".into());
    });
    verify_golden_fixtures(inner, &artifact_path)?;
    let receipt_after =
        build_verification_receipt(&inner.runtime_settings, &artifact_path, sidecar_path)
            .inspect_err(|error| {
                log::warn!("could not fingerprint verified semantic-search inputs: {error}");
            })
            .ok();
    if receipt_before
        .as_ref()
        .zip(receipt_after.as_ref())
        .is_some_and(|(before, after)| before != after)
    {
        return Err(SearchError::InvalidArtifact(
            "the model or llama-server changed during verification".into(),
        ));
    }
    if let Some(receipt) = receipt_after {
        if let Err(error) = write_verification_receipt(&inner.data_root, &receipt) {
            log::warn!("could not cache semantic-search verification: {error}");
        }
    }
    Ok(())
}

fn prepare_managed_artifact(inner: &SearchServiceInner) -> Result<PathBuf, SearchError> {
    if inner.model_path.is_file() && verify_artifact(&inner.model_path, &inner.manifest).is_ok() {
        update_status(inner, |status| {
            status.phase = SemanticSearchPhase::Verifying;
            status.downloaded_bytes = inner.manifest.config.model_file_size;
            status.message = Some("Verified local semantic-search model".into());
        });
        return Ok(inner.model_path.clone());
    }
    let parent = inner
        .model_path
        .parent()
        .ok_or_else(|| SearchError::InvalidArtifact("model path has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let partial_path = inner.model_path.with_extension("gguf.part");
    let mut downloaded = partial_path
        .metadata()
        .map(|value| value.len())
        .unwrap_or(0);
    if downloaded > inner.manifest.config.model_file_size {
        OpenOptions::new()
            .write(true)
            .open(&partial_path)?
            .set_len(0)?;
        downloaded = 0;
    }
    if downloaded == inner.manifest.config.model_file_size {
        if verify_artifact(&partial_path, &inner.manifest).is_ok() {
            fs::rename(&partial_path, &inner.model_path)?;
            sync_directory(parent)?;
            return Ok(inner.model_path.clone());
        }
        OpenOptions::new()
            .write(true)
            .open(&partial_path)?
            .set_len(0)?;
        downloaded = 0;
    }
    update_status(inner, |status| {
        status.phase = SemanticSearchPhase::Downloading;
        status.downloaded_bytes = downloaded;
        status.message = Some("Downloading the semantic-search model".into());
    });

    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        inner.manifest.model_name, inner.manifest.model_revision, inner.manifest.config.model_file
    );
    let mut request = inner.http.get(url);
    if downloaded > 0 {
        request = request.header(RANGE, format!("bytes={downloaded}-"));
    }
    let mut response = request.send()?;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE
        && downloaded == inner.manifest.config.model_file_size
    {
        verify_artifact(&partial_path, &inner.manifest)?;
    } else {
        response = response.error_for_status()?;
        let resumed = response.status() == StatusCode::PARTIAL_CONTENT;
        if downloaded > 0 && !resumed {
            downloaded = 0;
        }
        if resumed {
            validate_content_range(response.headers().get(CONTENT_RANGE), downloaded)?;
        }
        let mut output = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(!resumed)
            .open(&partial_path)?;
        if resumed {
            output.seek(SeekFrom::End(0))?;
        }
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if inner.shutdown.load(Ordering::Relaxed) {
                return Err(SearchError::Runtime(
                    "download canceled during shutdown".into(),
                ));
            }
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            downloaded = downloaded.saturating_add(read as u64);
            update_status(inner, |status| status.downloaded_bytes = downloaded);
        }
        output.sync_all()?;
        verify_artifact(&partial_path, &inner.manifest)?;
    }
    fs::rename(&partial_path, &inner.model_path)?;
    sync_directory(parent)?;
    update_status(inner, |status| {
        status.phase = SemanticSearchPhase::Verifying;
        status.downloaded_bytes = inner.manifest.config.model_file_size;
        status.message = Some("Verifying the semantic-search model".into());
    });
    Ok(inner.model_path.clone())
}

fn validate_content_range(
    header: Option<&reqwest::header::HeaderValue>,
    expected_start: u64,
) -> Result<(), SearchError> {
    let value = header
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            SearchError::InvalidArtifact("resume response omitted Content-Range".into())
        })?;
    let expected_prefix = format!("bytes {expected_start}-");
    if !value.starts_with(&expected_prefix) {
        return Err(SearchError::InvalidArtifact(format!(
            "resume response started at the wrong byte: {value}"
        )));
    }
    Ok(())
}

fn verify_artifact(path: &Path, manifest: &TextEmbeddingIndexManifest) -> Result<(), SearchError> {
    let metadata = path.metadata().map_err(|error| {
        SearchError::InvalidArtifact(format!("cannot read {}: {error}", path.display()))
    })?;
    if metadata.len() != manifest.config.model_file_size {
        return Err(SearchError::InvalidArtifact(format!(
            "{} has {} bytes; expected {}",
            path.display(),
            metadata.len(),
            manifest.config.model_file_size
        )));
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let observed = hex(&digest.finalize());
    if observed != manifest.model_file_sha256 {
        return Err(SearchError::InvalidArtifact(format!(
            "{} has SHA-256 {observed}; expected {}",
            path.display(),
            manifest.model_file_sha256
        )));
    }
    Ok(())
}

fn build_verification_receipt(
    runtime_settings: &LlamaRuntimeSettings,
    model_path: &Path,
    sidecar_path: &Path,
) -> Result<VerificationReceipt, SearchError> {
    Ok(VerificationReceipt {
        receipt_version: VERIFICATION_RECEIPT_VERSION,
        manifest_sha256: sha256_text(embedding_index::JINA_V1_MANIFEST_JSON),
        golden_fixtures_sha256: sha256_text(embedding_index::JINA_V1_GOLDEN_JSON),
        model: verification_file_fingerprint(model_path)?,
        sidecar: verification_file_fingerprint(sidecar_path)?,
        runtime: runtime_settings.clone(),
    })
}

fn verification_file_fingerprint(path: &Path) -> Result<VerificationFileFingerprint, SearchError> {
    let canonical_path = fs::canonicalize(path).map_err(|error| {
        SearchError::InvalidArtifact(format!("cannot resolve {}: {error}", path.display()))
    })?;
    let metadata = canonical_path.metadata().map_err(|error| {
        SearchError::InvalidArtifact(format!(
            "cannot inspect {}: {error}",
            canonical_path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(SearchError::InvalidArtifact(format!(
            "{} is not a file",
            canonical_path.display()
        )));
    }
    let modified_at_unix_nanos = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            SearchError::InvalidArtifact(format!(
                "{} has a modification time before the Unix epoch",
                canonical_path.display()
            ))
        })?
        .as_nanos()
        .try_into()
        .map_err(|_| {
            SearchError::InvalidArtifact(format!(
                "{} has an unsupported modification time",
                canonical_path.display()
            ))
        })?;

    #[cfg(unix)]
    let (
        unix_device,
        unix_inode,
        unix_change_time_seconds,
        unix_change_time_nanoseconds,
        unix_mode,
    ) = {
        use std::os::unix::fs::MetadataExt;
        (
            Some(metadata.dev()),
            Some(metadata.ino()),
            Some(metadata.ctime()),
            Some(metadata.ctime_nsec()),
            Some(metadata.mode()),
        )
    };
    #[cfg(not(unix))]
    let (
        unix_device,
        unix_inode,
        unix_change_time_seconds,
        unix_change_time_nanoseconds,
        unix_mode,
    ) = (None, None, None, None, None);

    Ok(VerificationFileFingerprint {
        canonical_path: canonical_path.to_string_lossy().into_owned(),
        byte_length: metadata.len(),
        modified_at_unix_nanos,
        unix_device,
        unix_inode,
        unix_change_time_seconds,
        unix_change_time_nanoseconds,
        unix_mode,
    })
}

fn verification_receipt_matches(data_root: &Path, expected: &VerificationReceipt) -> bool {
    match read_verification_receipt(data_root) {
        Ok(Some(cached)) => cached == *expected,
        Ok(None) => false,
        Err(error) => {
            log::warn!("ignoring invalid semantic-search verification receipt: {error}");
            false
        }
    }
}

fn read_verification_receipt(data_root: &Path) -> Result<Option<VerificationReceipt>, SearchError> {
    let path = verification_receipt_path(data_root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(Into::into)
}

fn write_verification_receipt(
    data_root: &Path,
    receipt: &VerificationReceipt,
) -> Result<(), SearchError> {
    fs::create_dir_all(data_root)?;
    let path = verification_receipt_path(data_root);
    let temporary = path.with_extension("json.tmp");
    let mut bytes = serde_json::to_vec_pretty(receipt)?;
    bytes.push(b'\n');
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temporary)?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    drop(output);
    fs::rename(&temporary, &path)?;
    sync_directory(data_root)?;
    Ok(())
}

fn invalidate_verification_receipt(data_root: &Path) {
    let path = verification_receipt_path(data_root);
    if let Err(error) = fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "could not invalidate semantic-search verification receipt {}: {error}",
                path.display()
            );
        }
    }
}

fn verification_receipt_path(data_root: &Path) -> PathBuf {
    data_root.join(VERIFICATION_RECEIPT_FILE)
}

fn sha256_text(value: &str) -> String {
    hex(&Sha256::digest(value.as_bytes()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenFixtureFile {
    fixture_version: u32,
    model_file_sha256: String,
    generated_with: GoldenRuntime,
    tolerance: GoldenTolerance,
    cases: Vec<GoldenCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenRuntime {
    runtime: String,
    revision: String,
    build: u32,
    device: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenTolerance {
    minimum_cosine_similarity: f64,
    maximum_absolute_difference: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GoldenCase {
    name: String,
    input: String,
    embedding: Vec<f32>,
}

fn verify_golden_fixtures(
    inner: &SearchServiceInner,
    model_path: &Path,
) -> Result<(), SearchError> {
    let fixtures: GoldenFixtureFile = serde_json::from_str(embedding_index::JINA_V1_GOLDEN_JSON)?;
    if fixtures.fixture_version != 1
        || fixtures.model_file_sha256 != inner.manifest.model_file_sha256
    {
        return Err(SearchError::InvalidArtifact(
            "golden fixtures do not match the active model manifest".into(),
        ));
    }
    log::info!(
        "verifying {} golden fixtures generated by {} {} build {} on {}",
        fixtures.cases.len(),
        fixtures.generated_with.runtime,
        fixtures.generated_with.revision,
        fixtures.generated_with.build,
        fixtures.generated_with.device,
    );
    for case in fixtures.cases {
        let observed = embed_with_model(inner, model_path, &case.input)?;
        embedding_index::validate_embedding(&observed, inner.manifest.dimension as usize)?;
        embedding_index::validate_embedding(&case.embedding, inner.manifest.dimension as usize)?;
        let cosine = case
            .embedding
            .iter()
            .zip(&observed)
            .map(|(expected, actual)| f64::from(*expected) * f64::from(*actual))
            .sum::<f64>();
        let maximum_difference = case
            .embedding
            .iter()
            .zip(&observed)
            .map(|(expected, actual)| (f64::from(*expected) - f64::from(*actual)).abs())
            .fold(0.0_f64, f64::max);
        if cosine < fixtures.tolerance.minimum_cosine_similarity
            || maximum_difference > fixtures.tolerance.maximum_absolute_difference
        {
            return Err(SearchError::Runtime(format!(
                "golden fixture {} failed (cosine {cosine}, max difference {maximum_difference})",
                case.name
            )));
        }
    }
    Ok(())
}

fn reconcile_embeddings(inner: &SearchServiceInner) -> Result<(), SearchError> {
    let progress = inner.database.load_embedding_index_progress()?;
    update_index_progress(inner, progress);
    let documents = inner
        .database
        .load_embedding_reconciliation_batch(EMBEDDING_RECONCILIATION_BATCH_SIZE)?;
    if documents.is_empty() {
        inner.database.activate_embedding_index_if_complete()?;
        let progress = inner.database.load_embedding_index_progress()?;
        update_index_progress(inner, progress);
        return Ok(());
    }
    if !progress.active {
        update_status(inner, |status| {
            status.phase = SemanticSearchPhase::Indexing;
            status.message = Some("Building the local semantic index".into());
        });
    }
    for document in documents {
        if inner.shutdown.load(Ordering::Relaxed) {
            break;
        }
        let prompt = format!("{}{}", inner.manifest.config.document_prefix, document.body);
        let embedding = embed(inner, &prompt)?;
        if inner.database.install_text_embedding(document, embedding)?
            == InstallEmbeddingDisposition::Installed
        {
            let progress = inner.database.load_embedding_index_progress()?;
            update_index_progress(inner, progress);
        }
    }
    Ok(())
}

fn update_index_progress(
    inner: &SearchServiceInner,
    progress: embedding_index::EmbeddingIndexProgress,
) {
    update_status(inner, |status| {
        status.indexed_documents = progress.current_documents;
        status.total_documents = progress.total_documents;
        if progress.active {
            status.phase = SemanticSearchPhase::Ready;
            status.message = (progress.current_documents < progress.total_documents)
                .then(|| "Refreshing edited cards in the semantic index".into());
        } else {
            status.phase = SemanticSearchPhase::Indexing;
            status.message = Some("Building the local semantic index".into());
        }
    });
}

#[derive(Default)]
struct SidecarRuntime {
    child: Option<Child>,
    endpoint: Option<String>,
    model_path: Option<PathBuf>,
    last_used: Option<Instant>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

fn embed(inner: &SearchServiceInner, input: &str) -> Result<Vec<f32>, SearchError> {
    let model_path = inner
        .model_override
        .as_deref()
        .unwrap_or(inner.model_path.as_path());
    embed_with_model(inner, model_path, input)
}

fn embed_with_model(
    inner: &SearchServiceInner,
    model_path: &Path,
    input: &str,
) -> Result<Vec<f32>, SearchError> {
    let endpoint = ensure_sidecar(inner, model_path)?;
    let response = inner
        .http
        .post(format!("{endpoint}/v1/embeddings"))
        .timeout(EMBEDDING_REQUEST_TIMEOUT)
        .json(&serde_json::json!({ "input": input }))
        .send()?
        .error_for_status()?
        .json::<EmbeddingResponse>()?;
    let embedding = response
        .data
        .into_iter()
        .next()
        .ok_or_else(|| SearchError::Runtime("embedding response had no vectors".into()))?
        .embedding;
    embedding_index::validate_embedding(&embedding, inner.manifest.dimension as usize)?;
    let mut runtime = lock(&inner.runtime);
    runtime.last_used = Some(Instant::now());
    Ok(embedding)
}

fn ensure_sidecar(inner: &SearchServiceInner, model_path: &Path) -> Result<String, SearchError> {
    let _startup = lock(&inner.sidecar_startup);
    if inner.shutdown.load(Ordering::Acquire) {
        return Err(SearchError::Runtime("sidecar start canceled".into()));
    }
    let mut runtime = lock(&inner.runtime);
    if let Some(child) = runtime.child.as_mut() {
        if child.try_wait()?.is_none() && runtime.model_path.as_deref() == Some(model_path) {
            runtime.last_used = Some(Instant::now());
            return runtime
                .endpoint
                .clone()
                .ok_or_else(|| SearchError::Runtime("sidecar endpoint was lost".into()));
        }
        stop_runtime(&mut runtime, &inner.data_root);
    }

    let sidecar_path = inner.sidecar_path.as_deref().ok_or_else(|| {
        SearchError::RuntimeUnavailable("no llama-server executable was found".into())
    })?;
    sweep_stale_sidecar(&inner.data_root, sidecar_path, model_path);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let log_dir = inner.data_root.join("logs");
    fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join("llama-server.log");
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = stdout.try_clone()?;
    let mut command = Command::new(sidecar_path);
    command
        .arg("--model")
        .arg(model_path)
        .arg("--embedding")
        .arg("--pooling")
        .arg(&inner.runtime_settings.pooling)
        .arg("--embd-normalize")
        .arg(&inner.runtime_settings.embedding_normalization)
        .arg("--n-gpu-layers")
        .arg(&inner.runtime_settings.gpu_layers)
        .arg("--parallel")
        .arg(&inner.runtime_settings.parallel_slots)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some(device) = inner.runtime_settings.device.as_deref() {
        command.arg("--device").arg(device);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    if let Err(error) = write_pidfile(&inner.data_root, pid, sidecar_path, model_path) {
        terminate_process_group(&mut child);
        return Err(error);
    }
    let endpoint = format!("http://127.0.0.1:{port}");
    runtime.child = Some(child);
    runtime.endpoint = Some(endpoint.clone());
    runtime.model_path = Some(model_path.to_owned());
    runtime.last_used = Some(Instant::now());
    drop(runtime);

    let startup_deadline = Instant::now() + SIDECAR_STARTUP_TIMEOUT;
    while Instant::now() < startup_deadline {
        if inner.shutdown.load(Ordering::Relaxed) {
            stop_sidecar(inner);
            return Err(SearchError::Runtime("sidecar start canceled".into()));
        }
        {
            let mut runtime = lock(&inner.runtime);
            let status = runtime
                .child
                .as_mut()
                .map(Child::try_wait)
                .transpose()?
                .flatten();
            if let Some(status) = status {
                stop_runtime(&mut runtime, &inner.data_root);
                return Err(SearchError::Runtime(format!(
                    "llama-server exited during startup with {status}; see {}",
                    log_path.display()
                )));
            }
        }
        if inner
            .http
            .get(format!("{endpoint}/health"))
            .timeout(Duration::from_secs(1))
            .send()
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(endpoint);
        }
        thread::sleep(HEALTH_POLL_DELAY);
    }
    stop_sidecar(inner);
    Err(SearchError::Runtime(format!(
        "llama-server did not become healthy; see {}",
        log_path.display()
    )))
}

fn reap_idle_sidecar(inner: &SearchServiceInner) {
    let mut runtime = lock(&inner.runtime);
    if runtime
        .last_used
        .is_some_and(|last_used| last_used.elapsed() >= SIDECAR_IDLE_TIMEOUT)
    {
        stop_runtime(&mut runtime, &inner.data_root);
    }
}

fn stop_sidecar(inner: &SearchServiceInner) {
    let mut runtime = lock(&inner.runtime);
    stop_runtime(&mut runtime, &inner.data_root);
}

fn stop_runtime(runtime: &mut SidecarRuntime, data_root: &Path) {
    if let Some(mut child) = runtime.child.take() {
        let pid = child.id();
        terminate_process_group(&mut child);
        remove_pidfile_if_owned(data_root, pid);
    }
    runtime.endpoint = None;
    runtime.model_path = None;
    runtime.last_used = None;
}

#[cfg(unix)]
fn terminate_process_group(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let process_group = -(child.id() as i32);
    // SAFETY: kill receives a process-group ID created by process_group(0).
    unsafe {
        libc::kill(process_group, libc::SIGTERM);
    }
    for _ in 0..20 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: the child still belongs to the process group created above.
    unsafe {
        libc::kill(process_group, libc::SIGKILL);
    }
    let _ = child.wait();
}

#[cfg(not(unix))]
fn terminate_process_group(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn sweep_stale_sidecar(data_root: &Path, sidecar_path: &Path, model_path: &Path) {
    let Ok(contents) = fs::read_to_string(pidfile_path(data_root)) else {
        return;
    };
    let mut lines = contents.lines();
    let Some(pid) = lines.next().and_then(|value| value.parse::<u32>().ok()) else {
        return;
    };
    let recorded_sidecar = lines.next().unwrap_or_default();
    let recorded_model = lines.next().unwrap_or_default();
    if recorded_sidecar != sidecar_path.to_string_lossy()
        || recorded_model != model_path.to_string_lossy()
    {
        return;
    }
    if !process_matches_sidecar(pid, recorded_sidecar, recorded_model) {
        return;
    }
    terminate_stale_process_group(pid, recorded_sidecar, recorded_model);
    remove_pidfile_if_owned(data_root, pid);
}

fn process_matches_sidecar(pid: u32, sidecar_path: &str, model_path: &str) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
    else {
        return false;
    };
    let command = String::from_utf8_lossy(&output.stdout);
    output.status.success()
        && command.contains(sidecar_path)
        && command.contains(model_path)
        && command.contains("--embedding")
}

#[cfg(unix)]
fn terminate_stale_process_group(pid: u32, sidecar_path: &str, model_path: &str) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };
    // SAFETY: the caller verified that this process group is Dara's sidecar.
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    for _ in 0..20 {
        if !process_exists(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if !process_matches_sidecar(pid as u32, sidecar_path, model_path) {
        return;
    }
    // SAFETY: the verified sidecar did not exit after SIGTERM.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    for _ in 0..20 {
        if !process_exists(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn process_exists(pid: i32) -> bool {
    // SAFETY: signal 0 checks for a process without sending a signal.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn terminate_stale_process_group(_pid: u32, _sidecar_path: &str, _model_path: &str) {}

fn write_pidfile(
    data_root: &Path,
    pid: u32,
    sidecar_path: &Path,
    model_path: &Path,
) -> Result<(), SearchError> {
    let path = pidfile_path(data_root);
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        format!(
            "{pid}\n{}\n{}\n",
            sidecar_path.display(),
            model_path.display()
        ),
    )?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn pidfile_path(data_root: &Path) -> PathBuf {
    data_root.join("llama-server.pid")
}

fn remove_pidfile_if_owned(data_root: &Path, expected_pid: u32) {
    let path = pidfile_path(data_root);
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    if contents
        .lines()
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        == Some(expected_pid)
    {
        let _ = fs::remove_file(path);
    }
}

fn resolve_sidecar_path(resource_dir: &Path) -> Option<PathBuf> {
    if let Some(path) = env::var_os(SIDECAR_OVERRIDE_ENV).map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let mut candidates = vec![
        resource_dir.join("bin/llama-server"),
        resource_dir.join("llama-server"),
    ];
    if let Some(sibling) = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("llama-server")))
    {
        candidates.push(sibling);
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or_else(|| {
            Command::new("which")
                .arg("llama-server")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| {
                    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    (!value.is_empty()).then(|| PathBuf::from(value))
                })
        })
}

fn set_failure(inner: &SearchServiceInner, error: &SearchError) {
    let _failure_recording = lock(&inner.failure_recording);
    match failure_disposition(error, inner.shutdown.load(Ordering::Acquire)) {
        FailureDisposition::Ignore => return,
        FailureDisposition::Record => {}
        FailureDisposition::RecordAndInvalidateVerification => {
            invalidate_verification_receipt(&inner.data_root);
        }
    }
    update_status(inner, |status| {
        status.phase = match error {
            SearchError::RuntimeUnavailable(_) => SemanticSearchPhase::Unavailable,
            _ => SemanticSearchPhase::Failed,
        };
        status.message = Some(error.to_string());
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureDisposition {
    Ignore,
    Record,
    RecordAndInvalidateVerification,
}

fn failure_disposition(error: &SearchError, shutting_down: bool) -> FailureDisposition {
    if shutting_down {
        FailureDisposition::Ignore
    } else if matches!(error, SearchError::InvalidArtifact(_)) {
        // Runtime failures can be transient. Model, sidecar, and runtime-setting changes are
        // already detected by the receipt fingerprint on the next launch.
        FailureDisposition::RecordAndInvalidateVerification
    } else {
        FailureDisposition::Record
    }
}

fn acquire_lifecycle_lock(data_root: &Path) -> Result<File, SearchError> {
    fs::create_dir_all(data_root)?;
    let path = data_root.join(LIFECYCLE_LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    let deadline = Instant::now() + LIFECYCLE_LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(LIFECYCLE_LOCK_POLL_DELAY);
            }
            Err(TryLockError::WouldBlock) => {
                return Err(SearchError::RuntimeUnavailable(format!(
                    "another Dara instance is still stopping semantic search; timed out waiting for {}",
                    path.display()
                )));
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

fn release_lifecycle_lock(inner: &SearchServiceInner) {
    if let Some(file) = lock(&inner.lifecycle_lock).take() {
        if let Err(error) = file.unlock() {
            log::warn!("could not release semantic-search lifecycle lock: {error}");
        }
    }
}

fn update_status(inner: &SearchServiceInner, update: impl FnOnce(&mut SemanticSearchStatus)) {
    update(&mut lock(&inner.status));
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn interruptible_sleep(shutdown: &AtomicBool, duration: Duration) {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::Relaxed) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
}

fn sync_directory(path: &Path) -> Result<(), SearchError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_runtime_settings() -> LlamaRuntimeSettings {
        LlamaRuntimeSettings {
            device: Some("test-device".into()),
            gpu_layers: "all".into(),
            pooling: "last".into(),
            embedding_normalization: LLAMA_EMBEDDING_NORMALIZATION.into(),
            parallel_slots: LLAMA_PARALLEL_SLOTS.into(),
        }
    }

    #[test]
    fn content_range_must_resume_at_requested_offset() {
        let valid = reqwest::header::HeaderValue::from_static("bytes 100-199/200");
        validate_content_range(Some(&valid), 100).expect("valid range");
        let invalid = reqwest::header::HeaderValue::from_static("bytes 0-199/200");
        assert!(matches!(
            validate_content_range(Some(&invalid), 100),
            Err(SearchError::InvalidArtifact(_))
        ));
    }

    #[test]
    fn verification_receipt_round_trips_atomically_and_tolerates_corruption() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let model_path = directory.path().join("model.gguf");
        let sidecar_path = directory.path().join("llama-server");
        fs::write(&model_path, b"model").expect("model fixture");
        fs::write(&sidecar_path, b"sidecar").expect("sidecar fixture");
        let receipt =
            build_verification_receipt(&test_runtime_settings(), &model_path, &sidecar_path)
                .expect("verification receipt");

        assert!(!verification_receipt_matches(directory.path(), &receipt));
        write_verification_receipt(directory.path(), &receipt).expect("receipt write");
        assert!(verification_receipt_matches(directory.path(), &receipt));
        assert_eq!(
            read_verification_receipt(directory.path()).expect("receipt read"),
            Some(receipt.clone())
        );
        assert!(!verification_receipt_path(directory.path())
            .with_extension("json.tmp")
            .exists());

        fs::write(verification_receipt_path(directory.path()), b"{broken")
            .expect("corrupt receipt");
        assert!(!verification_receipt_matches(directory.path(), &receipt));
        write_verification_receipt(directory.path(), &receipt).expect("receipt replacement");
        assert!(verification_receipt_matches(directory.path(), &receipt));

        invalidate_verification_receipt(directory.path());
        assert_eq!(
            read_verification_receipt(directory.path()).expect("receipt absence"),
            None
        );
    }

    #[test]
    fn verification_receipt_invalidates_for_artifact_and_contract_changes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let model_path = directory.path().join("model.gguf");
        let sidecar_path = directory.path().join("llama-server");
        fs::write(&model_path, b"model").expect("model fixture");
        fs::write(&sidecar_path, b"sidecar").expect("sidecar fixture");
        let settings = test_runtime_settings();
        let original = build_verification_receipt(&settings, &model_path, &sidecar_path)
            .expect("verification receipt");
        write_verification_receipt(directory.path(), &original).expect("receipt write");

        fs::write(&model_path, b"changed model").expect("changed model");
        let changed_model = build_verification_receipt(&settings, &model_path, &sidecar_path)
            .expect("changed-model receipt");
        assert!(!verification_receipt_matches(
            directory.path(),
            &changed_model
        ));
        write_verification_receipt(directory.path(), &changed_model)
            .expect("changed-model receipt write");

        fs::write(&sidecar_path, b"changed sidecar").expect("changed sidecar");
        let changed_sidecar = build_verification_receipt(&settings, &model_path, &sidecar_path)
            .expect("changed-sidecar receipt");
        assert!(!verification_receipt_matches(
            directory.path(),
            &changed_sidecar
        ));
        write_verification_receipt(directory.path(), &changed_sidecar)
            .expect("changed-sidecar receipt write");

        let mut changed_runtime = changed_sidecar.clone();
        changed_runtime.runtime.gpu_layers = "0".into();
        assert!(!verification_receipt_matches(
            directory.path(),
            &changed_runtime
        ));

        let mut changed_receipt_version = changed_sidecar.clone();
        changed_receipt_version.receipt_version += 1;
        assert!(!verification_receipt_matches(
            directory.path(),
            &changed_receipt_version
        ));

        let mut changed_manifest = changed_sidecar.clone();
        changed_manifest.manifest_sha256 = "changed".into();
        assert!(!verification_receipt_matches(
            directory.path(),
            &changed_manifest
        ));

        let mut changed_golden_fixtures = changed_sidecar.clone();
        changed_golden_fixtures.golden_fixtures_sha256 = "changed".into();
        assert!(!verification_receipt_matches(
            directory.path(),
            &changed_golden_fixtures
        ));
    }

    #[test]
    fn failure_disposition_only_invalidates_receipts_for_invalid_artifacts() {
        assert_eq!(
            failure_disposition(&SearchError::Runtime("sidecar stopped".into()), false),
            FailureDisposition::Record
        );
        assert_eq!(
            failure_disposition(&SearchError::InvalidArtifact("model changed".into()), false),
            FailureDisposition::RecordAndInvalidateVerification
        );
        assert_eq!(
            failure_disposition(
                &SearchError::Database(DatabaseError::InvalidInput("query".into())),
                false
            ),
            FailureDisposition::Record
        );
        assert_eq!(
            failure_disposition(&SearchError::Runtime("sidecar stopped".into()), true),
            FailureDisposition::Ignore
        );
    }

    #[test]
    fn pidfile_is_only_removed_by_its_owner() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let sidecar_path = directory.path().join("llama-server");
        let model_path = directory.path().join("model.gguf");
        write_pidfile(directory.path(), 101, &sidecar_path, &model_path).expect("pidfile write");

        remove_pidfile_if_owned(directory.path(), 202);
        assert!(pidfile_path(directory.path()).exists());

        remove_pidfile_if_owned(directory.path(), 101);
        assert!(!pidfile_path(directory.path()).exists());
    }

    #[test]
    fn lifecycle_lock_excludes_overlapping_search_services() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let first = acquire_lifecycle_lock(directory.path()).expect("first lifecycle lock");
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.path().join(LIFECYCLE_LOCK_FILE))
            .expect("second lifecycle handle");

        assert!(matches!(second.try_lock(), Err(TryLockError::WouldBlock)));
        first.unlock().expect("first lifecycle unlock");
        second.try_lock().expect("second lifecycle lock");
        second.unlock().expect("second lifecycle unlock");
    }
}
