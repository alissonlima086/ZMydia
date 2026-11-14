CREATE TABLE thumbnails (
    media_id INTEGER PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
    cache_key TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'ready', 'error')),
    error_message TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
