CREATE TABLE playback_engagement_sessions (
    session_id TEXT PRIMARY KEY,
    media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    watched_ms INTEGER NOT NULL CHECK (watched_ms >= 0),
    duration_ms INTEGER NOT NULL CHECK (duration_ms > 0),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX idx_playback_engagement_media
    ON playback_engagement_sessions(media_id, updated_at DESC);

CREATE INDEX idx_playback_engagement_timeline
    ON playback_engagement_sessions(updated_at DESC, media_id);
