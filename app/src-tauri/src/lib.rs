mod database;
mod external;
mod media;
mod windows;

use std::path::PathBuf;

use tauri::{Emitter, Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .register_uri_scheme_protocol("dara-media", |context, request| {
            media::protocol_response(context.app_handle(), request)
        })
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
            database::commands::create_card_content,
            database::commands::delete_card_content,
            database::commands::load_home_stats,
            database::commands::load_review_context,
            database::commands::record_grade,
            database::commands::search_card_content,
            database::commands::select_next_review_card,
            database::commands::set_card_content_suspended,
            database::commands::undo_last_grade,
            database::commands::update_card_content,
            media::ingest_clipboard_image,
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
            let ocr = media::OcrCoordinator::start(database.client())?;
            app.manage(database);
            app.manage(ocr);

            windows::setup(app)?;
            Ok(())
        })
        .on_window_event(windows::handle_window_event)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if matches!(event, RunEvent::Resumed) {
            if let Err(error) = app.emit_to("main", "review-clock-refresh", ()) {
                log::error!("failed to refresh review clock after wake: {error}");
            }
        }
    });
}
