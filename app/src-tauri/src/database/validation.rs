use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use super::{
    connection::{self, DatabaseKind},
    embedding_index,
    error::{DatabaseError, Result},
    migrations,
};

const SUPPORTED_SQLITE_VEC_VERSION: &str = "v0.1.9";

pub fn validate_migrated_pair(
    main: &mut Connection,
    media: &mut Connection,
    main_path: &Path,
    media_path: &Path,
) -> Result<()> {
    connection::verify_application_id(main, main_path, DatabaseKind::Main)?;
    connection::verify_application_id(media, media_path, DatabaseKind::Media)?;
    validate_integrity(main, DatabaseKind::Main)?;
    validate_integrity(media, DatabaseKind::Media)?;
    validate_expected_heads(main, media)?;
    validate_required_sqlite_features(main)?;
    validate_main_schema(main)?;
    validate_media_schema(media)?;
    validate_foreign_keys(main, DatabaseKind::Main)?;
    validate_foreign_keys(media, DatabaseKind::Media)?;
    validate_fts(main)?;
    validate_vec(main)?;
    validate_media_relationship(main, media)?;
    Ok(())
}

pub fn validate_snapshot_pair(
    main: &mut Connection,
    media: &mut Connection,
    main_path: &Path,
    media_path: &Path,
) -> Result<bool> {
    connection::verify_application_id(main, main_path, DatabaseKind::Main)?;
    connection::verify_application_id(media, media_path, DatabaseKind::Media)?;
    validate_integrity(main, DatabaseKind::Main)?;
    validate_integrity(media, DatabaseKind::Media)?;
    migrations::inspect_main(main)?;
    migrations::inspect_media(media)?;
    validate_foreign_keys(main, DatabaseKind::Main)?;
    validate_foreign_keys(media, DatabaseKind::Media)?;
    validate_media_relationship(main, media)
}

pub fn validate_integrity(connection: &Connection, kind: DatabaseKind) -> Result<()> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result != "ok" {
        return invalid(kind, format!("integrity_check returned {result}"));
    }
    Ok(())
}

pub fn validate_foreign_keys(connection: &Connection, kind: DatabaseKind) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let mut rows = statement.query([])?;
    if let Some(row) = rows.next()? {
        let table: String = row.get(0)?;
        let rowid: Option<i64> = row.get(1)?;
        return invalid(
            kind,
            format!("foreign_key_check failed for {table} row {rowid:?}"),
        );
    }
    Ok(())
}

pub fn validate_media_relationship(main: &Connection, media: &Connection) -> Result<bool> {
    if !table_exists(main, "image")? || !table_exists(media, "media_blob")? {
        return Ok(true);
    }

    let mut image_statement = main.prepare("SELECT sha256 FROM image ORDER BY sha256")?;
    let hashes = image_statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
    let mut blob_statement = media.prepare("SELECT bytes FROM media_blob WHERE sha256 = ?1")?;

    for hash_result in hashes {
        let expected = hash_result?;
        let bytes = blob_statement
            .query_row(params![&expected], |row| row.get::<_, Vec<u8>>(0))
            .optional()?
            .ok_or_else(|| DatabaseError::Validation {
                kind: "database pair",
                reason: format!("media blob {} is missing", hex(&expected)),
            })?;
        let actual = Sha256::digest(&bytes);
        if actual.as_slice() != expected.as_slice() {
            return Err(DatabaseError::Validation {
                kind: "database pair",
                reason: format!("media blob {} does not match its digest", hex(&expected)),
            });
        }
    }
    Ok(true)
}

fn validate_expected_heads(main: &mut Connection, media: &mut Connection) -> Result<()> {
    let actual = migrations::current_heads(main, media)?;
    let expected = migrations::expected_heads();
    if actual != expected {
        return Err(DatabaseError::Validation {
            kind: "migration",
            reason: format!("heads are {actual:?}, expected {expected:?}"),
        });
    }
    Ok(())
}

fn validate_required_sqlite_features(connection: &Connection) -> Result<()> {
    let fts5: i64 = connection.query_row(
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
        [],
        |row| row.get(0),
    )?;
    if fts5 != 1 {
        return invalid(DatabaseKind::Main, "bundled SQLite lacks FTS5".into());
    }

    let json_valid: i64 =
        connection.query_row("SELECT json_valid('{\"ok\":true}')", [], |row| row.get(0))?;
    if json_valid != 1 {
        return invalid(
            DatabaseKind::Main,
            "bundled SQLite lacks JSON support".into(),
        );
    }

    let vec_version: String = connection.query_row("SELECT vec_version()", [], |row| row.get(0))?;
    if vec_version != SUPPORTED_SQLITE_VEC_VERSION {
        return invalid(
            DatabaseKind::Main,
            format!("sqlite-vec version is {vec_version}, expected {SUPPORTED_SQLITE_VEC_VERSION}"),
        );
    }
    Ok(())
}

