use std::{env, fs, path::Path};

use refinery::Runner;
use rusqlite::{params, Connection};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{
    connection::{self, DatabaseKind, FileState},
    domain::{
        CardContentType, OcclusionMaskColor, OcclusionMode, ReviewCardState, ReviewCardStatus,
        ReviewEventType,
    },
    initialize, migrations,
    settings::{Appearance, DaraCommand},
    snapshot, validation, DatabasePaths, ImageOcrStatus, InitializationOptions,
};

const FIXTURE_ENV: &str = "DARA_RELEASE_ACCEPTANCE_FIXTURE_DIR";
const FIXTURE_DESCRIPTION_FILE: &str = "release-acceptance-fixture.json";
const FIXTURE_FORMAT_VERSION: u32 = 1;
const FIXTURE_APPLICATION_VERSION: &str = "0.0.previous";
const PREVIOUS_MAIN_HEAD: i32 = 6;
const PREVIOUS_MEDIA_HEAD: i32 = 1;
const CREATED_AT: i64 = 1_785_110_400_000;
const UPDATED_AT: i64 = CREATED_AT + 1_000;
const DELETED_AT: i64 = UPDATED_AT + 1_000;
const REVIEWED_AT: i64 = CREATED_AT + 500;
const STUDY_DAY: i64 = 20_660;
const DEFAULT_CONFIG_ID: &str = "019f547b-6200-7000-8000-000000000001";
const BASIC_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000201";
const CLOZE_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000202";
const OCCLUSION_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000203";
const DELETED_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000204";
const IMAGE_ID: &str = "01980c8e-6c00-7000-8000-000000000205";
const OCCLUSION_ID: &str = "01980c8e-6c00-7000-8000-000000000206";
const OCCLUSION_LAYER_ID: &str = "01980c8e-6c00-7000-8000-000000000207";
const OCCLUSION_MASK_ID: &str = "01980c8e-6c00-7000-8000-000000000208";
const BASIC_REVIEW_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000301";
const CLOZE_ONE_REVIEW_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000302";
const CLOZE_TWO_REVIEW_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000303";
const OCCLUSION_REVIEW_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000304";
const DELETED_REVIEW_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000305";
const REVIEW_EVENT_ID: &str = "01980c8e-6c00-7000-8000-000000000401";
const REVOKE_EVENT_ID: &str = "01980c8e-6c00-7000-8000-000000000402";
const BASIC_VARIANT_KEY: &str = "basic";
const CLOZE_ONE_VARIANT_KEY: &str = "cloze:1";
const CLOZE_TWO_VARIANT_KEY: &str = "cloze:2";
const OCCLUSION_VARIANT_KEY: &str = "layer:01980c8e-6c00-7000-8000-000000000207";
const LEGACY_QUICK_ADD_ACCELERATOR: &str = "control+alt+super+KeyJ";
const LEGACY_REVIEW_ACCELERATOR: &str = "control+alt+super+KeyK";

const IMAGE_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5, 1, 1,
    39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[derive(Clone, Copy)]
enum LegacyDaraCommand {
    QuickAdd,
    Review,
}

impl LegacyDaraCommand {
    const fn as_db_str(self) -> &'static str {
        match self {
            Self::QuickAdd => "QUICK_ADD",
            Self::Review => "REVIEW",
        }
    }
}

#[test]
#[ignore = "writes a previous-head fixture beneath app/.data for packaged release acceptance"]
fn write_previous_release_fixture() {
    let requested = env::var_os(FIXTURE_ENV)
        .unwrap_or_else(|| panic!("{FIXTURE_ENV} must name a direct child of app/.data"));
    let requested = Path::new(&requested);
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let data_root = manifest_directory
        .parent()
        .expect("app directory")
        .join(".data");
    fs::create_dir_all(&data_root).expect("app/.data");
    let canonical_data_root = fs::canonicalize(&data_root).expect("canonical app/.data");
    let parent = requested.parent().expect("fixture parent");
    let canonical_parent = fs::canonicalize(parent).expect("canonical fixture parent");
    assert_eq!(
        canonical_parent, canonical_data_root,
        "release fixtures must be direct children of app/.data"
    );
    assert!(!requested.exists(), "fixture target already exists");

    let paths = DatabasePaths::new(requested);
    build_previous_release_fixture(&paths);
    println!("wrote {}", requested.display());
}

