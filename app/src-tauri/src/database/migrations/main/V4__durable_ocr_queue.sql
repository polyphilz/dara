ALTER TABLE image ADD COLUMN ocr_queue_state TEXT NOT NULL DEFAULT 'PENDING'
    CHECK (ocr_queue_state IN ('PENDING', 'RUNNING', 'RETRY_WAIT', 'READY', 'FAILED'));
ALTER TABLE image ADD COLUMN ocr_attempt_count INTEGER NOT NULL DEFAULT 0
    CHECK (ocr_attempt_count >= 0);
ALTER TABLE image ADD COLUMN ocr_next_attempt_at INTEGER
    CHECK (ocr_next_attempt_at IS NULL OR ocr_next_attempt_at >= 0);
ALTER TABLE image ADD COLUMN ocr_started_at INTEGER
    CHECK (ocr_started_at IS NULL OR ocr_started_at >= 0);

UPDATE image
SET ocr_queue_state = ocr_status,
    ocr_next_attempt_at = CASE WHEN ocr_status = 'PENDING' THEN updated_at ELSE NULL END;

DROP INDEX image_pending_ocr_idx;

CREATE INDEX image_ocr_eligible_idx
    ON image(ocr_next_attempt_at, created_at, id)
    WHERE deleted_at IS NULL AND ocr_queue_state IN ('PENDING', 'RETRY_WAIT');

CREATE INDEX image_ocr_running_idx
    ON image(ocr_started_at, id)
    WHERE deleted_at IS NULL AND ocr_queue_state = 'RUNNING';

CREATE TRIGGER image_ocr_state_after_insert
AFTER INSERT ON image
WHEN NOT (
    (
        new.ocr_status = 'PENDING'
        AND new.ocr_queue_state = 'PENDING'
        AND new.ocr_attempt_count = 0
        AND new.ocr_next_attempt_at IS NOT NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NULL
        AND new.ocr_text = ''
    )
    OR (
        new.ocr_status = 'PENDING'
        AND new.ocr_queue_state = 'RUNNING'
        AND new.ocr_attempt_count > 0
        AND new.ocr_next_attempt_at IS NULL
        AND new.ocr_started_at IS NOT NULL
        AND new.ocr_text = ''
    )
    OR (
        new.ocr_status = 'PENDING'
        AND new.ocr_queue_state = 'RETRY_WAIT'
        AND new.ocr_attempt_count > 0
        AND new.ocr_next_attempt_at IS NOT NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NOT NULL
        AND new.ocr_text = ''
    )
    OR (
        new.ocr_status = 'READY'
        AND new.ocr_queue_state = 'READY'
        AND new.ocr_next_attempt_at IS NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NULL
    )
    OR (
        new.ocr_status = 'FAILED'
        AND new.ocr_queue_state = 'FAILED'
        AND new.ocr_attempt_count > 0
        AND new.ocr_next_attempt_at IS NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NOT NULL
        AND new.ocr_text = ''
    )
)
BEGIN
    SELECT RAISE(ABORT, 'invalid image OCR state');
END;

CREATE TRIGGER image_ocr_state_after_update
AFTER UPDATE OF ocr_status, ocr_queue_state, ocr_error, ocr_attempt_count,
    ocr_next_attempt_at, ocr_started_at, ocr_text
ON image
WHEN NOT (
    (
        new.ocr_status = 'PENDING'
        AND new.ocr_queue_state = 'PENDING'
        AND new.ocr_attempt_count = 0
        AND new.ocr_next_attempt_at IS NOT NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NULL
        AND new.ocr_text = ''
    )
    OR (
        new.ocr_status = 'PENDING'
        AND new.ocr_queue_state = 'RUNNING'
        AND new.ocr_attempt_count > 0
        AND new.ocr_next_attempt_at IS NULL
        AND new.ocr_started_at IS NOT NULL
        AND new.ocr_text = ''
    )
    OR (
        new.ocr_status = 'PENDING'
        AND new.ocr_queue_state = 'RETRY_WAIT'
        AND new.ocr_attempt_count > 0
        AND new.ocr_next_attempt_at IS NOT NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NOT NULL
        AND new.ocr_text = ''
    )
    OR (
        new.ocr_status = 'READY'
        AND new.ocr_queue_state = 'READY'
        AND new.ocr_next_attempt_at IS NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NULL
    )
    OR (
        new.ocr_status = 'FAILED'
        AND new.ocr_queue_state = 'FAILED'
        AND new.ocr_attempt_count > 0
        AND new.ocr_next_attempt_at IS NULL
        AND new.ocr_started_at IS NULL
        AND new.ocr_error IS NOT NULL
        AND new.ocr_text = ''
    )
)
BEGIN
    SELECT RAISE(ABORT, 'invalid image OCR state');
END;
