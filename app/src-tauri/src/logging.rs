use tauri::{plugin::TauriPlugin, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

const APP_LOG_FILE_NAME: &str = "dara";
const APP_LOG_MAX_FILE_BYTES: u128 = 2 * 1024 * 1024;
const APP_LOG_ARCHIVE_COUNT: usize = 2;

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri_plugin_log::Builder::default()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir {
            file_name: Some(APP_LOG_FILE_NAME.into()),
        }))
        .level(log::LevelFilter::Info)
        .max_file_size(APP_LOG_MAX_FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(APP_LOG_ARCHIVE_COUNT));

    #[cfg(debug_assertions)]
    let builder = builder.target(Target::new(TargetKind::Stdout));

    builder.build()
}
