CREATE TABLE media (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    width INTEGER CHECK (width IS NULL OR width > 0),
    height INTEGER CHECK (height IS NULL OR height > 0),
    rotation INTEGER,
    is_short INTEGER NOT NULL DEFAULT 0 CHECK (is_short IN (0, 1)),
    is_available INTEGER NOT NULL DEFAULT 1 CHECK (is_available IN (0, 1)),
    inspected_at TEXT,
    inspection_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

INSERT INTO media (id, title, is_available)
SELECT id, title, is_available FROM media_files;

ALTER TABLE media_files ADD COLUMN media_id INTEGER REFERENCES media(id);
ALTER TABLE media_files ADD COLUMN container TEXT;
ALTER TABLE media_files ADD COLUMN video_codec TEXT;
ALTER TABLE media_files ADD COLUMN audio_codec TEXT;
ALTER TABLE media_files ADD COLUMN subtitle_codecs_json TEXT;
ALTER TABLE media_files ADD COLUMN probe_json TEXT;

UPDATE media_files SET media_id = id WHERE media_id IS NULL;

CREATE INDEX media_files_media_idx ON media_files(media_id);
CREATE INDEX media_available_short_idx ON media(is_available, is_short, id);
