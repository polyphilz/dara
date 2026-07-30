use std::{ptr::NonNull, sync::Mutex};

use block2::RcBlock;
use objc2::MainThreadMarker as ObjcMainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWindow,
    NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSSystemClockDidChangeNotification,
    NSSystemTimeZoneDidChangeNotification,
};
use objc2_web_kit::WKWebView;
use serde::{Deserialize, Serialize};
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, MenuItemKind, PredefinedMenuItem},
    tray::TrayIconBuilder,
    utils::config::BackgroundThrottlingPolicy,
    App, AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::{
    database::{
        commands::{run_writer, CommandError, CommandResult},
        validate_complete_bindings, AdoptLegacyZoomInput, DaraCommand, Database, KeyboardBinding,
        SetAppearanceInput, SetKeyboardBindingsInput, SetZoomPercentInput, StoredSettings,
        DEFAULT_HOME_ACCELERATOR, DEFAULT_QUICK_ADD_ACCELERATOR,
    },
    recovery_startup::{ApplicationLaunchContext, ApplicationLaunchMode},
};

const MAIN_LABEL: &str = "main";
const QUICK_ADD_LABEL: &str = "quick-add";
const TRAY_ID: &str = "dara-tray";
const TRAY_ICON_BYTES: &[u8] = include_bytes!("../../icons/tray-icon.png");
const EDIT_MENU_TEXT: &str = "Edit";
const VIEW_MENU_TEXT: &str = "View";
const SETTINGS_MENU_ID: &str = "open-settings";
const SETTINGS_CHANGED_EVENT: &str = "settings-changed";
const OPEN_SETTINGS_EVENT: &str = "open-settings";
const OPEN_HOME_EVENT: &str = "open-home";
const ZOOM_COMMAND_EVENT: &str = "app-zoom-command";
const BROWSE_COMMAND_EVENT: &str = "browse-command";
pub(crate) const REVIEW_CLOCK_REFRESH_EVENT: &str = "review-clock-refresh";

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
    ShowSettings,
    ShowQuickAdd,
    Quit,
}

