-- Migration: 202611070002_downloads.sql
-- Tabela para gerenciar histórico e fila de downloads de vídeos do YouTube

CREATE TABLE download_jobs (
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
        CHECK (status IN ('queued', 'probing', 'downloading', 'organizing', 'completed', 'failed', 'cancelled')),
    progress_percent REAL NOT NULL DEFAULT 0.0,
    speed_text TEXT,
    eta_text TEXT,
    output_path TEXT,
    media_id INTEGER REFERENCES media(id) ON DELETE SET NULL,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX download_jobs_created_idx ON download_jobs(created_at DESC);
CREATE INDEX download_jobs_status_idx ON download_jobs(status);
