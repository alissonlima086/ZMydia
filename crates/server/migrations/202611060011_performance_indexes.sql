-- Índices de alta performance para ordenação, junções e histórico

-- 1. Histórico de reprodução: ordenação instantânea por data de visualização DESC
CREATE INDEX IF NOT EXISTS idx_playback_state_updated ON playback_state(updated_at DESC);

-- 2. Favoritos: ordenação por data de criação DESC
CREATE INDEX IF NOT EXISTS idx_favorites_created ON favorites(created_at DESC);

-- 3. Mídia: ordenação de feed / catálogo por mais recentes
CREATE INDEX IF NOT EXISTS idx_media_available_created ON media(is_available, created_at DESC);

-- 4. Mídia: ordenação por título A-Z / Z-A
CREATE INDEX IF NOT EXISTS idx_media_available_title ON media(is_available, title COLLATE NOCASE);

-- 5. Mídia: ordenação por duração (mais longos / mais curtos)
CREATE INDEX IF NOT EXISTS idx_media_available_duration ON media(is_available, duration_ms);

-- 6. Junção de arquivos de mídia disponíveis para streaming e thumbnails
CREATE INDEX IF NOT EXISTS idx_media_files_media_avail ON media_files(media_id, is_available);
