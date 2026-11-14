-- Migration: Adiciona coluna published_at na tabela media
ALTER TABLE media ADD COLUMN published_at TEXT;
CREATE INDEX IF NOT EXISTS media_published_at_idx ON media(published_at);
