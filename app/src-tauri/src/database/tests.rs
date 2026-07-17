use std::{fs, path::Path};

use refinery::{Migration, Runner};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{
    connection::{self, DatabaseKind, FileState, MAIN_APPLICATION_ID, MEDIA_APPLICATION_ID},
    domain::{CardContentType, ReviewCardState, ReviewCardStatus, ReviewEventType},
    embedding_index, initialize,
    media::OcrQueueState,
    migrations, snapshot, DaraCommand, DatabaseError, DatabasePaths, ImageOcrStatus,
    InitializationOptions, DEFAULT_HOME_ACCELERATOR,
};

const BASIC_CONTENT_ID: &str = "01980c8e-6c00-7000-8000-000000000101";
const BASIC_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000102";
const SECOND_CARD_ID: &str = "01980c8e-6c00-7000-8000-000000000103";
const REVIEW_EVENT_ID: &str = "01980c8e-6c00-7000-8000-000000000104";
const IMAGE_ID: &str = "01980c8e-6c00-7000-8000-000000000105";
const DEFAULT_CONFIG_ID: &str = "019f547b-6200-7000-8000-000000000001";
const JINA_INDEX_ID: &str = "019f547b-6200-7000-8000-000000000002";

fn test_paths() -> (TempDir, DatabasePaths) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let paths = DatabasePaths::new(directory.path().join("data"));
    (directory, paths)
}

fn no_launch_snapshot() -> InitializationOptions {
    InitializationOptions {
        launch_snapshot: false,
    }
}

fn initialize_test(paths: &DatabasePaths) -> super::Database {
    initialize(paths.clone(), "test", no_launch_snapshot()).expect("database initialization")
}

fn create_identified_unmigrated_pair(paths: &DatabasePaths) {
    fs::create_dir_all(paths.root()).expect("database root");
    drop(
        connection::open_writer(&paths.main, DatabaseKind::Main, FileState::Fresh)
            .expect("unmigrated main"),
    );
    drop(
        connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Fresh)
            .expect("unmigrated media"),
    );
}

fn open_existing(path: &Path, kind: DatabaseKind) -> Connection {
    connection::open_writer(path, kind, FileState::Existing).expect("existing database")
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()
        .expect("table lookup")
        .is_some()
}

fn insert_basic_content(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (?1, 100, 100, NULL, ?2, 'question', 'answer', NULL)",
            params![BASIC_CONTENT_ID, CardContentType::Basic.as_db_str()],
        )
        .expect("basic content");
}

