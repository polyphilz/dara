ALTER TABLE image ADD COLUMN orphaned_at INTEGER
    CHECK (orphaned_at IS NULL OR orphaned_at >= created_at);

CREATE TABLE image_draft_lease (
    lease_id TEXT NOT NULL
        CHECK (
            length(lease_id) = 36
            AND lower(lease_id) = lease_id
            AND substr(lease_id, 9, 1) = '-'
            AND substr(lease_id, 14, 1) = '-'
            AND substr(lease_id, 15, 1) = '7'
            AND substr(lease_id, 19, 1) = '-'
            AND substr(lease_id, 20, 1) GLOB '[89ab]'
            AND substr(lease_id, 24, 1) = '-'
            AND length(replace(lease_id, '-', '')) = 32
            AND replace(lease_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    image_id TEXT NOT NULL,
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    expires_at INTEGER NOT NULL CHECK (expires_at >= updated_at),
    PRIMARY KEY (lease_id, image_id),
    FOREIGN KEY (image_id) REFERENCES image(id)
        ON UPDATE RESTRICT ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;

CREATE INDEX image_draft_lease_expiry_idx
    ON image_draft_lease(expires_at, image_id);

CREATE TABLE media_blob_reap_candidate (
    sha256 BLOB PRIMARY KEY CHECK (length(sha256) = 32),
    orphaned_at INTEGER NOT NULL CHECK (orphaned_at >= 0)
) STRICT, WITHOUT ROWID;

CREATE INDEX image_orphaned_idx
    ON image(orphaned_at, id)
    WHERE orphaned_at IS NOT NULL;

DROP TRIGGER image_prevent_delete;

CREATE TRIGGER image_prevent_live_delete
BEFORE DELETE ON image
WHEN old.orphaned_at IS NULL
BEGIN
    SELECT RAISE(ABORT, 'only durably orphaned images may be deleted');
END;
