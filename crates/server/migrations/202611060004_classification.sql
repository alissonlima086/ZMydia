CREATE TABLE channels (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    has_name_conflict INTEGER NOT NULL DEFAULT 0 CHECK (has_name_conflict IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    normalized_name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL
) STRICT;

CREATE TABLE media_channels (
    media_id INTEGER PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE RESTRICT,
    origin_path TEXT NOT NULL
) STRICT;

CREATE TABLE media_tags (
    media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    origin_path TEXT NOT NULL,
    PRIMARY KEY (media_id, tag_id)
) STRICT;

CREATE TABLE folder_metadata (
    source_id INTEGER NOT NULL REFERENCES media_sources(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    is_valid INTEGER NOT NULL CHECK (is_valid IN (0, 1)),
    last_valid_json TEXT,
    error_message TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (source_id, path)
) STRICT;

CREATE INDEX media_tags_tag_idx ON media_tags(tag_id, media_id);
CREATE INDEX media_channels_channel_idx ON media_channels(channel_id, media_id);
