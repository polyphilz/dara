ALTER TABLE offsite_backup_config
ADD COLUMN takeover_available INTEGER NOT NULL DEFAULT 0
    CHECK (takeover_available IN (0, 1));
