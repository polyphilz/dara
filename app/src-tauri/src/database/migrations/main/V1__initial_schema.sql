CREATE TABLE scheduler_config (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    algorithm TEXT NOT NULL CHECK (length(algorithm) > 0),
    algorithm_version INTEGER NOT NULL CHECK (algorithm_version > 0),
    scheduler_library TEXT NOT NULL CHECK (length(scheduler_library) > 0),
    library_version TEXT NOT NULL CHECK (length(library_version) > 0),
    config_schema_version INTEGER NOT NULL CHECK (config_schema_version > 0),
    config_json TEXT NOT NULL CHECK (json_valid(config_json))
) STRICT;

INSERT INTO scheduler_config (
    id,
    created_at,
    algorithm,
    algorithm_version,
    scheduler_library,
    library_version,
    config_schema_version,
    config_json
) VALUES (
    '019f547b-6200-7000-8000-000000000001',
    1783828800000,
    'FSRS',
    6,
    'ts-fsrs',
    '5.4.1',
    1,
    '{"parameters":[0.212,1.2931,2.3065,8.2956,6.4133,0.8334,3.0194,0.001,1.8722,0.1666,0.796,1.4835,0.0614,0.2629,1.6483,0.6014,1.8729,0.5425,0.0912,0.0658,0.1542],"desiredRetention":0.9,"maximumInterval":36500,"learningSteps":["10m"],"relearningSteps":["10m"],"fuzzEnabled":true,"fuzzStrategyVersion":1}'
);

CREATE TABLE card_content (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    type TEXT NOT NULL CHECK (type IN ('BASIC', 'CLOZE', 'OCCLUSION')),
    front_md TEXT NOT NULL,
    back_md TEXT NOT NULL,
    source TEXT,
    CHECK (
        (type = 'BASIC' AND length(front_md) > 0 AND length(back_md) > 0)
        OR (type = 'CLOZE' AND length(front_md) > 0)
        OR type = 'OCCLUSION'
    )
) STRICT;

