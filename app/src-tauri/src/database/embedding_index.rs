use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::{now_millis, DatabaseError, Result};

pub(crate) const JINA_V1_MANIFEST_JSON: &str =
    include_str!("../../resources/embedding-indexes/jina-v1.json");

pub(crate) const JINA_V1_GOLDEN_JSON: &str =
    include_str!("../../resources/embedding-indexes/jina-v1-golden.json");

pub(crate) const JINA_V1_VEC_TABLE: &str = "text_embedding_vec_jina_v1";
pub(crate) const EMBEDDING_RECONCILIATION_BATCH_SIZE: i64 = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TextEmbeddingIndexManifest {
    pub manifest_version: u32,
    pub id: String,
    pub created_at: i64,
    pub index_key: String,
    pub model_name: String,
    pub model_revision: String,
    pub model_file_sha256: String,
    pub dimension: u32,
    pub distance_metric: String,
    pub normalized: bool,
    pub index_schema_version: u32,
    pub config: TextEmbeddingIndexConfig,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TextEmbeddingIndexConfig {
    pub schema_version: u32,
    pub model_file: String,
    pub model_file_size: u64,
    pub quantization: String,
    pub pooling: String,
    pub normalization: String,
    pub query_prefix: String,
    pub document_prefix: String,
    pub document_construction_version: u32,
}

pub(crate) fn jina_v1_manifest() -> TextEmbeddingIndexManifest {
    let manifest: TextEmbeddingIndexManifest = serde_json::from_str(JINA_V1_MANIFEST_JSON)
        .expect("embedded Jina v1 manifest must be valid");
    let key = TextEmbeddingIndexKey::from_str(&manifest.index_key)
        .expect("embedded embedding index key must be supported");
    assert_eq!(key, TextEmbeddingIndexKey::JinaV1);
    assert_eq!(key.vector_table(), JINA_V1_VEC_TABLE);
    manifest
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextEmbeddingIndexKey {
    JinaV1,
}

impl TextEmbeddingIndexKey {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "jina_v1" => Some(Self::JinaV1),
            _ => None,
        }
    }

    const fn vector_table(self) -> &'static str {
        match self {
            Self::JinaV1 => JINA_V1_VEC_TABLE,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingEmbeddingDocument {
    pub rowid: i64,
    pub body: String,
    pub content_hash: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstallEmbeddingDisposition {
    Installed,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddingIndexProgress {
    pub current_documents: i64,
    pub total_documents: i64,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchMaintenanceOperation {
    IntegrityCheck,
    RebuildFts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMaintenanceReport {
    pub operation: SearchMaintenanceOperation,
    pub search_documents: i64,
    pub fts_rows: i64,
    pub indexed_documents: i64,
    pub total_embedding_documents: i64,
    pub semantic_index_active: bool,
}

pub(super) fn invalidate_search_document(
    transaction: &Transaction<'_>,
    search_document_id: i64,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM text_embedding_vec_jina_v1 WHERE rowid = ?1",
        [search_document_id],
    )?;
    transaction.execute(
        "DELETE FROM text_embedding WHERE search_document_id = ?1",
        [search_document_id],
    )?;
    Ok(())
}

pub(super) fn invalidate_card_content(
    transaction: &Transaction<'_>,
    card_content_id: &str,
) -> Result<()> {
    let search_document_id = transaction
        .query_row(
            "SELECT rowid FROM search_document WHERE card_content_id = ?1",
            [card_content_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(search_document_id) = search_document_id {
        invalidate_search_document(transaction, search_document_id)?;
    }
    Ok(())
}

pub(super) fn load_reconciliation_batch(
    transaction: &Transaction<'_>,
    limit: i64,
) -> Result<Vec<PendingEmbeddingDocument>> {
    if !(1..=EMBEDDING_RECONCILIATION_BATCH_SIZE).contains(&limit) {
        return Err(DatabaseError::InvalidInput(format!(
            "embedding reconciliation limit must be between 1 and {EMBEDDING_RECONCILIATION_BATCH_SIZE}"
        )));
    }
    let manifest = jina_v1_manifest();
    let mut statement = transaction.prepare(
        "SELECT document.rowid, document.body, document.content_hash
         FROM search_document AS document
         LEFT JOIN text_embedding AS metadata
           ON metadata.search_document_id = document.rowid
          AND metadata.text_embedding_index_id = ?1
         WHERE metadata.search_document_id IS NULL
            OR metadata.content_hash != document.content_hash
            OR NOT EXISTS (
                SELECT 1 FROM text_embedding_vec_jina_v1 AS vector
                WHERE vector.rowid = document.rowid
            )
         ORDER BY document.updated_at, document.rowid
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![manifest.id, limit], |row| {
        Ok(PendingEmbeddingDocument {
            rowid: row.get(0)?,
            body: row.get(1)?,
            content_hash: row.get(2)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(super) fn install_embedding(
    transaction: &Transaction<'_>,
    document: &PendingEmbeddingDocument,
    embedding: &[f32],
) -> Result<InstallEmbeddingDisposition> {
    let manifest = jina_v1_manifest();
    validate_embedding(embedding, manifest.dimension as usize)?;
    let current_hash = transaction
        .query_row(
            "SELECT content_hash FROM search_document WHERE rowid = ?1",
            [document.rowid],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    if current_hash.as_deref() != Some(document.content_hash.as_slice()) {
        return Ok(InstallEmbeddingDisposition::Stale);
    }

    invalidate_search_document(transaction, document.rowid)?;
    let vector_json = serde_json::to_string(embedding)?;
    transaction.execute(
        "INSERT INTO text_embedding_vec_jina_v1(rowid, embedding) VALUES (?1, ?2)",
        params![document.rowid, vector_json],
    )?;
    transaction.execute(
        "INSERT INTO text_embedding (
            search_document_id, text_embedding_index_id, content_hash, updated_at
         ) VALUES (?1, ?2, ?3, ?4)",
        params![
            document.rowid,
            manifest.id,
            document.content_hash,
            now_millis()?,
        ],
    )?;
    Ok(InstallEmbeddingDisposition::Installed)
}

pub(super) fn index_progress(transaction: &Transaction<'_>) -> Result<EmbeddingIndexProgress> {
    let manifest = jina_v1_manifest();
    let total_documents =
        transaction.query_row("SELECT count(*) FROM search_document", [], |row| row.get(0))?;
    let current_documents = transaction.query_row(
        "SELECT count(*)
         FROM search_document AS document
         JOIN text_embedding AS metadata
           ON metadata.search_document_id = document.rowid
          AND metadata.text_embedding_index_id = ?1
          AND metadata.content_hash = document.content_hash
         WHERE EXISTS (
             SELECT 1 FROM text_embedding_vec_jina_v1 AS vector
             WHERE vector.rowid = document.rowid
         )",
        [manifest.id.as_str()],
        |row| row.get(0),
    )?;
    let active_index_id = transaction.query_row(
        "SELECT active_text_embedding_index_id FROM app_settings WHERE singleton_id = 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )?;
    Ok(EmbeddingIndexProgress {
        current_documents,
        total_documents,
        active: active_index_id.as_deref() == Some(manifest.id.as_str()),
    })
}

pub(super) fn activate_index_if_complete(transaction: &Transaction<'_>) -> Result<bool> {
    let progress = index_progress(transaction)?;
    if progress.current_documents != progress.total_documents {
        return Ok(false);
    }
    let manifest = jina_v1_manifest();
    transaction.execute(
        "UPDATE app_settings
         SET active_text_embedding_index_id = ?1,
             updated_at = max(updated_at + 1, ?2)
         WHERE singleton_id = 1
           AND active_text_embedding_index_id IS NOT ?1",
        params![manifest.id, now_millis()?],
    )?;
    Ok(true)
}

pub(super) fn maintain_search(
    transaction: &Transaction<'_>,
    operation: SearchMaintenanceOperation,
) -> Result<SearchMaintenanceReport> {
    if operation == SearchMaintenanceOperation::RebuildFts {
        transaction.execute(
            "INSERT INTO search_document_fts(search_document_fts) VALUES('rebuild')",
            [],
        )?;
    }
    transaction.execute(
        "INSERT INTO search_document_fts(search_document_fts) VALUES('integrity-check')",
        [],
    )?;
    let search_documents =
        transaction.query_row("SELECT count(*) FROM search_document", [], |row| row.get(0))?;
    let fts_rows =
        transaction.query_row("SELECT count(*) FROM search_document_fts", [], |row| {
            row.get(0)
        })?;
    let progress = index_progress(transaction)?;
    Ok(SearchMaintenanceReport {
        operation,
        search_documents,
        fts_rows,
        indexed_documents: progress.current_documents,
        total_embedding_documents: progress.total_documents,
        semantic_index_active: progress.active,
    })
}

pub(crate) fn validate_embedding(embedding: &[f32], dimension: usize) -> Result<()> {
    if embedding.len() != dimension {
        return Err(DatabaseError::InvalidInput(format!(
            "embedding has {} dimensions; expected {dimension}",
            embedding.len()
        )));
    }
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(DatabaseError::InvalidInput(
            "embedding contains a non-finite value".into(),
        ));
    }
    let norm = embedding
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if (norm - 1.0).abs() > 0.001 {
        return Err(DatabaseError::InvalidInput(format!(
            "embedding must be L2-normalized; observed norm {norm}"
        )));
    }
    Ok(())
}
