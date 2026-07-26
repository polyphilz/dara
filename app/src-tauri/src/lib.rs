mod database;
mod diagnostics;
mod external;
#[cfg(not(feature = "e2e"))]
mod logging;
mod media;
mod search;
mod windows;

use std::path::PathBuf;

use tauri::{Emitter, Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(not(feature = "e2e"))]
    let builder = builder.plugin(logging::plugin());
    #[cfg(feature = "e2e")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());

    let app = builder
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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            database::commands::create_card_content,
            database::commands::delete_card_content,
            database::commands::load_card_content,
            database::commands::load_home_stats,
            database::commands::load_review_context,
            database::commands::load_scheduler_replay_snapshot,
            database::commands::maintain_media,
            database::commands::maintain_search,
            database::commands::record_grade,
            database::commands::prepare_desired_retention_replay,
            database::commands::install_scheduler_replay,
            diagnostics::load_diagnostics,
            database::commands::renew_media_lease,
            database::commands::search_card_content,
            database::commands::search_status,
            database::commands::select_next_review_card,
            database::commands::set_card_content_suspended,
            database::commands::undo_last_grade,
            database::commands::update_card_content,
            media::ingest_clipboard_image,
            media::ingest_image_bytes,
            external::open_external_url,
            windows::macos::dismiss_quick_add,
            windows::macos::get_spike_status,
            windows::macos::load_settings,
            windows::macos::set_appearance,
            windows::macos::set_keyboard_bindings,
            windows::macos::set_launch_at_login,
            windows::macos::set_quick_add_file_dialog_open,
            windows::macos::set_zoom_percent,
            windows::macos::adopt_legacy_zoom,
            windows::macos::show_main,
            windows::macos::show_quick_add,
        ])
        .setup(|app| {
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
            log::info!("database ready");
            let resource_dir = app.path().resource_dir()?;
            let search = search::SearchService::start(
                database.client(),
                database.paths().root(),
                &resource_dir,
            )?;
            let ocr = media::OcrCoordinator::start(database.client())?;
            let startup_settings = database.client().load_settings()?;
            app.manage(database);
            app.manage(search);
            app.manage(ocr);

            windows::setup(app, startup_settings)?;
            Ok(())
        })
        .on_window_event(windows::handle_window_event)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if matches!(&event, RunEvent::Resumed) {
            if let Err(error) = app.emit_to("main", windows::macos::REVIEW_CLOCK_REFRESH_EVENT, ())
            {
                log::error!("failed to refresh review clock after wake: {error}");
            }
        }
        if matches!(&event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
            app.state::<search::SearchService>().shutdown();
        }
    });
}
