CREATE TABLE media_music_tracks (
    media_id INTEGER PRIMARY KEY REFERENCES media(id) ON DELETE CASCADE,
    recording_mbid TEXT NOT NULL,
    artist_name TEXT NOT NULL,
    track_name TEXT NOT NULL,
    release_name TEXT,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_scrobbled_at TEXT
) STRICT;

CREATE INDEX media_music_tracks_mbid_idx ON media_music_tracks(recording_mbid);