#[test]
fn rich_previous_release_fixture_migrates_and_preserves_its_snapshot() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let paths = DatabasePaths::new(directory.path().join("upgrade"));
    build_previous_release_fixture(&paths);

    let database = initialize(
        paths.clone(),
        "release-acceptance-test",
        InitializationOptions {
            launch_snapshot: true,
        },
    )
    .expect("upgrade fixture");
    let launch_snapshot = database
        .wait_for_launch_snapshot()
        .expect("launch snapshot result")
        .expect("post-migration launch snapshot");
    assert_eq!(
        (
            launch_snapshot.manifest.main.migration_head,
            launch_snapshot.manifest.media.migration_head,
        ),
        (
            migrations::expected_heads().main,
            migrations::expected_heads().media
        )
    );
    drop(database);

    assert_fixture_state(&paths, migrations::expected_heads());

    let manifests = snapshot_manifests(&paths);
    assert_eq!(
        manifests.len(),
        2,
        "protected pre-migration and managed launch snapshots"
    );
    let pre_migration = manifests
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("migration-safety-"))
        })
        .map(|path| snapshot::load_and_validate_snapshot(path).expect("pre-migration snapshot"))
        .expect("protected pre-migration snapshot");
    assert_eq!(
        (
            pre_migration.manifest.main.migration_head,
            pre_migration.manifest.media.migration_head,
        ),
        (Some(PREVIOUS_MAIN_HEAD), Some(PREVIOUS_MEDIA_HEAD))
    );

    let restored = DatabasePaths::new(directory.path().join("restored"));
    snapshot::restore_snapshot_pair(&manifests[0], &restored).expect("restorable snapshot");
    assert_fixture_state(
        &restored,
        migrations::MigrationHeads {
            main: Some(PREVIOUS_MAIN_HEAD),
            media: Some(PREVIOUS_MEDIA_HEAD),
        },
    );
}

fn build_previous_release_fixture(paths: &DatabasePaths) {
    fs::create_dir_all(paths.root()).expect("fixture directory");
    let mut main = connection::open_writer(&paths.main, DatabaseKind::Main, FileState::Fresh)
        .expect("fixture main database");
    let mut media = connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Fresh)
        .expect("fixture media database");

    run_through(&mut main, migrations::main_runner(), PREVIOUS_MAIN_HEAD);
    run_through(&mut media, migrations::media_runner(), PREVIOUS_MEDIA_HEAD);
    seed_fixture(&main, &media);

    validation::validate_snapshot_pair(&mut main, &mut media, &paths.main, &paths.media)
        .expect("valid previous-head pair");
    main.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .expect("main checkpoint");
    media
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .expect("media checkpoint");
    drop(main);
    drop(media);

    fs::write(
        paths.root().join(FIXTURE_DESCRIPTION_FILE),
        serde_json::to_vec_pretty(&fixture_description()).expect("fixture description JSON"),
    )
    .expect("fixture description");
}

fn run_through(connection: &mut Connection, runner: Runner, head: i32) {
    let selected = runner
        .get_migrations()
        .iter()
        .filter(|migration| migration.version() <= head)
        .cloned()
        .collect::<Vec<_>>();
    Runner::new(&selected)
        .set_grouped(true)
        .run(connection)
        .unwrap_or_else(|error| panic!("migrations through V{head}: {error}"));
}

