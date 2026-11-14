CREATE TABLE playback_scrobbles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    watched_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX idx_playback_scrobbles_timeline
    ON playback_scrobbles(watched_at DESC, media_id);

CREATE INDEX idx_playback_scrobbles_media
    ON playback_scrobbles(media_id, watched_at DESC);
