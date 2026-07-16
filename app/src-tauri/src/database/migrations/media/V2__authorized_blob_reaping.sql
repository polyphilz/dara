DROP TRIGGER media_blob_prevent_delete;

CREATE TABLE media_blob_reap_authorization (
    sha256 BLOB PRIMARY KEY CHECK (length(sha256) = 32)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER media_blob_prevent_unauthorized_delete
BEFORE DELETE ON media_blob
WHEN NOT EXISTS (
    SELECT 1 FROM media_blob_reap_authorization
    WHERE sha256 = old.sha256
)
BEGIN
    SELECT RAISE(ABORT, 'media_blob deletion requires reaper authorization');
END;

CREATE TRIGGER media_blob_clear_reap_authorization
AFTER DELETE ON media_blob
BEGIN
    DELETE FROM media_blob_reap_authorization WHERE sha256 = old.sha256;
END;

CREATE TRIGGER media_blob_reap_authorization_prevent_update
BEFORE UPDATE ON media_blob_reap_authorization
BEGIN
    SELECT RAISE(ABORT, 'media blob reaper authorizations are immutable');
END;