CREATE TABLE image (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    sha256 BLOB NOT NULL UNIQUE CHECK (length(sha256) = 32),
    mime_type TEXT NOT NULL CHECK (length(mime_type) > 0),
    natural_width INTEGER NOT NULL CHECK (natural_width > 0),
    natural_height INTEGER NOT NULL CHECK (natural_height > 0),
    ocr_text TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE TABLE card_content_image (
    card_content_id TEXT NOT NULL,
    image_id TEXT NOT NULL,
    PRIMARY KEY (card_content_id, image_id),
    FOREIGN KEY (card_content_id) REFERENCES card_content(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (image_id) REFERENCES image(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;

CREATE TABLE card_occlusion_content (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    card_content_id TEXT NOT NULL,
    source_image_id TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('HIDE_ONE_GUESS_ONE', 'HIDE_ALL_GUESS_ONE')),
    FOREIGN KEY (card_content_id) REFERENCES card_content(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (source_image_id) REFERENCES image(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE UNIQUE INDEX card_occlusion_content_active_content_uq
    ON card_occlusion_content(card_content_id)
    WHERE deleted_at IS NULL;

CREATE INDEX card_occlusion_content_source_image_idx
    ON card_occlusion_content(source_image_id)
    WHERE deleted_at IS NULL;

CREATE TABLE card_occlusion_mask_layer (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    card_occlusion_content_id TEXT NOT NULL,
    label TEXT,
    sort_order INTEGER NOT NULL CHECK (sort_order >= 0),
    FOREIGN KEY (card_occlusion_content_id) REFERENCES card_occlusion_content(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE UNIQUE INDEX card_occlusion_mask_layer_active_sort_uq
    ON card_occlusion_mask_layer(card_occlusion_content_id, sort_order)
    WHERE deleted_at IS NULL;

CREATE INDEX card_occlusion_mask_layer_content_idx
    ON card_occlusion_mask_layer(card_occlusion_content_id)
    WHERE deleted_at IS NULL;

CREATE TABLE card_occlusion_mask (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    card_occlusion_mask_layer_id TEXT NOT NULL,
    x REAL NOT NULL CHECK (x >= 0.0 AND x < 1.0),
    y REAL NOT NULL CHECK (y >= 0.0 AND y < 1.0),
    width REAL NOT NULL CHECK (width > 0.0 AND width <= 1.0),
    height REAL NOT NULL CHECK (height > 0.0 AND height <= 1.0),
    color TEXT NOT NULL CHECK (color IN ('WHITE', 'BLACK')),
    CHECK (x + width <= 1.0),
    CHECK (y + height <= 1.0),
    FOREIGN KEY (card_occlusion_mask_layer_id) REFERENCES card_occlusion_mask_layer(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE INDEX card_occlusion_mask_layer_idx
    ON card_occlusion_mask(card_occlusion_mask_layer_id)
    WHERE deleted_at IS NULL;

CREATE TABLE review_card (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    deleted_at INTEGER CHECK (deleted_at IS NULL OR deleted_at >= created_at),
    card_content_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('ACTIVE', 'SUSPENDED')),
    suspended_at INTEGER CHECK (suspended_at IS NULL OR suspended_at >= created_at),
    variant_key TEXT NOT NULL CHECK (length(variant_key) > 0),
    state TEXT NOT NULL CHECK (state IN ('NEW', 'LEARNING', 'REVIEW', 'RELEARNING')),
    due_at INTEGER,
    due_study_day INTEGER,
    last_review_at INTEGER,
    reps INTEGER NOT NULL CHECK (reps >= 0),
    lapses INTEGER NOT NULL CHECK (lapses >= 0 AND lapses <= reps),
    scheduler_config_id TEXT,
    scheduler_state_schema_version INTEGER CHECK (
        scheduler_state_schema_version IS NULL OR scheduler_state_schema_version > 0
    ),
    scheduler_state_json TEXT CHECK (
        scheduler_state_json IS NULL OR json_valid(scheduler_state_json)
    ),
    CHECK (
        (status = 'ACTIVE' AND suspended_at IS NULL)
        OR (status = 'SUSPENDED' AND suspended_at IS NOT NULL)
    ),
    CHECK (
        (
            state = 'NEW'
            AND due_at IS NULL
            AND due_study_day IS NULL
            AND last_review_at IS NULL
            AND reps = 0
            AND lapses = 0
            AND scheduler_config_id IS NULL
            AND scheduler_state_schema_version IS NULL
            AND scheduler_state_json IS NULL
        )
        OR (
            state IN ('LEARNING', 'RELEARNING')
            AND due_at IS NOT NULL
            AND due_study_day IS NULL
            AND last_review_at IS NOT NULL
            AND reps > 0
            AND scheduler_config_id IS NOT NULL
            AND scheduler_state_schema_version IS NOT NULL
            AND scheduler_state_json IS NOT NULL
        )
        OR (
            state = 'REVIEW'
            AND due_at IS NULL
            AND due_study_day IS NOT NULL
            AND last_review_at IS NOT NULL
            AND reps > 0
            AND scheduler_config_id IS NOT NULL
            AND scheduler_state_schema_version IS NOT NULL
            AND scheduler_state_json IS NOT NULL
        )
    ),
    FOREIGN KEY (card_content_id) REFERENCES card_content(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (scheduler_config_id) REFERENCES scheduler_config(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE UNIQUE INDEX review_card_active_variant_uq
    ON review_card(card_content_id, variant_key)
    WHERE deleted_at IS NULL;

CREATE INDEX review_card_intraday_queue_idx
    ON review_card(due_at, id)
    WHERE deleted_at IS NULL
      AND status = 'ACTIVE'
      AND state IN ('LEARNING', 'RELEARNING');

CREATE INDEX review_card_review_queue_idx
    ON review_card(due_study_day, id)
    WHERE deleted_at IS NULL
      AND status = 'ACTIVE'
      AND state = 'REVIEW';

CREATE INDEX review_card_new_queue_idx
    ON review_card(created_at, id)
    WHERE deleted_at IS NULL
      AND status = 'ACTIVE'
      AND state = 'NEW';

CREATE TABLE review_event (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    event_schema_version INTEGER NOT NULL CHECK (event_schema_version > 0),
    event_type TEXT NOT NULL CHECK (event_type IN ('REVIEW', 'REVOKE')),
    review_card_id TEXT NOT NULL,
    card_sequence INTEGER NOT NULL CHECK (card_sequence > 0),
    reviewed_at INTEGER,
    study_day INTEGER,
    timezone_id TEXT,
    utc_offset_minutes INTEGER,
    grade INTEGER,
    scheduler_config_id TEXT NOT NULL,
    scheduler_log_json TEXT CHECK (
        scheduler_log_json IS NULL OR json_valid(scheduler_log_json)
    ),
    target_event_id TEXT,
    CHECK (
        (
            event_type = 'REVIEW'
            AND reviewed_at IS NOT NULL
            AND study_day IS NOT NULL
            AND timezone_id IS NOT NULL
            AND length(timezone_id) > 0
            AND utc_offset_minutes BETWEEN -840 AND 840
            AND grade BETWEEN 1 AND 4
            AND scheduler_log_json IS NOT NULL
            AND target_event_id IS NULL
        )
        OR (
            event_type = 'REVOKE'
            AND reviewed_at IS NULL
            AND study_day IS NULL
            AND timezone_id IS NULL
            AND utc_offset_minutes IS NULL
            AND grade IS NULL
            AND scheduler_log_json IS NULL
            AND target_event_id IS NOT NULL
        )
    ),
    UNIQUE (review_card_id, card_sequence),
    FOREIGN KEY (review_card_id) REFERENCES review_card(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (scheduler_config_id) REFERENCES scheduler_config(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (target_event_id) REFERENCES review_event(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE UNIQUE INDEX review_event_single_revoke_uq
    ON review_event(target_event_id)
    WHERE event_type = 'REVOKE';

CREATE INDEX review_event_card_sequence_idx
    ON review_event(review_card_id, card_sequence);

CREATE INDEX review_event_reviewed_at_idx
    ON review_event(reviewed_at)
    WHERE event_type = 'REVIEW';

CREATE INDEX review_event_study_day_idx
    ON review_event(study_day)
    WHERE event_type = 'REVIEW';

CREATE TABLE search_document (
    rowid INTEGER PRIMARY KEY,
    card_content_id TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL,
    content_hash BLOB NOT NULL CHECK (length(content_hash) = 32),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    FOREIGN KEY (card_content_id) REFERENCES card_content(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE VIRTUAL TABLE search_document_fts USING fts5(
    body,
    content = 'search_document',
    content_rowid = 'rowid',
    tokenize = 'unicode61'
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

CREATE TABLE text_embedding_index (
    id TEXT PRIMARY KEY
        CHECK (
            length(id) = 36
            AND lower(id) = id
            AND substr(id, 9, 1) = '-'
            AND substr(id, 14, 1) = '-'
            AND substr(id, 15, 1) = '7'
            AND substr(id, 19, 1) = '-'
            AND substr(id, 20, 1) GLOB '[89ab]'
            AND substr(id, 24, 1) = '-'
            AND length(replace(id, '-', '')) = 32
            AND replace(id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    index_key TEXT NOT NULL UNIQUE CHECK (length(index_key) > 0),
    model_name TEXT NOT NULL CHECK (length(model_name) > 0),
    model_revision TEXT NOT NULL CHECK (length(model_revision) > 0),
    model_file_sha256 BLOB NOT NULL CHECK (length(model_file_sha256) = 32),
    dimension INTEGER NOT NULL CHECK (dimension > 0),
    distance_metric TEXT NOT NULL CHECK (distance_metric = 'COSINE'),
    normalized INTEGER NOT NULL CHECK (normalized IN (0, 1)),
    index_schema_version INTEGER NOT NULL CHECK (index_schema_version > 0),
    config_json TEXT NOT NULL CHECK (json_valid(config_json))
) STRICT;

INSERT INTO text_embedding_index (
    id,
    created_at,
    index_key,
    model_name,
    model_revision,
    model_file_sha256,
    dimension,
    distance_metric,
    normalized,
    index_schema_version,
    config_json
) VALUES (
    '019f547b-6200-7000-8000-000000000002',
    1783828800000,
    'jina_v1',
    'jinaai/jina-embeddings-v5-text-nano-retrieval-GGUF',
    '59cfaceeeb7d738c404659435af4c0da74d06c96',
    X'86b6e6279e9b9e71389f02a082764a2ac2b15a50e37482c26f98d69092f12442',
    768,
    'COSINE',
    1,
    1,
    '{"schemaVersion":1,"modelFile":"v5-nano-retrieval-Q8_0.gguf","modelFileSize":232883776,"quantization":"Q8_0","pooling":"last","normalization":"L2","queryPrefix":"Query: ","documentPrefix":"Document: ","documentConstructionVersion":1}'
);

CREATE VIRTUAL TABLE text_embedding_vec_jina_v1 USING vec0(
    embedding float[768] distance_metric=cosine
);

CREATE TABLE text_embedding (
    search_document_id INTEGER NOT NULL,
    text_embedding_index_id TEXT NOT NULL,
    content_hash BLOB NOT NULL CHECK (length(content_hash) = 32),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    PRIMARY KEY (text_embedding_index_id, search_document_id),
    FOREIGN KEY (search_document_id) REFERENCES search_document(rowid)
        ON UPDATE RESTRICT ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (text_embedding_index_id) REFERENCES text_embedding_index(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT, WITHOUT ROWID;

CREATE TABLE app_settings (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
    active_scheduler_config_id TEXT NOT NULL,
    active_text_embedding_index_id TEXT,
    FOREIGN KEY (active_scheduler_config_id) REFERENCES scheduler_config(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (active_text_embedding_index_id) REFERENCES text_embedding_index(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED
) STRICT;

INSERT INTO app_settings (
    singleton_id,
    created_at,
    updated_at,
    active_scheduler_config_id,
    active_text_embedding_index_id
) VALUES (
    1,
    1783828800000,
    1783828800000,
    '019f547b-6200-7000-8000-000000000001',
    NULL
);

CREATE TRIGGER scheduler_config_prevent_update
BEFORE UPDATE ON scheduler_config
BEGIN
    SELECT RAISE(ABORT, 'scheduler_config rows are immutable');
END;

CREATE TRIGGER app_settings_prevent_delete
BEFORE DELETE ON app_settings
BEGIN
    SELECT RAISE(ABORT, 'app_settings singleton cannot be deleted');
END;

CREATE TRIGGER scheduler_config_prevent_delete
BEFORE DELETE ON scheduler_config
BEGIN
    SELECT RAISE(ABORT, 'scheduler_config rows are immutable');
END;

CREATE TRIGGER review_event_prevent_update
BEFORE UPDATE ON review_event
BEGIN
    SELECT RAISE(ABORT, 'review_event rows are append-only');
END;

CREATE TRIGGER review_event_prevent_delete
BEFORE DELETE ON review_event
BEGIN
    SELECT RAISE(ABORT, 'review_event rows are append-only');
END;

CREATE TRIGGER text_embedding_index_prevent_update
BEFORE UPDATE ON text_embedding_index
BEGIN
    SELECT RAISE(ABORT, 'text_embedding_index rows are immutable');
END;

CREATE TRIGGER text_embedding_index_prevent_delete
BEFORE DELETE ON text_embedding_index
BEGIN
    SELECT RAISE(ABORT, 'text_embedding_index rows are immutable');
END;

CREATE TRIGGER card_content_prevent_delete
BEFORE DELETE ON card_content
BEGIN
    SELECT RAISE(ABORT, 'card_content uses tombstones');
END;

CREATE TRIGGER image_prevent_delete
BEFORE DELETE ON image
BEGIN
    SELECT RAISE(ABORT, 'image uses tombstones');
END;

CREATE TRIGGER card_occlusion_content_prevent_delete
BEFORE DELETE ON card_occlusion_content
BEGIN
    SELECT RAISE(ABORT, 'card_occlusion_content uses tombstones');
END;

CREATE TRIGGER card_occlusion_mask_layer_prevent_delete
BEFORE DELETE ON card_occlusion_mask_layer
BEGIN
    SELECT RAISE(ABORT, 'card_occlusion_mask_layer uses tombstones');
END;

CREATE TRIGGER card_occlusion_mask_prevent_delete
BEFORE DELETE ON card_occlusion_mask
BEGIN
    SELECT RAISE(ABORT, 'card_occlusion_mask uses tombstones');
END;

CREATE TRIGGER review_card_prevent_delete
BEFORE DELETE ON review_card
BEGIN
    SELECT RAISE(ABORT, 'review_card uses tombstones');
END;