impl TrayMenuAction {
    const fn id(self) -> &'static str {
        match self {
            Self::ShowMain => "show-main",
            Self::ShowSettings => "show-settings",
            Self::ShowQuickAdd => "show-quick-add",
            Self::Quit => "quit",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        if id == Self::ShowMain.id() {
            Some(Self::ShowMain)
        } else if id == Self::ShowSettings.id() {
            Some(Self::ShowSettings)
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
    home_shortcut: String,
    shortcut_errors: Vec<String>,
}

impl Default for SpikeStatus {
    fn default() -> Self {
        Self {
            quick_add_ready: false,
            quick_add_shortcut: shortcut_label(DEFAULT_QUICK_ADD_ACCELERATOR),
            home_shortcut: shortcut_label(DEFAULT_HOME_ACCELERATOR),
            shortcut_errors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    #[serde(flatten)]
    stored: StoredSettings,
    launch_at_login: bool,
    launch_at_login_error: Option<String>,
    shortcut_errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetLaunchAtLoginInput {
    enabled: bool,
}

#[derive(Default)]
struct FocusContext {
    dismissing: bool,
    file_dialog_open: bool,
    previous_external_pid: Option<i32>,
    quick_add_visible: bool,
    restore_main: bool,
}

impl FocusContext {
    const fn should_dismiss_quick_add_on_focus_loss(&self) -> bool {
        self.quick_add_visible && !self.dismissing && !self.file_dialog_open
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MainWindowCloseAction {
    Exit,
    Hide,
}

const fn main_window_close_action(
    launch_mode: Option<ApplicationLaunchMode>,
) -> MainWindowCloseAction {
    match launch_mode {
        Some(ApplicationLaunchMode::Recovery) => MainWindowCloseAction::Exit,
        Some(ApplicationLaunchMode::Normal) | None => MainWindowCloseAction::Hide,
    }
}

pub fn setup(app: &mut App, settings: StoredSettings) -> tauri::Result<()> {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    app.get_webview_window(MAIN_LABEL)
        .ok_or(tauri::Error::WebviewNotFound)?
        .set_title(&app.package_info().name)?;
    enable_main_navigation_gestures(app)?;
    let state = SpikeState::default();
    update_shortcut_status(&state, &settings.keyboard_bindings, Vec::new());
    app.manage(state);

    install_application_menu(app)?;
    create_quick_add_window(app.handle())?;
    install_tray(app, &settings.keyboard_bindings)?;
    register_shortcuts(app.handle(), &settings.keyboard_bindings);
    install_clock_change_observers(app.handle());

    Ok(())
}

pub fn setup_recovery(app: &mut App) -> tauri::Result<()> {
    app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or(tauri::Error::WebviewNotFound)?;
    window.set_title(&app.package_info().name)?;
    window.center()?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

fn enable_main_navigation_gestures(app: &App) -> tauri::Result<()> {
    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or(tauri::Error::WebviewNotFound)?;
    window.with_webview(|webview| unsafe {
        // SAFETY: Tauri supplies the main-thread WKWebView owned by this window for the
        // duration of the closure.
        let view: &WKWebView = &*webview.inner().cast();
        view.setAllowsBackForwardNavigationGestures(true);
    })
}

fn install_clock_change_observers(app: &AppHandle) {
    let app = app.clone();
    let refresh = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Err(error) = app.emit_to(MAIN_LABEL, REVIEW_CLOCK_REFRESH_EVENT, ()) {
            log::error!("failed to refresh review clock after a system clock change: {error}");
        }
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: these extern statics are immutable notification-name constants supplied by
    // Foundation for the lifetime of the process.
    let notifications = unsafe {
        [
            NSSystemClockDidChangeNotification,
            NSSystemTimeZoneDidChangeNotification,
        ]
    };
    for notification in notifications {
        // SAFETY: the notification names are Foundation constants, no sender is constrained,
        // and NotificationCenter copies and retains the correctly typed block for app lifetime.
        unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(notification),
                None,
                None,
                &refresh,
            );
        }
    }
}

fn install_application_menu(app: &mut App) -> tauri::Result<()> {
    let menu = Menu::default(app.handle())?;
    if let Some(application_menu) = menu.items()?.into_iter().find_map(|item| match item {
        MenuItemKind::Submenu(submenu) => Some(submenu),
        _ => None,
    }) {
        let settings = MenuItemBuilder::with_id(SETTINGS_MENU_ID, "Settings…")
            .accelerator("CmdOrCtrl+,")
            .build(app)?;
        let separator = PredefinedMenuItem::separator(app)?;
        application_menu.append_items(&[&separator, &settings])?;
    }
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
        if event.id().as_ref() == SETTINGS_MENU_ID {
            if let Err(error) = dispatch_to_main_thread(app, "show settings", show_settings_inner) {
                log::error!("failed to show Settings: {error}");
            }
            return;
        }
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

fn tray_menu(app: &AppHandle, bindings: &[KeyboardBinding]) -> tauri::Result<Menu<tauri::Wry>> {
    let app_name = &app.package_info().name;
    let quick_add_label = binding_for(bindings, DaraCommand::QuickAdd)
        .map(|binding| shortcut_label(&binding.accelerator))
        .unwrap_or_else(|| shortcut_label(DEFAULT_QUICK_ADD_ACCELERATOR));
    MenuBuilder::new(app)
        .text(TrayMenuAction::ShowMain.id(), format!("Open {app_name}"))
        .text(TrayMenuAction::ShowSettings.id(), "Settings…")
        .text(
            TrayMenuAction::ShowQuickAdd.id(),
            format!("Quick Add  {quick_add_label}"),
        )
        .separator()
        .text(TrayMenuAction::Quit.id(), format!("Quit {app_name}"))
        .build()
}

fn install_tray(app: &App, bindings: &[KeyboardBinding]) -> tauri::Result<()> {
    let menu = tray_menu(app.handle(), bindings)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip(app.package_info().name.clone())
        .icon(load_tray_icon()?)
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
                Some(TrayMenuAction::ShowSettings) => {
                    if let Err(error) =
                        dispatch_to_main_thread(app, "show settings", show_settings_inner)
                    {
                        log::error!("failed to show Settings: {error}");
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
        )
        .build(app)?;
    Ok(())
}

fn load_tray_icon() -> tauri::Result<Image<'static>> {
    let icon = image::load_from_memory(TRAY_ICON_BYTES)
        .map_err(|error| tauri::Error::InvalidIcon(std::io::Error::other(error)))?
        .into_rgba8();
    let width = icon.width();
    let height = icon.height();
    Ok(Image::new_owned(icon.into_raw(), width, height))
}

fn register_shortcuts(app: &AppHandle, bindings: &[KeyboardBinding]) {
    for binding in bindings {
        if let Err(error) = register_binding(app, binding) {
            record_shortcut_error(
                app,
                format!(
                    "Could not register {}: {error}",
                    shortcut_label(&binding.accelerator)
                ),
            );
        }
    }
}

fn register_binding(app: &AppHandle, binding: &KeyboardBinding) -> Result<(), String> {
    let shortcut = binding
        .accelerator
        .parse::<Shortcut>()
        .map_err(|error| format!("invalid shortcut: {error}"))?;
    match binding.command {
        DaraCommand::QuickAdd => app
            .global_shortcut()
            .on_shortcut(shortcut, |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    if let Err(error) =
                        dispatch_to_main_thread(app, "show quick add", show_quick_add_inner)
                    {
                        log::error!("quick-add shortcut failed: {error}");
                    }
                }
            })
            .map_err(|error| error.to_string()),
        DaraCommand::Home => app
            .global_shortcut()
            .on_shortcut(shortcut, |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    if let Err(error) = dispatch_to_main_thread(app, "show home", show_home_inner) {
                        log::error!("home shortcut failed: {error}");
                    }
                }
            })
            .map_err(|error| error.to_string()),
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

fn update_shortcut_status(
    state: &SpikeState,
    bindings: &[KeyboardBinding],
    shortcut_errors: Vec<String>,
) {
    let mut status = state.status.lock().expect("spike status poisoned");
    status.quick_add_shortcut = binding_for(bindings, DaraCommand::QuickAdd)
        .map(|binding| shortcut_label(&binding.accelerator))
        .unwrap_or_else(|| shortcut_label(DEFAULT_QUICK_ADD_ACCELERATOR));
    status.home_shortcut = binding_for(bindings, DaraCommand::Home)
        .map(|binding| shortcut_label(&binding.accelerator))
        .unwrap_or_else(|| shortcut_label(DEFAULT_HOME_ACCELERATOR));
    status.shortcut_errors = shortcut_errors;
}

fn binding_for(bindings: &[KeyboardBinding], command: DaraCommand) -> Option<&KeyboardBinding> {
    bindings.iter().find(|binding| binding.command == command)
}

fn shortcut_label(accelerator: &str) -> String {
    let Ok(shortcut) = accelerator.parse::<Shortcut>() else {
        return accelerator.to_owned();
    };
    let mut label = String::new();
    if shortcut.mods.contains(Modifiers::CONTROL) {
        label.push('⌃');
    }
    if shortcut.mods.contains(Modifiers::ALT) {
        label.push('⌥');
    }
    if shortcut.mods.contains(Modifiers::SHIFT) {
        label.push('⇧');
    }
    if shortcut.mods.contains(Modifiers::SUPER) {
        label.push('⌘');
    }
    let key = shortcut.key.to_string();
    label.push_str(
        key.strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(&key),
    );
    label
}

fn settings_snapshot(app: &AppHandle, stored: StoredSettings) -> SettingsSnapshot {
    let (launch_at_login, launch_at_login_error) = match app.autolaunch().is_enabled() {
        Ok(enabled) => (enabled, None),
        Err(error) => (
            false,
            Some(format!("Could not read login-item status: {error}")),
        ),
    };
    let shortcut_errors = app
        .state::<SpikeState>()
        .status
        .lock()
        .expect("spike status poisoned")
        .shortcut_errors
        .clone();
    SettingsSnapshot {
        stored,
        launch_at_login,
        launch_at_login_error,
        shortcut_errors,
    }
}

fn emit_settings(app: &AppHandle, snapshot: &SettingsSnapshot) {
    if let Err(error) = app.emit(SETTINGS_CHANGED_EVENT, snapshot.clone()) {
        log::error!("failed to broadcast settings: {error}");
    }
}

#[tauri::command]
pub async fn load_settings(
    app: AppHandle,
    database: State<'_, Database>,
) -> CommandResult<SettingsSnapshot> {
    let client = database.client();
    let stored = run_writer(move || client.load_settings()).await?;
    Ok(settings_snapshot(&app, stored))
}

#[tauri::command]
pub async fn set_appearance(
    app: AppHandle,
    database: State<'_, Database>,
    input: SetAppearanceInput,
) -> CommandResult<SettingsSnapshot> {
    let client = database.client();
    let stored = run_writer(move || client.set_appearance(input)).await?;
    let snapshot = settings_snapshot(&app, stored);
    emit_settings(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn set_zoom_percent(
    app: AppHandle,
    database: State<'_, Database>,
    input: SetZoomPercentInput,
) -> CommandResult<SettingsSnapshot> {
    let client = database.client();
    let stored = run_writer(move || client.set_zoom_percent(input)).await?;
    let snapshot = settings_snapshot(&app, stored);
    emit_settings(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn adopt_legacy_zoom(
    app: AppHandle,
    database: State<'_, Database>,
    input: AdoptLegacyZoomInput,
) -> CommandResult<SettingsSnapshot> {
    let client = database.client();
    let stored = run_writer(move || client.adopt_legacy_zoom(input)).await?;
    let snapshot = settings_snapshot(&app, stored);
    emit_settings(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn set_launch_at_login(
    app: AppHandle,
    database: State<'_, Database>,
    input: SetLaunchAtLoginInput,
) -> CommandResult<SettingsSnapshot> {
    let manager = app.autolaunch();
    let result = if input.enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|error| {
        CommandError::invalid_input(format!("Could not change launch-at-login: {error}"))
    })?;
    let client = database.client();
    let stored = run_writer(move || client.load_settings()).await?;
    let snapshot = settings_snapshot(&app, stored);
    emit_settings(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn set_keyboard_bindings(
    app: AppHandle,
    database: State<'_, Database>,
    input: SetKeyboardBindingsInput,
) -> CommandResult<SettingsSnapshot> {
    let candidate = input
        .keyboard_bindings
        .iter()
        .map(|binding| KeyboardBinding {
            command: binding.command,
            accelerator: binding.accelerator.clone(),
        })
        .collect::<Vec<_>>();
    validate_complete_bindings(&candidate).map_err(CommandError::from)?;

    let client = database.client();
    let current = run_writer({
        let client = client.clone();
        move || client.load_settings()
    })
    .await?;
    if current.revision != input.expected_revision {
        return Err(CommandError::from(
            crate::database::DatabaseError::StaleSettings(format!(
                "settings revision is {}, expected {}",
                current.revision, input.expected_revision
            )),
        ));
    }

    replace_runtime_shortcuts(&app, &current.keyboard_bindings, &candidate)
        .map_err(CommandError::invalid_input)?;
    if let Err(error) = update_tray_menu(&app, &candidate) {
        restore_runtime_shortcuts(&app, &candidate, &current.keyboard_bindings);
        return Err(CommandError::invalid_input(format!(
            "Could not update shortcut labels: {error}"
        )));
    }

    let stored = match run_writer(move || client.set_keyboard_bindings(input)).await {
        Ok(stored) => stored,
        Err(error) => {
            restore_runtime_shortcuts(&app, &candidate, &current.keyboard_bindings);
            if let Err(menu_error) = update_tray_menu(&app, &current.keyboard_bindings) {
                log::error!("failed to restore tray shortcut labels: {menu_error}");
            }
            return Err(error);
        }
    };
    update_shortcut_status(
        app.state::<SpikeState>().inner(),
        &stored.keyboard_bindings,
        Vec::new(),
    );
    let snapshot = settings_snapshot(&app, stored);
    emit_settings(&app, &snapshot);
    Ok(snapshot)
}

fn replace_runtime_shortcuts(
    app: &AppHandle,
    current: &[KeyboardBinding],
    candidate: &[KeyboardBinding],
) -> Result<(), String> {
    unregister_bindings(app, current)?;
    let mut registered = Vec::new();
    for binding in candidate {
        if let Err(error) = register_binding(app, binding) {
            let _ = unregister_bindings(app, &registered);
            for old in current {
                if let Err(restore_error) = register_binding(app, old) {
                    log::error!("failed to restore {}: {restore_error}", old.accelerator);
                }
            }
            return Err(format!(
                "{} is unavailable: {error}",
                shortcut_label(&binding.accelerator)
            ));
        }
        registered.push(binding.clone());
    }
    Ok(())
}

fn restore_runtime_shortcuts(
    app: &AppHandle,
    candidate: &[KeyboardBinding],
    current: &[KeyboardBinding],
) {
    if let Err(error) = unregister_bindings(app, candidate) {
        log::error!("failed to unregister candidate shortcuts: {error}");
    }
    for binding in current {
        if let Err(error) = register_binding(app, binding) {
            record_shortcut_error(
                app,
                format!(
                    "Could not restore {}: {error}",
                    shortcut_label(&binding.accelerator)
                ),
            );
        }
    }
}

fn unregister_bindings(app: &AppHandle, bindings: &[KeyboardBinding]) -> Result<(), String> {
    for binding in bindings {
        if app
            .global_shortcut()
            .is_registered(binding.accelerator.as_str())
        {
            app.global_shortcut()
                .unregister(binding.accelerator.as_str())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn update_tray_menu(app: &AppHandle, bindings: &[KeyboardBinding]) -> Result<(), String> {
    let menu = tray_menu(app, bindings).map_err(|error| error.to_string())?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "tray icon is unavailable".to_string())?;
    tray.set_menu(Some(menu)).map_err(|error| error.to_string())
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

#[tauri::command]
pub fn set_quick_add_file_dialog_open(state: State<'_, SpikeState>, open: bool) {
    let mut context = state.focus.lock().expect("focus context poisoned");
    context.file_dialog_open = open && context.quick_add_visible;
}

fn dismiss_quick_add_inner(app: &AppHandle, focus: DismissFocus) -> Result<(), String> {
    let target = {
        let state = app.state::<SpikeState>();
        let mut context = state.focus.lock().expect("focus context poisoned");
        if context.dismissing || !context.quick_add_visible {
            return Ok(());
        }
        context.dismissing = true;
        context.file_dialog_open = false;
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
    if app.try_state::<SpikeState>().is_some() {
        dismiss_quick_add_inner(app, DismissFocus::PreserveCurrent)?;
    }
    activate_main_window(app)
}

fn show_settings_inner(app: &AppHandle) -> Result<(), String> {
    show_main_inner(app)?;
    app.emit_to(MAIN_LABEL, OPEN_SETTINGS_EVENT, ())
        .map_err(|error| format!("could not open Settings: {error}"))
}

fn show_home_inner(app: &AppHandle) -> Result<(), String> {
    show_main_inner(app)?;
    app.emit_to(MAIN_LABEL, OPEN_HOME_EVENT, ())
        .map_err(|error| format!("could not open Home: {error}"))
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
    app.emit_to(MAIN_LABEL, REVIEW_CLOCK_REFRESH_EVENT, ())
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
                let launch_mode = window
                    .app_handle()
                    .try_state::<ApplicationLaunchContext>()
                    .map(|context| context.mode);
                match main_window_close_action(launch_mode) {
                    MainWindowCloseAction::Exit => window.app_handle().exit(0),
                    MainWindowCloseAction::Hide => {
                        if let Err(error) = window.hide() {
                            log::error!("failed to hide main window: {error}");
                        }
                        enter_resident_mode(window.app_handle());
                    }
                }
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
                context.should_dismiss_quick_add_on_focus_loss()
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
            if let Err(error) = window.emit(REVIEW_CLOCK_REFRESH_EVENT, ()) {
                log::error!("failed to refresh review clock after activation: {error}");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod focus_tests {
    use super::{
        load_tray_icon, main_window_close_action, ApplicationLaunchMode, FocusContext,
        MainWindowCloseAction,
    };

    #[test]
    fn tray_icon_is_a_transparent_template_image() {
        let icon = load_tray_icon().expect("tray icon should decode");
        assert_eq!((icon.width(), icon.height()), (32, 32));
        assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] == 255));
    }

    #[test]
    fn quick_add_stays_open_while_its_file_dialog_has_focus() {
        let mut context = FocusContext {
            quick_add_visible: true,
            ..FocusContext::default()
        };
        assert!(context.should_dismiss_quick_add_on_focus_loss());

        context.file_dialog_open = true;
        assert!(!context.should_dismiss_quick_add_on_focus_loss());
    }

    #[test]
    fn recovery_close_exits_instead_of_hiding_the_only_window() {
        assert_eq!(
            main_window_close_action(Some(ApplicationLaunchMode::Recovery)),
            MainWindowCloseAction::Exit
        );
        assert_eq!(
            main_window_close_action(Some(ApplicationLaunchMode::Normal)),
            MainWindowCloseAction::Hide
        );
        assert_eq!(main_window_close_action(None), MainWindowCloseAction::Hide);
    }
}
