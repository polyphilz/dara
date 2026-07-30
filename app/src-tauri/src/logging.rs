use std::path::PathBuf;

use tauri::{plugin::TauriPlugin, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

const APP_LOG_FILE_NAME: &str = "dara";
const APP_LOG_DIRECTORY_NAME: &str = "logs";
const APP_LOG_MAX_FILE_BYTES: u128 = 2 * 1024 * 1024;
const APP_LOG_ARCHIVE_COUNT: usize = 2;

/// A run that owns an explicit data directory keeps its log beside that data, so a
/// development or recovery run never appends to the installed application's log.
fn log_target_kind() -> TargetKind {
    match std::env::var_os("DARA_DATA_DIR") {
        Some(data_root) => TargetKind::Folder {
            path: PathBuf::from(data_root).join(APP_LOG_DIRECTORY_NAME),
            file_name: Some(APP_LOG_FILE_NAME.into()),
        },
        None => TargetKind::LogDir {
            file_name: Some(APP_LOG_FILE_NAME.into()),
        },
    }
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri_plugin_log::Builder::default()
        .clear_targets()
        .target(Target::new(log_target_kind()))
        .level(log::LevelFilter::Info)
        .max_file_size(APP_LOG_MAX_FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(APP_LOG_ARCHIVE_COUNT));

    #[cfg(debug_assertions)]
    let builder = builder.target(Target::new(TargetKind::Stdout));

    builder.build()
}
