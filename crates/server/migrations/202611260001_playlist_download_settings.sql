-- Rebuild the playlist tables so profiles are no longer restricted to the
-- original conservative-only CHECK constraint. The child table is rebuilt as
-- well to keep its foreign key attached to the new parent table.
ALTER TABLE download_playlist_items RENAME TO download_playlist_items_old;
ALTER TABLE download_playlists RENAME TO download_playlists_old;

CREATE TABLE download_playlists (
    id TEXT PRIMARY KEY NOT NULL,
    source_url TEXT NOT NULL,
    youtube_playlist_id TEXT NOT NULL,
    title TEXT NOT NULL,
    channel_name TEXT,
    thumbnail_url TEXT,
    status TEXT NOT NULL DEFAULT 'ready'
        CHECK (status IN ('importing', 'ready', 'running', 'paused', 'paused_rate_limit', 'completed', 'completed_with_errors', 'cancelled', 'failed')),
    quality TEXT NOT NULL DEFAULT 'best',
    format_container TEXT NOT NULL DEFAULT 'mkv' CHECK (format_container IN ('mkv', 'mp4')),
    target_source_id INTEGER REFERENCES media_sources(id) ON DELETE SET NULL,
    owner_key TEXT NOT NULL,
    create_local_playlist INTEGER NOT NULL DEFAULT 1 CHECK (create_local_playlist IN (0, 1)),
    local_playlist_id TEXT REFERENCES playlists(id) ON DELETE SET NULL,
    download_profile TEXT NOT NULL DEFAULT 'conservative'
        CHECK (download_profile IN ('conservative', 'balanced', 'fast', 'custom')),
    cooldown_until TEXT,
    protection_reason TEXT,
    cooldown_reason TEXT,
    bypass_budget_once INTEGER NOT NULL DEFAULT 0 CHECK (bypass_budget_once IN (0, 1)),
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

INSERT INTO download_playlists (
    id, source_url, youtube_playlist_id, title, channel_name, thumbnail_url,
    status, quality, format_container, target_source_id, owner_key,
    create_local_playlist, local_playlist_id, download_profile, cooldown_until,
    protection_reason, started_at, finished_at, created_at, updated_at
)
SELECT id, source_url, youtube_playlist_id, title, channel_name, thumbnail_url,
       status, quality, format_container, target_source_id, owner_key,
       create_local_playlist, local_playlist_id, download_profile, cooldown_until,
       protection_reason, started_at, finished_at, created_at, updated_at
FROM download_playlists_old;

UPDATE download_playlists
SET cooldown_reason = CASE
        WHEN protection_reason LIKE '%GiB em 24 horas%' THEN 'daily_bytes:30'
        WHEN protection_reason LIKE '%vídeos em 24 horas%' THEN 'daily_videos:6'
        ELSE cooldown_reason
    END,
    protection_reason = CASE
        WHEN protection_reason LIKE '%GiB em 24 horas%'
          OR protection_reason LIKE '%vídeos em 24 horas%' THEN NULL
        ELSE protection_reason
    END;

CREATE TABLE download_playlist_items (
    download_playlist_id TEXT NOT NULL REFERENCES download_playlists(id) ON DELETE CASCADE,
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
        CHECK (status IN ('pending', 'unavailable', 'skipped_existing', 'queued', 'downloading', 'processing', 'completed', 'failed', 'cancelled')),
    download_job_id TEXT REFERENCES download_jobs(id) ON DELETE SET NULL,
    media_id INTEGER REFERENCES media(id) ON DELETE SET NULL,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (download_playlist_id, position)
) STRICT;

INSERT INTO download_playlist_items SELECT * FROM download_playlist_items_old;
DROP TABLE download_playlist_items_old;
DROP TABLE download_playlists_old;

CREATE INDEX download_playlists_owner_created_idx ON download_playlists(owner_key, created_at DESC);
CREATE INDEX download_playlists_scheduler_idx ON download_playlists(status, cooldown_until, updated_at);
CREATE INDEX download_playlist_items_status_idx ON download_playlist_items(download_playlist_id, status, selected, position);
CREATE INDEX download_playlist_items_youtube_idx ON download_playlist_items(youtube_id);
