mod database;
mod external;
mod windows;

use std::path::PathBuf;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _cwd| {
                if let Err(error) = windows::macos::show_main(app.clone()) {
                    log::error!("failed to show Dara for the secondary launch: {error}");
                }
            },
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            database::commands::create_basic_card,
            database::commands::load_review_context,
            database::commands::record_grade,
            database::commands::select_next_review_card,
            database::commands::undo_last_grade,
            external::open_external_url,
            windows::macos::dismiss_quick_add,
            windows::macos::get_spike_status,
            windows::macos::show_main,
            windows::macos::show_quick_add,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            database::register_sqlite_vec()?;
            let data_root = std::env::var_os("DARA_DATA_DIR")
                .map(PathBuf::from)
                .map(Ok)
                .unwrap_or_else(|| app.path().data_dir().map(|path| path.join("dara")))?;
            let database = database::initialize(
                database::DatabasePaths::new(data_root),
                env!("CARGO_PKG_VERSION"),
                database::InitializationOptions::default(),
            )?;
            log::info!("database ready at {}", database.paths().root().display());
            app.manage(database);

            windows::setup(app)?;
            Ok(())
        })
        .on_window_event(windows::handle_window_event)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
