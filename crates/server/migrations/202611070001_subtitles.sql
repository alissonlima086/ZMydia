-- Migration: 202611070001_subtitles.sql
-- Tabela para indexar legendas (WebVTT/SRT) vinculadas a mídias

CREATE TABLE media_subtitles (
    id INTEGER PRIMARY KEY,
    media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    lang_code TEXT NOT NULL,
    label TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('youtube', 'youtube_auto', 'local', 'embedded')),
    file_path TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(media_id, lang_code, source)
) STRICT;

CREATE INDEX media_subtitles_media_idx ON media_subtitles(media_id);
