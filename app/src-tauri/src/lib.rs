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
mod recovery_startup;
mod search;
mod windows;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Instant,
};

use tauri::{AppHandle, Emitter, Manager, RunEvent};

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

    fn complete_shutdown_once(&self, shutdown: impl FnOnce()) {
        if self.start_once() {
            shutdown();
            self.mark_complete();
        }
    }
}

#[derive(Clone, Copy)]
enum ManagedShutdownService {
    OffsiteCheckpoint,
    OffsiteMedia,
    SemanticSearch,
    Litestream,
}

impl ManagedShutdownService {
    const fn label(self) -> &'static str {
        match self {
            Self::OffsiteCheckpoint => "off-site checkpoint coordinator",
            Self::OffsiteMedia => "off-site media coordinator",
            Self::SemanticSearch => "semantic-search service and llama-server",
            Self::Litestream => "Litestream supervisor and daemon",
        }
    }
}

fn shutdown_managed_services(app: &tauri::AppHandle) {
    let shutdown_started = Instant::now();
    log::info!("application shutdown started");
    if let Some(service) = app.try_state::<backup::checkpoint::CheckpointCoordinator>() {
        shutdown_service(ManagedShutdownService::OffsiteCheckpoint, || {
            service.shutdown()
        });
    }
    if let Some(service) = app.try_state::<backup::media_reconciliation::MediaBackupCoordinator>() {
        shutdown_service(ManagedShutdownService::OffsiteMedia, || service.shutdown());
    }
    if let Some(service) = app.try_state::<search::SearchService>() {
        shutdown_service(ManagedShutdownService::SemanticSearch, || {
            service.shutdown()
        });
    }
    if let Some(service) = app.try_state::<backup::litestream_runtime::LitestreamRuntimeService>() {
        shutdown_service(ManagedShutdownService::Litestream, || service.shutdown());
    }
    log::info!(
        "application shutdown completed in {} ms",
        shutdown_started.elapsed().as_millis()
    );
}

