-- Persistent playlist downloads. Items remain outside the regular download queue
-- until the playlist scheduler promotes them in small batches.
CREATE TABLE download_playlists (
    id TEXT PRIMARY KEY NOT NULL,
    source_url TEXT NOT NULL,
    youtube_playlist_id TEXT NOT NULL,
    title TEXT NOT NULL,
    channel_name TEXT,
    thumbnail_url TEXT,
    status TEXT NOT NULL DEFAULT 'ready'
        CHECK (status IN (
            'importing', 'ready', 'running', 'paused', 'paused_rate_limit',
            'completed', 'completed_with_errors', 'cancelled', 'failed'
        )),
    quality TEXT NOT NULL DEFAULT 'best',
    format_container TEXT NOT NULL DEFAULT 'mkv'
        CHECK (format_container IN ('mkv', 'mp4')),
    target_source_id INTEGER REFERENCES media_sources(id) ON DELETE SET NULL,
    owner_key TEXT NOT NULL,
    create_local_playlist INTEGER NOT NULL DEFAULT 1
        CHECK (create_local_playlist IN (0, 1)),
    local_playlist_id TEXT REFERENCES playlists(id) ON DELETE SET NULL,
    download_profile TEXT NOT NULL DEFAULT 'conservative'
        CHECK (download_profile IN ('conservative')),
    cooldown_until TEXT,
    protection_reason TEXT,
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE download_playlist_items (
    download_playlist_id TEXT NOT NULL
        REFERENCES download_playlists(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    youtube_id TEXT,
    url TEXT,
    title TEXT NOT NULL,
    channel_name TEXT,
    thumbnail_url TEXT,
    duration_ms INTEGER,
    availability TEXT NOT NULL DEFAULT 'public'
        CHECK (availability IN ('public', 'unlisted', 'private', 'premium_only', 'subscriber_only', 'needs_auth', 'unavailable', 'unknown')),
    selected INTEGER NOT NULL DEFAULT 1 CHECK (selected IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN (
            'pending', 'unavailable', 'skipped_existing', 'queued',
            'downloading', 'processing', 'completed', 'failed', 'cancelled'
        )),
    download_job_id TEXT REFERENCES download_jobs(id) ON DELETE SET NULL,
    media_id INTEGER REFERENCES media(id) ON DELETE SET NULL,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (download_playlist_id, position)
) STRICT;

CREATE INDEX download_playlists_owner_created_idx
    ON download_playlists(owner_key, created_at DESC);
CREATE INDEX download_playlists_scheduler_idx
    ON download_playlists(status, cooldown_until, updated_at);
CREATE INDEX download_playlist_items_status_idx
    ON download_playlist_items(download_playlist_id, status, selected, position);
CREATE INDEX download_playlist_items_youtube_idx
    ON download_playlist_items(youtube_id);
