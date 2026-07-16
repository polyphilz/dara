ALTER TABLE image ADD COLUMN ocr_status TEXT NOT NULL DEFAULT 'PENDING'
    CHECK (ocr_status IN ('PENDING', 'READY', 'FAILED'));

ALTER TABLE image ADD COLUMN ocr_error TEXT;

UPDATE image
SET ocr_status = CASE
    WHEN length(ocr_text) > 0 THEN 'READY'
    ELSE 'PENDING'
END;

CREATE INDEX image_pending_ocr_idx
    ON image(created_at, id)
    WHERE deleted_at IS NULL AND ocr_status = 'PENDING';
