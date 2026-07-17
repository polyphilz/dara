use std::sync::Mutex;

use objc2::MainThreadMarker as ObjcMainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWindow,
    NSWindowCollectionBehavior, NSWorkspace,
};
use serde::Serialize;
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder, MenuItemKind, PredefinedMenuItem},
    tray::TrayIconBuilder,
    utils::config::BackgroundThrottlingPolicy,
    App, AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const MAIN_LABEL: &str = "main";
const QUICK_ADD_LABEL: &str = "quick-add";
const QUICK_ADD_SHORTCUT_LABEL: &str = "⌃⌥⌘D";
const REVIEW_SHORTCUT_LABEL: &str = "⌃⌥⌘R";
const TRAY_ID: &str = "dara-tray";
const EDIT_MENU_TEXT: &str = "Edit";
const VIEW_MENU_TEXT: &str = "View";
const ZOOM_COMMAND_EVENT: &str = "app-zoom-command";
const BROWSE_COMMAND_EVENT: &str = "browse-command";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum AppZoomCommand {
    ZoomIn,
    ZoomOut,
    Reset,
}

impl AppZoomCommand {
    const fn menu_id(self) -> &'static str {
        match self {
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::Reset => "zoom-reset",
        }
    }

    fn from_menu_id(id: &str) -> Option<Self> {
        [Self::ZoomIn, Self::ZoomOut, Self::Reset]
            .into_iter()
            .find(|command| command.menu_id() == id)
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum BrowseCommand {
    FocusSearch,
    ToggleSelectedSuspension,
}

impl BrowseCommand {
    const fn menu_id(self) -> &'static str {
        match self {
            Self::FocusSearch => "browse-focus-search",
            Self::ToggleSelectedSuspension => "browse-toggle-selected-suspension",
        }
    }

    fn from_menu_id(id: &str) -> Option<Self> {
        [Self::FocusSearch, Self::ToggleSelectedSuspension]
            .into_iter()
            .find(|command| command.menu_id() == id)
    }
}

#[derive(Clone, Copy)]
enum TrayMenuAction {
    ShowMain,
    ShowQuickAdd,
    Quit,
}

