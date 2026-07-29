CREATE TABLE offsite_credential_cleanup (
    backup_set_id TEXT PRIMARY KEY
        CHECK (
            length(backup_set_id) = 36
            AND lower(backup_set_id) = backup_set_id
            AND substr(backup_set_id, 9, 1) = '-'
            AND substr(backup_set_id, 14, 1) = '-'
            AND substr(backup_set_id, 15, 1) = '7'
            AND substr(backup_set_id, 19, 1) = '-'
            AND substr(backup_set_id, 20, 1) GLOB '[89ab]'
            AND substr(backup_set_id, 24, 1) = '-'
            AND length(replace(backup_set_id, '-', '')) = 32
            AND replace(backup_set_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT, WITHOUT ROWID;