fn shutdown_service(service: ManagedShutdownService, shutdown: impl FnOnce()) {
    let started = Instant::now();
    let name = service.label();
    log::info!("application shutdown stopping {name}");
    shutdown();
    log::info!(
        "application shutdown stopped {name} in {} ms",
        started.elapsed().as_millis()
    );
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

fn finish_exit_during_loop_destroyed(app: &tauri::AppHandle, state: &ExitShutdownState) {
    state.complete_shutdown_once(|| shutdown_managed_services(app));
}

fn initialize_normal_services(
    app: &AppHandle,
    database_paths: &database::DatabasePaths,
) -> Result<database::StoredSettings, Box<dyn std::error::Error>> {
    database::register_sqlite_vec()?;
    let database = database::initialize(
        database_paths.clone(),
        env!("CARGO_PKG_VERSION"),
        database::InitializationOptions::default(),
    )?;
    if recovery::restored_offsite_takeover_required(database_paths)? {
        let client = database.client();
        if let Some(config) = client.load_offsite_backup_config()? {
            client.set_offsite_backup_takeover_reason(
                config.backup_set_id,
                Some(database::OffsiteBackupTakeoverReason::RestoredBackup),
            )?;
        }
    }
    log::info!("database ready");
    let resource_dir = app.path().resource_dir()?;
    let search =
        search::SearchService::start(database.client(), database.paths().root(), &resource_dir)?;
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
    let offsite_checkpoint = backup::checkpoint::CheckpointCoordinator::start(
        database.client(),
        offsite_media.checkpoint_handle(),
        litestream.checkpoint_handle(),
        database.paths().root.clone(),
        database.paths().main.clone(),
        resource_dir,
        env!("CARGO_PKG_VERSION").to_owned(),
    );
    let startup_settings = database.client().load_settings()?;
    app.manage(database);
    app.manage(search);
    app.manage(ocr);
    app.manage(offsite_media);
    app.manage(litestream);
    app.manage(offsite_checkpoint);
    app.manage(backup::commands::OffsiteBackupOperationRegistry::default());

    Ok(startup_settings)
}

fn finish_normal_runtime(
    app: &AppHandle,
    database_paths: &database::DatabasePaths,
) -> Result<(), Box<dyn std::error::Error>> {
    if recovery_startup::take_show_main_after_restart_request(database_paths)? {
        windows::macos::show_main(app.clone()).map_err(std::io::Error::other)?;
    }
    recovery::confirm_restored_launch(database_paths)?;
    Ok(())
}

pub(crate) fn initialize_normal_runtime_after_recovery(
    app: &AppHandle,
    database_paths: database::DatabasePaths,
) -> Result<(), Box<dyn std::error::Error>> {
    let startup_settings = initialize_normal_services(app, &database_paths)?;

    windows::macos::setup_handle(app, startup_settings)?;
    finish_normal_runtime(app, &database_paths)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(
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
            Some(vec![windows::macos::AUTOSTART_ARGUMENT]),
        ))
        .invoke_handler(tauri::generate_handler![
            backup::commands::change_offsite_backup_target,
            backup::commands::create_offsite_backup_now,
            backup::commands::disable_offsite_backup,
            backup::commands::load_offsite_backup_status,
            backup::commands::load_restored_offsite_backup_takeover_required,
            backup::commands::remove_offsite_backup_credentials,
            backup::commands::replace_offsite_backup_credentials,
            backup::commands::run_offsite_restore_drill,
            backup::commands::take_over_restored_offsite_backup,
            backup::commands::test_and_enable_offsite_backup,
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
            recovery_startup::discover_remote_backups,
            recovery_startup::load_application_launch_context,
            recovery_startup::restore_remote_backup,
            recovery_startup::start_fresh_install,
            windows::macos::dismiss_quick_add,
            windows::macos::get_spike_status,
            windows::macos::load_settings,
            windows::macos::set_appearance,
            windows::macos::set_automatic_update_checks,
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
            let pair_state = recovery_startup::inspect_database_pair(&database_paths)?;
            let launch_context = recovery_startup::launch_context(pair_state);
            #[cfg(feature = "e2e")]
            let launch_context = if pair_state == recovery_startup::DatabasePairState::Fresh
                && recovery_startup::e2e_start_fresh_requested()
            {
                recovery_startup::ApplicationLaunchContext::new(
                    recovery_startup::ApplicationLaunchMode::Normal,
                )
            } else {
                launch_context
            };
            let launch_mode = launch_context.mode();
            app.manage(launch_context);
            if launch_mode == recovery_startup::ApplicationLaunchMode::Recovery {
                app.manage(recovery_startup::FreshInstallRecoveryState::default());
                windows::macos::setup_recovery(app)?;
                return Ok(());
            }
            let startup_settings = initialize_normal_services(app.handle(), &database_paths)?;
            windows::setup(app, startup_settings)?;
            finish_normal_runtime(app.handle(), &database_paths)
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
            if let Some(service) =
                app.try_state::<backup::media_reconciliation::MediaBackupCoordinator>()
            {
                service.connectivity_restored();
            }
            if let Some(service) =
                app.try_state::<backup::litestream_runtime::LitestreamRuntimeService>()
            {
                service.connectivity_restored();
            }
            if let Some(service) = app.try_state::<backup::checkpoint::CheckpointCoordinator>() {
                service.wake();
            }
        }
        match event {
            // Clicking the Dock icon of an application whose windows are all hidden reaches
            // the process only as a reopen request; nothing else brings the window back.
            RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    if let Err(error) = windows::macos::show_main(app.clone()) {
                        log::error!("failed to show Dara from its Dock icon: {error}");
                    }
                }
            }
            RunEvent::ExitRequested { code, api, .. } => {
                if exit_shutdown.should_prevent_exit() {
                    api.prevent_exit();
                    if exit_shutdown.start_once() {
                        windows::macos::log_application_quit_context(app);
                        finish_exit_after_shutdown(
                            app,
                            code.unwrap_or_default(),
                            Arc::clone(&exit_shutdown),
                        );
                    }
                }
            }
            // On macOS, choosing Quit can move directly to LoopDestroyed without
            // first emitting ExitRequested. The event loop cannot be held open at
            // this point, so synchronously stop owned helpers before it returns.
            RunEvent::Exit => {
                if exit_shutdown.should_prevent_exit() {
                    windows::macos::log_application_quit_context(app);
                }
                finish_exit_during_loop_destroyed(app, &exit_shutdown);
            }
            _ => {}
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

    #[test]
    fn loop_destroyed_completes_shutdown_exactly_once_without_an_exit_request() {
        let state = ExitShutdownState::default();
        let shutdowns = std::sync::atomic::AtomicUsize::new(0);

        state.complete_shutdown_once(|| {
            shutdowns.fetch_add(1, Ordering::AcqRel);
        });
        state.complete_shutdown_once(|| {
            shutdowns.fetch_add(1, Ordering::AcqRel);
        });

        assert_eq!(shutdowns.load(Ordering::Acquire), 1);
        assert!(!state.should_prevent_exit());
    }
}
