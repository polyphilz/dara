use tempfile::TempDir;

use super::{
    initialize,
    settings::{Appearance, KeyboardBindingInput},
    AdoptLegacyZoomInput, DaraCommand, Database, DatabaseError, DatabasePaths,
    InitializationOptions, SetAppearanceInput, SetAutomaticUpdateChecksInput,
    SetKeyboardBindingsInput, SetZoomPercentInput, DEFAULT_HOME_ACCELERATOR,
    DEFAULT_QUICK_ADD_ACCELERATOR,
};

fn test_database() -> (TempDir, Database) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = initialize(
        DatabasePaths::new(directory.path().join("data")),
        "test",
        InitializationOptions {
            launch_snapshot: false,
        },
    )
    .expect("database initialization");
    (directory, database)
}

#[test]
fn settings_defaults_are_complete_and_typed() {
    let (_directory, database) = test_database();
    let settings = database.load_settings().expect("load settings");

    assert_eq!(settings.revision, 1);
    assert_eq!(settings.appearance, Appearance::System);
    assert!(settings.automatic_update_checks_enabled);
    assert_eq!(settings.zoom_percent, 100);
    assert!(!settings.legacy_zoom_migrated);
    assert_eq!(settings.desired_retention, 0.9);
    assert_eq!(settings.keyboard_bindings.len(), 2);
    assert!(settings.keyboard_bindings.iter().any(|binding| {
        binding.command == DaraCommand::QuickAdd
            && binding.accelerator == DEFAULT_QUICK_ADD_ACCELERATOR
    }));
    assert!(settings.keyboard_bindings.iter().any(|binding| {
        binding.command == DaraCommand::Home && binding.accelerator == DEFAULT_HOME_ACCELERATOR
    }));
}

#[test]
fn scalar_settings_advance_the_revision_and_reject_stale_writes() {
    let (_directory, database) = test_database();
    let initial = database.load_settings().expect("initial settings");
    let dark = database
        .set_appearance(SetAppearanceInput {
            expected_revision: initial.revision,
            appearance: Appearance::Dark,
        })
        .expect("dark appearance");
    assert_eq!(dark.revision, initial.revision + 1);
    assert_eq!(dark.appearance, Appearance::Dark);

    let automatic_checks_disabled = database
        .set_automatic_update_checks(SetAutomaticUpdateChecksInput {
            expected_revision: dark.revision,
            enabled: false,
        })
        .expect("disable automatic update checks");
    assert!(!automatic_checks_disabled.automatic_update_checks_enabled);

    let stale = database.set_zoom_percent(SetZoomPercentInput {
        expected_revision: initial.revision,
        zoom_percent: 120,
    });
    assert!(matches!(stale, Err(DatabaseError::StaleSettings(_))));
    assert_eq!(
        database.load_settings().expect("unchanged").zoom_percent,
        100
    );

    let zoomed = database
        .set_zoom_percent(SetZoomPercentInput {
            expected_revision: automatic_checks_disabled.revision,
            zoom_percent: 120,
        })
        .expect("zoom update");
    assert_eq!(zoomed.zoom_percent, 120);
}

#[test]
fn legacy_zoom_is_adopted_exactly_once() {
    let (_directory, database) = test_database();
    let initial = database.load_settings().expect("initial settings");
    let adopted = database
        .adopt_legacy_zoom(AdoptLegacyZoomInput {
            expected_revision: initial.revision,
            zoom_percent: 130,
        })
        .expect("legacy zoom adoption");
    assert!(adopted.legacy_zoom_migrated);
    assert_eq!(adopted.zoom_percent, 130);

    let second = database.adopt_legacy_zoom(AdoptLegacyZoomInput {
        expected_revision: adopted.revision,
        zoom_percent: 140,
    });
    assert!(matches!(second, Err(DatabaseError::StaleSettings(_))));
    assert_eq!(
        database.load_settings().expect("preserved").zoom_percent,
        130
    );
}

#[test]
fn keyboard_bindings_are_replaced_as_a_complete_conflict_free_set() {
    let (_directory, database) = test_database();
    let initial = database.load_settings().expect("initial settings");
    let replaced = database
        .set_keyboard_bindings(SetKeyboardBindingsInput {
            expected_revision: initial.revision,
            keyboard_bindings: vec![
                KeyboardBindingInput {
                    command: DaraCommand::QuickAdd,
                    accelerator: "control+alt+super+KeyQ".into(),
                },
                KeyboardBindingInput {
                    command: DaraCommand::Home,
                    accelerator: "control+alt+super+KeyW".into(),
                },
            ],
        })
        .expect("binding replacement");
    assert_eq!(replaced.revision, initial.revision + 1);

    let duplicate = database.set_keyboard_bindings(SetKeyboardBindingsInput {
        expected_revision: replaced.revision,
        keyboard_bindings: vec![
            KeyboardBindingInput {
                command: DaraCommand::QuickAdd,
                accelerator: "control+alt+super+KeyX".into(),
            },
            KeyboardBindingInput {
                command: DaraCommand::Home,
                accelerator: "control+alt+super+KeyX".into(),
            },
        ],
    });
    assert!(matches!(duplicate, Err(DatabaseError::InvalidInput(_))));
    assert_eq!(
        database.load_settings().expect("bindings preserved"),
        replaced
    );
}

#[test]
fn zoom_validation_rejects_out_of_range_and_partial_steps() {
    let (_directory, database) = test_database();
    for zoom_percent in [40, 105, 210] {
        let current = database.load_settings().expect("current settings");
        let result = database.set_zoom_percent(SetZoomPercentInput {
            expected_revision: current.revision,
            zoom_percent,
        });
        assert!(matches!(result, Err(DatabaseError::InvalidInput(_))));
    }
}
