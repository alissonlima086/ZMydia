ALTER TABLE channels ADD COLUMN youtube_channel_id TEXT;
ALTER TABLE channels ADD COLUMN handle TEXT;
ALTER TABLE channels ADD COLUMN avatar_url TEXT;

ALTER TABLE media ADD COLUMN description TEXT;
ALTER TABLE media ADD COLUMN youtube_id TEXT;

ALTER TABLE thumbnails ADD COLUMN is_official INTEGER NOT NULL DEFAULT 0 CHECK (is_official IN (0, 1));
ALTER TABLE thumbnails ADD COLUMN source TEXT NOT NULL DEFAULT 'generated' CHECK (source IN ('generated', 'youtube_id', 'youtube_search', 'manual'));

CREATE INDEX IF NOT EXISTS thumbnails_official_idx ON thumbnails(is_official);
CREATE INDEX IF NOT EXISTS media_youtube_id_idx ON media(youtube_id);
CREATE INDEX IF NOT EXISTS channels_yt_id_idx ON channels(youtube_channel_id);
