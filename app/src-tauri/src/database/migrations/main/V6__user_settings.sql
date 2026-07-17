CREATE TABLE user_preferences (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    revision INTEGER NOT NULL CHECK (revision > 0),
    appearance TEXT NOT NULL CHECK (appearance IN ('SYSTEM', 'LIGHT', 'DARK')),
    zoom_percent INTEGER NOT NULL CHECK (
        zoom_percent BETWEEN 50 AND 200
        AND zoom_percent % 10 = 0
    ),
    legacy_zoom_migrated INTEGER NOT NULL CHECK (legacy_zoom_migrated IN (0, 1))
) STRICT;

INSERT INTO user_preferences (
    singleton_id,
    created_at,
    updated_at,
    revision,
    appearance,
    zoom_percent,
    legacy_zoom_migrated
) VALUES (1, 1783828800000, 1783828800000, 1, 'SYSTEM', 100, 0);

CREATE TABLE keyboard_binding (
    command TEXT PRIMARY KEY CHECK (command IN ('QUICK_ADD', 'REVIEW')),
    accelerator TEXT NOT NULL CHECK (length(accelerator) BETWEEN 3 AND 96)
) STRICT, WITHOUT ROWID;

INSERT INTO keyboard_binding (command, accelerator) VALUES
    ('QUICK_ADD', 'control+alt+super+KeyD'),
    ('REVIEW', 'control+alt+super+KeyR');

CREATE TRIGGER user_preferences_prevent_delete
BEFORE DELETE ON user_preferences
BEGIN
    SELECT RAISE(ABORT, 'user_preferences singleton cannot be deleted');
END;

CREATE TRIGGER keyboard_binding_prevent_delete
BEFORE DELETE ON keyboard_binding
BEGIN
    SELECT RAISE(ABORT, 'keyboard bindings cannot be deleted');
END;
