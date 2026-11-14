-- Índice único parcial garantindo que cada vídeo do YouTube seja indexado uma única vez
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_youtube_id_unique
ON media(youtube_id)
WHERE youtube_id IS NOT NULL AND youtube_id <> '';