#[test]
fn fresh_pair_migrates_reopens_and_is_idempotent() {
    let (_directory, paths) = test_paths();
    let database = initialize_test(&paths);
    assert_eq!(database.paths(), &paths);
    drop(database);

    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");
    let main_id: i32 = main
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .expect("main application id");
    let media_id: i32 = media
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .expect("media application id");
    assert_eq!(main_id, MAIN_APPLICATION_ID);
    assert_eq!(media_id, MEDIA_APPLICATION_ID);
    assert_eq!(
        main.query_row(
            "SELECT json_array_length(config_json, '$.parameters') FROM scheduler_config",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("scheduler parameters"),
        21
    );
    assert_eq!(
        main.query_row("SELECT count(*) FROM text_embedding_index", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("shipped index definition"),
        1
    );
    drop(main);
    drop(media);

    let reopened = initialize_test(&paths);
    drop(reopened);

    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let history_rows: i64 = main
        .query_row("SELECT count(*) FROM refinery_schema_history", [], |row| {
            row.get(0)
        })
        .expect("history count");
    assert_eq!(history_rows, 7);
}

#[test]
fn settings_migration_adds_defaults_to_an_existing_database() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let first_five = migrations::main_runner()
        .get_migrations()
        .iter()
        .filter(|migration| migration.version() <= 5)
        .cloned()
        .collect::<Vec<_>>();
    Runner::new(&first_five)
        .set_grouped(true)
        .run(&mut main)
        .expect("pre-settings migrations");
    main.execute(
        "UPDATE app_settings SET updated_at = updated_at + 1 WHERE singleton_id = 1",
        [],
    )
    .expect("existing application state");

    migrations::run_main(&mut main).expect("settings migration");

    let preferences = main
        .query_row(
            "SELECT revision, appearance, zoom_percent, legacy_zoom_migrated
             FROM user_preferences WHERE singleton_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )
        .expect("migrated preferences");
    assert_eq!(preferences, (1, "SYSTEM".into(), 100, false));
    assert_eq!(
        main.query_row("SELECT count(*) FROM keyboard_binding", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("default keyboard bindings"),
        2
    );
    assert_eq!(
        main.query_row(
            "SELECT command, accelerator
             FROM keyboard_binding
             WHERE command = ?1",
            [DaraCommand::Home.as_db_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("default home binding"),
        (
            DaraCommand::Home.as_db_str().to_owned(),
            DEFAULT_HOME_ACCELERATOR.to_owned(),
        )
    );
    assert_eq!(
        main.query_row(
            "SELECT updated_at FROM app_settings WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("preserved application state"),
        1_783_828_800_001
    );
}

#[test]
fn home_shortcut_migration_preserves_a_custom_review_accelerator() {
    const LEGACY_REVIEW_COMMAND: &str = "REVIEW";
    const CUSTOM_ACCELERATOR: &str = "control+alt+super+KeyW";

    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let through_user_settings = migrations::main_runner()
        .get_migrations()
        .iter()
        .filter(|migration| migration.version() <= 6)
        .cloned()
        .collect::<Vec<_>>();
    Runner::new(&through_user_settings)
        .set_grouped(true)
        .run(&mut main)
        .expect("user-settings migrations");
    main.execute(
        "UPDATE keyboard_binding SET accelerator = ?1 WHERE command = ?2",
        params![CUSTOM_ACCELERATOR, LEGACY_REVIEW_COMMAND],
    )
    .expect("custom legacy binding");

    migrations::run_main(&mut main).expect("home-shortcut migration");

    assert_eq!(
        main.query_row(
            "SELECT command, accelerator
             FROM keyboard_binding
             WHERE command = ?1",
            [DaraCommand::Home.as_db_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("migrated home binding"),
        (
            DaraCommand::Home.as_db_str().to_owned(),
            CUSTOM_ACCELERATOR.to_owned(),
        )
    );
}

#[test]
fn applied_durable_image_ocr_migration_checksum_stays_stable() {
    let runner = migrations::main_runner();
    let migration = runner
        .get_migrations()
        .iter()
        .find(|migration| migration.version() == 3)
        .expect("V3 migration");
    assert_eq!(migration.checksum(), 8_411_579_165_145_044_789);
}

#[test]
fn jina_v1_seed_matches_the_canonical_manifest_and_starts_inactive() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    let manifest = embedding_index::jina_v1_manifest();

    let row = main
        .query_row(
            "SELECT
                id, created_at, index_key, model_name, model_revision,
                lower(hex(model_file_sha256)), dimension, distance_metric, normalized,
                index_schema_version, config_json
             FROM text_embedding_index
             WHERE id = ?1",
            [JINA_INDEX_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, u32>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .expect("Jina v1 seed");

    assert_eq!(row.0, manifest.id);
    assert_eq!(row.1, manifest.created_at);
    assert_eq!(row.2, manifest.index_key);
    assert_eq!(row.3, manifest.model_name);
    assert_eq!(row.4, manifest.model_revision);
    assert_eq!(row.5, manifest.model_file_sha256);
    assert_eq!(row.6, manifest.dimension);
    assert_eq!(row.7, manifest.distance_metric);
    assert_eq!(row.8, manifest.normalized);
    assert_eq!(row.9, manifest.index_schema_version);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&row.10).expect("seed config JSON"),
        serde_json::to_value(&manifest.config).expect("manifest config JSON")
    );
    assert_eq!(manifest.manifest_version, 1);

    let active: Option<String> = main
        .query_row(
            "SELECT active_text_embedding_index_id FROM app_settings WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("active embedding index");
    assert_eq!(active, None);

    assert!(main
        .execute(
            "UPDATE text_embedding_index SET model_revision = 'changed' WHERE id = ?1",
            [JINA_INDEX_ID],
        )
        .is_err());
    assert!(main
        .execute(
            "DELETE FROM text_embedding_index WHERE id = ?1",
            [JINA_INDEX_ID],
        )
        .is_err());
}

#[test]
fn jina_v1_golden_fixtures_match_the_manifest() {
    let manifest = embedding_index::jina_v1_manifest();
    let fixtures: serde_json::Value =
        serde_json::from_str(embedding_index::JINA_V1_GOLDEN_JSON).expect("golden fixture JSON");

    assert_eq!(fixtures["fixtureVersion"], 1);
    assert_eq!(
        fixtures["modelFileSha256"],
        manifest.model_file_sha256.as_str()
    );
    let cases = fixtures["cases"].as_array().expect("golden cases");
    assert_eq!(cases.len(), 2);

    for case in cases {
        let name = case["name"].as_str().expect("fixture name");
        let input = case["input"].as_str().expect("fixture input");
        let expected_prefix = match name {
            "query" => &manifest.config.query_prefix,
            "document" => &manifest.config.document_prefix,
            _ => panic!("unexpected fixture {name}"),
        };
        assert!(input.starts_with(expected_prefix));

        let embedding = case["embedding"].as_array().expect("fixture embedding");
        assert_eq!(embedding.len(), manifest.dimension as usize);
        let norm = embedding
            .iter()
            .map(|value| {
                let value = value.as_f64().expect("finite embedding component");
                value * value
            })
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() <= 0.00001, "{name} norm is {norm}");
    }
}

#[test]
fn ordinary_tables_are_strict_and_media_stores_only_blob_lifecycle_state() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");

    for table in [
        "card_content",
        "image",
        "image_draft_lease",
        "media_blob_reap_candidate",
        "review_card",
        "review_event",
        "scheduler_config",
        "search_document",
        "text_embedding",
        "text_embedding_index",
    ] {
        let strict: i64 = main
            .query_row(
                "SELECT strict FROM pragma_table_list WHERE name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("strict table");
        assert_eq!(strict, 1, "{table} must be STRICT");
    }

    let media_tables: Vec<String> = media
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .expect("media tables")
        .query_map([], |row| row.get(0))
        .expect("media rows")
        .collect::<rusqlite::Result<_>>()
        .expect("media table names");
    assert_eq!(
        media_tables,
        vec![
            "media_blob",
            "media_blob_reap_authorization",
            "refinery_schema_history"
        ]
    );
}

#[test]
fn fts_triggers_work_with_trusted_schema_disabled() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    insert_basic_content(&main);
    main.execute(
        "INSERT INTO search_document(rowid, card_content_id, body, content_hash, updated_at)
         VALUES(1, ?1, 'mitochondria produce energy', zeroblob(32), 100)",
        [BASIC_CONTENT_ID],
    )
    .expect("search document insert through FTS trigger");

    let match_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'mitochondria'",
            [],
            |row| row.get(0),
        )
        .expect("FTS query");
    assert_eq!(match_count, 1);

    main.execute(
        "UPDATE search_document SET body = 'ribosome translation', updated_at = 101 WHERE rowid = 1",
        [],
    )
    .expect("search document update through FTS trigger");
    let old_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'mitochondria'",
            [],
            |row| row.get(0),
        )
        .expect("old FTS query");
    let new_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'ribosome'",
            [],
            |row| row.get(0),
        )
        .expect("new FTS query");
    assert_eq!((old_count, new_count), (0, 1));
}

#[test]
fn trigram_search_migration_rebuilds_existing_documents() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let v1 = migrations::main_runner()
        .get_migrations()
        .iter()
        .find(|migration| migration.version() == 1)
        .expect("V1 migration")
        .clone();
    Runner::new(&[v1])
        .set_grouped(true)
        .run(&mut main)
        .expect("V1 schema");

    insert_basic_content(&main);
    main.execute(
        "INSERT INTO search_document(rowid, card_content_id, body, content_hash, updated_at)
         VALUES(1, ?1, 'prefix ffsef suffix', zeroblob(32), 100)",
        [BASIC_CONTENT_ID],
    )
    .expect("pre-migration search document");

    migrations::run_main(&mut main).expect("trigram migration");
    let match_count: i64 = main
        .query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'sef'",
            [],
            |row| row.get(0),
        )
        .expect("substring query after rebuild");
    assert_eq!(match_count, 1);
}

