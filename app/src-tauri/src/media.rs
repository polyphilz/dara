use std::{
    io::Cursor,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use image::{imageops::FilterType, GenericImageView, ImageDecoder, ImageReader, Limits};
use tauri::{http, ipc::InvokeBody, ipc::Request, AppHandle, Manager, State};

use crate::database::{
    commands::CommandError, CanonicalImage, Database, DatabaseClient, DatabaseError,
    ImageOcrStatus, ImageRecord, OcrJob,
};

const MAX_SOURCE_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_DECODED_IMAGE_DIMENSION: u32 = 32_768;
const MAX_CANONICAL_IMAGE_EDGE: u32 = 1_600;
const WEBP_QUALITY: f32 = 80.0;
const MEDIA_SCHEME_PATH_PREFIX: &str = "/image/";
const LEASE_ID_PREFIX_BYTES: usize = 36;
const OCR_RECONCILIATION_INTERVAL: Duration = Duration::from_secs(5 * 60);
const OCR_STALE_ATTEMPT_AGE: Duration = Duration::from_secs(2 * 60);
const MEDIA_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug)]
enum OcrWorkerSignal {
    WorkAvailable,
}

pub struct OcrCoordinator {
    sender: mpsc::SyncSender<OcrWorkerSignal>,
    last_media_maintenance: Arc<Mutex<Option<crate::database::MediaMaintenanceReport>>>,
}

impl OcrCoordinator {
    pub fn start(client: DatabaseClient) -> Result<Self, DatabaseError> {
        let now = crate::database::now_millis()?;
        let report = client.maintain_media(now, crate::database::MEDIA_ORPHAN_GRACE_MILLIS)?;
        log_media_maintenance("launch", &report);
        let last_media_maintenance = Arc::new(Mutex::new(Some(report)));
        let recovery = client.recover_interrupted_ocr_jobs(i64::MAX, now)?;
        log_ocr_recovery("launch", recovery);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_client = client.clone();
        let worker_last_media_maintenance = Arc::clone(&last_media_maintenance);
        thread::Builder::new()
            .name("dara-image-ocr".into())
            .spawn(move || ocr_worker(worker_client, receiver, worker_last_media_maintenance))?;
        let coordinator = Self {
            sender,
            last_media_maintenance,
        };
        coordinator.wake();
        Ok(coordinator)
    }

    fn wake(&self) {
        match self.sender.try_send(OcrWorkerSignal::WorkAvailable) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => {
                log::error!("image OCR worker is unavailable");
            }
        }
    }

    pub fn last_media_maintenance(&self) -> Option<crate::database::MediaMaintenanceReport> {
        self.last_media_maintenance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn record_media_maintenance(&self, report: crate::database::MediaMaintenanceReport) {
        *self
            .last_media_maintenance
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report);
    }
}

#[tauri::command]
pub async fn ingest_clipboard_image(
    app: AppHandle,
    database: State<'_, Database>,
    ocr: State<'_, OcrCoordinator>,
    offsite_media: State<'_, crate::backup::media_reconciliation::MediaBackupCoordinator>,
    lease_id: String,
) -> Result<ImageRecord, CommandError> {
    let raw = read_clipboard_image(&app).await?;
    ingest_image(raw, lease_id, &database, &ocr, &offsite_media).await
}

#[tauri::command]
pub async fn ingest_image_bytes(
    request: Request<'_>,
    database: State<'_, Database>,
    ocr: State<'_, OcrCoordinator>,
    offsite_media: State<'_, crate::backup::media_reconciliation::MediaBackupCoordinator>,
) -> Result<ImageRecord, CommandError> {
    let payload = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(_) => {
            return Err(CommandError::from(DatabaseError::InvalidInput(
                "image upload requires a binary payload".into(),
            )))
        }
    };
    if payload.len() <= LEASE_ID_PREFIX_BYTES {
        return Err(CommandError::from(DatabaseError::InvalidInput(
            "the selected image is empty".into(),
        )));
    }
    let lease_id = std::str::from_utf8(&payload[..LEASE_ID_PREFIX_BYTES])
        .map_err(|_| {
            CommandError::from(DatabaseError::InvalidInput(
                "image upload has an invalid media lease".into(),
            ))
        })?
        .to_owned();
    let raw = payload[LEASE_ID_PREFIX_BYTES..].to_vec();
    if raw.len() > MAX_SOURCE_IMAGE_BYTES {
        return Err(CommandError::from(DatabaseError::InvalidInput(
            "the selected image is larger than 64 MiB".into(),
        )));
    }
    ingest_image(raw, lease_id, &database, &ocr, &offsite_media).await
}

