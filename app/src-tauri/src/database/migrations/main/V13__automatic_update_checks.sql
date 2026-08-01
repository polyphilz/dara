ALTER TABLE user_preferences
ADD COLUMN automatic_update_checks_enabled INTEGER NOT NULL DEFAULT 1
CHECK (automatic_update_checks_enabled IN (0, 1));