fn validate_main_schema(connection: &Connection) -> Result<()> {
    const TABLES: &[&str] = &[
        "app_settings",
        "card_content",
        "card_content_image",
        "card_occlusion_content",
        "card_occlusion_mask",
        "card_occlusion_mask_layer",
        "image",
        "image_draft_lease",
        "media_blob_reap_candidate",
        "review_card",
        "review_event",
        "scheduler_config",
        "search_document",
        "search_document_fts",
        "text_embedding",
        "text_embedding_index",
        "text_embedding_vec_jina_v1",
    ];
    for table in TABLES {
        if !table_exists(connection, table)? {
            return invalid(
                DatabaseKind::Main,
                format!("required table {table} is missing"),
            );
        }
    }

    validate_jina_v1_definition(connection)?;

    let settings_rows: i64 =
        connection.query_row("SELECT count(*) FROM app_settings", [], |row| row.get(0))?;
    if settings_rows != 1 {
        return invalid(
            DatabaseKind::Main,
            format!("app_settings contains {settings_rows} rows"),
        );
    }
    Ok(())
}

fn validate_jina_v1_definition(connection: &Connection) -> Result<()> {
    let manifest = embedding_index::jina_v1_manifest();
    let stored = connection
        .query_row(
            "SELECT
                id, created_at, index_key, model_name, model_revision,
                lower(hex(model_file_sha256)), dimension, distance_metric, normalized,
                index_schema_version, config_json
             FROM text_embedding_index
             WHERE index_key = ?1",
            [&manifest.index_key],
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
        .optional()?;
    let Some(stored) = stored else {
        return invalid(
            DatabaseKind::Main,
            "the shipped Jina v1 embedding-index definition is missing".into(),
        );
    };
    let stored_config: embedding_index::TextEmbeddingIndexConfig =
        serde_json::from_str(&stored.10)?;

    if stored.0 != manifest.id
        || stored.1 != manifest.created_at
        || stored.2 != manifest.index_key
        || stored.3 != manifest.model_name
        || stored.4 != manifest.model_revision
        || stored.5 != manifest.model_file_sha256
        || stored.6 != manifest.dimension
        || stored.7 != manifest.distance_metric
        || stored.8 != manifest.normalized
        || stored.9 != manifest.index_schema_version
        || stored_config != manifest.config
    {
        return invalid(
            DatabaseKind::Main,
            "the Jina v1 embedding-index definition does not match the shipped manifest".into(),
        );
    }
    Ok(())
}

fn validate_media_schema(connection: &Connection) -> Result<()> {
    for table in ["media_blob", "media_blob_reap_authorization"] {
        if !table_exists(connection, table)? {
            return invalid(
                DatabaseKind::Media,
                format!("required table {table} is missing"),
            );
        }
    }
    Ok(())
}

fn validate_fts(connection: &Connection) -> Result<()> {
    connection.execute(
        "INSERT INTO search_document_fts(search_document_fts) VALUES('integrity-check')",
        [],
    )?;
    Ok(())
}

fn validate_vec(connection: &mut Connection) -> Result<()> {
    let vector = zero_vector_json(768);
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO text_embedding_vec_jina_v1(rowid, embedding) VALUES(-1, ?1)",
        params![&vector],
    )?;
    let rowid: i64 = transaction.query_row(
        "SELECT rowid
         FROM text_embedding_vec_jina_v1
         WHERE embedding MATCH ?1 AND k = 1
         ORDER BY distance",
        params![&vector],
        |row| row.get(0),
    )?;
    if rowid != -1 {
        return invalid(
            DatabaseKind::Main,
            format!("sqlite-vec smoke query returned rowid {rowid}"),
        );
    }
    transaction.rollback()?;
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            params![table],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(exists)
}

fn zero_vector_json(dimension: usize) -> String {
    let mut vector = String::with_capacity(dimension * 2 + 1);
    vector.push('[');
    for index in 0..dimension {
        if index > 0 {
            vector.push(',');
        }
        vector.push('0');
    }
    vector.push(']');
    vector
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn invalid<T>(kind: DatabaseKind, reason: String) -> Result<T> {
    Err(DatabaseError::Validation {
        kind: kind.label(),
        reason,
    })
}