#[test]
fn durable_ocr_migration_queues_unrecognized_images_and_preserves_existing_text() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    let mut main = open_existing(&paths.main, DatabaseKind::Main);
    let available = migrations::main_runner().get_migrations().clone();
    let first_two = [1, 2]
        .map(|version| {
            available
                .iter()
                .find(|migration| migration.version() == version)
                .unwrap_or_else(|| panic!("V{version} migration"))
                .clone()
        })
        .to_vec();
    Runner::new(&first_two)
        .set_grouped(true)
        .run(&mut main)
        .expect("V1 and V2 schema");

    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text
         ) VALUES (?1, 100, 120, NULL, zeroblob(32), 'image/webp', 10, 20, '')",
        [IMAGE_ID],
    )
    .expect("unrecognized image");
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text
         ) VALUES (
            '01980c8e-6c00-7000-8000-000000000106', 100, 130, NULL,
            randomblob(32), 'image/webp', 10, 20, 'existing OCR'
         )",
        [],
    )
    .expect("recognized image");

    migrations::run_main(&mut main).expect("durable OCR migration");
    let pending = main
        .query_row(
            "SELECT ocr_status, ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at,
                    ocr_started_at, ocr_error
             FROM image WHERE id = ?1",
            [IMAGE_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .expect("pending migrated image");
    assert_eq!(
        pending,
        (
            ImageOcrStatus::Pending.as_db_str().into(),
            OcrQueueState::Pending.as_db_str().into(),
            0,
            Some(120),
            None,
            None,
        )
    );
    let ready = main
        .query_row(
            "SELECT ocr_status, ocr_queue_state, ocr_attempt_count, ocr_next_attempt_at,
                    ocr_started_at, ocr_error, ocr_text
             FROM image WHERE id = '01980c8e-6c00-7000-8000-000000000106'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .expect("ready migrated image");
    assert_eq!(
        ready,
        (
            ImageOcrStatus::Ready.as_db_str().into(),
            OcrQueueState::Ready.as_db_str().into(),
            0,
            None,
            None,
            None,
            "existing OCR".into(),
        )
    );
}

#[test]
fn state_checks_partial_uniqueness_and_append_only_history_are_enforced() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    assert!(main
        .execute(
            "INSERT INTO card_content (
                id, created_at, updated_at, deleted_at, type, front_md, back_md, source
             ) VALUES (
                ?1, 100, 100, NULL, ?2, 'question', 'answer', NULL
             )",
            params![
                "01980c8e-6c00-7000-8000-00000000010-",
                CardContentType::Basic.as_db_str()
            ],
        )
        .is_err());
    insert_basic_content(&main);

    let invalid_new = main.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
            variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
            scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, 100, 100, NULL, ?2, ?3, NULL, 'basic', ?4, 100, NULL, NULL,
            0, 0, NULL, NULL, NULL
         )",
        params![
            BASIC_CARD_ID,
            BASIC_CONTENT_ID,
            ReviewCardStatus::Active.as_db_str(),
            ReviewCardState::New.as_db_str()
        ],
    );
    assert!(invalid_new.is_err());

    main.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
            variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
            scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, 100, 100, NULL, ?2, ?3, NULL, 'basic', ?4, NULL, 200,
            100, 1, 0, ?5, 1, '{}'
         )",
        params![
            BASIC_CARD_ID,
            BASIC_CONTENT_ID,
            ReviewCardStatus::Active.as_db_str(),
            ReviewCardState::Review.as_db_str(),
            DEFAULT_CONFIG_ID
        ],
    )
    .expect("valid review card");

    let duplicate = main.execute(
        "INSERT INTO review_card (
            id, created_at, updated_at, deleted_at, card_content_id, status, suspended_at,
            variant_key, state, due_at, due_study_day, last_review_at, reps, lapses,
            scheduler_config_id, scheduler_state_schema_version, scheduler_state_json
         ) VALUES (
            ?1, 101, 101, NULL, ?2, ?3, NULL, 'basic', ?4, NULL, NULL, NULL,
            0, 0, NULL, NULL, NULL
         )",
        params![
            SECOND_CARD_ID,
            BASIC_CONTENT_ID,
            ReviewCardStatus::Active.as_db_str(),
            ReviewCardState::New.as_db_str()
        ],
    );
    assert!(duplicate.is_err());

    main.execute(
        "INSERT INTO review_event (
            id, created_at, event_schema_version, event_type, review_card_id, card_sequence,
            reviewed_at, study_day, timezone_id, utc_offset_minutes, grade,
            scheduler_config_id, scheduler_log_json, target_event_id
         ) VALUES (
            ?1, 100, 1, ?2, ?3, 1, 100, 20000, 'America/New_York', -240, 3,
            ?4, '{}', NULL
         )",
        params![
            REVIEW_EVENT_ID,
            ReviewEventType::Review.as_db_str(),
            BASIC_CARD_ID,
            DEFAULT_CONFIG_ID
        ],
    )
    .expect("review event");
    assert!(main
        .execute(
            "UPDATE review_event SET grade = 4 WHERE id = ?1",
            [REVIEW_EVENT_ID]
        )
        .is_err());
    assert!(main
        .execute("DELETE FROM review_card WHERE id = ?1", [BASIC_CARD_ID])
        .is_err());
    assert!(main.execute("DELETE FROM app_settings", []).is_err());
}

