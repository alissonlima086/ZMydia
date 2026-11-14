CREATE VIRTUAL TABLE media_fts USING fts5(
    media_id UNINDEXED,
    title,
    channel_name,
    tags,
    folder_path,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO media_fts (media_id, title, channel_name, tags, folder_path)
SELECT
    m.id,
    m.title,
    COALESCE(c.name, ''),
    COALESCE((SELECT GROUP_CONCAT(t.display_name, ' ') FROM media_tags mt JOIN tags t ON t.id = mt.tag_id WHERE mt.media_id = m.id), ''),
    COALESCE((SELECT f.path FROM media_files f WHERE f.media_id = m.id AND f.is_available = 1 LIMIT 1), '')
FROM media m
LEFT JOIN media_channels mc ON mc.media_id = m.id
LEFT JOIN channels c ON c.id = mc.channel_id
WHERE m.is_available = 1;
