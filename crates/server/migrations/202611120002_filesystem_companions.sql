CREATE TABLE filesystem_operation_companions (
    id INTEGER PRIMARY KEY,
    operation_id INTEGER NOT NULL REFERENCES filesystem_operations(id) ON DELETE CASCADE,
    subtitle_id INTEGER REFERENCES media_subtitles(id) ON DELETE SET NULL,
    from_path TEXT NOT NULL,
    to_path TEXT NOT NULL,
    checksum_sha256 TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'planned'
        CHECK (state IN ('planned', 'moved', 'verified', 'done', 'undone', 'failed')),
    error_message TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(operation_id, from_path)
) STRICT;