impl TrayMenuAction {
    const fn id(self) -> &'static str {
        match self {
            Self::ShowMain => "show-main",
            Self::ShowQuickAdd => "show-quick-add",
            Self::Quit => "quit",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        if id == Self::ShowMain.id() {
            Some(Self::ShowMain)
        } else if id == Self::ShowQuickAdd.id() {
            Some(Self::ShowQuickAdd)
        } else if id == Self::Quit.id() {
            Some(Self::Quit)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SpikeStatus {
    quick_add_ready: bool,
    quick_add_shortcut: String,
    review_shortcut: String,
    shortcut_errors: Vec<String>,
}

impl Default for SpikeStatus {
    fn default() -> Self {
        Self {
            quick_add_ready: false,
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
    quick_add_visible: bool,
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
    create_quick_add_window(app.handle())?;
    install_tray(app)?;
    register_shortcuts(app.handle());

    Ok(())
}

fn install_application_menu(app: &mut App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
    if let Some(edit_menu) = menu.items()?.into_iter().find_map(|item| match item {
        MenuItemKind::Submenu(submenu)
            if submenu.text().ok().as_deref() == Some(EDIT_MENU_TEXT) =>
        {
            Some(submenu)
        }
        _ => None,
    }) {
        let separator = PredefinedMenuItem::separator(app)?;
        let focus_search =
            MenuItemBuilder::with_id(BrowseCommand::FocusSearch.menu_id(), "Find Cards")
                .accelerator("CmdOrCtrl+F")
                .build(app)?;
        let toggle_suspension = MenuItemBuilder::with_id(
            BrowseCommand::ToggleSelectedSuspension.menu_id(),
            "Pause or Resume Card",
        )
        .accelerator("CmdOrCtrl+J")
        .build(app)?;
        edit_menu.append_items(&[&separator, &focus_search, &toggle_suspension])?;
    }
    if let Some(view_menu) = menu.items()?.into_iter().find_map(|item| match item {
        MenuItemKind::Submenu(submenu)
            if submenu.text().ok().as_deref() == Some(VIEW_MENU_TEXT) =>
        {
            Some(submenu)
        }
        _ => None,
    }) {
        let zoom_in = MenuItemBuilder::with_id(AppZoomCommand::ZoomIn.menu_id(), "Zoom In")
            .accelerator("CmdOrCtrl+Shift+=")
            .build(app)?;
        let zoom_out = MenuItemBuilder::with_id(AppZoomCommand::ZoomOut.menu_id(), "Zoom Out")
            .accelerator("CmdOrCtrl+-")
            .build(app)?;
        let reset = MenuItemBuilder::with_id(AppZoomCommand::Reset.menu_id(), "Actual Size")
            .accelerator("CmdOrCtrl+0")
            .build(app)?;
        let separator = PredefinedMenuItem::separator(app)?;
        view_menu.prepend_items(&[&zoom_in, &zoom_out, &reset, &separator])?;
    }

    app.on_menu_event(|app, event| {
        if let Some(command) = AppZoomCommand::from_menu_id(event.id().as_ref()) {
            let target = [QUICK_ADD_LABEL, MAIN_LABEL]
                .into_iter()
                .find(|label| {
                    app.get_webview_window(label)
                        .and_then(|window| window.is_focused().ok())
                        .unwrap_or(false)
                })
                .unwrap_or(MAIN_LABEL);
            if let Err(error) = app.emit_to(target, ZOOM_COMMAND_EVENT, command) {
                log::error!("failed to dispatch zoom command: {error}");
            }
            return;
        }
        let Some(command) = BrowseCommand::from_menu_id(event.id().as_ref()) else {
            return;
        };
        let main_is_focused = app
            .get_webview_window(MAIN_LABEL)
            .and_then(|window| window.is_focused().ok())
            .unwrap_or(false);
        if main_is_focused {
            if let Err(error) = app.emit_to(MAIN_LABEL, BROWSE_COMMAND_EVENT, command) {
                log::error!("failed to dispatch Browse command: {error}");
            }
        }
    });
    app.set_menu(menu)?;
    Ok(())
}

fn create_quick_add_window(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(
        app,
        QUICK_ADD_LABEL,
        WebviewUrl::App("quick-add.html".into()),
    )
    .title("Quick Add")
    .inner_size(920.0, 760.0)
    .visible(false)
    .focused(false)
    .focusable(true)
    .decorations(false)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .shadow(true)
    .transparent(true)
    .background_throttling(BackgroundThrottlingPolicy::Disabled)
    .build()?;

    window.with_webview(|webview| unsafe {
        let ns_window: &NSWindow = &*webview.ns_window().cast();
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
    })?;

    app.state::<SpikeState>()
        .status
        .lock()
        .expect("spike status poisoned")
        .quick_add_ready = true;

    Ok(())
}

fn install_tray(app: &App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(TrayMenuAction::ShowMain.id(), "Open Dara")
        .text(
            TrayMenuAction::ShowQuickAdd.id(),
            format!("Quick Add  {QUICK_ADD_SHORTCUT_LABEL}"),
        )
        .separator()
        .text(TrayMenuAction::Quit.id(), "Quit Dara")
        .build()?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .title("d")
        .tooltip("Dara")
        .icon_as_template(true)
        .on_menu_event(
            |app, event| match TrayMenuAction::from_id(event.id().as_ref()) {
                Some(TrayMenuAction::ShowMain) => {
                    if let Err(error) =
                        dispatch_to_main_thread(app, "show main window", show_main_inner)
                    {
                        log::error!("failed to show main window: {error}");
                    }
                }
                Some(TrayMenuAction::ShowQuickAdd) => {
                    if let Err(error) =
                        dispatch_to_main_thread(app, "show quick add", show_quick_add_inner)
                    {
                        log::error!("failed to show quick add: {error}");
                    }
                }
                Some(TrayMenuAction::Quit) => app.exit(0),
                None => {}
            },
        );

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
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
    let window = app
        .get_webview_window(QUICK_ADD_LABEL)
        .ok_or_else(|| "quick-add window unavailable".to_string())?;
    let already_visible = app
        .state::<SpikeState>()
        .focus
        .lock()
        .expect("focus context poisoned")
        .quick_add_visible;

    if !already_visible {
        capture_focus_context(app);
    }
    position_quick_add_on_cursor_monitor(app)?;
    app.state::<SpikeState>()
        .focus
        .lock()
        .expect("focus context poisoned")
        .quick_add_visible = true;

    let result = (|| {
        window
            .show()
            .map_err(|error| format!("could not show quick add: {error}"))?;
        activate_quick_add_window(&window)?;
        app.emit_to(QUICK_ADD_LABEL, "quick-add-shown", ())
            .map_err(|error| format!("could not focus quick add editor: {error}"))?;
        Ok(())
    })();

    if let Err(error) = result {
        if let Err(cleanup_error) = dismiss_quick_add_inner(app, DismissFocus::RestorePrevious) {
            log::error!("failed to clean up Quick Add after show error: {cleanup_error}");
        }
        return Err(error);
    }

    Ok(())
}

fn activate_quick_add_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    let marker = ObjcMainThreadMarker::new()
        .ok_or_else(|| "quick-add activation was not dispatched to the main thread".to_string())?;
    let application = NSApplication::sharedApplication(marker);
    application.activate();
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);

    window
        .set_focus()
        .map_err(|error| format!("could not focus quick add window: {error}"))
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

fn position_quick_add_on_cursor_monitor(app: &AppHandle) -> Result<(), String> {
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
    let window_size = window
        .outer_size()
        .map_err(|error| format!("could not read quick-add size: {error}"))?;

    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let centered_x =
        monitor_position.x + ((monitor_size.width as i32 - window_size.width as i32) / 2).max(0);
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
        dismiss_quick_add_inner(app, DismissFocus::RestorePrevious)
    })
}

fn dismiss_quick_add_inner(app: &AppHandle, focus: DismissFocus) -> Result<(), String> {
    let target = {
        let state = app.state::<SpikeState>();
        let mut context = state.focus.lock().expect("focus context poisoned");
        if context.dismissing || !context.quick_add_visible {
            return Ok(());
        }
        context.dismissing = true;
        context.quick_add_visible = false;
        RestoreTarget {
            external_pid: context.previous_external_pid.take(),
            main: std::mem::take(&mut context.restore_main),
        }
    };

    let result = (|| {
        let window = app
            .get_webview_window(QUICK_ADD_LABEL)
            .ok_or_else(|| "quick-add window unavailable".to_string())?;
        window
            .hide()
            .map_err(|error| format!("could not hide quick add: {error}"))?;

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
                if application.activateWithOptions(NSApplicationActivationOptions::empty()) {
                    return Ok(());
                }
            }
        }

        let main_is_visible = app
            .get_webview_window(MAIN_LABEL)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if main_is_visible {
            return activate_main_window(app);
        }
        enter_resident_mode(app);
    }

    Ok(())
}

#[tauri::command]
pub fn show_main(app: AppHandle) -> Result<(), String> {
    dispatch_to_main_thread(&app, "show main window", show_main_inner)
}

fn show_main_inner(app: &AppHandle) -> Result<(), String> {
    dismiss_quick_add_inner(app, DismissFocus::PreserveCurrent)?;
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
    app.emit_to(MAIN_LABEL, "review-clock-refresh", ())
        .map_err(|error| format!("could not refresh the review clock: {error}"))?;
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
    match event {
        WindowEvent::CloseRequested { api, .. } => match window.label() {
            MAIN_LABEL => {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    log::error!("failed to hide main window: {error}");
                }
                enter_resident_mode(window.app_handle());
            }
            QUICK_ADD_LABEL => {
                api.prevent_close();
                if let Err(error) =
                    dismiss_quick_add_inner(window.app_handle(), DismissFocus::RestorePrevious)
                {
                    log::error!("failed to hide quick add: {error}");
                }
            }
            _ => {}
        },
        WindowEvent::Focused(false) if window.label() == QUICK_ADD_LABEL => {
            let should_dismiss = {
                let state = window.app_handle().state::<SpikeState>();
                let context = state.focus.lock().expect("focus context poisoned");
                context.quick_add_visible && !context.dismissing
            };
            if should_dismiss {
                if let Err(error) =
                    dismiss_quick_add_inner(window.app_handle(), DismissFocus::PreserveCurrent)
                {
                    log::error!("failed to dismiss Quick Add after focus loss: {error}");
                }
            }
        }
        WindowEvent::Focused(true) if window.label() == MAIN_LABEL => {
            if let Err(error) = window.emit("review-clock-refresh", ()) {
                log::error!("failed to refresh review clock after activation: {error}");
            }
        }
        _ => {}
    }
}
