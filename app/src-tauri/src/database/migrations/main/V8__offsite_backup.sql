CREATE TABLE offsite_backup_config (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    revision INTEGER NOT NULL CHECK (revision > 0),
    backup_set_id TEXT NOT NULL UNIQUE
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
    replica_epoch_id TEXT NOT NULL
        CHECK (
            length(replica_epoch_id) = 36
            AND lower(replica_epoch_id) = replica_epoch_id
            AND substr(replica_epoch_id, 9, 1) = '-'
            AND substr(replica_epoch_id, 14, 1) = '-'
            AND substr(replica_epoch_id, 15, 1) = '7'
            AND substr(replica_epoch_id, 19, 1) = '-'
            AND substr(replica_epoch_id, 20, 1) GLOB '[89ab]'
            AND substr(replica_epoch_id, 24, 1) = '-'
            AND length(replace(replica_epoch_id, '-', '')) = 32
            AND replace(replica_epoch_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    provider TEXT NOT NULL CHECK (provider = 'R2'),
    jurisdiction TEXT NOT NULL
        CHECK (jurisdiction IN ('DEFAULT', 'EU', 'FEDRAMP')),
    account_id TEXT NOT NULL
        CHECK (
            length(account_id) = 32
            AND lower(account_id) = account_id
            AND account_id NOT GLOB '*[^0-9a-f]*'
        ),
    bucket TEXT NOT NULL
        CHECK (
            length(bucket) BETWEEN 3 AND 63
            AND lower(bucket) = bucket
            AND bucket NOT GLOB '*[^a-z0-9-]*'
            AND substr(bucket, 1, 1) GLOB '[a-z0-9]'
            AND substr(bucket, -1, 1) GLOB '[a-z0-9]'
        ),
    prefix TEXT NOT NULL
        CHECK (
            length(CAST(prefix AS BLOB)) BETWEEN 1 AND 512
            AND substr(prefix, 1, 1) <> '/'
            AND substr(prefix, -1, 1) <> '/'
            AND instr(prefix, '//') = 0
            AND instr(prefix, '..') = 0
            AND instr(prefix, char(92)) = 0
            AND instr(prefix, '?') = 0
            AND instr(prefix, '#') = 0
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at)
) STRICT;

CREATE TABLE offsite_media_object (
    backup_set_id TEXT NOT NULL
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
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    state TEXT NOT NULL
        CHECK (state IN ('PENDING', 'RETRY_WAIT', 'VERIFIED', 'BLOCKED')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_attempt_at INTEGER CHECK (next_attempt_at IS NULL OR next_attempt_at >= 0),
    last_attempt_at INTEGER CHECK (last_attempt_at IS NULL OR last_attempt_at >= 0),
    last_verified_at INTEGER CHECK (last_verified_at IS NULL OR last_verified_at >= 0),
    last_error_code TEXT
        CHECK (
            last_error_code IS NULL
            OR (
                length(last_error_code) BETWEEN 1 AND 64
                AND last_error_code NOT GLOB '*[^A-Z0-9_]*'
            )
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    PRIMARY KEY (backup_set_id, sha256),
    CHECK (state <> 'RETRY_WAIT' OR next_attempt_at IS NOT NULL),
    CHECK (state <> 'VERIFIED' OR last_verified_at IS NOT NULL)
) STRICT, WITHOUT ROWID;

CREATE INDEX offsite_media_object_work_idx
    ON offsite_media_object(backup_set_id, state, next_attempt_at, created_at);

CREATE TABLE offsite_backup_checkpoint (
    checkpoint_id TEXT PRIMARY KEY
        CHECK (
            length(checkpoint_id) = 36
            AND lower(checkpoint_id) = checkpoint_id
            AND substr(checkpoint_id, 9, 1) = '-'
            AND substr(checkpoint_id, 14, 1) = '-'
            AND substr(checkpoint_id, 15, 1) = '7'
            AND substr(checkpoint_id, 19, 1) = '-'
            AND substr(checkpoint_id, 20, 1) GLOB '[89ab]'
            AND substr(checkpoint_id, 24, 1) = '-'
            AND length(replace(checkpoint_id, '-', '')) = 32
            AND replace(checkpoint_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    backup_set_id TEXT NOT NULL
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
    replica_epoch_id TEXT NOT NULL
        CHECK (
            length(replica_epoch_id) = 36
            AND lower(replica_epoch_id) = replica_epoch_id
            AND substr(replica_epoch_id, 9, 1) = '-'
            AND substr(replica_epoch_id, 14, 1) = '-'
            AND substr(replica_epoch_id, 15, 1) = '7'
            AND substr(replica_epoch_id, 19, 1) = '-'
            AND substr(replica_epoch_id, 20, 1) GLOB '[89ab]'
            AND substr(replica_epoch_id, 24, 1) = '-'
            AND length(replace(replica_epoch_id, '-', '')) = 32
            AND replace(replica_epoch_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    phase TEXT NOT NULL
        CHECK (phase IN ('PREPARED', 'FENCED', 'REPLICATED', 'PUBLISHED', 'FAILED')),
    content_revision INTEGER NOT NULL CHECK (content_revision >= 0),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    dara_version TEXT NOT NULL CHECK (length(dara_version) BETWEEN 1 AND 64),
    main_migration_head INTEGER NOT NULL CHECK (main_migration_head > 0),
    media_migration_head INTEGER NOT NULL CHECK (media_migration_head > 0),
    referenced_hash_count INTEGER NOT NULL CHECK (referenced_hash_count >= 0),
    referenced_total_bytes INTEGER NOT NULL CHECK (referenced_total_bytes >= 0),
    referenced_hash_set_sha256 BLOB NOT NULL
        CHECK (length(referenced_hash_set_sha256) = 32),
    litestream_txid TEXT
        CHECK (
            litestream_txid IS NULL
            OR (
                length(litestream_txid) = 16
                AND lower(litestream_txid) = litestream_txid
                AND litestream_txid NOT GLOB '*[^0-9a-f]*'
            )
        ),
    manifest_object_key TEXT
        CHECK (
            manifest_object_key IS NULL
            OR length(CAST(manifest_object_key AS BLOB)) BETWEEN 1 AND 1024
        ),
    last_error_code TEXT
        CHECK (
            last_error_code IS NULL
            OR (
                length(last_error_code) BETWEEN 1 AND 64
                AND last_error_code NOT GLOB '*[^A-Z0-9_]*'
            )
        ),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    CHECK (
        phase IN ('PREPARED', 'FAILED')
        OR litestream_txid IS NOT NULL
    ),
    CHECK (
        phase <> 'PUBLISHED'
        OR (litestream_txid IS NOT NULL AND manifest_object_key IS NOT NULL)
    )
) STRICT;

CREATE INDEX offsite_backup_checkpoint_set_created_idx
    ON offsite_backup_checkpoint(backup_set_id, created_at DESC);

CREATE TABLE offsite_backup_content_clock (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT;

INSERT INTO offsite_backup_content_clock(singleton_id, revision) VALUES (1, 0);

CREATE TRIGGER offsite_backup_content_clock_prevent_delete
BEFORE DELETE ON offsite_backup_content_clock
BEGIN
    SELECT RAISE(ABORT, 'offsite backup content clock cannot be deleted');
END;
