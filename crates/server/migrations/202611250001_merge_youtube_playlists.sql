-- Local playlists created by playlist downloads are identified by the stable
-- YouTube playlist ID. Merge legacy per-run playlists without losing items.
CREATE TABLE youtube_playlist_local_merge (
    youtube_playlist_id TEXT PRIMARY KEY NOT NULL,
    local_playlist_id TEXT NOT NULL
) STRICT;

INSERT INTO youtube_playlist_local_merge (youtube_playlist_id, local_playlist_id)
SELECT youtube_playlist_id, local_playlist_id
FROM (
    SELECT dp.youtube_playlist_id,
           dp.local_playlist_id,
           ROW_NUMBER() OVER (
               PARTITION BY dp.youtube_playlist_id
               ORDER BY (
                   SELECT COUNT(*) FROM playlist_items pi
                   WHERE pi.playlist_id = dp.local_playlist_id
               ) DESC,
               dp.created_at,
               dp.id
           ) AS preference
    FROM download_playlists dp
    JOIN playlists p ON p.id = dp.local_playlist_id
    WHERE dp.local_playlist_id IS NOT NULL
)
WHERE preference = 1;

INSERT OR IGNORE INTO playlist_items (playlist_id, media_id, position, added_at)
SELECT merge.local_playlist_id, items.media_id, items.position, items.added_at
FROM youtube_playlist_local_merge merge
JOIN download_playlists downloads
  ON downloads.youtube_playlist_id = merge.youtube_playlist_id
JOIN playlist_items items ON items.playlist_id = downloads.local_playlist_id;

UPDATE download_playlists
SET local_playlist_id = (
    SELECT merge.local_playlist_id
    FROM youtube_playlist_local_merge merge
    WHERE merge.youtube_playlist_id = download_playlists.youtube_playlist_id
)
WHERE youtube_playlist_id IN (
    SELECT youtube_playlist_id FROM youtube_playlist_local_merge
);

-- The old generated description was language-specific and added no value.
UPDATE playlists
SET description = NULL
WHERE description = 'Playlist criada durante um download do YouTube.';

-- Remove only legacy auto-created playlists that became unreferenced after
-- the merge or that are still empty. ON DELETE SET NULL leaves an empty run
-- ready for lazy creation when its first real media item is associated.
DELETE FROM playlists
WHERE id LIKE 'pl_youtube_dlp_%'
  AND (
      NOT EXISTS (
          SELECT 1 FROM download_playlists dp WHERE dp.local_playlist_id = playlists.id
      )
      OR NOT EXISTS (
          SELECT 1 FROM playlist_items pi WHERE pi.playlist_id = playlists.id
      )
  );

DROP TABLE youtube_playlist_local_merge;
