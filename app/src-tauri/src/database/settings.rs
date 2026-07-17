use std::{collections::HashSet, str::FromStr};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

use super::{now_millis, DatabaseError, Result};

pub const MIN_ZOOM_PERCENT: i64 = 50;
pub const MAX_ZOOM_PERCENT: i64 = 200;
pub const ZOOM_STEP_PERCENT: i64 = 10;
pub const DEFAULT_QUICK_ADD_ACCELERATOR: &str = "control+alt+super+KeyD";
pub const DEFAULT_REVIEW_ACCELERATOR: &str = "control+alt+super+KeyR";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    const fn as_db_str(self) -> &'static str {
        match self {
            Self::System => "SYSTEM",
            Self::Light => "LIGHT",
            Self::Dark => "DARK",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "SYSTEM" => Ok(Self::System),
            "LIGHT" => Ok(Self::Light),
            "DARK" => Ok(Self::Dark),
            _ => Err(DatabaseError::InvalidStoredSettings(format!(
                "unknown appearance {value}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DaraCommand {
    QuickAdd,
    Review,
}

impl DaraCommand {
    pub const ALL: [Self; 2] = [Self::QuickAdd, Self::Review];

    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::QuickAdd => "QUICK_ADD",
            Self::Review => "REVIEW",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "QUICK_ADD" => Ok(Self::QuickAdd),
            "REVIEW" => Ok(Self::Review),
            _ => Err(DatabaseError::InvalidStoredSettings(format!(
                "unknown Dara command {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardBinding {
    pub command: DaraCommand,
    pub accelerator: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSettings {
    pub revision: i64,
    pub appearance: Appearance,
    pub zoom_percent: i64,
    pub legacy_zoom_migrated: bool,
    pub desired_retention: f64,
    pub keyboard_bindings: Vec<KeyboardBinding>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetAppearanceInput {
    pub expected_revision: i64,
    pub appearance: Appearance,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetZoomPercentInput {
    pub expected_revision: i64,
    pub zoom_percent: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdoptLegacyZoomInput {
    pub expected_revision: i64,
    pub zoom_percent: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetKeyboardBindingsInput {
    pub expected_revision: i64,
    pub keyboard_bindings: Vec<KeyboardBindingInput>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyboardBindingInput {
    pub command: DaraCommand,
    pub accelerator: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchedulerConfigPreference {
    desired_retention: f64,
}

enum ScalarPreference {
    Appearance(Appearance),
    ZoomPercent(i64),
}

pub(super) fn load_settings(connection: &Connection) -> Result<StoredSettings> {
    let row = connection
        .query_row(
            "SELECT
                preferences.revision,
                preferences.appearance,
                preferences.zoom_percent,
                preferences.legacy_zoom_migrated,
                scheduler.config_json
             FROM user_preferences AS preferences
             JOIN app_settings AS active ON active.singleton_id = 1
             JOIN scheduler_config AS scheduler
               ON scheduler.id = active.active_scheduler_config_id
             WHERE preferences.singleton_id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            DatabaseError::InvalidStoredSettings("user preferences singleton is missing".into())
        })?;
    let scheduler: SchedulerConfigPreference = serde_json::from_str(&row.4).map_err(|error| {
        DatabaseError::InvalidStoredSettings(format!(
            "active scheduler preferences are invalid: {error}"
        ))
    })?;
    let mut statement = connection.prepare(
        "SELECT command, accelerator
         FROM keyboard_binding
         ORDER BY command",
    )?;
    let bindings = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut keyboard_bindings = Vec::new();
    for binding in bindings {
        let (command, accelerator) = binding?;
        validate_accelerator(&accelerator)?;
        keyboard_bindings.push(KeyboardBinding {
            command: DaraCommand::from_db(&command)?,
            accelerator,
        });
    }
    validate_complete_bindings(&keyboard_bindings)?;

    Ok(StoredSettings {
        revision: row.0,
        appearance: Appearance::from_db(&row.1)?,
        zoom_percent: row.2,
        legacy_zoom_migrated: row.3,
        desired_retention: scheduler.desired_retention,
        keyboard_bindings,
    })
}

pub(super) fn set_appearance(
    connection: &mut Connection,
    input: SetAppearanceInput,
) -> Result<StoredSettings> {
    update_preferences(
        connection,
        input.expected_revision,
        ScalarPreference::Appearance(input.appearance),
        None,
    )
}

pub(super) fn set_zoom_percent(
    connection: &mut Connection,
    input: SetZoomPercentInput,
) -> Result<StoredSettings> {
    validate_zoom_percent(input.zoom_percent)?;
    update_preferences(
        connection,
        input.expected_revision,
        ScalarPreference::ZoomPercent(input.zoom_percent),
        None,
    )
}

pub(super) fn adopt_legacy_zoom(
    connection: &mut Connection,
    input: AdoptLegacyZoomInput,
) -> Result<StoredSettings> {
    validate_zoom_percent(input.zoom_percent)?;
    update_preferences(
        connection,
        input.expected_revision,
        ScalarPreference::ZoomPercent(input.zoom_percent),
        Some(false),
    )
}

pub(super) fn set_keyboard_bindings(
    connection: &mut Connection,
    input: SetKeyboardBindingsInput,
) -> Result<StoredSettings> {
    let bindings = input
        .keyboard_bindings
        .into_iter()
        .map(|binding| KeyboardBinding {
            command: binding.command,
            accelerator: binding.accelerator,
        })
        .collect::<Vec<_>>();
    validate_complete_bindings(&bindings)?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_revision(&transaction, input.expected_revision)?;
    for binding in &bindings {
        transaction.execute(
            "UPDATE keyboard_binding SET accelerator = ?1 WHERE command = ?2",
            params![binding.accelerator, binding.command.as_db_str()],
        )?;
    }
    advance_revision(&transaction, input.expected_revision)?;
    transaction.commit()?;
    load_settings(connection)
}

fn update_preferences(
    connection: &mut Connection,
    expected_revision: i64,
    preference: ScalarPreference,
    require_legacy_migrated: Option<bool>,
) -> Result<StoredSettings> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_revision(&transaction, expected_revision)?;
    if require_legacy_migrated == Some(false) {
        let migrated: bool = transaction.query_row(
            "SELECT legacy_zoom_migrated FROM user_preferences WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )?;
        if migrated {
            return Err(DatabaseError::StaleSettings(
                "legacy zoom has already been migrated".into(),
            ));
        }
    }
    let changed = match preference {
        ScalarPreference::Appearance(appearance) => transaction.execute(
            "UPDATE user_preferences
             SET appearance = ?1
             WHERE singleton_id = 1 AND revision = ?2",
            params![appearance.as_db_str(), expected_revision],
        )?,
        ScalarPreference::ZoomPercent(zoom_percent) => transaction.execute(
            "UPDATE user_preferences
             SET zoom_percent = ?1
             WHERE singleton_id = 1 AND revision = ?2",
            params![zoom_percent, expected_revision],
        )?,
    };
    if changed != 1 {
        return Err(DatabaseError::StaleSettings(
            "preferences changed before the update".into(),
        ));
    }
    if require_legacy_migrated == Some(false) {
        transaction.execute(
            "UPDATE user_preferences SET legacy_zoom_migrated = 1 WHERE singleton_id = 1",
            [],
        )?;
    }
    advance_revision(&transaction, expected_revision)?;
    transaction.commit()?;
    load_settings(connection)
}

fn require_revision(connection: &Connection, expected_revision: i64) -> Result<()> {
    if expected_revision <= 0 {
        return Err(DatabaseError::InvalidInput(
            "expectedRevision must be positive".into(),
        ));
    }
    let revision: i64 = connection.query_row(
        "SELECT revision FROM user_preferences WHERE singleton_id = 1",
        [],
        |row| row.get(0),
    )?;
    if revision != expected_revision {
        return Err(DatabaseError::StaleSettings(format!(
            "settings revision is {revision}, expected {expected_revision}"
        )));
    }
    Ok(())
}

fn advance_revision(connection: &Connection, expected_revision: i64) -> Result<()> {
    let now = now_millis()?;
    let changed = connection.execute(
        "UPDATE user_preferences
         SET revision = revision + 1,
             updated_at = max(updated_at + 1, ?1)
         WHERE singleton_id = 1 AND revision = ?2",
        params![now, expected_revision],
    )?;
    if changed != 1 {
        return Err(DatabaseError::StaleSettings(
            "preferences changed before commit".into(),
        ));
    }
    Ok(())
}

fn validate_zoom_percent(percent: i64) -> Result<()> {
    if !(MIN_ZOOM_PERCENT..=MAX_ZOOM_PERCENT).contains(&percent) || percent % ZOOM_STEP_PERCENT != 0
    {
        return Err(DatabaseError::InvalidInput(format!(
            "zoomPercent must be {MIN_ZOOM_PERCENT}–{MAX_ZOOM_PERCENT} in {ZOOM_STEP_PERCENT}-point steps"
        )));
    }
    Ok(())
}

pub fn validate_complete_bindings(bindings: &[KeyboardBinding]) -> Result<()> {
    if bindings.len() != DaraCommand::ALL.len() {
        return Err(DatabaseError::InvalidInput(
            "keyboardBindings must contain every Dara command exactly once".into(),
        ));
    }
    let commands = bindings
        .iter()
        .map(|binding| binding.command)
        .collect::<HashSet<_>>();
    if commands.len() != DaraCommand::ALL.len()
        || DaraCommand::ALL
            .iter()
            .any(|command| !commands.contains(command))
    {
        return Err(DatabaseError::InvalidInput(
            "keyboardBindings contains duplicate or missing commands".into(),
        ));
    }
    let mut shortcut_ids = HashSet::new();
    for binding in bindings {
        let shortcut = validate_accelerator(&binding.accelerator)?;
        if !shortcut_ids.insert(shortcut.id()) {
            return Err(DatabaseError::InvalidInput(
                "two Dara commands cannot use the same shortcut".into(),
            ));
        }
    }
    Ok(())
}

pub fn validate_accelerator(accelerator: &str) -> Result<Shortcut> {
    let shortcut = Shortcut::from_str(accelerator).map_err(|error| {
        DatabaseError::InvalidInput(format!("invalid global shortcut: {error}"))
    })?;
    if shortcut
        .mods
        .intersection(Modifiers::SHIFT | Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
        == Modifiers::empty()
    {
        return Err(DatabaseError::InvalidInput(
            "global shortcuts must include at least one modifier".into(),
        ));
    }
    if !supported_shortcut_key(shortcut.key) {
        return Err(DatabaseError::InvalidInput(
            "that key is not supported for Dara global shortcuts".into(),
        ));
    }
    Ok(shortcut)
}

fn supported_shortcut_key(key: Code) -> bool {
    !matches!(
        key,
        Code::CapsLock
            | Code::NumLock
            | Code::Pause
            | Code::PrintScreen
            | Code::ScrollLock
            | Code::AudioVolumeDown
            | Code::AudioVolumeMute
            | Code::AudioVolumeUp
            | Code::MediaPause
            | Code::MediaPlay
            | Code::MediaPlayPause
            | Code::MediaStop
            | Code::MediaTrackNext
            | Code::MediaTrackPrevious
    )
}
