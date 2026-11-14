-- A download is only complete after the destination scan has indexed the file.
CREATE TABLE download_jobs_new (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    youtube_id TEXT,
    title TEXT,
    channel_name TEXT,
    thumbnail_url TEXT,
    duration_ms INTEGER,
    format_container TEXT NOT NULL DEFAULT 'mkv' CHECK (format_container IN ('mkv', 'mp4')),
    quality TEXT NOT NULL DEFAULT 'best',
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'probing', 'downloading', 'organizing', 'awaiting_index', 'completed', 'failed', 'cancelled')),
    progress_percent REAL NOT NULL DEFAULT 0.0,
    speed_text TEXT,
    eta_text TEXT,
    output_path TEXT,
    media_id INTEGER REFERENCES media(id) ON DELETE SET NULL,
    target_source_id INTEGER REFERENCES media_sources(id) ON DELETE SET NULL,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

INSERT INTO download_jobs_new (
    id, url, youtube_id, title, channel_name, thumbnail_url, duration_ms,
    format_container, quality, status, progress_percent, speed_text, eta_text,
    output_path, media_id, error_message, created_at, updated_at
)
SELECT
    id, url, youtube_id, title, channel_name, thumbnail_url, duration_ms,
    format_container, quality, status, progress_percent, speed_text, eta_text,
    output_path, media_id, error_message, created_at, updated_at
FROM download_jobs;

DROP TABLE download_jobs;
ALTER TABLE download_jobs_new RENAME TO download_jobs;
CREATE INDEX download_jobs_created_idx ON download_jobs(created_at DESC);
CREATE INDEX download_jobs_status_idx ON download_jobs(status);
CREATE INDEX download_jobs_target_source_idx ON download_jobs(target_source_id, status);