#[test]
fn pending_migrations_require_a_valid_snapshot_before_changes() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);

    let database = initialize_test(&paths);
    drop(database);

    let manifests: Vec<_> = fs::read_dir(&paths.backups)
        .expect("backups")
        .map(|entry| entry.expect("backup entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    assert_eq!(manifests.len(), 1);
    let manifest = snapshot::load_and_validate_manifest(&manifests[0]).expect("manifest");
    assert_eq!(manifest.main.migration_head, None);
    assert_eq!(manifest.media.migration_head, None);

    let restore_directory = tempfile::tempdir().expect("restore directory");
    let restored = DatabasePaths::new(restore_directory.path().join("restored"));
    snapshot::restore_snapshot_pair(&manifests[0], &restored).expect("restore");
    let mut restored_main =
        connection::open_read_only(&restored.main, DatabaseKind::Main).expect("restored main");
    let mut restored_media =
        connection::open_read_only(&restored.media, DatabaseKind::Media).expect("restored media");
    assert_eq!(
        migrations::current_heads(&mut restored_main, &mut restored_media).expect("restored heads"),
        migrations::MigrationHeads {
            main: None,
            media: None
        }
    );
}

#[test]
fn failed_snapshot_gate_leaves_unmigrated_files_unchanged() {
    let (_directory, paths) = test_paths();
    create_identified_unmigrated_pair(&paths);
    fs::write(&paths.backups, b"not a directory").expect("blocking backup path");

    assert!(initialize(paths.clone(), "test", no_launch_snapshot()).is_err());
    let main = connection::open_read_only(&paths.main, DatabaseKind::Main).expect("main");
    let media = connection::open_read_only(&paths.media, DatabaseKind::Media).expect("media");
    assert!(!table_exists(&main, "refinery_schema_history"));
    assert!(!table_exists(&media, "refinery_schema_history"));
}

#[test]
fn swapped_or_incomplete_database_pairs_fail_closed() {
    let (_directory, paths) = test_paths();
    fs::create_dir_all(paths.root()).expect("database root");
    drop(
        connection::open_writer(&paths.main, DatabaseKind::Media, FileState::Fresh)
            .expect("swapped main"),
    );
    drop(
        connection::open_writer(&paths.media, DatabaseKind::Main, FileState::Fresh)
            .expect("swapped media"),
    );
    assert!(matches!(
        initialize(paths.clone(), "test", no_launch_snapshot()),
        Err(DatabaseError::WrongApplicationId { .. })
    ));

    let (_directory, incomplete) = test_paths();
    fs::create_dir_all(incomplete.root()).expect("database root");
    drop(
        connection::open_writer(&incomplete.main, DatabaseKind::Main, FileState::Fresh)
            .expect("main only"),
    );
    assert!(matches!(
        initialize(incomplete, "test", no_launch_snapshot()),
        Err(DatabaseError::IncompletePair { .. })
    ));
}

#[test]
fn changed_checksums_and_future_heads_are_rejected() {
    let (_directory, divergent) = test_paths();
    drop(initialize_test(&divergent));
    let main = open_existing(&divergent.main, DatabaseKind::Main);
    main.execute(
        "UPDATE refinery_schema_history SET checksum = '0' WHERE version = 1",
        [],
    )
    .expect("change checksum");
    drop(main);
    assert!(matches!(
        initialize(divergent, "test", no_launch_snapshot()),
        Err(DatabaseError::IncompatibleMigrationHistory { .. })
    ));

    let (_directory, future) = test_paths();
    drop(initialize_test(&future));
    let main = open_existing(&future.main, DatabaseKind::Main);
    main.execute(
        "INSERT INTO refinery_schema_history(version, name, applied_on, checksum)
         SELECT 8, 'future', applied_on, '0'
         FROM refinery_schema_history WHERE version = 1",
        [],
    )
    .expect("future migration");
    drop(main);
    assert!(matches!(
        initialize(future, "test", no_launch_snapshot()),
        Err(DatabaseError::IncompatibleMigrationHistory { .. })
    ));
}

#[test]
fn grouped_refinery_run_rolls_back_all_pending_migrations() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let mut main = open_existing(&paths.main, DatabaseKind::Main);

    let mut all = migrations::main_runner().get_migrations().clone();
    all.push(
        Migration::unapplied(
            "V8__grouped_good.sql",
            "CREATE TABLE grouped_good(id INTEGER PRIMARY KEY) STRICT;",
        )
        .expect("V7 migration"),
    );
    all.push(
        Migration::unapplied(
            "V9__grouped_failure.sql",
            "CREATE TABLE grouped_failure(id INTEGER) STRICT; THIS IS NOT SQL;",
        )
        .expect("V8 migration"),
    );
    let runner = Runner::new(&all).set_grouped(true);
    assert!(runner.run(&mut main).is_err());
    assert!(!table_exists(&main, "grouped_good"));
    assert_eq!(
        migrations::main_runner()
            .get_last_applied_migration(&mut main)
            .expect("last migration")
            .expect("V7")
            .version(),
        7
    );
}