async fn ingest_image(
    raw: Vec<u8>,
    lease_id: String,
    database: &Database,
    ocr: &OcrCoordinator,
    offsite_media: &crate::backup::media_reconciliation::MediaBackupCoordinator,
) -> Result<ImageRecord, CommandError> {
    let client = database.client();
    let image = tauri::async_runtime::spawn_blocking(move || canonicalize_image(&raw))
        .await
        .map_err(|error| {
            CommandError::from(DatabaseError::InvalidInput(format!(
                "image processing worker failed: {error}"
            )))
        })??;
    let record = tauri::async_runtime::spawn_blocking(move || client.ingest_image(image, lease_id))
        .await
        .map_err(|error| {
            CommandError::from(DatabaseError::WriterUnavailable).with_context(error.to_string())
        })??;
    let ocr_status: ImageOcrStatus = record.ocr_status;
    if ocr_status.needs_worker_wake() {
        ocr.wake();
    }
    offsite_media.wake();
    Ok(record)
}

pub fn protocol_response(
    app: &AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    if request.method() != http::Method::GET {
        return empty_response(http::StatusCode::METHOD_NOT_ALLOWED);
    }
    if request.uri().host() != Some("localhost") {
        return empty_response(http::StatusCode::BAD_REQUEST);
    }
    let Some(image_id) = request.uri().path().strip_prefix(MEDIA_SCHEME_PATH_PREFIX) else {
        return empty_response(http::StatusCode::BAD_REQUEST);
    };
    if image_id.is_empty() || image_id.contains('/') {
        return empty_response(http::StatusCode::BAD_REQUEST);
    }
    let Some(database) = app.try_state::<Database>() else {
        return empty_response(http::StatusCode::SERVICE_UNAVAILABLE);
    };
    match database.client().load_media_payload(image_id.into()) {
        Ok(payload) => http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::CONTENT_TYPE, payload.mime_type)
            .header(
                http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            )
            .header(http::header::ETAG, format!("\"{}\"", hex(&payload.sha256)))
            .header("X-Content-Type-Options", "nosniff")
            .body(payload.bytes)
            .expect("valid media response"),
        Err(DatabaseError::NotFound { .. }) | Err(DatabaseError::InvalidInput(_)) => {
            empty_response(http::StatusCode::NOT_FOUND)
        }
        Err(error) => {
            log::error!("local media request failed: {error}");
            empty_response(http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn read_clipboard_image(app: &AppHandle) -> Result<Vec<u8>, CommandError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = sender.send(read_clipboard_image_on_main_thread());
    })
    .map_err(|error| {
        CommandError::from(DatabaseError::InvalidInput(format!(
            "could not access the clipboard: {error}"
        )))
    })?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv())
        .await
        .map_err(|error| {
            CommandError::from(DatabaseError::InvalidInput(format!(
                "clipboard worker failed: {error}"
            )))
        })?
        .map_err(|_| CommandError::from(DatabaseError::WriterUnavailable))?
        .map_err(CommandError::from)
}

