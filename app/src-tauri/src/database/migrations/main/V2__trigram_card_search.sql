DROP TRIGGER search_document_fts_after_update;
DROP TRIGGER search_document_fts_after_delete;
DROP TRIGGER search_document_fts_after_insert;
DROP TABLE search_document_fts;

CREATE VIRTUAL TABLE search_document_fts USING fts5(
    body,
    content = 'search_document',
    content_rowid = 'rowid',
    tokenize = 'trigram'
);

CREATE TRIGGER search_document_fts_after_insert
AFTER INSERT ON search_document
BEGIN
    INSERT INTO search_document_fts(rowid, body)
    VALUES (new.rowid, new.body);
END;

CREATE TRIGGER search_document_fts_after_delete
AFTER DELETE ON search_document
BEGIN
    INSERT INTO search_document_fts(search_document_fts, rowid, body)
    VALUES ('delete', old.rowid, old.body);
END;

CREATE TRIGGER search_document_fts_after_update
AFTER UPDATE OF rowid, body ON search_document
BEGIN
    INSERT INTO search_document_fts(search_document_fts, rowid, body)
    VALUES ('delete', old.rowid, old.body);
    INSERT INTO search_document_fts(rowid, body)
    VALUES (new.rowid, new.body);
END;

INSERT INTO search_document_fts(search_document_fts) VALUES ('rebuild');
