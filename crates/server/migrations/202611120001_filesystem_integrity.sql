ALTER TABLE media_sources ADD COLUMN mode TEXT NOT NULL DEFAULT 'read_only'
    CHECK (mode IN ('read_only', 'managed'));

CREATE TABLE filesystem_operations (
    id INTEGER PRIMARY KEY,
    operation_type TEXT NOT NULL CHECK (operation_type IN ('organize', 'deduplicate', 'restore', 'discard')),
    source_id INTEGER NOT NULL REFERENCES media_sources(id) ON DELETE RESTRICT,
    media_file_id INTEGER REFERENCES media_files(id) ON DELETE SET NULL,
    from_path TEXT NOT NULL,
    to_path TEXT NOT NULL,
    quarantine_path TEXT,
    state TEXT NOT NULL DEFAULT 'planned'
        CHECK (state IN ('planned', 'copied', 'verified', 'database_committed', 'quarantined', 'done', 'undone', 'failed')),
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    checksum_sha256 TEXT,
    error_message TEXT,
    retention_until TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    completed_at TEXT
) STRICT;

CREATE INDEX filesystem_operations_state_idx
    ON filesystem_operations(state, created_at);
CREATE INDEX filesystem_operations_source_idx
    ON filesystem_operations(source_id, created_at DESC);

CREATE TABLE duplicate_candidates (
    id INTEGER PRIMARY KEY,
    winner_media_file_id INTEGER NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    duplicate_media_file_id INTEGER NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    score INTEGER NOT NULL,
    reason_json TEXT NOT NULL CHECK (json_valid(reason_json)),
    winner_checksum_sha256 TEXT,
    duplicate_checksum_sha256 TEXT,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'rejected', 'quarantined', 'restored', 'discarded')),
    decided_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(winner_media_file_id, duplicate_media_file_id)
) STRICT;

CREATE INDEX duplicate_candidates_status_idx
    ON duplicate_candidates(status, score DESC);