#[cfg(target_os = "macos")]
fn read_clipboard_image_on_main_thread() -> Result<Vec<u8>, DatabaseError> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};

    MainThreadMarker::new().ok_or_else(|| {
        DatabaseError::InvalidInput("the clipboard was not read on the macOS main thread".into())
    })?;
    let pasteboard = NSPasteboard::generalPasteboard();
    for pasteboard_type in [unsafe { NSPasteboardTypePNG }, unsafe {
        NSPasteboardTypeTIFF
    }] {
        if let Some(data) = pasteboard.dataForType(pasteboard_type) {
            let bytes = data.to_vec();
            if bytes.len() > MAX_SOURCE_IMAGE_BYTES {
                return Err(DatabaseError::InvalidInput(
                    "the pasted image is larger than 64 MiB".into(),
                ));
            }
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
    }
    Err(DatabaseError::InvalidInput(
        "the clipboard does not contain a supported image".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
fn read_clipboard_image_on_main_thread() -> Result<Vec<u8>, DatabaseError> {
    Err(DatabaseError::InvalidInput(
        "clipboard image ingestion is available only on macOS".into(),
    ))
}

fn canonicalize_image(bytes: &[u8]) -> Result<CanonicalImage, CommandError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| invalid_image(error.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODED_IMAGE_DIMENSION);
    reader.limits(limits);
    let format = reader
        .format()
        .ok_or_else(|| invalid_image("the image format is unsupported".into()))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| invalid_image(error.to_string()))?;
    let orientation = decoder
        .orientation()
        .map_err(|error| invalid_image(error.to_string()))?;
    drop(decoder);

    let mut decode_reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODED_IMAGE_DIMENSION);
    decode_reader.limits(limits);
    let mut decoded = decode_reader
        .decode()
        .map_err(|error| invalid_image(error.to_string()))?;
    decoded.apply_orientation(orientation);
    let (source_width, source_height) = decoded.dimensions();
    let resized = if source_width.max(source_height) > MAX_CANONICAL_IMAGE_EDGE {
        decoded.resize(
            MAX_CANONICAL_IMAGE_EDGE,
            MAX_CANONICAL_IMAGE_EDGE,
            FilterType::Lanczos3,
        )
    } else {
        decoded
    };
    let rgba = resized.to_rgba8();
    let (natural_width, natural_height) = rgba.dimensions();
    let encoded = webp::Encoder::from_rgba(rgba.as_raw(), natural_width, natural_height)
        .encode_simple(false, WEBP_QUALITY)
        .map_err(|error| invalid_image(format!("WebP encoding failed: {error:?}")))?;
    Ok(CanonicalImage {
        bytes: encoded.to_vec(),
        natural_width,
        natural_height,
    })
}

fn invalid_image(reason: String) -> CommandError {
    CommandError::from(DatabaseError::InvalidInput(format!(
        "could not process the image: {reason}"
    )))
}

fn ocr_worker(
    client: DatabaseClient,
    receiver: mpsc::Receiver<OcrWorkerSignal>,
    last_media_maintenance: Arc<Mutex<Option<crate::database::MediaMaintenanceReport>>>,
) {
    let mut next_reconciliation = Instant::now() + OCR_RECONCILIATION_INTERVAL;
    let mut next_media_maintenance = Instant::now() + MEDIA_MAINTENANCE_INTERVAL;
    loop {
        let next_wake = next_reconciliation.min(next_media_maintenance);
        let wait = next_wake.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(wait) {
            Ok(OcrWorkerSignal::WorkAvailable) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if Instant::now() >= next_reconciliation {
            reconcile_ocr_queue(&client);
            next_reconciliation = Instant::now() + OCR_RECONCILIATION_INTERVAL;
        }
        if Instant::now() >= next_media_maintenance {
            reconcile_media(&client, &last_media_maintenance);
            next_media_maintenance = Instant::now() + MEDIA_MAINTENANCE_INTERVAL;
        }
        drain_ocr_queue(&client, recognize_text);
    }
}

fn reconcile_media(
    client: &DatabaseClient,
    last_media_maintenance: &Mutex<Option<crate::database::MediaMaintenanceReport>>,
) {
    let result = crate::database::now_millis()
        .and_then(|now| client.maintain_media(now, crate::database::MEDIA_ORPHAN_GRACE_MILLIS));
    match result {
        Ok(report) => {
            log_media_maintenance("periodic reconciliation", &report);
            *last_media_maintenance
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report);
        }
        Err(error) => log::error!("could not reconcile orphaned media: {error}"),
    }
}

