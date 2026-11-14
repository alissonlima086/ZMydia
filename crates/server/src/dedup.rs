use sqlx::SqlitePool;

/// Computes a deterministic quality score for a media file.
/// Higher score means higher visual/audio fidelity.
///
/// Rules:
/// - Any video stream (width > 0 && height > 0) strictly beats audio-only formats.
/// - Higher pixel count (resolution) beats lower resolution (e.g. 1080p beats 720p).
/// - Modern efficient codecs (AV1, VP9, HEVC) get a tie-breaker bonus at equal resolution.
/// - Higher file size / bitrate wins between two files of the same resolution and codec.
pub fn compute_quality_score(
    width: Option<i64>,
    height: Option<i64>,
    size_bytes: i64,
    video_codec: Option<&str>,
) -> i64 {
    let w = width.unwrap_or(0);
    let h = height.unwrap_or(0);
    let is_video = if w > 0 && h > 0 { 1 } else { 0 };

    // Pixel count: 1920x1080 is 2,073,600
    let pixels = w * h;

    // Codec bonus for modern efficient codecs if pixels are equal
    let codec_score: i64 = match video_codec.map(|c| c.to_lowercase()).as_deref() {
        Some(c) if c.contains("av01") || c.contains("av1") => 300_000,
        Some(c) if c.contains("vp9") || c.contains("hevc") || c.contains("h265") => 200_000,
        Some(c) if c.contains("h264") || c.contains("avc") => 100_000,
        _ => 0,
    };

    // Size contribution in kilobytes to avoid overflow, clamped
    let size_kb = (size_bytes / 1024).clamp(0, 100_000_000);

    // Weights:
    // Video presence: 10,000,000,000 (video always beats audio)
    // Resolution (pixels): 100 per pixel (1080p = 207,360,000)
    // Codec bonus: up to 300,000
    // Size/bitrate (size_kb): 1 per KB (50MB = 50,000)
    (is_video * 10_000_000_000) + (pixels * 100) + codec_score + size_kb
}

/// Helper struct representing candidate media during deduplication
#[derive(Debug, Clone)]
pub struct DuplicateCandidate {
    pub media_id: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub size_bytes: i64,
    pub video_codec: Option<String>,
    pub path: String,
    pub score: i64,
}

/// Persists an operator decision instead of consolidating duplicate media
/// automatically. Physical files and logical metadata remain untouched.
pub async fn propose_duplicate_media(
    pool: &SqlitePool,
    first_media_id: i64,
    second_media_id: i64,
    reason: &str,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, i64, Option<i64>, Option<i64>, i64, Option<String>)>(
        "SELECT m.id, mf.id, m.width, m.height, mf.size_bytes, mf.video_codec
         FROM media m JOIN media_files mf ON mf.media_id = m.id
         WHERE m.id IN (?, ?) AND mf.is_available = 1
         ORDER BY m.id, mf.id",
    )
    .bind(first_media_id)
    .bind(second_media_id)
    .fetch_all(pool)
    .await?;
    let first = rows.iter().find(|row| row.0 == first_media_id);
    let second = rows.iter().find(|row| row.0 == second_media_id);
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(0);
    };
    let first_score = compute_quality_score(first.2, first.3, first.4, first.5.as_deref());
    let second_score = compute_quality_score(second.2, second.3, second.4, second.5.as_deref());
    let (winner, duplicate, winner_score, duplicate_score) = if first_score >= second_score {
        (first.1, second.1, first_score, second_score)
    } else {
        (second.1, first.1, second_score, first_score)
    };
    let reason_json = serde_json::json!({
        "match": reason,
        "winner_score": winner_score,
        "duplicate_score": duplicate_score,
        "rule": "resolution_codec_size",
        "requires_user_decision": true
    });
    let result = sqlx::query(
        "INSERT INTO duplicate_candidates
            (winner_media_file_id, duplicate_media_file_id, score, reason_json)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(winner_media_file_id, duplicate_media_file_id) DO UPDATE SET
            score = excluded.score, reason_json = excluded.reason_json
         WHERE duplicate_candidates.status = 'pending'",
    )
    .bind(winner)
    .bind(duplicate)
    .bind(winner_score.saturating_sub(duplicate_score))
    .bind(reason_json.to_string())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() as usize)
}