fn seed_fixture(main: &Connection, media: &Connection) {
    let decoded_image = image::load_from_memory(IMAGE_BYTES).expect("fixture PNG");
    assert_eq!(
        (decoded_image.width(), decoded_image.height()),
        (1, 1),
        "fixture PNG dimensions"
    );
    let image_hash = Sha256::digest(IMAGE_BYTES);
    media
        .execute(
            "INSERT INTO media_blob (sha256, bytes) VALUES (?1, ?2)",
            params![image_hash.as_slice(), IMAGE_BYTES],
        )
        .expect("fixture media blob");

    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_status, ocr_error,
            ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at, ocr_started_at, orphaned_at
         ) VALUES (
            ?1, ?2, ?3, NULL, ?4, 'image/png', 1, 1, 'release acceptance diagram',
            ?5, NULL, ?5, 0, NULL, NULL, NULL
         )",
        params![
            IMAGE_ID,
            CREATED_AT,
            UPDATED_AT,
            image_hash.as_slice(),
            ImageOcrStatus::Ready.as_db_str(),
        ],
    )
    .expect("fixture image");

    insert_content(
        main,
        BASIC_CONTENT_ID,
        CardContentType::Basic,
        "release acceptance mitochondria",
        "the powerhouse fixture survives",
        None,
    );
    insert_content(
        main,
        CLOZE_CONTENT_ID,
        CardContentType::Cloze,
        "{{c1::release}} {{c2::acceptance}} cloze",
        "",
        None,
    );
    insert_content(
        main,
        OCCLUSION_CONTENT_ID,
        CardContentType::Occlusion,
        "",
        "",
        None,
    );
    insert_content(
        main,
        DELETED_CONTENT_ID,
        CardContentType::Basic,
        "deleted release fixture",
        "must remain tombstoned",
        Some(DELETED_AT),
    );

    main.execute(
        "INSERT INTO card_content_image (card_content_id, image_id) VALUES (?1, ?2)",
        params![OCCLUSION_CONTENT_ID, IMAGE_ID],
    )
    .expect("fixture content image");
    main.execute(
        "INSERT INTO card_occlusion_content (
            id, created_at, updated_at, deleted_at, card_content_id, source_image_id, mode
         ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6)",
        params![
            OCCLUSION_ID,
            CREATED_AT,
            UPDATED_AT,
            OCCLUSION_CONTENT_ID,
            IMAGE_ID,
            OcclusionMode::HideOneGuessOne.as_db_str(),
        ],
    )
    .expect("fixture occlusion content");
    main.execute(
        "INSERT INTO card_occlusion_mask_layer (
            id, created_at, updated_at, deleted_at, card_occlusion_content_id, label, sort_order
         ) VALUES (?1, ?2, ?3, NULL, ?4, 'acceptance layer', 0)",
        params![OCCLUSION_LAYER_ID, CREATED_AT, UPDATED_AT, OCCLUSION_ID],
    )
    .expect("fixture occlusion layer");
    main.execute(
        "INSERT INTO card_occlusion_mask (
            id, created_at, updated_at, deleted_at, card_occlusion_mask_layer_id,
            x, y, width, height, color
         ) VALUES (?1, ?2, ?3, NULL, ?4, 0.1, 0.2, 0.3, 0.4, ?5)",
        params![
            OCCLUSION_MASK_ID,
            CREATED_AT,
            UPDATED_AT,
            OCCLUSION_LAYER_ID,
            OcclusionMaskColor::Black.as_db_str(),
        ],
    )
    .expect("fixture occlusion mask");

    insert_review_card(
        main,
        BASIC_REVIEW_CARD_ID,
        BASIC_CONTENT_ID,
        BASIC_VARIANT_KEY,
        ReviewCardStatus::Active,
        None,
        None,
    );
    insert_review_card(
        main,
        CLOZE_ONE_REVIEW_CARD_ID,
        CLOZE_CONTENT_ID,
        CLOZE_ONE_VARIANT_KEY,
        ReviewCardStatus::Active,
        None,
        None,
    );
    insert_review_card(
        main,
        CLOZE_TWO_REVIEW_CARD_ID,
        CLOZE_CONTENT_ID,
        CLOZE_TWO_VARIANT_KEY,
        ReviewCardStatus::Suspended,
        Some(UPDATED_AT),
        None,
    );
    insert_review_card(
        main,
        OCCLUSION_REVIEW_CARD_ID,
        OCCLUSION_CONTENT_ID,
        OCCLUSION_VARIANT_KEY,
        ReviewCardStatus::Active,
        None,
        None,
    );
    insert_review_card(
        main,
        DELETED_REVIEW_CARD_ID,
        DELETED_CONTENT_ID,
        BASIC_VARIANT_KEY,
        ReviewCardStatus::Active,
        None,
        Some(DELETED_AT),
    );

    main.execute(
        "INSERT INTO review_event (
            id, created_at, event_schema_version, event_type, review_card_id, card_sequence,
            reviewed_at, study_day, timezone_id, utc_offset_minutes, grade,
            scheduler_config_id, scheduler_log_json, target_event_id
         ) VALUES (
            ?1, ?2, 1, ?3, ?4, 1, ?5, ?6, 'America/New_York', -240, 3,
            ?7, '{}', NULL
         )",
        params![
            REVIEW_EVENT_ID,
            REVIEWED_AT,
            ReviewEventType::Review.as_db_str(),
            BASIC_REVIEW_CARD_ID,
            REVIEWED_AT,
            STUDY_DAY,
            DEFAULT_CONFIG_ID,
        ],
    )
    .expect("fixture review event");
    main.execute(
        "INSERT INTO review_event (
            id, created_at, event_schema_version, event_type, review_card_id, card_sequence,
            reviewed_at, study_day, timezone_id, utc_offset_minutes, grade,
            scheduler_config_id, scheduler_log_json, target_event_id
         ) VALUES (
            ?1, ?2, 1, ?3, ?4, 2, NULL, NULL, NULL, NULL, NULL,
            ?5, NULL, ?6
         )",
        params![
            REVOKE_EVENT_ID,
            REVIEWED_AT + 1,
            ReviewEventType::Revoke.as_db_str(),
            BASIC_REVIEW_CARD_ID,
            DEFAULT_CONFIG_ID,
            REVIEW_EVENT_ID,
        ],
    )
    .expect("fixture revoke event");

    insert_search_document(
        main,
        1,
        BASIC_CONTENT_ID,
        "release acceptance mitochondria the powerhouse fixture survives",
    );
    insert_search_document(main, 2, CLOZE_CONTENT_ID, "release acceptance cloze");
    insert_search_document(main, 3, OCCLUSION_CONTENT_ID, "release acceptance diagram");

    main.execute(
        "UPDATE user_preferences
         SET updated_at = ?1, revision = 4, appearance = ?2, zoom_percent = 130,
             legacy_zoom_migrated = 1
         WHERE singleton_id = 1",
        params![UPDATED_AT, Appearance::Dark.as_db_str()],
    )
    .expect("fixture preferences");
    main.execute(
        "UPDATE keyboard_binding SET accelerator = ?1 WHERE command = ?2",
        params![
            LEGACY_QUICK_ADD_ACCELERATOR,
            LegacyDaraCommand::QuickAdd.as_db_str()
        ],
    )
    .expect("fixture Quick Add shortcut");
    main.execute(
        "UPDATE keyboard_binding SET accelerator = ?1 WHERE command = ?2",
        params![
            LEGACY_REVIEW_ACCELERATOR,
            LegacyDaraCommand::Review.as_db_str()
        ],
    )
    .expect("fixture Review shortcut");
}

