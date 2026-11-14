use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::web::AppState;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct DashboardStats {
    pub total_videos: i64,
    pub official_thumbnails: i64,
    pub generated_thumbnails: i64,
    pub with_youtube_id: i64,
    pub videos_with_subtitles: i64,
    pub total_subtitles: i64,
    pub total_channels: i64,
    pub total_playlists: i64,
    pub total_favorites: i64,
    pub total_size_bytes: i64,
    pub total_duration_ms: i64,
}

pub(crate) async fn get_dashboard_stats(
    State(state): State<AppState>,
) -> Result<Json<DashboardStats>, (StatusCode, String)> {
    fetch_stats(&state.pool)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn fetch_stats(pool: &SqlitePool) -> Result<DashboardStats, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)>(
        r#"
        SELECT
            (SELECT COUNT(*) FROM media WHERE is_available = 1) AS total_videos,
            (SELECT COUNT(*) FROM thumbnails t JOIN media m ON m.id = t.media_id WHERE m.is_available = 1 AND t.is_official = 1) AS official_thumbnails,
            (SELECT COUNT(*) FROM thumbnails t JOIN media m ON m.id = t.media_id WHERE m.is_available = 1 AND (t.is_official = 0 OR t.is_official IS NULL)) AS generated_thumbnails,
            (SELECT COUNT(*) FROM media WHERE is_available = 1 AND youtube_id IS NOT NULL) AS with_youtube_id,
            (SELECT COUNT(DISTINCT media_id) FROM media_subtitles) AS videos_with_subtitles,
            (SELECT COUNT(*) FROM media_subtitles) AS total_subtitles,
            (SELECT COUNT(*) FROM channels) AS total_channels,
            (SELECT COUNT(*) FROM playlists) AS total_playlists,
            (SELECT COUNT(*) FROM favorites) AS total_favorites,
            COALESCE((SELECT SUM(size_bytes) FROM media_files WHERE is_available = 1), 0) AS total_size_bytes,
            COALESCE((SELECT SUM(duration_ms) FROM media WHERE is_available = 1), 0) AS total_duration_ms
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(DashboardStats {
        total_videos: row.0,
        official_thumbnails: row.1,
        generated_thumbnails: row.2,
        with_youtube_id: row.3,
        videos_with_subtitles: row.4,
        total_subtitles: row.5,
        total_channels: row.6,
        total_playlists: row.7,
        total_favorites: row.8,
        total_size_bytes: row.9,
        total_duration_ms: row.10,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_stats_empty_pool() {
        let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
        // Run minimal schema needed
        sqlx::query(
            "CREATE TABLE media (id INTEGER PRIMARY KEY, is_available INTEGER, youtube_id TEXT, duration_ms INTEGER);
             CREATE TABLE media_files (id INTEGER PRIMARY KEY, is_available INTEGER, size_bytes INTEGER);
             CREATE TABLE thumbnails (media_id INTEGER PRIMARY KEY, is_official INTEGER);
             CREATE TABLE media_subtitles (id INTEGER PRIMARY KEY, media_id INTEGER);
             CREATE TABLE channels (id TEXT PRIMARY KEY);
             CREATE TABLE playlists (id INTEGER PRIMARY KEY);
             CREATE TABLE favorites (media_id INTEGER PRIMARY KEY);"
        )
        .execute(&pool)
        .await
        .unwrap();

        let stats = fetch_stats(&pool).await.unwrap();
        assert_eq!(stats.total_videos, 0);
        assert_eq!(stats.official_thumbnails, 0);
        assert_eq!(stats.total_size_bytes, 0);
    }
}
