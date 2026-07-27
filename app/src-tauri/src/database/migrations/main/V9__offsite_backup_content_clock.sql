-- The clock is deliberately driven by schema triggers instead of follow-up writer calls.
-- That keeps each increment in the same SQLite transaction as the recoverable change.
-- Backup bookkeeping tables are intentionally absent so publishing a checkpoint cannot
-- schedule another checkpoint by itself.

ALTER TABLE offsite_backup_checkpoint
ADD COLUMN config_revision INTEGER NOT NULL DEFAULT 0 CHECK (config_revision >= 0);

CREATE TRIGGER scheduler_config_backup_clock_after_insert
AFTER INSERT ON scheduler_config
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_content_backup_clock_after_insert
AFTER INSERT ON card_content
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_content_backup_clock_after_update
AFTER UPDATE ON card_content
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER image_backup_clock_after_insert
AFTER INSERT ON image
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER image_backup_clock_after_update
AFTER UPDATE ON image
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_content_image_backup_clock_after_insert
AFTER INSERT ON card_content_image
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_content_image_backup_clock_after_delete
AFTER DELETE ON card_content_image
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_occlusion_content_backup_clock_after_insert
AFTER INSERT ON card_occlusion_content
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_occlusion_content_backup_clock_after_update
AFTER UPDATE ON card_occlusion_content
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_occlusion_mask_layer_backup_clock_after_insert
AFTER INSERT ON card_occlusion_mask_layer
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_occlusion_mask_layer_backup_clock_after_update
AFTER UPDATE ON card_occlusion_mask_layer
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_occlusion_mask_backup_clock_after_insert
AFTER INSERT ON card_occlusion_mask
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER card_occlusion_mask_backup_clock_after_update
AFTER UPDATE ON card_occlusion_mask
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER review_card_backup_clock_after_insert
AFTER INSERT ON review_card
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER review_card_backup_clock_after_update
AFTER UPDATE ON review_card
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER review_event_backup_clock_after_insert
AFTER INSERT ON review_event
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER search_document_backup_clock_after_insert
AFTER INSERT ON search_document
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER search_document_backup_clock_after_update
AFTER UPDATE ON search_document
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER search_document_backup_clock_after_delete
AFTER DELETE ON search_document
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER text_embedding_index_backup_clock_after_insert
AFTER INSERT ON text_embedding_index
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER text_embedding_backup_clock_after_insert
AFTER INSERT ON text_embedding
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER text_embedding_backup_clock_after_delete
AFTER DELETE ON text_embedding
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER app_settings_backup_clock_after_update
AFTER UPDATE ON app_settings
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER image_draft_lease_backup_clock_after_insert
AFTER INSERT ON image_draft_lease
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER image_draft_lease_backup_clock_after_update
AFTER UPDATE ON image_draft_lease
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER image_draft_lease_backup_clock_after_delete
AFTER DELETE ON image_draft_lease
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER media_blob_reap_candidate_backup_clock_after_insert
AFTER INSERT ON media_blob_reap_candidate
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER media_blob_reap_candidate_backup_clock_after_update
AFTER UPDATE ON media_blob_reap_candidate
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER media_blob_reap_candidate_backup_clock_after_delete
AFTER DELETE ON media_blob_reap_candidate
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER user_preferences_backup_clock_after_update
AFTER UPDATE ON user_preferences
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER keyboard_binding_backup_clock_after_insert
AFTER INSERT ON keyboard_binding
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER keyboard_binding_backup_clock_after_update
AFTER UPDATE ON keyboard_binding
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;

CREATE TRIGGER keyboard_binding_backup_clock_after_delete
AFTER DELETE ON keyboard_binding
BEGIN
    UPDATE offsite_backup_content_clock SET revision = revision + 1 WHERE singleton_id = 1;
END;
