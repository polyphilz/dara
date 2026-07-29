ALTER TABLE offsite_backup_config
ADD COLUMN restored_takeover_required INTEGER NOT NULL DEFAULT 0
    CHECK (restored_takeover_required IN (0, 1));