#[test]
fn refinery_rejects_a_missing_migration_from_an_applied_history() {
    let mut connection = Connection::open_in_memory().expect("in-memory database");
    let first = Migration::unapplied(
        "V1__first.sql",
        "CREATE TABLE first(id INTEGER PRIMARY KEY) STRICT;",
    )
    .expect("first migration");
    let second = Migration::unapplied(
        "V2__second.sql",
        "CREATE TABLE second(id INTEGER PRIMARY KEY) STRICT;",
    )
    .expect("second migration");
    Runner::new(&[first.clone(), second.clone()])
        .set_grouped(true)
        .run(&mut connection)
        .expect("initial history");

    let missing_runner = Runner::new(&[second])
        .set_abort_missing(true)
        .set_abort_divergent(true);
    assert!(missing_runner.run(&mut connection).is_err());
}

#[test]
fn launch_snapshot_runs_in_background_and_retention_keeps_seven_daily_points() {
    let (_directory, paths) = test_paths();
    let database = initialize(
        paths.clone(),
        "test",
        InitializationOptions {
            launch_snapshot: true,
        },
    )
    .expect("database with launch snapshot");
    let launch = database
        .wait_for_launch_snapshot()
        .expect("launch snapshot result")
        .expect("launch snapshot");
    assert!(launch.manifest_path.exists());
    assert_eq!(launch.manifest.main.migration_head, Some(7));
    drop(database);

    let base = launch.manifest.created_at;
    for age_days in 1_i64..10 {
        let mut created = snapshot::create_snapshot_pair(&paths, "test").expect("snapshot");
        created.manifest.created_at = base - age_days * 86_400_000;
        fs::write(
            &created.manifest_path,
            serde_json::to_vec_pretty(&created.manifest).expect("manifest JSON"),
        )
        .expect("rewrite test timestamp");
    }
    snapshot::prune_snapshots(&paths.backups).expect("retention");
    let manifests = fs::read_dir(&paths.backups)
        .expect("backups")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .count();
    assert!((7..10).contains(&manifests));
}

