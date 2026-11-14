use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{MEDIA_SELECT_COLUMNS, MediaDbRow, MediaItem},
    web::AppState,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlaybackStateResponse {
    pub media_id: i64,
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub completed: bool,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryItem {
    #[serde(flatten)]
    pub media: MediaItem,
    pub watched_at: String,
}

#[derive(sqlx::FromRow, Debug)]
pub struct HistoryDbRow {
    #[sqlx(flatten)]
    pub media_row: MediaDbRow,
    pub watched_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlaybackRequest {
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RecordPlaybackScrobbleRequest {
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RecordPlaybackEngagementRequest {
    pub session_id: String,
    pub watched_ms: i64,
    pub duration_ms: i64,
    #[serde(default)]
    pub completed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlaybackScrobbleResponse {
    pub id: i64,
    pub media_id: i64,
    pub duration_ms: Option<i64>,
    pub watched_at: String,
}

#[derive(Debug, Deserialize)]
pub struct PlaybackStatsQuery {
    pub period: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopVideoStat {
    pub id: i64,
    pub media_id: i64,
    pub title: String,
    pub duration_ms: Option<i64>,
    pub is_short: bool,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub channel_avatar_url: Option<String>,
    pub thumbnail_url: String,
    pub play_count: i64,
    pub total_watch_time_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopChannelStat {
    pub id: String,
    pub channel_id: String,
    pub name: String,
    pub channel_name: String,
    pub avatar_url: Option<String>,
    pub handle: Option<String>,
    pub play_count: i64,
    pub unique_videos_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopTagStat {
    pub id: i64,
    pub tag_id: i64,
    pub display_name: String,
    pub normalized_name: String,
    pub tag_name: String,
    pub play_count: i64,
    pub unique_videos_count: i64,
    pub video_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlaybackStatsResponse {
    pub period: String,
    pub total_scrobbles: i64,
    pub total_watch_time_ms: i64,
    pub top_videos: Vec<TopVideoStat>,
    pub top_channels: Vec<TopChannelStat>,
    pub top_tags: Vec<TopTagStat>,
}

/// Lista limpa e centralizada de tags genéricas de SEO e ruído a serem ignoradas nas estatísticas.
pub const NOISE_TAGS: &[&str] = &[
    "official video",
    "official music video",
    "music video",
    "official audio",
    "official visualizer",
    "official trailer",
    "official teaser",
    "official",
    "video",
    "audio",
    "visualizer",
    "lyric video",
    "lyrics",
    "channel",
    "records",
    "recordings",
    "full album",
    "hd",
    "4k",
    "1080p",
    "720p",
    "hq",
    "trailer",
    "teaser",
    "preview",
    "clip",
    "shorts",
    "short",
    "subscribe",
];

/// Formata o display_name de uma tag para apresentação consistente.
/// Se já possuir alguma letra maiúscula (ex: "Symphonic Metal", "Example Artist"), mantém.
/// Se for toda minúscula, capitaliza cada palavra.
pub fn format_tag_display_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().any(|c| c.is_uppercase()) {
        return trimmed.to_string();
    }
    trimmed
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) async fn get_playback_state(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
) -> Result<Json<PlaybackStateResponse>, StatusCode> {
    let existing = sqlx::query_as::<_, (i64, Option<i64>, bool, Option<String>, String)>(
        "SELECT position_ms, duration_ms, completed, completed_at, updated_at
         FROM playback_state
         WHERE media_id = ?",
    )
    .bind(media_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, media_id, "failed to get playback state");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some((position_ms, duration_ms, completed, completed_at, updated_at)) = existing {
        return Ok(Json(PlaybackStateResponse {
            media_id,
            position_ms,
            duration_ms,
            completed,
            completed_at,
            updated_at,
        }));
    }

    // Se ainda não houver estado de reprodução salvo, verificar se a mídia existe
    let media_row =
        sqlx::query_as::<_, (Option<i64>,)>("SELECT duration_ms FROM media WHERE id = ?")
            .bind(media_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match media_row {
        Some((duration_ms,)) => Ok(Json(PlaybackStateResponse {
            media_id,
            position_ms: 0,
            duration_ms,
            completed: false,
            completed_at: None,
            updated_at: String::new(),
        })),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub(crate) async fn update_playback_state(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
    Json(payload): Json<UpdatePlaybackRequest>,
) -> Result<Json<PlaybackStateResponse>, StatusCode> {
    if payload.position_ms < 0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let media_row =
        sqlx::query_as::<_, (Option<i64>,)>("SELECT duration_ms FROM media WHERE id = ?")
            .bind(media_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let Some((media_duration_ms,)) = media_row else {
        return Err(StatusCode::NOT_FOUND);
    };

    let effective_duration = payload.duration_ms.or(media_duration_ms);
    let is_completed = effective_duration
        .is_some_and(|dur| dur > 0 && (payload.position_ms as f64) >= (dur as f64 * 0.90));

    let completed_flag = if is_completed { 1 } else { 0 };

    let row = sqlx::query_as::<_, (i64, Option<i64>, bool, Option<String>, String)>(
        "INSERT INTO playback_state (media_id, position_ms, duration_ms, completed, completed_at, updated_at)
         VALUES (?, ?, ?, ?, CASE WHEN ? = 1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE NULL END, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(media_id) DO UPDATE SET
             position_ms = excluded.position_ms,
             duration_ms = COALESCE(excluded.duration_ms, playback_state.duration_ms),
             completed = CASE WHEN playback_state.completed = 1 THEN 1 ELSE excluded.completed END,
             completed_at = CASE
                 WHEN playback_state.completed_at IS NOT NULL THEN playback_state.completed_at
                 WHEN excluded.completed = 1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 ELSE NULL
             END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         RETURNING position_ms, duration_ms, completed, completed_at, updated_at",
    )
    .bind(media_id)
    .bind(payload.position_ms)
    .bind(effective_duration)
    .bind(completed_flag)
    .bind(completed_flag)
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, media_id, "failed to update playback state");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(PlaybackStateResponse {
        media_id,
        position_ms: row.0,
        duration_ms: row.1,
        completed: row.2,
        completed_at: row.3,
        updated_at: row.4,
    }))
}

pub(crate) async fn list_history(
    State(state): State<AppState>,
) -> Result<Json<Vec<HistoryItem>>, StatusCode> {
    let query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS},
                ps.updated_at as watched_at
         FROM playback_state ps
         JOIN media m ON m.id = ps.media_id
         LEFT JOIN favorites fav ON fav.media_id = m.id
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE m.is_available = 1 AND (ps.position_ms > 0 OR ps.completed = 1)
         ORDER BY ps.updated_at DESC
         LIMIT 300"
    );
    let rows = sqlx::query_as::<_, HistoryDbRow>(sqlx::AssertSqlSafe(query.as_str()))
        .fetch_all(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to query history");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let media_ids: Vec<i64> = rows.iter().map(|r| r.media_row.id).collect();
    let mut tag_map = crate::catalog::fetch_tags_for_media(&state.pool, &media_ids).await?;

    let items = rows
        .into_iter()
        .map(|r| HistoryItem {
            media: r.media_row.into_media_item(&mut tag_map),
            watched_at: r.watched_at,
        })
        .collect();

    Ok(Json(items))
}

pub(crate) async fn clear_history(State(state): State<AppState>) -> Result<StatusCode, StatusCode> {
    let mut transaction = state.pool.begin().await.map_err(|error| {
        tracing::error!(%error, "failed to begin history cleanup");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    sqlx::query("DELETE FROM playback_state")
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to clear history");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    sqlx::query("DELETE FROM playback_scrobbles")
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to clear playback scrobbles");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    sqlx::query("DELETE FROM playback_engagement_sessions")
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to clear playback engagement");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    transaction.commit().await.map_err(|error| {
        tracing::error!(%error, "failed to commit history cleanup");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn remove_history_item(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let mut transaction = state.pool.begin().await.map_err(|error| {
        tracing::error!(%error, media_id, "failed to begin history item cleanup");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    sqlx::query("DELETE FROM playback_state WHERE media_id = ?")
        .bind(media_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, media_id, "failed to remove item from history");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    sqlx::query("DELETE FROM playback_scrobbles WHERE media_id = ?")
        .bind(media_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, media_id, "failed to remove item scrobbles");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    sqlx::query("DELETE FROM playback_engagement_sessions WHERE media_id = ?")
        .bind(media_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            tracing::error!(%error, media_id, "failed to remove item engagement");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    transaction.commit().await.map_err(|error| {
        tracing::error!(%error, media_id, "failed to commit history item cleanup");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn record_playback_engagement(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
    Json(payload): Json<RecordPlaybackEngagementRequest>,
) -> Result<StatusCode, StatusCode> {
    let valid_session_id = (16..=80).contains(&payload.session_id.len())
        && payload.session_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });
    let valid_measurement = payload.watched_ms >= 0
        && payload.duration_ms > 0
        && payload.duration_ms <= 24 * 60 * 60 * 1000
        && payload.watched_ms <= payload.duration_ms;
    if !valid_session_id || !valid_measurement {
        return Err(StatusCode::BAD_REQUEST);
    }

    let result = sqlx::query(
        "INSERT INTO playback_engagement_sessions
            (session_id, media_id, watched_ms, duration_ms, completed)
         SELECT ?, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM media WHERE id = ?)
         ON CONFLICT(session_id) DO UPDATE SET
            watched_ms = MAX(playback_engagement_sessions.watched_ms, excluded.watched_ms),
            duration_ms = excluded.duration_ms,
            completed = MAX(playback_engagement_sessions.completed, excluded.completed),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE playback_engagement_sessions.media_id = excluded.media_id",
    )
    .bind(&payload.session_id)
    .bind(media_id)
    .bind(payload.watched_ms)
    .bind(payload.duration_ms)
    .bind(payload.completed)
    .bind(media_id)
    .execute(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, media_id, "failed to record playback engagement");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn record_playback_scrobble(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
    Json(payload): Json<RecordPlaybackScrobbleRequest>,
) -> Result<(StatusCode, Json<PlaybackScrobbleResponse>), StatusCode> {
    // 1. Verificar se a mídia existe
    let media_exists = sqlx::query_scalar::<_, bool>("SELECT 1 FROM media WHERE id = ?")
        .bind(media_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(%e, media_id, "failed to check media existence for scrobble");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .unwrap_or(false);

    if !media_exists {
        return Err(StatusCode::NOT_FOUND);
    }

    // 2. Proteção contra duplicatas no backend: se já registrou scrobble para este media_id
    // nos últimos 10 segundos, retorna OK com o registro existente (idempotência)
    let recent = sqlx::query_as::<_, (i64, i64, Option<i64>, String)>(
        "SELECT id, media_id, duration_ms, watched_at
         FROM playback_scrobbles
         WHERE media_id = ?
           AND watched_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 seconds')
         ORDER BY id DESC
         LIMIT 1",
    )
    .bind(media_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, media_id, "failed to check recent scrobble");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some((id, media_id, duration_ms, watched_at)) = recent {
        return Ok((
            StatusCode::OK,
            Json(PlaybackScrobbleResponse {
                id,
                media_id,
                duration_ms,
                watched_at,
            }),
        ));
    }

    // 3. Inserir novo scrobble
    let row = sqlx::query_as::<_, (i64, i64, Option<i64>, String)>(
        "INSERT INTO playback_scrobbles (media_id, duration_ms, watched_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, media_id, duration_ms, watched_at",
    )
    .bind(media_id)
    .bind(payload.duration_ms)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, media_id, "failed to record playback scrobble");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tracing::info!(
        media_id,
        scrobble_id = row.0,
        "internal video scrobble recorded (>=50%)"
    );

    Ok((
        StatusCode::CREATED,
        Json(PlaybackScrobbleResponse {
            id: row.0,
            media_id: row.1,
            duration_ms: row.2,
            watched_at: row.3,
        }),
    ))
}

pub(crate) async fn get_playback_stats(
    State(state): State<AppState>,
    Query(query): Query<PlaybackStatsQuery>,
) -> Result<Json<PlaybackStatsResponse>, StatusCode> {
    let period = query.period.as_deref().unwrap_or("week");
    let since_modifier = match period {
        "month" => "-30 days",
        "year" => "-365 days",
        "all" => "-100 years",
        _ => "-7 days", // "week" ou default
    };

    let canonical_period = match period {
        "month" => "month",
        "year" => "year",
        "all" => "all",
        _ => "week",
    };

    // 1. Resumo (total de scrobbles e tempo total assistido)
    let summary = sqlx::query_as::<_, (i64, i64)>(
        "SELECT COUNT(s.id),
                COALESCE(SUM(COALESCE(s.duration_ms, m.duration_ms, 0)), 0)
         FROM playback_scrobbles s
         JOIN media m ON m.id = s.media_id
         WHERE m.is_available = 1 AND s.watched_at >= datetime('now', ?)",
    )
    .bind(since_modifier)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, "failed to query playback stats summary");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 2. Top 10 Vídeos
    let video_rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<i64>,
            bool,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            i64,
        ),
    >(
        "SELECT m.id,
                m.title,
                m.duration_ms,
                m.is_short,
                c.id,
                c.name,
                c.avatar_url,
                strftime('%s', t.updated_at),
                COUNT(s.id),
                COALESCE(SUM(COALESCE(s.duration_ms, m.duration_ms, 0)), 0)
         FROM playback_scrobbles s
         JOIN media m ON m.id = s.media_id
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE m.is_available = 1 AND s.watched_at >= datetime('now', ?)
         GROUP BY m.id
         ORDER BY COUNT(s.id) DESC, COALESCE(SUM(COALESCE(s.duration_ms, m.duration_ms, 0)), 0) DESC
         LIMIT 10",
    )
    .bind(since_modifier)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, "failed to query top videos stats");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let top_videos = video_rows
        .into_iter()
        .map(
            |(
                id,
                title,
                duration_ms,
                is_short,
                channel_id,
                channel_name,
                channel_avatar_url,
                thumb_ver,
                play_count,
                total_watch_time_ms,
            )| {
                let thumbnail_url = match &thumb_ver {
                    Some(v) if !v.is_empty() => format!("/api/media/{id}/thumbnail?v={v}"),
                    _ => format!("/api/media/{id}/thumbnail"),
                };
                let resolved_channel_avatar = if channel_avatar_url.is_some() {
                    channel_id
                        .as_ref()
                        .map(|cid| format!("/api/channels/{cid}/avatar"))
                } else {
                    None
                };
                TopVideoStat {
                    id,
                    media_id: id,
                    title,
                    duration_ms,
                    is_short,
                    channel_id,
                    channel_name,
                    channel_avatar_url: resolved_channel_avatar,
                    thumbnail_url,
                    play_count,
                    total_watch_time_ms,
                }
            },
        )
        .collect();

    // 3. Top 10 Canais
    let channel_rows =
        sqlx::query_as::<_, (String, String, Option<String>, Option<String>, i64, i64)>(
            "SELECT c.id,
                c.name,
                c.avatar_url,
                c.handle,
                COUNT(s.id),
                COUNT(DISTINCT m.id)
         FROM playback_scrobbles s
         JOIN media m ON m.id = s.media_id
         JOIN media_channels mc ON mc.media_id = m.id
         JOIN channels c ON c.id = mc.channel_id
         WHERE m.is_available = 1 AND s.watched_at >= datetime('now', ?)
         GROUP BY c.id
         ORDER BY COUNT(s.id) DESC, COUNT(DISTINCT m.id) DESC
         LIMIT 10",
        )
        .bind(since_modifier)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to query top channels stats");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let top_channels = channel_rows
        .into_iter()
        .map(
            |(id, name, avatar_url, handle, play_count, unique_videos_count)| {
                let resolved_avatar = if avatar_url.is_some() {
                    Some(format!("/api/channels/{id}/avatar"))
                } else {
                    None
                };
                TopChannelStat {
                    id: id.clone(),
                    channel_id: id,
                    name: name.clone(),
                    channel_name: name,
                    avatar_url: resolved_avatar,
                    handle,
                    play_count,
                    unique_videos_count,
                }
            },
        )
        .collect();

    // 4. Top 10 Tags (com agrupamento unificado por casing, filtro de ruído e diversidade por vídeos distintos)
    let noise_in_clause = NOISE_TAGS
        .iter()
        .map(|t| format!("'{}'", t.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    let tag_query = format!(
        "SELECT MIN(tg.id),
                MIN(tg.display_name),
                LOWER(TRIM(tg.normalized_name)),
                COUNT(DISTINCT s.id),
                COUNT(DISTINCT m.id)
         FROM playback_scrobbles s
         JOIN media m ON m.id = s.media_id
         JOIN media_tags mt ON mt.media_id = m.id
         JOIN tags tg ON tg.id = mt.tag_id
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         WHERE m.is_available = 1
           AND s.watched_at >= datetime('now', ?)
           AND LENGTH(TRIM(tg.normalized_name)) > 1
           AND LOWER(TRIM(tg.normalized_name)) NOT IN ({noise_in_clause})
           AND (c.name IS NULL OR LOWER(TRIM(tg.normalized_name)) <> LOWER(TRIM(c.name)))
           AND (c.handle IS NULL OR LOWER(TRIM(tg.normalized_name)) <> LOWER(TRIM(REPLACE(c.handle, '@', ''))))
         GROUP BY LOWER(TRIM(tg.normalized_name))
         ORDER BY COUNT(DISTINCT m.id) DESC, COUNT(DISTINCT s.id) DESC, LOWER(TRIM(tg.normalized_name)) ASC
         LIMIT 10"
    );

    let tag_rows = sqlx::query_as::<_, (i64, String, String, i64, i64)>(sqlx::AssertSqlSafe(
        tag_query.as_str(),
    ))
    .bind(since_modifier)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, "failed to query top tags stats");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let top_tags = tag_rows
        .into_iter()
        .map(
            |(id, raw_display_name, normalized_name, play_count, unique_videos_count)| {
                let display_name = format_tag_display_name(&raw_display_name);
                TopTagStat {
                    id,
                    tag_id: id,
                    display_name,
                    normalized_name: normalized_name.clone(),
                    tag_name: normalized_name,
                    play_count,
                    unique_videos_count,
                    video_count: unique_videos_count,
                }
            },
        )
        .collect();

    Ok(Json(PlaybackStatsResponse {
        period: canonical_period.to_string(),
        total_scrobbles: summary.0,
        total_watch_time_ms: summary.1,
        top_videos,
        top_channels,
        top_tags,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_90_percent_completion_threshold() {
        let duration = Some(100_000); // 100 segundos
        assert!(!duration.is_some_and(|dur| dur > 0 && 89_000.0 >= (dur as f64 * 0.90)));
        assert!(duration.is_some_and(|dur| dur > 0 && 90_000.0 >= (dur as f64 * 0.90)));
        assert!(duration.is_some_and(|dur| dur > 0 && 95_000.0 >= (dur as f64 * 0.90)));
    }

    #[tokio::test]
    async fn test_history_crud_lifecycle() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Insert media
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (1, 'Video 1', 60000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (2, 'Video 2', 60000)")
            .execute(&pool)
            .await
            .unwrap();

        // 1. Initial history is empty
        let hist = list_history(State(state.clone())).await.unwrap().0;
        assert_eq!(hist.len(), 0);

        // 2. Update playback for video 1
        let _ = update_playback_state(
            State(state.clone()),
            Path(1),
            Json(UpdatePlaybackRequest {
                position_ms: 15000,
                duration_ms: Some(60000),
            }),
        )
        .await
        .unwrap();

        let hist = list_history(State(state.clone())).await.unwrap().0;
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].media.id, 1);
        assert_eq!(hist[0].media.playback.as_ref().unwrap().position_ms, 15000);

        sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms) VALUES (1, 60000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO playback_engagement_sessions (session_id, media_id, watched_ms, duration_ms) VALUES ('history-session-0001', 1, 15000, 60000)")
            .execute(&pool)
            .await
            .unwrap();

        // 3. Remove item 1
        remove_history_item(State(state.clone()), Path(1))
            .await
            .unwrap();
        let hist = list_history(State(state.clone())).await.unwrap().0;
        assert_eq!(hist.len(), 0);
        let remaining_signals: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM playback_scrobbles WHERE media_id=1)
                  + (SELECT COUNT(*) FROM playback_engagement_sessions WHERE media_id=1)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_signals, 0);

        // 4. Update playback for video 2, then clear history
        let _ = update_playback_state(
            State(state.clone()),
            Path(2),
            Json(UpdatePlaybackRequest {
                position_ms: 55000,
                duration_ms: Some(60000),
            }),
        )
        .await
        .unwrap();

        let hist = list_history(State(state.clone())).await.unwrap().0;
        assert_eq!(hist.len(), 1);

        sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms) VALUES (2, 60000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO playback_engagement_sessions (session_id, media_id, watched_ms, duration_ms) VALUES ('history-session-0002', 2, 55000, 60000)")
            .execute(&pool)
            .await
            .unwrap();

        clear_history(State(state.clone())).await.unwrap();
        let hist_cleared = list_history(State(state)).await.unwrap().0;
        assert_eq!(hist_cleared.len(), 0);
        let remaining_signals: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM playback_scrobbles)
                  + (SELECT COUNT(*) FROM playback_engagement_sessions)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_signals, 0);
    }

    #[tokio::test]
    async fn playback_engagement_is_idempotent_per_session() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (9, 'Retention', 100000)")
            .execute(&pool)
            .await
            .unwrap();

        for watched_ms in [20_000, 65_000] {
            let status = record_playback_engagement(
                State(state.clone()),
                Path(9),
                Json(RecordPlaybackEngagementRequest {
                    session_id: "retention-session-0001".to_string(),
                    watched_ms,
                    duration_ms: 100_000,
                    completed: false,
                }),
            )
            .await
            .unwrap();
            assert_eq!(status, StatusCode::NO_CONTENT);
        }

        let stored: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), MAX(watched_ms) FROM playback_engagement_sessions WHERE media_id=9",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, (1, 65_000));

        let invalid = record_playback_engagement(
            State(state),
            Path(9),
            Json(RecordPlaybackEngagementRequest {
                session_id: "short".to_string(),
                watched_ms: 120_000,
                duration_ms: 100_000,
                completed: false,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(invalid, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_playback_scrobble_lifecycle() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query(
            "INSERT INTO media (id, title, duration_ms) VALUES (10, 'Music Video', 180000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 1. Gravar primeiro scrobble
        let (status, Json(scrobble)) = record_playback_scrobble(
            State(state.clone()),
            Path(10),
            Json(RecordPlaybackScrobbleRequest {
                duration_ms: Some(180000),
            }),
        )
        .await
        .unwrap();

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(scrobble.media_id, 10);
        assert_eq!(scrobble.duration_ms, Some(180000));
        assert!(!scrobble.watched_at.is_empty());

        // 2. Tentar registrar novamente em menos de 10s (deve retornar OK idempotente sem duplicar)
        let (status_dup, Json(scrobble_dup)) = record_playback_scrobble(
            State(state.clone()),
            Path(10),
            Json(RecordPlaybackScrobbleRequest {
                duration_ms: Some(180000),
            }),
        )
        .await
        .unwrap();

        assert_eq!(status_dup, StatusCode::OK);
        assert_eq!(scrobble_dup.id, scrobble.id);

        let total_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM playback_scrobbles WHERE media_id = 10")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(total_count, 1);

        // 3. Mídia inexistente retorna 404
        let err = record_playback_scrobble(
            State(state.clone()),
            Path(9999),
            Json(RecordPlaybackScrobbleRequest { duration_ms: None }),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::NOT_FOUND);

        // 4. Deleção da mídia apaga scrobbles via CASCADE
        sqlx::query("DELETE FROM media WHERE id = 10")
            .execute(&pool)
            .await
            .unwrap();
        let count_after_delete: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM playback_scrobbles WHERE media_id = 10")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count_after_delete, 0);
    }

    #[tokio::test]
    async fn test_playback_stats_endpoint() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Setup channel, tag and media
        sqlx::query("INSERT INTO channels (id, name, avatar_url) VALUES ('ch_rock', 'Rock Channel', 'https://yt3.googleusercontent.com/avatar123')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'Metal', 'metal')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (50, 'Rock Song', 200000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (50, 'ch_rock', 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (50, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        // Inserir scrobble recente
        sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms, watched_at) VALUES (50, 200000, datetime('now'))")
            .execute(&pool)
            .await
            .unwrap();

        // Inserir scrobble antigo (há 60 dias)
        sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms, watched_at) VALUES (50, 200000, datetime('now', '-60 days'))")
            .execute(&pool)
            .await
            .unwrap();

        // Testar semana (apenas o scrobble recente deve entrar)
        let res_week = get_playback_stats(
            State(state.clone()),
            Query(PlaybackStatsQuery {
                period: Some("week".to_string()),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(res_week.period, "week");
        assert_eq!(res_week.total_scrobbles, 1);
        assert_eq!(res_week.top_videos.len(), 1);
        assert_eq!(res_week.top_videos[0].id, 50);
        assert_eq!(res_week.top_videos[0].play_count, 1);
        assert_eq!(
            res_week.top_videos[0].channel_avatar_url,
            Some("/api/channels/ch_rock/avatar".to_string())
        );
        assert_eq!(res_week.top_channels.len(), 1);
        assert_eq!(res_week.top_channels[0].id, "ch_rock");
        assert_eq!(
            res_week.top_channels[0].avatar_url,
            Some("/api/channels/ch_rock/avatar".to_string())
        );
        assert_eq!(res_week.top_tags.len(), 1);
        assert_eq!(res_week.top_tags[0].display_name, "Metal");

        // Testar ano (ambos os scrobbles devem entrar)
        let res_year = get_playback_stats(
            State(state.clone()),
            Query(PlaybackStatsQuery {
                period: Some("year".to_string()),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(res_year.period, "year");
        assert_eq!(res_year.total_scrobbles, 2);
        assert_eq!(res_year.top_videos[0].play_count, 2);
    }

    #[test]
    fn test_format_tag_display_name() {
        assert_eq!(format_tag_display_name("metal"), "Metal");
        assert_eq!(
            format_tag_display_name("female fronted metal"),
            "Female Fronted Metal"
        );
        assert_eq!(format_tag_display_name("Example Artist"), "Example Artist");
        assert_eq!(
            format_tag_display_name("Symphonic Metal"),
            "Symphonic Metal"
        );
        assert_eq!(format_tag_display_name("   "), "");
    }

    #[tokio::test]
    async fn test_playback_stats_tags_diversity_unification_and_noise_filtering() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Generic channel; its name and handle must be treated as tag noise.
        sqlx::query(
            "INSERT INTO channels (id, name, handle) VALUES ('ch_example', 'Example Creator', '@examplecreator')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Tags:
        // 1: "symphonic metal" (válida, presente em 2 vídeos)
        // 2: "official video" (ruído, deve ser filtrada)
        // 3: channel name (must be filtered)
        // 4: "gothic metal" (válida, presente em apenas 1 vídeo, mas com 5 scrobbles)
        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'symphonic metal', 'symphonic metal')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (2, 'official video', 'official video')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tags (id, display_name, normalized_name) VALUES (3, 'Example Creator', 'example creator')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (4, 'gothic metal', 'gothic metal')")
            .execute(&pool)
            .await
            .unwrap();

        // Video 1 has a useful tag and two noise tags.
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (101, 'Song 1', 180000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (101, 'ch_example', 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (101, 1, 'manual'), (101, 2, 'manual'), (101, 3, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        // Vídeo 2 (com tag: symphonic metal)
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (102, 'Song 2', 200000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (102, 'ch_example', 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (102, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        // Vídeo 3 (com tag: gothic metal)
        sqlx::query("INSERT INTO media (id, title, duration_ms) VALUES (103, 'Song 3', 220000)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (103, 'ch_example', 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (103, 4, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        // Scrobbles:
        // Vídeo 1: 1 reprodução
        sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms, watched_at) VALUES (101, 180000, datetime('now'))")
            .execute(&pool)
            .await
            .unwrap();
        // Vídeo 2: 1 reprodução
        sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms, watched_at) VALUES (102, 200000, datetime('now'))")
            .execute(&pool)
            .await
            .unwrap();
        // Vídeo 3: 5 reproduções (mas todas no mesmo vídeo 103)
        for _ in 0..5 {
            sqlx::query("INSERT INTO playback_scrobbles (media_id, duration_ms, watched_at) VALUES (103, 220000, datetime('now'))")
                .execute(&pool)
                .await
                .unwrap();
        }

        let res = get_playback_stats(
            State(state.clone()),
            Query(PlaybackStatsQuery {
                period: Some("week".to_string()),
            }),
        )
        .await
        .unwrap()
        .0;

        // "official video" and the channel name must be ignored.
        assert_eq!(res.top_tags.len(), 2);

        // #1 deve ser "Symphonic Metal" (2 vídeos distintos > 1 vídeo distinto, regra de diversidade)
        assert_eq!(res.top_tags[0].normalized_name, "symphonic metal");
        assert_eq!(res.top_tags[0].display_name, "Symphonic Metal");
        assert_eq!(res.top_tags[0].unique_videos_count, 2);
        assert_eq!(res.top_tags[0].play_count, 2);

        // #2 deve ser "Gothic Metal" (1 vídeo distinto, apesar de ter 5 plays)
        assert_eq!(res.top_tags[1].normalized_name, "gothic metal");
        assert_eq!(res.top_tags[1].display_name, "Gothic Metal");
        assert_eq!(res.top_tags[1].unique_videos_count, 1);
        assert_eq!(res.top_tags[1].play_count, 5);
    }
}