/// Merges a loser media into a winner media:
/// - Re-links favorites, playlists, playback progress, tags, subtitles and files.
/// - Keeps every physical file on disk; cleanup is an explicit operator decision.
/// - Removes only the redundant logical media row from the database.
pub async fn merge_duplicate_media(
    pool: &SqlitePool,
    winner_id: i64,
    loser_id: i64,
) -> Result<(), sqlx::Error> {
    if winner_id == loser_id {
        return Ok(());
    }

    tracing::info!(
        winner_id,
        loser_id,
        "merging duplicate media into the higher-quality record"
    );

    // Atomic database updates
    let mut tx = pool.begin().await?;

    // Copy description, user_notes and published_at if winner lacks them
    sqlx::query(
        "UPDATE media SET
            description = COALESCE(media.description, (SELECT description FROM media WHERE id = ?)),
            user_notes = COALESCE(media.user_notes, (SELECT user_notes FROM media WHERE id = ?)),
            published_at = COALESCE(media.published_at, (SELECT published_at FROM media WHERE id = ?))
         WHERE id = ?",
    )
    .bind(loser_id)
    .bind(loser_id)
    .bind(loser_id)
    .bind(winner_id)
    .execute(&mut *tx)
    .await?;

    // Repoint favorites
    sqlx::query(
        "INSERT INTO favorites (media_id, created_at)
         SELECT ?, created_at FROM favorites WHERE media_id = ?
         ON CONFLICT(media_id) DO NOTHING",
    )
    .bind(winner_id)
    .bind(loser_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM favorites WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Repoint playlist_items
    sqlx::query(
        "UPDATE playlist_items
         SET media_id = ?
         WHERE media_id = ?
           AND playlist_id NOT IN (SELECT playlist_id FROM playlist_items WHERE media_id = ?)",
    )
    .bind(winner_id)
    .bind(loser_id)
    .bind(winner_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM playlist_items WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Repoint playback_state (preserve highest position and completion)
    sqlx::query(
        "INSERT INTO playback_state (media_id, position_ms, completed, updated_at)
         SELECT ?, position_ms, completed, updated_at FROM playback_state WHERE media_id = ?
         ON CONFLICT(media_id) DO UPDATE SET
            position_ms = MAX(playback_state.position_ms, excluded.position_ms),
            completed = MAX(playback_state.completed, excluded.completed),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(winner_id)
    .bind(loser_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM playback_state WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Repoint media_subtitles
    sqlx::query(
        "UPDATE media_subtitles
         SET media_id = ?
         WHERE media_id = ?
           AND (lang_code, label) NOT IN (SELECT lang_code, label FROM media_subtitles WHERE media_id = ?)",
    )
    .bind(winner_id)
    .bind(loser_id)
    .bind(winner_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM media_subtitles WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Repoint media_tags
    sqlx::query(
        "INSERT INTO media_tags (media_id, tag_id, origin)
         SELECT ?, tag_id, origin FROM media_tags WHERE media_id = ?
         ON CONFLICT(media_id, tag_id) DO NOTHING",
    )
    .bind(winner_id)
    .bind(loser_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM media_tags WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Repoint media_channels if winner has none
    sqlx::query(
        "INSERT INTO media_channels (media_id, channel_id, origin)
         SELECT ?, channel_id, origin FROM media_channels WHERE media_id = ?
         ON CONFLICT(media_id) DO NOTHING",
    )
    .bind(winner_id)
    .bind(loser_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM media_channels WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Repoint download_jobs
    sqlx::query("UPDATE download_jobs SET media_id = ? WHERE media_id = ?")
        .bind(winner_id)
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Clean up attempts and thumbnails
    sqlx::query("DELETE FROM youtube_sync_attempts WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM thumbnails WHERE media_id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Preserve all indexed paths and associate them with the surviving logical media.
    sqlx::query("UPDATE media_files SET media_id = ? WHERE media_id = ?")
        .bind(winner_id)
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    // Delete loser media record
    sqlx::query("DELETE FROM media WHERE id = ?")
        .bind(loser_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(())
}

/// Scans the entire database for any duplicate `youtube_id` entries,
/// identifies the highest quality version and merges logical metadata while
/// preserving every indexed physical copy.
pub async fn consolidate_all_duplicates(pool: &SqlitePool) -> Result<usize, sqlx::Error> {
    let duplicate_ids = sqlx::query_scalar::<_, String>(
        "SELECT youtube_id
         FROM media
         WHERE youtube_id IS NOT NULL AND youtube_id <> ''
         GROUP BY youtube_id
         HAVING count(id) > 1",
    )
    .fetch_all(pool)
    .await?;

    if duplicate_ids.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        duplicate_groups = duplicate_ids.len(),
        "starting duplicate video consolidation by YouTube ID"
    );

    let mut merged_count = 0;

    for yid in duplicate_ids {
        let rows = sqlx::query_as::<
            _,
            (
                i64,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT m.id, m.width, m.height, mf.size_bytes, mf.video_codec, mf.path
             FROM media m
             LEFT JOIN media_files mf ON mf.media_id = m.id
             WHERE m.youtube_id = ?",
        )
        .bind(&yid)
        .fetch_all(pool)
        .await?;

        if rows.len() <= 1 {
            continue;
        }

        let mut candidates: Vec<DuplicateCandidate> = rows
            .into_iter()
            .map(|(media_id, width, height, size_bytes, video_codec, path)| {
                let size = size_bytes.unwrap_or(0);
                let score = compute_quality_score(width, height, size, video_codec.as_deref());
                DuplicateCandidate {
                    media_id,
                    width,
                    height,
                    size_bytes: size,
                    video_codec,
                    path: path.unwrap_or_default(),
                    score,
                }
            })
            .collect();

        // Sort descending: highest quality score first.
        // Tie-breaker: avoid path containing `_dup`, then lower media_id (older/original).
        candidates.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| {
                    let a_dup = a.path.contains("_dup");
                    let b_dup = b.path.contains("_dup");
                    a_dup.cmp(&b_dup)
                })
                .then_with(|| a.media_id.cmp(&b.media_id))
        });

        let winner = candidates[0].clone();
        for loser in &candidates[1..] {
            if let Err(e) = merge_duplicate_media(pool, winner.media_id, loser.media_id).await {
                tracing::error!(
                    winner_id = winner.media_id,
                    loser_id = loser.media_id,
                    %e,
                    "failed to merge duplicate media copy"
                );
            } else {
                merged_count += 1;
            }
        }
    }

    // Enforce partial unique index in SQLite
    let _ = sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_media_youtube_id_unique
         ON media(youtube_id)
         WHERE youtube_id IS NOT NULL AND youtube_id <> ''",
    )
    .execute(pool)
    .await;

    // Resync FTS
    if let Ok(mut conn) = pool.acquire().await {
        let _ = crate::catalog::sync_media_fts(&mut conn).await;
    }

    tracing::info!(merged_count, "duplicate consolidation completed");

    Ok(merged_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_quality_score() {
        // 1080p video should beat 720p
        let score_1080p = compute_quality_score(Some(1920), Some(1080), 50_000_000, Some("h264"));
        let score_720p = compute_quality_score(Some(1280), Some(720), 50_000_000, Some("h264"));
        assert!(score_1080p > score_720p);

        // 1080p video should beat 480p
        let score_480p = compute_quality_score(Some(854), Some(480), 30_000_000, Some("h264"));
        assert!(score_1080p > score_480p);

        // Any video should beat audio-only
        let score_audio = compute_quality_score(None, None, 100_000_000, None);
        assert!(score_480p > score_audio);

        // Equal resolution: higher bitrate / file size wins
        let score_high_bitrate =
            compute_quality_score(Some(1920), Some(1080), 80_000_000, Some("h264"));
        let score_low_bitrate =
            compute_quality_score(Some(1920), Some(1080), 40_000_000, Some("h264"));
        assert!(score_high_bitrate > score_low_bitrate);

        // Equal resolution and size: AV1 beats H264
        let score_av1 =
            compute_quality_score(Some(1920), Some(1080), 50_000_000, Some("av01.0.08M.08"));
        let score_h264 = compute_quality_score(Some(1920), Some(1080), 50_000_000, Some("h264"));
        assert!(score_av1 > score_h264);
    }

    #[tokio::test]
    async fn test_merge_duplicate_media_preserves_favorites_and_playlists() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Drop unique index temporarily to simulate legacy duplicate data
        sqlx::query("DROP INDEX IF EXISTS idx_media_youtube_id_unique")
            .execute(&pool)
            .await
            .unwrap();

        let m1 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media (title, youtube_id, width, height) VALUES ('Video 1080p', 'yt123', 1920, 1080) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let m2 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media (title, youtube_id, width, height) VALUES ('Video 720p', 'yt123', 1280, 720) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let temp_dir = std::env::temp_dir().join(format!(
            "mymedia_dedup_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let winner_path = temp_dir.join("winner.mp4");
        let loser_path = temp_dir.join("loser.mp4");
        std::fs::write(&winner_path, b"winner").unwrap();
        std::fs::write(&loser_path, b"loser").unwrap();
        sqlx::query(
            "INSERT INTO media_files (media_id, path, title, extension, size_bytes, modified_at_unix)
             VALUES (?, ?, 'winner', 'mp4', 6, 1), (?, ?, 'loser', 'mp4', 5, 1)",
        )
        .bind(m1)
        .bind(winner_path.to_string_lossy().as_ref())
        .bind(m2)
        .bind(loser_path.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO favorites (media_id) VALUES (?)")
            .bind(m2)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO playlists (id, title) VALUES ('pl1', 'Minha Playlist')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO playlist_items (playlist_id, media_id, position) VALUES ('pl1', ?, 1)",
        )
        .bind(m2)
        .execute(&pool)
        .await
        .unwrap();

        let merged = consolidate_all_duplicates(&pool).await.unwrap();
        assert_eq!(merged, 1);

        let count_m1 = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media WHERE id = ?")
            .bind(m1)
            .fetch_one(&pool)
            .await
            .unwrap();
        let count_m2 = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media WHERE id = ?")
            .bind(m2)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count_m1, 1);
        assert_eq!(count_m2, 0);

        let fav = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM favorites WHERE media_id = ?")
            .bind(m1)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fav, 1);

        let pl_item =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM playlist_items WHERE media_id = ?")
                .bind(m1)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pl_item, 1);

        assert!(winner_path.is_file());
        assert!(loser_path.is_file());
        let linked_files =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media_files WHERE media_id = ?")
                .bind(m1)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked_files, 2);
        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