#[test]
fn snapshot_hash_tampering_is_detected_before_restore() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let created = snapshot::create_snapshot_pair(&paths, "test").expect("snapshot");
    let main_path = paths.backups.join(&created.manifest.main.file_name);
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(main_path)
        .expect("snapshot file");
    file.write_all(b"tamper").expect("tamper snapshot");
    assert!(matches!(
        snapshot::load_and_validate_manifest(&created.manifest_path),
        Err(DatabaseError::InvalidSnapshot(_))
    ));
}

#[test]
fn foreign_keys_mask_bounds_and_fts_rebuild_are_enforced() {
    let (_directory, paths) = test_paths();
    drop(initialize_test(&paths));
    let main = open_existing(&paths.main, DatabaseKind::Main);
    insert_basic_content(&main);

    assert!(main
        .execute(
            "INSERT INTO card_content_image(card_content_id, image_id) VALUES(?1, ?2)",
            params![BASIC_CONTENT_ID, IMAGE_ID],
        )
        .is_err());

    main.execute(
        "INSERT INTO search_document(rowid, card_content_id, body, content_hash, updated_at)
         VALUES(1, ?1, 'deterministic rebuild', zeroblob(32), 100)",
        [BASIC_CONTENT_ID],
    )
    .expect("search document");
    main.execute(
        "INSERT INTO search_document_fts(search_document_fts) VALUES('rebuild')",
        [],
    )
    .expect("FTS rebuild");
    assert_eq!(
        main.query_row(
            "SELECT count(*) FROM search_document_fts
             WHERE search_document_fts MATCH 'deterministic'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("rebuilt FTS query"),
        1
    );

    let media = open_existing(&paths.media, DatabaseKind::Media);
    let bytes = b"occlusion image";
    let hash = Sha256::digest(bytes).to_vec();
    media
        .execute(
            "INSERT INTO media_blob(sha256, bytes) VALUES(?1, ?2)",
            params![&hash, bytes.as_slice()],
        )
        .expect("media blob");
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_next_attempt_at
         ) VALUES (?1, 100, 100, NULL, ?2, 'image/webp', 100, 100, '', 100)",
        params![IMAGE_ID, &hash],
    )
    .expect("image");
    assert!(main
        .execute(
            "UPDATE image SET ocr_queue_state = ?1 WHERE id = ?2",
            params![OcrQueueState::Running.as_db_str(), IMAGE_ID],
        )
        .is_err());
    main.execute(
        "INSERT INTO card_occlusion_content (
            id, created_at, updated_at, deleted_at, card_content_id, source_image_id, mode
         ) VALUES (
            '01980c8e-6c00-7000-8000-000000000106', 100, 100, NULL, ?1, ?2,
            'HIDE_ONE_GUESS_ONE'
         )",
        params![BASIC_CONTENT_ID, IMAGE_ID],
    )
    .expect("occlusion content");
    main.execute(
        "INSERT INTO card_occlusion_mask_layer (
            id, created_at, updated_at, deleted_at, card_occlusion_content_id, label, sort_order
         ) VALUES (
            '01980c8e-6c00-7000-8000-000000000107', 100, 100, NULL,
            '01980c8e-6c00-7000-8000-000000000106', NULL, 0
         )",
        [],
    )
    .expect("mask layer");
    assert!(main
        .execute(
            "INSERT INTO card_occlusion_mask (
                id, created_at, updated_at, deleted_at, card_occlusion_mask_layer_id,
                x, y, width, height, color
             ) VALUES (
                '01980c8e-6c00-7000-8000-000000000108', 100, 100, NULL,
                '01980c8e-6c00-7000-8000-000000000107', 0.8, 0.1, 0.3, 0.2, 'WHITE'
             )",
            [],
        )
        .is_err());
}