fn log_media_maintenance(context: &str, report: &crate::database::MediaMaintenanceReport) {
    let integrity = &report.integrity;
    let cleanup = report.cleanup;
    if !integrity.orphaned_image_ids.is_empty()
        || !integrity.extra_blob_sha256.is_empty()
        || !integrity.missing_referenced_blob_image_ids.is_empty()
        || cleanup.retired_image_count > 0
        || cleanup.deleted_blob_count > 0
    {
        log::info!(
            "media {context}: {} orphaned image row(s), {} extra blob(s), {} missing referenced blob(s); retired {}, deleted {} and reclaimed {} bytes",
            integrity.orphaned_image_ids.len(),
            integrity.extra_blob_sha256.len(),
            integrity.missing_referenced_blob_image_ids.len(),
            cleanup.retired_image_count,
            cleanup.deleted_blob_count,
            cleanup.reclaimed_bytes,
        );
    }
}

fn reconcile_ocr_queue(client: &DatabaseClient) {
    let result = crate::database::now_millis().and_then(|now| {
        let stale_age = i64::try_from(OCR_STALE_ATTEMPT_AGE.as_millis())
            .map_err(|_| DatabaseError::InvalidSystemTime)?;
        let stale_started_at_or_before = now.saturating_sub(stale_age);
        client.recover_interrupted_ocr_jobs(stale_started_at_or_before, now)
    });
    match result {
        Ok(recovery) => log_ocr_recovery("periodic reconciliation", recovery),
        Err(error) => log::error!("could not reconcile the OCR queue: {error}"),
    }
}

fn log_ocr_recovery(context: &str, recovery: crate::database::OcrQueueRecovery) {
    if recovery.requeued > 0 || recovery.terminally_failed > 0 {
        log::warn!(
            "OCR {context} requeued {} interrupted attempt(s) and terminally failed {}",
            recovery.requeued,
            recovery.terminally_failed,
        );
    }
}

fn drain_ocr_queue(client: &DatabaseClient, recognize: impl Fn(&[u8]) -> Result<String, String>) {
    loop {
        let now = match crate::database::now_millis() {
            Ok(now) => now,
            Err(error) => {
                log::error!("could not read the clock for OCR: {error}");
                return;
            }
        };
        let job = match client.claim_next_ocr_job(now) {
            Ok(Some(job)) => job,
            Ok(None) => return,
            Err(error) => {
                log::error!("could not claim the next OCR job: {error}");
                return;
            }
        };
        let result = recognize_without_panicking(&job, &recognize);
        let completed_at = match crate::database::now_millis() {
            Ok(now) => now,
            Err(error) => {
                log::error!("could not read the OCR completion clock: {error}");
                continue;
            }
        };
        if let Err(error) = client.complete_image_ocr(
            job.image_id.clone(),
            job.attempt_count,
            result,
            completed_at,
        ) {
            log::error!("could not persist OCR for image {}: {error}", job.image_id);
        }
    }
}

fn recognize_without_panicking(
    job: &OcrJob,
    recognize: &impl Fn(&[u8]) -> Result<String, String>,
) -> Result<String, String> {
    catch_unwind(AssertUnwindSafe(|| recognize(&job.bytes)))
        .unwrap_or_else(|_| Err("the OCR recognizer panicked".into()))
}

#[cfg(target_os = "macos")]
fn recognize_text(bytes: &[u8]) -> Result<String, String> {
    use objc2::{rc::autoreleasepool, runtime::AnyObject, AnyThread};
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    autoreleasepool(|_| {
        let data = NSData::with_bytes(bytes);
        let options = NSDictionary::<VNImageOption, AnyObject>::init(NSDictionary::<
            VNImageOption,
            AnyObject,
        >::alloc());
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &options,
        );
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);
        let requests = NSArray::<VNRequest>::from_slice(&[&request]);
        handler
            .performRequests_error(&requests)
            .map_err(|error| error.localizedDescription().to_string())?;

        let mut lines = Vec::new();
        if let Some(results) = request.results() {
            for observation in results.iter() {
                if let Some(candidate) = observation.topCandidates(1).firstObject() {
                    let line = candidate.string().to_string();
                    let line = line.trim();
                    if !line.is_empty() {
                        lines.push(line.to_owned());
                    }
                }
            }
        }
        Ok(lines.join("\n"))
    })
}

