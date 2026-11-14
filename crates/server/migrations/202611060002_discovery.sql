ALTER TABLE media_sources ADD COLUMN removed_at TEXT;

CREATE TABLE media_files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    extension TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    modified_at_unix INTEGER NOT NULL,
    is_available INTEGER NOT NULL DEFAULT 1 CHECK (is_available IN (0, 1)),
    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE source_files (
    source_id INTEGER NOT NULL REFERENCES media_sources(id) ON DELETE CASCADE,
    media_file_id INTEGER NOT NULL REFERENCES media_files(id) ON DELETE CASCADE,
    is_available INTEGER NOT NULL DEFAULT 1 CHECK (is_available IN (0, 1)),
    last_seen_scan_id INTEGER REFERENCES scan_jobs(id) ON DELETE SET NULL,
    PRIMARY KEY (source_id, media_file_id)
) STRICT;

CREATE INDEX source_files_media_idx ON source_files(media_file_id, is_available);
CREATE INDEX media_files_available_idx ON media_files(is_available, id);