#[test]
fn media_reconciliation_detects_missing_or_corrupt_blobs_and_tolerates_extras() {
    let (_directory, valid) = test_paths();
    drop(initialize_test(&valid));
    let media = open_existing(&valid.media, DatabaseKind::Media);
    let bytes = b"canonical webp bytes";
    let hash = Sha256::digest(bytes).to_vec();
    media
        .execute(
            "INSERT INTO media_blob(sha256, bytes) VALUES(?1, ?2)",
            params![&hash, bytes.as_slice()],
        )
        .expect("media first");
    drop(media);
    let main = open_existing(&valid.main, DatabaseKind::Main);
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_next_attempt_at
         ) VALUES (?1, 100, 100, NULL, ?2, 'image/webp', 10, 20, '', 100)",
        params![IMAGE_ID, &hash],
    )
    .expect("image metadata second");
    drop(main);
    let database = initialize_test(&valid);
    drop(database);

    let (_directory, missing) = test_paths();
    drop(initialize_test(&missing));
    let main = open_existing(&missing.main, DatabaseKind::Main);
    main.execute(
        "INSERT INTO image (
            id, created_at, updated_at, deleted_at, sha256, mime_type,
            natural_width, natural_height, ocr_text, ocr_next_attempt_at
         ) VALUES (?1, 100, 100, NULL, zeroblob(32), 'image/webp', 10, 20, '', 100)",
        [IMAGE_ID],
    )
    .expect("dangling image metadata");
    drop(main);
    assert!(matches!(
        initialize(missing, "test", no_launch_snapshot()),
        Err(DatabaseError::Validation {
            kind: "database pair",
            ..
        })
    ));

    let (_directory, extra) = test_paths();
    drop(initialize_test(&extra));
    let media = open_existing(&extra.media, DatabaseKind::Media);
    media
        .execute(
            "INSERT INTO media_blob(sha256, bytes) VALUES(?1, ?2)",
            params![&hash, bytes.as_slice()],
        )
        .expect("unreferenced blob");
    drop(media);
    drop(initialize_test(&extra));
}
