// tauri-nspanel's event macro requires an explicit `-> ()` callback signature.
#![allow(clippy::unused_unit)]

use std::{ptr::NonNull, sync::Mutex};

use block2::RcBlock;
use objc2::MainThreadMarker as ObjcMainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSEvent as AppKitEvent, NSEventMask,
    NSRunningApplication, NSWorkspace,
};
use serde::Serialize;
use tauri::{
    menu::{Menu, MenuBuilder},
    tray::TrayIconBuilder,
    App, AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Size, State, WebviewUrl,
    WindowEvent,
};
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const MAIN_LABEL: &str = "main";
const QUICK_ADD_LABEL: &str = "quick-add";
const QUICK_ADD_SHORTCUT_LABEL: &str = "⌃⌥⌘D";
const REVIEW_SHORTCUT_LABEL: &str = "⌃⌥⌘R";

tauri_panel! {
    panel!(QuickAddPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })

    panel_event!(QuickAddPanelEventHandler {
        window_did_resign_key(notification: &NSNotification) -> ()
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct SpikeStatus {
    panel_ready: bool,
    quick_add_shortcut: String,
    review_shortcut: String,
    shortcut_errors: Vec<String>,
}

impl Default for SpikeStatus {
    fn default() -> Self {
        Self {
            panel_ready: false,
            quick_add_shortcut: QUICK_ADD_SHORTCUT_LABEL.into(),
            review_shortcut: REVIEW_SHORTCUT_LABEL.into(),
            shortcut_errors: Vec::new(),
        }
    }
}

#[derive(Default)]
struct FocusContext {
    dismissing: bool,
    previous_external_pid: Option<i32>,
    restore_main: bool,
}

#[derive(Default)]
pub struct SpikeState {
    focus: Mutex<FocusContext>,
    status: Mutex<SpikeStatus>,
}

#[derive(Default)]
struct RestoreTarget {
    external_pid: Option<i32>,
    main: bool,
}

#[derive(Clone, Copy)]
enum DismissFocus {
    RestorePrevious,
    PreserveCurrent,
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    app.manage(SpikeState::default());

    install_application_menu(app)?;
    create_quick_add_panel(app.handle())?;
    install_outside_click_monitor(app.handle());
    install_tray(app)?;
    register_shortcuts(app.handle());

    Ok(())
}

fn install_application_menu(app: &mut App) -> tauri::Result<()> {
    app.set_menu(Menu::default(app.handle())?)?;
    Ok(())
}

fn create_quick_add_panel(app: &AppHandle) -> tauri::Result<()> {
    let panel = PanelBuilder::<_, QuickAddPanel>::new(app, QUICK_ADD_LABEL)
        .url(WebviewUrl::App("quick-add.html".into()))
        .title("Quick Add")
        .size(Size::Logical(LogicalSize::new(640.0, 430.0)))
        .level(PanelLevel::Floating)
        .floating(true)
        .has_shadow(true)
        .hides_on_deactivate(false)
        .works_when_modal(true)
        .released_when_closed(false)
        .corner_radius(18.0)
        .transparent(true)
        .style_mask(StyleMask::empty().nonactivating_panel())
        .collection_behavior(
            CollectionBehavior::new()
                .move_to_active_space()
                .full_screen_auxiliary(),
        )
        .with_window(|window| {
            window
                .visible(false)
                .decorations(false)
                .resizable(false)
                .skip_taskbar(true)
                .transparent(true)
                .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        })
        .no_activate(true)
        .build()?;

    let handler = QuickAddPanelEventHandler::new();
    let app_handle = app.clone();
    handler.window_did_resign_key(move |_notification| {
        if let Err(error) =
            dismiss_quick_add_inner(&app_handle, true, DismissFocus::PreserveCurrent)
        {
            log::error!("failed to dismiss quick add after resigning key: {error}");
        }
    });
    panel.set_event_handler(Some(handler.as_ref()));

    app.state::<SpikeState>()
        .status
        .lock()
        .expect("spike status poisoned")
        .panel_ready = true;

    Ok(())
}

fn install_tray(app: &App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("show-main", "Open Dara")
        .text(
            "show-quick-add",
            format!("Quick Add  {QUICK_ADD_SHORTCUT_LABEL}"),
        )
        .separator()
        .text("quit", "Quit Dara")
        .build()?;

    let mut tray = TrayIconBuilder::with_id("dara-tray")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .title("d")
        .tooltip("Dara")
        .icon_as_template(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-main" => {
                if let Err(error) =
                    dispatch_to_main_thread(app, "show main window", show_main_inner)
                {
                    log::error!("failed to show main window: {error}");
                }
            }
            "show-quick-add" => {
                if let Err(error) =
                    dispatch_to_main_thread(app, "show quick add", show_quick_add_inner)
                {
                    log::error!("failed to show quick add: {error}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

fn install_outside_click_monitor(app: &AppHandle) {
    let app_handle = app.clone();
    let handler = RcBlock::new(move |_event: NonNull<AppKitEvent>| {
        let panel_is_visible = app_handle
            .get_webview_panel(QUICK_ADD_LABEL)
            .map(|panel| panel.is_visible())
            .unwrap_or(false);

        if panel_is_visible {
            if let Err(error) = dispatch_to_main_thread(
                &app_handle,
                "dismiss quick add after outside click",
                |app| dismiss_quick_add_inner(app, false, DismissFocus::PreserveCurrent),
            ) {
                log::error!("outside-click dismissal failed: {error}");
            }
        }
    });
    let mask =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;

    if let Some(monitor) =
        AppKitEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &handler)
    {
        // The monitor is process-lifetime infrastructure and must remain retained until exit.
        std::mem::forget(monitor);
    } else {
        log::error!("failed to install the global mouse monitor for quick-add dismissal");
    }
}

fn register_shortcuts(app: &AppHandle) {
    let modifiers = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER;
    let quick_add = Shortcut::new(Some(modifiers), Code::KeyD);
    let review = Shortcut::new(Some(modifiers), Code::KeyR);

    if let Err(error) = app
        .global_shortcut()
        .on_shortcut(quick_add, |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                if let Err(error) =
                    dispatch_to_main_thread(app, "show quick add", show_quick_add_inner)
                {
                    log::error!("quick-add shortcut failed: {error}");
                }
            }
        })
    {
        record_shortcut_error(
            app,
            format!("Could not register {QUICK_ADD_SHORTCUT_LABEL}: {error}"),
        );
    }

    if let Err(error) = app
        .global_shortcut()
        .on_shortcut(review, |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                if let Err(error) =
                    dispatch_to_main_thread(app, "show main window", show_main_inner)
                {
                    log::error!("review shortcut failed: {error}");
                }
            }
        })
    {
        record_shortcut_error(
            app,
            format!("Could not register {REVIEW_SHORTCUT_LABEL}: {error}"),
        );
    }
}

fn record_shortcut_error(app: &AppHandle, message: String) {
    log::error!("{message}");
    app.state::<SpikeState>()
        .status
        .lock()
        .expect("spike status poisoned")
        .shortcut_errors
        .push(message);
}

#[tauri::command]
pub fn get_spike_status(state: State<'_, SpikeState>) -> SpikeStatus {
    state.status.lock().expect("spike status poisoned").clone()
}

#[tauri::command]
pub fn show_quick_add(app: AppHandle) -> Result<(), String> {
    dispatch_to_main_thread(&app, "show quick add", show_quick_add_inner)
}

fn show_quick_add_inner(app: &AppHandle) -> Result<(), String> {
    let panel = app
        .get_webview_panel(QUICK_ADD_LABEL)
        .map_err(|error| format!("quick-add panel unavailable: {error:?}"))?;

    capture_focus_context(app);
    position_panel_on_cursor_monitor(app)?;
    panel.show_and_make_key();
    app.emit_to(QUICK_ADD_LABEL, "quick-add-shown", ())
        .map_err(|error| format!("could not focus quick add: {error}"))?;

    Ok(())
}

fn capture_focus_context(app: &AppHandle) {
    let current_pid = std::process::id() as i32;
    let frontmost_pid = frontmost_application_pid();
    let main_is_focused = app
        .get_webview_window(MAIN_LABEL)
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);

    let state = app.state::<SpikeState>();
    let mut context = state.focus.lock().expect("focus context poisoned");
    context.previous_external_pid = frontmost_pid.filter(|pid| *pid != current_pid);
    context.restore_main = frontmost_pid == Some(current_pid) && main_is_focused;
}

