use serde::Deserialize;

pub(crate) const JINA_V1_MANIFEST_JSON: &str =
    include_str!("../../resources/embedding-indexes/jina-v1.json");

#[cfg(test)]
pub(crate) const JINA_V1_GOLDEN_JSON: &str =
    include_str!("../../resources/embedding-indexes/jina-v1-golden.json");

#[derive(Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Debug, Deserialize, Eq, PartialEq, serde::Serialize)]
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
    serde_json::from_str(JINA_V1_MANIFEST_JSON).expect("embedded Jina v1 manifest must be valid")
}
