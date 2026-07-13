CREATE TABLE media_blob (
    sha256 BLOB PRIMARY KEY CHECK (length(sha256) = 32),
    bytes BLOB NOT NULL
) STRICT;

CREATE TRIGGER media_blob_prevent_update
BEFORE UPDATE ON media_blob
BEGIN
    SELECT RAISE(ABORT, 'media_blob rows are immutable');
END;

CREATE TRIGGER media_blob_prevent_delete
BEFORE DELETE ON media_blob
BEGIN
    SELECT RAISE(ABORT, 'media_blob rows are append-only');
END;
