mod app_lock;
#[allow(dead_code)]
mod backup;
mod database;
mod diagnostics;
mod external;
#[cfg(not(feature = "e2e"))]
mod logging;
mod media;
mod recovery;
mod search;
mod windows;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use tauri::{Emitter, Manager, RunEvent};

pub use recovery::run_from_args as run_recovery_from_args;

#[derive(Default)]
struct ExitShutdownState {
    started: AtomicBool,
    complete: AtomicBool,
}

impl ExitShutdownState {
    fn should_prevent_exit(&self) -> bool {
        !self.complete.load(Ordering::Acquire)
    }

    fn start_once(&self) -> bool {
        !self.started.swap(true, Ordering::AcqRel)
    }

    fn mark_complete(&self) {
        self.complete.store(true, Ordering::Release);
    }
}

fn shutdown_managed_services(app: &tauri::AppHandle) {
    app.state::<backup::media_reconciliation::MediaBackupCoordinator>()
        .shutdown();
    app.state::<backup::litestream_runtime::LitestreamRuntimeService>()
        .shutdown();
    app.state::<search::SearchService>().shutdown();
}

fn finish_exit_after_shutdown(
    app: &tauri::AppHandle,
    exit_code: i32,
    state: Arc<ExitShutdownState>,
) {
    let shutdown_app = app.clone();
    let shutdown_state = Arc::clone(&state);
    let spawned = thread::Builder::new()
        .name("dara-exit-shutdown".into())
        .spawn(move || {
            shutdown_managed_services(&shutdown_app);
            shutdown_state.mark_complete();
            shutdown_app.exit(exit_code);
        });

    if let Err(error) = spawned {
        log::error!("could not start graceful application shutdown: {error}");
        shutdown_managed_services(app);
        state.mark_complete();
        app.exit(exit_code);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_single_instance::init(
        |app, _arguments, _cwd| {
            if let Err(error) = windows::macos::show_main(app.clone()) {
                log::error!("failed to show Dara for the secondary launch: {error}");
            }
        },
    ));
    let builder = builder.plugin(app_lock::plugin());
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
            let data_root = app.state::<app_lock::AppDataLock>().data_root().to_owned();
            let database_paths = database::DatabasePaths::new(data_root);
            recovery::recover_interrupted_restore(&database_paths)?;
            database::register_sqlite_vec()?;
            let database = database::initialize(
                database_paths.clone(),
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
            let offsite_media = backup::media_reconciliation::MediaBackupCoordinator::start(
                database.client(),
                database.paths().media.clone(),
            );
            let litestream = backup::litestream_runtime::LitestreamRuntimeService::start(
                database.client(),
                database.paths().root.clone(),
                database.paths().main.clone(),
                resource_dir.clone(),
            );
            let startup_settings = database.client().load_settings()?;
            app.manage(database);
            app.manage(search);
            app.manage(ocr);
            app.manage(offsite_media);
            app.manage(litestream);

            windows::setup(app, startup_settings)?;
            recovery::confirm_restored_launch(&database_paths)?;
            Ok(())
        })
        .on_window_event(windows::handle_window_event)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    let exit_shutdown = Arc::new(ExitShutdownState::default());
    app.run(move |app, event| {
        if matches!(&event, RunEvent::Resumed) {
            if let Err(error) = app.emit_to("main", windows::macos::REVIEW_CLOCK_REFRESH_EVENT, ())
            {
                log::error!("failed to refresh review clock after wake: {error}");
            }
            app.state::<backup::media_reconciliation::MediaBackupCoordinator>()
                .connectivity_restored();
            app.state::<backup::litestream_runtime::LitestreamRuntimeService>()
                .connectivity_restored();
        }
        if let RunEvent::ExitRequested { code, api, .. } = event {
            if exit_shutdown.should_prevent_exit() {
                api.prevent_exit();
                if exit_shutdown.start_once() {
                    finish_exit_after_shutdown(
                        app,
                        code.unwrap_or_default(),
                        Arc::clone(&exit_shutdown),
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_shutdown_starts_once_and_blocks_exit_until_complete() {
        let state = ExitShutdownState::default();

        assert!(state.should_prevent_exit());
        assert!(state.start_once());
        assert!(!state.start_once());

        state.mark_complete();

        assert!(!state.should_prevent_exit());
    }
}
