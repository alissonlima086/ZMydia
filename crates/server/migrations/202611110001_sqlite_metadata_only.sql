PRAGMA foreign_keys = OFF;

ALTER TABLE media_channels RENAME TO media_channels_legacy;

CREATE TABLE media_channels (
    media_id INTEGER PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    origin TEXT NOT NULL CHECK (origin IN ('manual', 'youtube', 'migration', 'inferred'))
);

INSERT INTO media_channels (media_id, channel_id, origin)
SELECT media_id, channel_id,
       CASE WHEN origin_path = 'youtube' THEN 'youtube' ELSE 'migration' END
FROM media_channels_legacy;

DROP TABLE media_channels_legacy;
CREATE INDEX idx_media_channels_channel ON media_channels(channel_id, media_id);

ALTER TABLE media_tags RENAME TO media_tags_legacy;

CREATE TABLE media_tags (
    media_id INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    origin TEXT NOT NULL CHECK (origin IN ('manual', 'youtube', 'migration', 'inferred')),
    PRIMARY KEY (media_id, tag_id)
);

INSERT INTO media_tags (media_id, tag_id, origin)
SELECT media_id, tag_id,
       CASE WHEN origin_path = 'youtube' THEN 'youtube' ELSE 'migration' END
FROM media_tags_legacy;

DROP TABLE media_tags_legacy;
CREATE INDEX idx_media_tags_tag ON media_tags(tag_id, media_id);

DROP TABLE folder_metadata;

PRAGMA foreign_keys = ON;
