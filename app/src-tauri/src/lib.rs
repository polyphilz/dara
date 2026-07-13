mod windows;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            windows::macos::dismiss_quick_add,
            windows::macos::get_spike_status,
            windows::macos::save_spike_card,
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

            windows::setup(app)?;
            Ok(())
        })
        .on_window_event(windows::handle_window_event)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
