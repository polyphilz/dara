DROP TRIGGER keyboard_binding_prevent_delete;

ALTER TABLE keyboard_binding RENAME TO keyboard_binding_legacy;

CREATE TABLE keyboard_binding (
    command TEXT PRIMARY KEY CHECK (command IN ('QUICK_ADD', 'HOME')),
    accelerator TEXT NOT NULL CHECK (length(accelerator) BETWEEN 3 AND 96)
) STRICT, WITHOUT ROWID;

INSERT INTO keyboard_binding (command, accelerator)
SELECT
    CASE command
        WHEN 'REVIEW' THEN 'HOME'
        ELSE command
    END,
    CASE
        WHEN command = 'REVIEW'
            AND accelerator = 'control+alt+super+KeyR'
            THEN 'control+alt+super+KeyH'
        ELSE accelerator
    END
FROM keyboard_binding_legacy;

DROP TABLE keyboard_binding_legacy;

CREATE TRIGGER keyboard_binding_prevent_delete
BEFORE DELETE ON keyboard_binding
BEGIN
    SELECT RAISE(ABORT, 'keyboard bindings cannot be deleted');
END;