fn insert_content(
    connection: &Connection,
    id: &str,
    content_type: CardContentType,
    front_md: &str,
    back_md: &str,
    deleted_at: Option<i64>,
) {
    let updated_at = deleted_at.unwrap_or(UPDATED_AT);
    connection
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'release acceptance fixture')",
            params![
                id,
                CREATED_AT,
                updated_at,
                deleted_at,
                content_type.as_db_str(),
                front_md,
                back_md,
            ],
        )
        .unwrap_or_else(|error| panic!("fixture content {id}: {error}"));
}

fn insert_review_card(
    connection: &Connection,
    id: &str,
    card_content_id: &str,
    variant_key: &str,
    status: ReviewCardStatus,
    suspended_at: Option<i64>,
    deleted_at: Option<i64>,
) {
    let updated_at = deleted_at.or(suspended_at).unwrap_or(UPDATED_AT);
    connection
        .execute(
            "INSERT INTO review_card (
                id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
                variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
                scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                NULL, NULL, NULL, 0, 0, NULL, NULL, NULL
             )",
            params![
                id,
                CREATED_AT,
                updated_at,
                deleted_at,
                card_content_id,
                status.as_db_str(),
                suspended_at,
                variant_key,
                ReviewCardState::New.as_db_str(),
            ],
        )
        .unwrap_or_else(|error| panic!("fixture review card {id}: {error}"));
}