#[cfg(not(target_os = "macos"))]
fn recognize_text(_bytes: &[u8]) -> Result<String, String> {
    Err("Apple Vision OCR is available only on macOS".into())
}

fn empty_response(status: http::StatusCode) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header("X-Content-Type-Options", "nosniff")
        .body(Vec::new())
        .expect("valid empty response")
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
    use std::{
        cell::{Cell, RefCell},
        io::Cursor,
    };

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    use super::*;
    use crate::database::{
        initialize, CanonicalImage, DatabasePaths, ImageOcrStatus, InitializationOptions,
    };

    const TEST_MEDIA_LEASE_ID: &str = "01980c8e-6c00-7000-8000-000000000902";

    #[test]
    fn canonicalization_downscales_and_emits_decodable_webp() {
        let source = RgbaImage::from_pixel(2_400, 1_200, Rgba([32, 96, 160, 255]));
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(source)
            .write_to(&mut png, ImageFormat::Png)
            .expect("PNG fixture");

        let canonical = canonicalize_image(png.get_ref()).expect("canonical image");
        assert_eq!(
            (canonical.natural_width, canonical.natural_height),
            (1_600, 800)
        );
        assert_eq!(&canonical.bytes[..4], b"RIFF");
        assert_eq!(&canonical.bytes[8..12], b"WEBP");
        let decoded = image::load_from_memory(&canonical.bytes).expect("canonical WebP");
        assert_eq!(decoded.dimensions(), (1_600, 800));
    }

    #[test]
    fn serial_worker_drains_every_eligible_image_one_at_a_time() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = initialize(
            DatabasePaths::new(directory.path().join("data")),
            "test",
            InitializationOptions {
                launch_snapshot: false,
            },
        )
        .expect("database initialization");
        let images = (0..3)
            .map(|index| CanonicalImage {
                bytes: format!("worker test image {index}").into_bytes(),
                natural_width: 800,
                natural_height: 600,
            })
            .collect::<Vec<_>>();
        for image in &images {
            database
                .client()
                .ingest_image(image.clone(), TEST_MEDIA_LEASE_ID.into())
                .expect("image ingestion");
        }

        let active = Cell::new(0_u32);
        let max_active = Cell::new(0_u32);
        let recognized = RefCell::new(Vec::new());
        drain_ocr_queue(&database.client(), |bytes| {
            active.set(active.get() + 1);
            max_active.set(max_active.get().max(active.get()));
            recognized.borrow_mut().push(bytes.to_vec());
            active.set(active.get() - 1);
            Ok("recognized text".into())
        });

        assert_eq!(recognized.borrow().len(), images.len());
        assert_eq!(max_active.get(), 1);
        assert!(database
            .client()
            .claim_next_ocr_job(crate::database::now_millis().expect("claim time"))
            .expect("empty OCR claim")
            .is_none());
        for image in images {
            let record = database
                .client()
                .ingest_image(image, TEST_MEDIA_LEASE_ID.into())
                .expect("deduplicated image");
            assert_eq!(record.ocr_status, ImageOcrStatus::Ready);
        }
    }

    #[test]
    fn coordinator_tracks_the_latest_media_maintenance_report() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let coordinator = OcrCoordinator {
            sender,
            last_media_maintenance: Arc::new(Mutex::new(None)),
        };
        let report = crate::database::MediaMaintenanceReport {
            inspected_at: 42,
            ..Default::default()
        };

        coordinator.record_media_maintenance(report.clone());

        assert_eq!(coordinator.last_media_maintenance(), Some(report));
    }
}
