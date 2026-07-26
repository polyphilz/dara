use rusqlite::Connection;
use serde::Serialize;

use super::{
    domain::{self, SchedulerAlgorithm, SchedulerLibrary},
    embedding_index, migrations, settings, Result,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseDiagnosticsSnapshot {
    pub migration_heads: migrations::MigrationHeads,
    pub scheduler: SchedulerDiagnostics,
    pub semantic_index: SemanticIndexDiagnostics,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerDiagnostics {
    pub algorithm: SchedulerAlgorithm,
    pub algorithm_version: i64,
    pub scheduler_library: SchedulerLibrary,
    pub library_version: String,
    pub desired_retention: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticIndexDiagnostics {
    pub id: String,
    pub active: bool,
    pub indexed_documents: i64,
    pub total_documents: i64,
}

pub(super) fn load_database_diagnostics(
    main: &mut Connection,
    media: &mut Connection,
) -> Result<DatabaseDiagnosticsSnapshot> {
    let migration_heads = migrations::current_heads(main, media)?;
    let active_scheduler = domain::load_active_scheduler_config(main)?;
    let desired_retention = settings::load_settings(main)?.desired_retention;
    let index_manifest = embedding_index::jina_v1_manifest();
    let index_progress = embedding_index::index_progress(main)?;

    Ok(DatabaseDiagnosticsSnapshot {
        migration_heads,
        scheduler: SchedulerDiagnostics {
            algorithm: active_scheduler.algorithm,
            algorithm_version: active_scheduler.algorithm_version,
            scheduler_library: active_scheduler.scheduler_library,
            library_version: active_scheduler.library_version,
            desired_retention,
        },
        semantic_index: SemanticIndexDiagnostics {
            id: index_manifest.id,
            active: index_progress.active,
            indexed_documents: index_progress.current_documents,
            total_documents: index_progress.total_documents,
        },
    })
}