fn insert_search_document(connection: &Connection, rowid: i64, card_content_id: &str, body: &str) {
    let content_hash = Sha256::digest(body.as_bytes());
    connection
        .execute(
            "INSERT INTO search_document (
                rowid, card_content_id, body, content_hash, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                rowid,
                card_content_id,
                body,
                content_hash.as_slice(),
                UPDATED_AT
            ],
        )
        .unwrap_or_else(|error| panic!("fixture search document {rowid}: {error}"));
}

fn fixture_description() -> serde_json::Value {
    json!({
        "formatVersion": FIXTURE_FORMAT_VERSION,
        "applicationVersion": FIXTURE_APPLICATION_VERSION,
        "migrationHeads": {
            "main": PREVIOUS_MAIN_HEAD,
            "media": PREVIOUS_MEDIA_HEAD,
        },
        "expected": {
            "activeCardContents": 3,
            "deletedCardContents": 1,
            "reviewCards": 5,
            "suspendedReviewCards": 1,
            "reviewEvents": 2,
            "revokedReviewEvents": 1,
            "searchDocuments": 3,
            "images": 1,
            "mediaBlobs": 1,
            "occlusionMasks": 1,
            "appearance": Appearance::Dark.as_db_str(),
            "zoomPercent": 130,
            "quickAddAccelerator": LEGACY_QUICK_ADD_ACCELERATOR,
            "homeAccelerator": LEGACY_REVIEW_ACCELERATOR,
            "homeCommand": DaraCommand::Home.as_db_str(),
            "imageSha256": format!("{image_hash:x}", image_hash = Sha256::digest(IMAGE_BYTES)),
        },
        "ids": {
            "basicContent": BASIC_CONTENT_ID,
            "clozeContent": CLOZE_CONTENT_ID,
            "occlusionContent": OCCLUSION_CONTENT_ID,
            "deletedContent": DELETED_CONTENT_ID,
            "image": IMAGE_ID,
        },
    })
}

fn assert_fixture_state(paths: &DatabasePaths, expected_heads: migrations::MigrationHeads) {
    let mut main =
        connection::open_read_only(&paths.main, DatabaseKind::Main).expect("fixture main");
    let mut media =
        connection::open_read_only(&paths.media, DatabaseKind::Media).expect("fixture media");
    assert_eq!(
        migrations::current_heads(&mut main, &mut media).expect("fixture heads"),
        expected_heads
    );
    assert_eq!(
        main.query_row(
            "SELECT count(*) FROM card_content WHERE deleted_at IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("active content count"),
        3
    );
    assert_eq!(
        main.query_row("SELECT count(*) FROM review_event", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("review event count"),
        2
    );
    assert_eq!(
        main.query_row("SELECT count(*) FROM search_document", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("search document count"),
        3
    );
    assert_eq!(
        media
            .query_row("SELECT count(*) FROM media_blob", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("media blob count"),
        1
    );
    validation::validate_snapshot_pair(&mut main, &mut media, &paths.main, &paths.media)
        .expect("fixture pair remains valid");
}

fn snapshot_manifests(paths: &DatabasePaths) -> Vec<std::path::PathBuf> {
    let mut manifests = fs::read_dir(&paths.backups)
        .expect("fixture backups")
        .map(|entry| entry.expect("backup entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    manifests.sort();
    manifests
}
