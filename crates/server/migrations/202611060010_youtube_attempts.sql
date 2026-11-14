CREATE TABLE IF NOT EXISTS youtube_sync_attempts (
    media_id INTEGER PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
    attempts INTEGER NOT NULL DEFAULT 1,
    last_status TEXT NOT NULL,
    last_attempt_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX IF NOT EXISTS youtube_sync_attempts_idx ON youtube_sync_attempts(attempts);