fn position_panel_on_cursor_monitor(app: &AppHandle) -> Result<(), String> {
    let cursor = app
        .cursor_position()
        .map_err(|error| format!("could not read cursor position: {error}"))?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(|error| format!("could not find cursor monitor: {error}"))?
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or_else(|| "no monitor available for quick add".to_string())?;
    let window = app
        .get_webview_window(QUICK_ADD_LABEL)
        .ok_or_else(|| "quick-add webview unavailable".to_string())?;
    let panel_size = window
        .outer_size()
        .map_err(|error| format!("could not read quick-add size: {error}"))?;

    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let centered_x =
        monitor_position.x + ((monitor_size.width as i32 - panel_size.width as i32) / 2).max(0);
    let top_offset = ((monitor_size.height as f64 * 0.14).round() as i32).clamp(72, 150);
    let y = monitor_position.y + top_offset;

    window
        .set_position(PhysicalPosition::new(centered_x, y))
        .map_err(|error| format!("could not position quick add: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn dismiss_quick_add(app: AppHandle) -> Result<(), String> {
    dispatch_to_main_thread(&app, "dismiss quick add", |app| {
        dismiss_quick_add_inner(app, false, DismissFocus::RestorePrevious)
    })
}

fn dismiss_quick_add_inner(
    app: &AppHandle,
    already_resigned: bool,
    focus: DismissFocus,
) -> Result<(), String> {
    let target = {
        let state = app.state::<SpikeState>();
        let mut context = state.focus.lock().expect("focus context poisoned");
        if context.dismissing {
            return Ok(());
        }
        context.dismissing = true;
        RestoreTarget {
            external_pid: context.previous_external_pid.take(),
            main: std::mem::take(&mut context.restore_main),
        }
    };

    let result = (|| {
        let panel = app
            .get_webview_panel(QUICK_ADD_LABEL)
            .map_err(|error| format!("quick-add panel unavailable: {error:?}"))?;
        if !already_resigned {
            panel.resign_key_window();
        }
        panel.hide();

        if matches!(focus, DismissFocus::RestorePrevious) {
            restore_previous_focus(app, target)?;
        }
        Ok(())
    })();

    app.state::<SpikeState>()
        .focus
        .lock()
        .expect("focus context poisoned")
        .dismissing = false;

    result
}

fn restore_previous_focus(app: &AppHandle, target: RestoreTarget) -> Result<(), String> {
    if target.main {
        return activate_main_window(app);
    }

    let current_pid = std::process::id() as i32;
    if frontmost_application_pid() == Some(current_pid) {
        if let Some(pid) = target.external_pid {
            if let Some(application) =
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            {
                application.activateWithOptions(NSApplicationActivationOptions::empty());
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn show_main(app: AppHandle) -> Result<(), String> {
    dispatch_to_main_thread(&app, "show main window", show_main_inner)
}

fn show_main_inner(app: &AppHandle) -> Result<(), String> {
    dismiss_quick_add_inner(app, false, DismissFocus::PreserveCurrent)?;
    activate_main_window(app)
}

fn activate_main_window(app: &AppHandle) -> Result<(), String> {
    let marker = ObjcMainThreadMarker::new().ok_or_else(|| {
        "main-window activation was not dispatched to the main thread".to_string()
    })?;
    app.set_activation_policy(tauri::ActivationPolicy::Regular)
        .map_err(|error| format!("could not enter regular-app mode: {error}"))?;

    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or_else(|| "main window unavailable".to_string())?;
    window
        .unminimize()
        .map_err(|error| format!("could not unminimize main window: {error}"))?;
    window
        .show()
        .map_err(|error| format!("could not show main window: {error}"))?;

    let application = NSApplication::sharedApplication(marker);
    application.activate();
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);

    window
        .set_focus()
        .map_err(|error| format!("could not focus main window: {error}"))?;
    Ok(())
}

fn frontmost_application_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|application| application.processIdentifier())
}

fn enter_resident_mode(app: &AppHandle) {
    if let Some(marker) = ObjcMainThreadMarker::new() {
        NSApplication::sharedApplication(marker).deactivate();
    }
    if let Err(error) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
        log::error!("failed to restore resident Accessory mode: {error}");
    }
}

fn dispatch_to_main_thread<F>(
    app: &AppHandle,
    operation_name: &'static str,
    operation: F,
) -> Result<(), String>
where
    F: FnOnce(&AppHandle) -> Result<(), String> + Send + 'static,
{
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(error) = operation(&app_handle) {
            log::error!("{operation_name} failed on the main thread: {error}");
        }
    })
    .map_err(|error| format!("could not dispatch {operation_name}: {error}"))
}

pub fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        match window.label() {
            MAIN_LABEL => {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    log::error!("failed to hide main window: {error}");
                }
                enter_resident_mode(window.app_handle());
            }
            QUICK_ADD_LABEL => {
                api.prevent_close();
                if let Err(error) = dismiss_quick_add_inner(
                    window.app_handle(),
                    false,
                    DismissFocus::RestorePrevious,
                ) {
                    log::error!("failed to hide quick add: {error}");
                }
            }
            _ => {}
        }
    }
}
