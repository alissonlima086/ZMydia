use std::collections::HashMap;

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::web::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MediaItem {
    pub id: i64,
    pub title: String,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub rotation: Option<i64>,
    pub is_short: bool,
    pub is_available: bool,
    pub is_favorite: bool,
    pub is_official_thumbnail: bool,
    pub description: Option<String>,
    pub user_notes: Option<String>,
    pub youtube_id: Option<String>,
    pub published_at: Option<String>,
    pub channel: Option<ChannelRef>,
    pub tags: Vec<TagRef>,
    pub thumbnail_url: String,
    pub playback: Option<PlaybackSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlaybackSummary {
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub completed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChannelRef {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TagRef {
    pub id: i64,
    pub display_name: String,
    pub normalized_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TagSummary {
    pub id: i64,
    pub display_name: String,
    pub normalized_name: String,
    pub media_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelSummary {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub youtube_channel_id: Option<String>,
    pub handle: Option<String>,
    pub has_name_conflict: bool,
    pub media_count: i64,
    pub latest_thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ChannelListFilter {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub exclude_home_hidden: Option<bool>,
}

#[derive(sqlx::FromRow, Debug)]
pub struct MediaDbRow {
    pub id: i64,
    pub title: String,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub rotation: Option<i64>,
    pub is_short: bool,
    pub is_available: bool,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub channel_avatar_url: Option<String>,
    pub position_ms: Option<i64>,
    pub completed: Option<bool>,
    pub is_favorite: bool,
    pub is_official_thumbnail: bool,
    pub thumbnail_version: Option<String>,
    pub description: Option<String>,
    pub user_notes: Option<String>,
    pub youtube_id: Option<String>,
    pub published_at: Option<String>,
}

impl MediaDbRow {
    pub fn into_media_item(self, tag_map: &mut HashMap<i64, Vec<TagRef>>) -> MediaItem {
        let channel = match (self.channel_id, self.channel_name) {
            (Some(id), Some(name)) => {
                let avatar_url = self
                    .channel_avatar_url
                    .map(|_| format!("/api/channels/{id}/avatar"));
                Some(ChannelRef {
                    id,
                    name,
                    avatar_url,
                })
            }
            _ => None,
        };
        let tags = tag_map.remove(&self.id).unwrap_or_default();
        let playback = match (self.position_ms, self.completed) {
            (Some(position_ms), Some(completed)) => Some(PlaybackSummary {
                position_ms,
                duration_ms: self.duration_ms,
                completed,
            }),
            _ => None,
        };
        let thumbnail_url = match &self.thumbnail_version {
            Some(v) if !v.is_empty() => format!("/api/media/{}/thumbnail?v={v}", self.id),
            _ => format!("/api/media/{}/thumbnail", self.id),
        };
        MediaItem {
            id: self.id,
            title: self.title,
            duration_ms: self.duration_ms,
            width: self.width,
            height: self.height,
            rotation: self.rotation,
            is_short: self.is_short,
            is_available: self.is_available,
            is_favorite: self.is_favorite,
            is_official_thumbnail: self.is_official_thumbnail,
            description: self.description,
            user_notes: self.user_notes,
            youtube_id: self.youtube_id,
            published_at: self.published_at,
            channel,
            tags,
            thumbnail_url,
            playback,
        }
    }
}

pub const MEDIA_SELECT_COLUMNS: &str = "\
    m.id, m.title, m.duration_ms, m.width, m.height, m.rotation, m.is_short, m.is_available, \
    c.id as channel_id, c.name as channel_name, c.avatar_url as channel_avatar_url, \
    ps.position_ms, ps.completed, \
    (fav.media_id IS NOT NULL) as is_favorite, \
    COALESCE(t.is_official, 0) as is_official_thumbnail, \
    strftime('%s', t.updated_at) as thumbnail_version, \
    m.description, m.user_notes, m.youtube_id, m.published_at\
";

#[derive(sqlx::FromRow, Debug)]
pub struct RelatedMediaDbRow {
    #[sqlx(flatten)]
    pub media_row: MediaDbRow,
    pub relevance_score: i64,
}

impl std::ops::Deref for RelatedMediaDbRow {
    type Target = MediaDbRow;
    fn deref(&self) -> &Self::Target {
        &self.media_row
    }
}

impl RelatedMediaDbRow {
    pub fn into_media_item(self, tag_map: &mut HashMap<i64, Vec<TagRef>>) -> MediaItem {
        self.media_row.into_media_item(tag_map)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct HomeSettings {
    pub hidden_tags: Vec<String>,
}

pub async fn load_home_settings(pool: &sqlx::SqlitePool) -> HomeSettings {
    let row =
        sqlx::query_as::<_, (String,)>("SELECT value_json FROM settings WHERE key = 'home_filter'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some((json,)) = row {
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        HomeSettings::default()
    }
}

pub async fn save_home_settings(
    pool: &sqlx::SqlitePool,
    settings: &HomeSettings,
) -> Result<(), sqlx::Error> {
    let mut normalized_tags: Vec<String> = settings
        .hidden_tags
        .iter()
        .map(|t| t.trim().trim_start_matches('#').to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    normalized_tags.sort();
    normalized_tags.dedup();

    let clean = HomeSettings {
        hidden_tags: normalized_tags,
    };
    let json_str = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".to_string());

    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES ('home_filter', ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at",
    )
    .bind(json_str)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Deserialize, Default)]
pub struct MediaFilter {
    pub tag: Option<String>,
    pub channel: Option<String>,
    pub is_short: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub sort: Option<String>,
    pub exclude_home_hidden: Option<bool>,
}

pub(crate) async fn list_media(
    State(state): State<AppState>,
    Query(filter): Query<MediaFilter>,
) -> Result<Json<Vec<MediaItem>>, StatusCode> {
    let limit = filter.limit.unwrap_or(50).clamp(1, 200);
    let offset = filter.offset.unwrap_or(0).max(0);

    let hidden_tags = if filter.exclude_home_hidden == Some(true) {
        load_home_settings(&state.pool).await.hidden_tags
    } else {
        Vec::new()
    };

    let mut query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS}
         FROM media m
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN playback_state ps ON ps.media_id = m.id
         LEFT JOIN favorites fav ON fav.media_id = m.id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE m.is_available = 1",
    );

    if filter.tag.is_some() {
        query.push_str(
            " AND m.id IN (
                SELECT mt.media_id FROM media_tags mt
                JOIN tags t ON t.id = mt.tag_id
                WHERE t.normalized_name = lower(?) OR t.display_name = ?
            )",
        );
    }

    if filter.channel.is_some() {
        query.push_str(" AND (mc.channel_id = ? OR c.name = ?)");
    }

    if !hidden_tags.is_empty() {
        let tag_conditions = hidden_tags
            .iter()
            .map(|_| "t.normalized_name LIKE ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        let title_conditions = hidden_tags
            .iter()
            .map(|_| "lower(m.title) NOT LIKE ?")
            .collect::<Vec<_>>()
            .join(" AND ");
        let channel_conditions = hidden_tags
            .iter()
            .map(|_| "(c.name IS NULL OR lower(c.name) NOT LIKE ?)")
            .collect::<Vec<_>>()
            .join(" AND ");
        query.push_str(&format!(
            " AND m.id NOT IN (
                SELECT mt.media_id FROM media_tags mt
                JOIN tags t ON t.id = mt.tag_id
                WHERE {tag_conditions}
            ) AND {title_conditions} AND {channel_conditions}"
        ));
    }

    if let Some(is_short) = filter.is_short {
        if is_short {
            query.push_str(" AND m.is_short = 1");
        } else {
            query.push_str(" AND m.is_short = 0");
        }
    }

    let order_clause = match filter.sort.as_deref() {
        Some("title_asc") => "m.title COLLATE NOCASE ASC",
        Some("title_desc") => "m.title COLLATE NOCASE DESC",
        Some("duration_desc") => "m.duration_ms DESC NULLS LAST",
        Some("duration_asc") => "m.duration_ms ASC NULLS LAST",
        Some("recent") | Some("published_desc") => {
            "m.published_at IS NULL ASC, m.published_at DESC, m.id DESC"
        }
        Some("published_asc") => "m.published_at IS NULL ASC, m.published_at ASC, m.id ASC",
        Some("oldest") => "m.id ASC",
        _ => "m.id DESC",
    };

    query.push_str(&format!(" ORDER BY {order_clause} LIMIT ? OFFSET ?"));

    let mut sqlx_query = sqlx::query_as::<_, MediaDbRow>(sqlx::AssertSqlSafe(query.as_str()));

    if let Some(tag) = &filter.tag {
        sqlx_query = sqlx_query.bind(tag).bind(tag);
    }

    if let Some(channel) = &filter.channel {
        sqlx_query = sqlx_query.bind(channel).bind(channel);
    }

    if !hidden_tags.is_empty() {
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
    }

    sqlx_query = sqlx_query.bind(limit).bind(offset);

    let rows = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query media list");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if rows.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let media_ids: Vec<i64> = rows.iter().map(|row| row.id).collect();

    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids).await?;

    let items = rows
        .into_iter()
        .map(|row| row.into_media_item(&mut tag_map))
        .collect();

    Ok(Json(items))
}

pub(crate) async fn get_media(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
) -> Result<Json<MediaItem>, StatusCode> {
    let query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS}
         FROM media m
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN playback_state ps ON ps.media_id = m.id
         LEFT JOIN favorites fav ON fav.media_id = m.id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE m.id = ?"
    );

    let row = sqlx::query_as::<_, MediaDbRow>(sqlx::AssertSqlSafe(query.as_str()))
        .bind(media_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to get media by id");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let mut tag_map = fetch_tags_for_media(&state.pool, &[media_id]).await?;
    Ok(Json(row.into_media_item(&mut tag_map)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotesRequest {
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateNotesResponse {
    pub user_notes: Option<String>,
}

pub(crate) async fn update_media_notes(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
    Json(payload): Json<UpdateNotesRequest>,
) -> Result<Json<UpdateNotesResponse>, StatusCode> {
    let clean_notes = payload
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let result = sqlx::query(
        "UPDATE media SET user_notes = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?"
    )
    .bind(&clean_notes)
    .bind(media_id)
    .execute(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to update media notes");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(UpdateNotesResponse {
        user_notes: clean_notes,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct RelatedFilter {
    pub exclude_hidden: Option<bool>,
}

pub(crate) async fn get_related_media(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
    Query(filter): Query<RelatedFilter>,
) -> Result<Json<Vec<MediaItem>>, StatusCode> {
    let rec_settings = crate::recommender::load_recommendation_settings(&state.pool).await;
    if rec_settings.enabled {
        let items = crate::recommender::get_recommended_related_media(
            &state,
            media_id,
            filter.exclude_hidden.unwrap_or(true),
            &rec_settings,
        )
        .await?;
        return Ok(Json(items));
    }

    let limit = 20i64;
    let should_exclude = filter.exclude_hidden.unwrap_or(true);
    let hidden_tags = if should_exclude {
        load_home_settings(&state.pool).await.hidden_tags
    } else {
        Vec::new()
    };

    let mut query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS},
               (
                   COALESCE((
                       SELECT COUNT(*) * 3
                       FROM media_tags mt
                       WHERE mt.media_id = m.id
                         AND mt.tag_id IN (SELECT tag_id FROM media_tags WHERE media_id = ?)
                   ), 0)
                   +
                   COALESCE((
                       SELECT CASE WHEN mc.channel_id = cur_c.channel_id THEN 5 ELSE 0 END
                       FROM media_channels cur_c
                       WHERE cur_c.media_id = ?
                   ), 0)
               ) as relevance_score
        FROM media m
        LEFT JOIN media_channels mc ON mc.media_id = m.id
        LEFT JOIN channels c ON c.id = mc.channel_id
        LEFT JOIN playback_state ps ON ps.media_id = m.id
        LEFT JOIN favorites fav ON fav.media_id = m.id
        LEFT JOIN thumbnails t ON t.media_id = m.id
        WHERE m.is_available = 1 AND m.is_short = 0 AND m.id <> ?",
    );

    if !hidden_tags.is_empty() {
        let tag_conditions = hidden_tags
            .iter()
            .map(|_| "t.normalized_name LIKE ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        let title_conditions = hidden_tags
            .iter()
            .map(|_| "lower(m.title) NOT LIKE ?")
            .collect::<Vec<_>>()
            .join(" AND ");
        let channel_conditions = hidden_tags
            .iter()
            .map(|_| "(c.name IS NULL OR lower(c.name) NOT LIKE ?)")
            .collect::<Vec<_>>()
            .join(" AND ");
        query.push_str(&format!(
            " AND m.id NOT IN (
                SELECT mt.media_id FROM media_tags mt
                JOIN tags t ON t.id = mt.tag_id
                WHERE {tag_conditions}
            ) AND {title_conditions} AND {channel_conditions}"
        ));
    }

    query.push_str(" ORDER BY relevance_score DESC, m.id DESC LIMIT ?");

    let mut sqlx_query =
        sqlx::query_as::<_, RelatedMediaDbRow>(sqlx::AssertSqlSafe(query.as_str()))
            .bind(media_id)
            .bind(media_id)
            .bind(media_id);

    if !hidden_tags.is_empty() {
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
    }

    sqlx_query = sqlx_query.bind(limit);

    let rows = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query related media");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if rows.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let media_ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids).await?;

    let items = rows
        .into_iter()
        .map(|row| row.into_media_item(&mut tag_map))
        .collect();

    Ok(Json(items))
}

pub(crate) async fn fetch_tags_for_media(
    pool: &sqlx::SqlitePool,
    media_ids: &[i64],
) -> Result<HashMap<i64, Vec<TagRef>>, StatusCode> {
    if media_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query_str = String::with_capacity(160 + media_ids.len() * 2);
    query_str.push_str(
        "SELECT mt.media_id, t.id, t.display_name, t.normalized_name
         FROM media_tags mt
         JOIN tags t ON t.id = mt.tag_id
         WHERE mt.media_id IN (",
    );
    for (i, _) in media_ids.iter().enumerate() {
        if i > 0 {
            query_str.push(',');
        }
        query_str.push('?');
    }
    query_str.push_str(") ORDER BY t.display_name ASC");

    let mut query =
        sqlx::query_as::<_, (i64, i64, String, String)>(sqlx::AssertSqlSafe(query_str.as_str()));
    for id in media_ids {
        query = query.bind(id);
    }

    let rows = query.fetch_all(pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query media tags");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut map: HashMap<i64, Vec<TagRef>> = HashMap::with_capacity(media_ids.len());
    for (media_id, tag_id, display_name, normalized_name) in rows {
        map.entry(media_id).or_default().push(TagRef {
            id: tag_id,
            display_name,
            normalized_name,
        });
    }

    Ok(map)
}

#[derive(Debug, Deserialize, Default)]
pub struct TagsFilter {
    pub exclude_home_hidden: Option<bool>,
}

pub(crate) async fn list_tags(
    State(state): State<AppState>,
    Query(filter): Query<TagsFilter>,
) -> Result<Json<Vec<TagSummary>>, StatusCode> {
    let hidden_tags = if filter.exclude_home_hidden == Some(true) {
        load_home_settings(&state.pool).await.hidden_tags
    } else {
        Vec::new()
    };

    let mut query = String::from(
        "SELECT t.id, t.display_name, t.normalized_name, COUNT(DISTINCT mt.media_id) as media_count
         FROM tags t
         JOIN media_tags mt ON mt.tag_id = t.id
         JOIN media m ON m.id = mt.media_id
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         WHERE m.is_available = 1",
    );

    if !hidden_tags.is_empty() {
        let tag_conditions = hidden_tags
            .iter()
            .map(|_| "t.normalized_name LIKE ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        let sub_tag_conditions = hidden_tags
            .iter()
            .map(|_| "t2.normalized_name LIKE ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        let title_conditions = hidden_tags
            .iter()
            .map(|_| "lower(m.title) NOT LIKE ?")
            .collect::<Vec<_>>()
            .join(" AND ");
        let channel_conditions = hidden_tags
            .iter()
            .map(|_| "(c.name IS NULL OR lower(c.name) NOT LIKE ?)")
            .collect::<Vec<_>>()
            .join(" AND ");
        query.push_str(&format!(
            " AND NOT ({tag_conditions})
              AND m.id NOT IN (
                  SELECT mt2.media_id FROM media_tags mt2
                  JOIN tags t2 ON t2.id = mt2.tag_id
                  WHERE {sub_tag_conditions}
              )
              AND {title_conditions}
              AND {channel_conditions}"
        ));
    }

    query.push_str(
        " GROUP BY t.id
         HAVING media_count > 0
         ORDER BY media_count DESC, t.display_name ASC",
    );

    let mut sqlx_query =
        sqlx::query_as::<_, (i64, String, String, i64)>(sqlx::AssertSqlSafe(query.as_str()));

    if !hidden_tags.is_empty() {
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
        for tag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{tag}%"));
        }
    }

    let rows = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query tags summary");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let tags = rows
        .into_iter()
        .map(
            |(id, display_name, normalized_name, media_count)| TagSummary {
                id,
                display_name,
                normalized_name,
                media_count,
            },
        )
        .collect();

    Ok(Json(tags))
}

pub(crate) async fn list_channels(
    State(state): State<AppState>,
    Query(filter): Query<ChannelListFilter>,
) -> Result<Json<Vec<ChannelSummary>>, StatusCode> {
    let search_term = filter
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let limit = filter.limit.map(|value| value.clamp(1, 100));
    let hidden_terms = if filter.exclude_home_hidden == Some(true) {
        load_home_settings(&state.pool).await.hidden_tags
    } else {
        Vec::new()
    };

    let mut query = String::from(
        "SELECT c.id, c.name, c.avatar_url, c.youtube_channel_id, c.handle, c.has_name_conflict,
                COUNT(DISTINCT mc.media_id) as media_count,
                MAX(mc.media_id) as latest_media_id
         FROM channels c
         JOIN media_channels mc ON mc.channel_id = c.id
         JOIN media m ON m.id = mc.media_id AND m.is_available = 1",
    );
    let mut conditions = Vec::new();
    if search_term.is_some() {
        conditions
            .push("(instr(lower(c.name), ?) > 0 OR instr(lower(COALESCE(c.handle, '')), ?) > 0)");
    }
    for _ in &hidden_terms {
        conditions
            .push("(instr(lower(c.name), ?) = 0 AND instr(lower(COALESCE(c.handle, '')), ?) = 0)");
    }
    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }
    query.push_str(" GROUP BY c.id");
    if search_term.is_some() {
        query.push_str(
            " ORDER BY CASE
                 WHEN lower(c.name) = ? OR lower(COALESCE(c.handle, '')) = ? THEN 0
                 WHEN instr(lower(c.name), ?) = 1 OR instr(lower(COALESCE(c.handle, '')), ?) = 1 THEN 1
                 ELSE 2
               END,
               media_count DESC,
               c.name COLLATE NOCASE ASC",
        );
    } else {
        query.push_str(" ORDER BY c.name COLLATE NOCASE ASC");
    }
    if limit.is_some() {
        query.push_str(" LIMIT ?");
    }

    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            bool,
            i64,
            Option<i64>,
        ),
    >(sqlx::AssertSqlSafe(query.as_str()));
    let mut rows_query = rows;
    if let Some(term) = &search_term {
        rows_query = rows_query.bind(term).bind(term);
    }
    for term in &hidden_terms {
        rows_query = rows_query.bind(term).bind(term);
    }
    if let Some(term) = &search_term {
        rows_query = rows_query.bind(term).bind(term).bind(term).bind(term);
    }
    if let Some(limit) = limit {
        rows_query = rows_query.bind(limit);
    }
    let rows = rows_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query channels summary");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let channels = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                avatar_url,
                youtube_channel_id,
                handle,
                has_name_conflict,
                media_count,
                latest_media_id,
            )| {
                let resolved_avatar = if avatar_url.is_some() {
                    Some(format!("/api/channels/{id}/avatar"))
                } else {
                    None
                };
                ChannelSummary {
                    id,
                    name,
                    avatar_url: resolved_avatar,
                    youtube_channel_id,
                    handle,
                    has_name_conflict,
                    media_count,
                    latest_thumbnail_url: latest_media_id
                        .map(|mid| format!("/api/media/{mid}/thumbnail")),
                }
            },
        )
        .collect();

    Ok(Json(channels))
}

pub(crate) async fn get_channel(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<ChannelSummary>, StatusCode> {
    if !crate::validation::is_valid_channel_id(&channel_id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            bool,
            i64,
            Option<i64>,
        ),
    >(
        "SELECT c.id, c.name, c.avatar_url, c.youtube_channel_id, c.handle, c.has_name_conflict,
                COUNT(DISTINCT mc.media_id) as media_count,
                MAX(mc.media_id) as latest_media_id
         FROM channels c
         LEFT JOIN media_channels mc ON mc.channel_id = c.id
         LEFT JOIN media m ON m.id = mc.media_id AND m.is_available = 1
         WHERE c.id = ?
         GROUP BY c.id",
    )
    .bind(&channel_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to query single channel");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match row {
        Some((
            id,
            name,
            avatar_url,
            youtube_channel_id,
            handle,
            has_name_conflict,
            media_count,
            latest_media_id,
        )) => {
            let resolved_avatar = if avatar_url.is_some() {
                Some(format!("/api/channels/{id}/avatar"))
            } else {
                None
            };
            Ok(Json(ChannelSummary {
                id,
                name,
                avatar_url: resolved_avatar,
                youtube_channel_id,
                handle,
                has_name_conflict,
                media_count,
                latest_thumbnail_url: latest_media_id
                    .map(|mid| format!("/api/media/{mid}/thumbnail")),
            }))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

fn title_mentions_channel(title: &str, channel_name: &str) -> bool {
    let title_terms = mention_terms(title);
    let channel_terms = mention_terms(channel_name);
    !channel_terms.is_empty()
        && channel_terms
            .iter()
            .all(|term| title_terms.iter().any(|candidate| candidate == term))
}

fn mention_terms(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) async fn get_channel_mentions(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<Vec<MediaItem>>, StatusCode> {
    if !crate::validation::is_valid_channel_id(&channel_id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let channel_name = sqlx::query_scalar::<_, String>("SELECT name FROM channels WHERE id = ?")
        .bind(&channel_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, channel_id, "failed to load channel for mentions");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    let terms = mention_terms(&channel_name);
    if terms.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let term_conditions = terms
        .iter()
        .map(|_| "lower(m.title) LIKE '%' || lower(?) || '%'")
        .collect::<Vec<_>>()
        .join(" AND ");

    let query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS}
         FROM media m
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN playback_state ps ON ps.media_id = m.id
         LEFT JOIN favorites fav ON fav.media_id = m.id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE m.is_available = 1
           AND (mc.channel_id IS NULL OR mc.channel_id <> ?)
           AND {term_conditions}
         ORDER BY COALESCE(m.published_at, '') DESC, m.id DESC
         LIMIT 500"
    );
    let mut rows_query =
        sqlx::query_as::<_, MediaDbRow>(sqlx::AssertSqlSafe(query.as_str())).bind(&channel_id);
    for term in &terms {
        rows_query = rows_query.bind(term);
    }
    let rows = rows_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, channel_id, "failed to query channel mentions");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let rows = rows
        .into_iter()
        .filter(|row| title_mentions_channel(&row.title, &channel_name))
        .collect::<Vec<_>>();
    let media_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| row.into_media_item(&mut tag_map))
            .collect(),
    ))
}

pub(crate) async fn get_channel_avatar(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Response<Body> {
    if !crate::validation::is_valid_channel_id(&channel_id) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let query = "
        SELECT c.id, c.name, c.avatar_url,
               (SELECT mf.path FROM media_files mf
                JOIN media_channels mc ON mc.media_id = mf.media_id
                WHERE mc.channel_id = c.id AND mf.is_available = 1
                LIMIT 1) AS sample_file_path,
               (SELECT s.path FROM media_sources s WHERE s.enabled = 1 AND s.removed_at IS NULL LIMIT 1) AS source_path
        FROM channels c
        WHERE c.id = ?
    ";
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(query)
    .bind(&channel_id)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((id, _name, db_avatar_url, sample_file_path, source_path))) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if !crate::validation::is_valid_channel_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }

    // 1. Check if channel-local avatar exists: {channel_dir}/.channel/avatar.jpg
    if let Some(sample) = &sample_file_path {
        let sample_path = std::path::Path::new(sample);
        if let Some(channel_dir) = sample_path.parent() {
            let local_avatar = channel_dir.join(".channel").join("avatar.jpg");
            if local_avatar.is_file()
                && let Ok(bytes) = tokio::fs::read(&local_avatar).await
            {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "image/jpeg")
                    .header(header::CACHE_CONTROL, "public, max-age=86400")
                    .body(Body::from(bytes))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        }
    }

    // 2. Check if centralized cache avatar exists: {source_path}/.mymedia/avatars/{channel_id}.jpg
    if let Some(src) = &source_path {
        let central_avatar = std::path::Path::new(src)
            .join(".mymedia")
            .join("avatars")
            .join(format!("{id}.jpg"));
        if central_avatar.is_file()
            && let Ok(bytes) = tokio::fs::read(&central_avatar).await
        {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "public, max-age=86400")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }

    // 3. Remote avatars are limited to known HTTPS image hosts, image content and 5 MiB.
    if let Some(url) = db_avatar_url.and_then(|value| crate::validation::youtube_image_url(&value))
    {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build();
        if let Ok(client) = client
            && let Ok(response) = client.get(url).send().await
            && response.status().is_success()
            && response
                .content_length()
                .is_none_or(|length| length <= 5 * 1024 * 1024)
            && response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("image/"))
            && let Ok(bytes) = response.bytes().await
            && !bytes.is_empty()
            && bytes.len() <= 5 * 1024 * 1024
        {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "public, max-age=86400")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }

    // 4. If avatar was not found, attempt on-demand metadata sync for this channel
    if crate::youtube_worker::sync_single_channel_metadata(&state.pool, &channel_id).await {
        if let Some(src) = &source_path {
            let central_avatar = std::path::Path::new(src)
                .join(".mymedia")
                .join("avatars")
                .join(format!("{id}.jpg"));
            if central_avatar.is_file()
                && let Ok(bytes) = tokio::fs::read(&central_avatar).await
            {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "image/jpeg")
                    .header(header::CACHE_CONTROL, "public, max-age=86400")
                    .body(Body::from(bytes))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        }
        if let Some(sample) = &sample_file_path {
            let sample_path = std::path::Path::new(sample);
            if let Some(channel_dir) = sample_path.parent() {
                let local_avatar = channel_dir.join(".channel").join("avatar.jpg");
                if local_avatar.is_file()
                    && let Ok(bytes) = tokio::fs::read(&local_avatar).await
                {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/jpeg")
                        .header(header::CACHE_CONTROL, "public, max-age=86400")
                        .body(Body::from(bytes))
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
                }
            }
        }
    }

    StatusCode::NOT_FOUND.into_response()
}

pub(crate) async fn sync_channel(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> StatusCode {
    if !crate::validation::is_valid_channel_id(&channel_id) {
        return StatusCode::BAD_REQUEST;
    }
    let success =
        crate::youtube_worker::sync_single_channel_metadata(&state.pool, &channel_id).await;
    if success {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

pub(crate) async fn sync_all_channels(State(state): State<AppState>) -> StatusCode {
    let pool = state.pool.clone();
    tokio::spawn(async move {
        crate::youtube_worker::sync_pending_channels(&pool).await;
    });
    StatusCode::ACCEPTED
}

pub async fn sync_media_fts(conn: &mut sqlx::SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM media_fts")
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO media_fts (media_id, title, channel_name, tags, folder_path)
         SELECT
             m.id,
             m.title,
             COALESCE(c.name, ''),
             COALESCE((SELECT GROUP_CONCAT(t.display_name, ' ') FROM media_tags mt JOIN tags t ON t.id = mt.tag_id WHERE mt.media_id = m.id), ''),
             COALESCE((SELECT f.path FROM media_files f WHERE f.media_id = m.id AND f.is_available = 1 LIMIT 1), '')
         FROM media m
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         WHERE m.is_available = 1",
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_and_custom_media_filters() {
        let default_filter = MediaFilter::default();
        assert!(default_filter.tag.is_none());
        assert!(default_filter.channel.is_none());
        assert!(default_filter.is_short.is_none());

        let custom_filter = MediaFilter {
            tag: Some("example-tag".to_owned()),
            channel: Some("example-channel".to_owned()),
            is_short: Some(true),
            limit: Some(25),
            offset: Some(10),
            sort: Some("title_asc".to_owned()),
            exclude_home_hidden: Some(true),
        };
        assert_eq!(custom_filter.tag.as_deref(), Some("example-tag"));
        assert_eq!(custom_filter.limit, Some(25));
        assert_eq!(custom_filter.sort.as_deref(), Some("title_asc"));
    }

    #[tokio::test]
    async fn recent_media_uses_publication_date_and_keeps_unknown_dates_last() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());
        sqlx::query(
            "INSERT INTO media (id, title, is_available, published_at) VALUES
             (1, 'Newest release imported first', 1, '2026-10-15'),
             (2, 'Unknown release imported second', 1, NULL),
             (3, 'Older release imported last', 1, '2020-04-10')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let items = list_media(
            State(state),
            Query(MediaFilter {
                sort: Some("recent".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(
            items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![1, 3, 2]
        );
    }

    #[tokio::test]
    async fn returns_related_media_sorted_by_tags_and_channels() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch1', 'Channel 1')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media (id, title, is_available, is_short) VALUES (1, 'Video 1', 1, 0), (2, 'Video 2 (Same Channel)', 1, 0), (3, 'Video 3 (Diff)', 1, 0), (4, 'Video 4 (Short)', 1, 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (1, 'ch1', 'manual'), (2, 'ch1', 'manual'), (4, 'ch1', 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'Music', 'music')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual'), (2, 1, 'manual'), (4, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        let state = AppState::for_test(pool.clone());

        let res = get_related_media(
            State(state.clone()),
            Path(1),
            Query(RelatedFilter::default()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].id, 2); // Video 2 matches both channel and tag!
        assert_eq!(res[1].id, 3);

        let media = get_media(State(state), Path(1)).await.unwrap().0;
        assert_eq!(media.id, 1);
        assert_eq!(media.title, "Video 1");
    }

    #[tokio::test]
    async fn returns_related_media_respects_exclude_hidden() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Main Video', 1), (2, 'Related Allowed', 1), (3, 'Related Hidden', 1), (4, 'Related Channel Hidden', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'General', 'general'), (2, 'Spoiler', 'spoiler')")
            .execute(&pool)
            .await
            .unwrap();
        // Video 1 has 'general'
        // Video 2 has 'general'
        // Video 3 has 'general' and 'spoiler'
        // Video 4 has 'general' and channel 'Spoiler TV'
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual'), (2, 1, 'manual'), (3, 1, 'manual'), (3, 2, 'manual'), (4, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_sp', 'Spoiler TV')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (4, 'ch_sp', 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        save_home_settings(
            &pool,
            &HomeSettings {
                hidden_tags: vec!["spoiler".to_string()],
            },
        )
        .await
        .unwrap();

        // 1. By default, get_related_media excludes videos with hidden tags or channel name -> only Video 2
        let rel_default = get_related_media(
            State(state.clone()),
            Path(1),
            Query(RelatedFilter::default()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(rel_default.len(), 1);
        assert_eq!(rel_default[0].id, 2);

        // 2. If exclude_hidden is explicitly false -> Video 2, 3 and 4
        let rel_all = get_related_media(
            State(state.clone()),
            Path(1),
            Query(RelatedFilter {
                exclude_hidden: Some(false),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(rel_all.len(), 3);
    }

    #[tokio::test]
    async fn home_settings_save_and_load_normalizes_tags() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let settings = HomeSettings {
            hidden_tags: vec![
                "  #Spoiler ".to_string(),
                "VLOG".to_string(),
                "spoiler".to_string(),
                "".to_string(),
                "   ".to_string(),
            ],
        };
        save_home_settings(&pool, &settings).await.unwrap();

        let loaded = load_home_settings(&pool).await;
        assert_eq!(loaded.hidden_tags, vec!["spoiler", "vlog"]);
    }

    #[tokio::test]
    async fn list_media_and_tags_respects_exclude_home_hidden() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Video 1 has tag 'gaming' and 'vlog'
        // Video 2 has tag 'gaming'
        // Video 3 has tag 'vlog'
        // Video 4 has no tags, but has 'vlog' in title
        // Video 5 has tag 'retro', title 'Clean Title', but belongs to channel 'Daily Vlog Channel'
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Vlog & Game', 1), (2, 'Only Game', 1), (3, 'Something Else', 1), (4, 'Special Vlog Episode', 1), (5, 'Clean Title', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'Gaming', 'gaming'), (2, 'Vlog', 'vlog'), (3, 'Retro', 'retro')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual'), (1, 2, 'manual'), (2, 1, 'manual'), (3, 2, 'manual'), (5, 3, 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_vlog', 'Daily Vlog Channel')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (5, 'ch_vlog', 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        // Save 'vlog' as hidden in home
        save_home_settings(
            &pool,
            &HomeSettings {
                hidden_tags: vec!["vlog".to_string()],
            },
        )
        .await
        .unwrap();

        // 1. list_media without exclude_home_hidden (e.g. Biblioteca)
        let all_media = list_media(State(state.clone()), Query(MediaFilter::default()))
            .await
            .unwrap()
            .0;
        assert_eq!(all_media.len(), 5);

        // 2. list_media with exclude_home_hidden: true (Home) -> only Video 2 should appear!
        // Video 5 is excluded because its channel name contains 'vlog'.
        let home_media = list_media(
            State(state.clone()),
            Query(MediaFilter {
                exclude_home_hidden: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(home_media.len(), 1);
        assert_eq!(home_media[0].id, 2);

        // 3. list_tags without exclude_home_hidden (Biblioteca) -> all 3 tags
        let all_tags = list_tags(State(state.clone()), Query(TagsFilter::default()))
            .await
            .unwrap()
            .0;
        assert_eq!(all_tags.len(), 3);

        // 4. list_tags with exclude_home_hidden: true (Home) -> only 'Gaming' should appear with count 1
        // 'Retro' is not shown because its only media (Video 5) was excluded due to channel name.
        let home_tags = list_tags(
            State(state.clone()),
            Query(TagsFilter {
                exclude_home_hidden: Some(true),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(home_tags.len(), 1);
        assert_eq!(home_tags[0].normalized_name, "gaming");
        assert_eq!(home_tags[0].media_count, 1);
    }

    #[tokio::test]
    async fn updates_and_persists_user_notes() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Test Video', 1)")
            .execute(&pool)
            .await
            .unwrap();

        let state = AppState::for_test(pool.clone());

        // 1. Initial state: user_notes is None
        let media = get_media(State(state.clone()), Path(1)).await.unwrap().0;
        assert_eq!(media.user_notes, None);

        // 2. Update user_notes
        let notes_text = "00:03 Intro\n03:20 Song 1\n06:39 Outro";
        let res = update_media_notes(
            State(state.clone()),
            Path(1),
            Json(UpdateNotesRequest {
                notes: Some(notes_text.to_owned()),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res.user_notes.as_deref(), Some(notes_text));

        // 3. Verify get_media returns updated notes
        let updated = get_media(State(state.clone()), Path(1)).await.unwrap().0;
        assert_eq!(updated.user_notes.as_deref(), Some(notes_text));

        // 4. Update with empty string clears notes to None
        let res_clear = update_media_notes(
            State(state.clone()),
            Path(1),
            Json(UpdateNotesRequest {
                notes: Some("   ".to_owned()),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res_clear.user_notes, None);

        let cleared = get_media(State(state.clone()), Path(1)).await.unwrap().0;
        assert_eq!(cleared.user_notes, None);

        // 5. Updating non-existent media returns 404
        let err = update_media_notes(
            State(state.clone()),
            Path(999),
            Json(UpdateNotesRequest {
                notes: Some("Hello".to_owned()),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::NOT_FOUND);
    }

    #[test]
    fn channel_mentions_require_exact_name_boundaries() {
        assert!(title_mentions_channel(
            "Fan cover: AURORA ATLAS - Starlight",
            "Aurora Atlas"
        ));
        assert!(title_mentions_channel("Tribute to Studio 42!", "Studio 42"));
        assert!(title_mentions_channel(
            "Studio: 42 — versão comentada",
            "Studio 42"
        ));
        assert!(title_mentions_channel(
            "42 retrospective: Studio explained",
            "Studio 42"
        ));
        assert!(title_mentions_channel(
            "Studio e outras histórias de 42",
            "Studio 42"
        ));
        assert!(!title_mentions_channel("Studio retrospective", "Studio 42"));
        assert!(!title_mentions_channel(
            "The Aurorian archive",
            "Aurora Atlas"
        ));
        assert!(!title_mentions_channel("Unrelated title", "Aurora Atlas"));
    }

    #[tokio::test]
    async fn channel_mentions_exclude_the_channels_own_videos() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());
        sqlx::query(
            "INSERT INTO channels (id, name) VALUES ('aurora-atlas', 'Aurora Atlas'), ('fan', 'Fan Channel')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Aurora Atlas - Official', 1), (2, 'AURORA ATLAS - Fan Cover', 1), (3, 'Aurorian Retrospective', 1), (4, 'Other Video', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (1, 'aurora-atlas', 'manual'), (2, 'fan', 'manual'), (3, 'fan', 'manual'), (4, 'fan', 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        let mentions = get_channel_mentions(State(state), Path("aurora-atlas".to_string()))
            .await
            .unwrap()
            .0;
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].id, 2);
    }

    #[tokio::test]
    async fn channel_list_filters_ranks_and_limits_suggestions() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());
        sqlx::query(
            "INSERT INTO channels (id, name, handle) VALUES
             ('aurora', 'Aurora', '@aurora'),
             ('aurora-atlas', 'Aurora Atlas', '@atlas'),
             ('aurora-live', 'The Aurora Live', '@aurora-live'),
             ('aurora-archive', 'Aurora Archive', '@archive'),
             ('other', 'Other Channel', '@other')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO media (id, title, is_available) VALUES
             (1, 'One', 1), (2, 'Two', 1), (3, 'Three', 1), (4, 'Four', 1), (5, 'Five', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO media_channels (media_id, channel_id, origin) VALUES
             (1, 'aurora', 'manual'),
             (2, 'aurora-atlas', 'manual'),
             (3, 'aurora-live', 'manual'),
             (4, 'aurora-archive', 'manual'),
             (5, 'other', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let channels = list_channels(
            State(state.clone()),
            Query(ChannelListFilter {
                q: Some("aurora".to_string()),
                limit: Some(3),
                exclude_home_hidden: None,
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(channels.len(), 3);
        assert_eq!(channels[0].id, "aurora");
        assert!(channels.iter().all(|channel| {
            channel.name.to_lowercase().contains("aurora")
                || channel
                    .handle
                    .as_deref()
                    .is_some_and(|handle| handle.contains("aurora"))
        }));

        save_home_settings(
            &pool,
            &HomeSettings {
                hidden_tags: vec!["atlas".to_string(), "live".to_string()],
            },
        )
        .await
        .unwrap();
        let filtered = list_channels(
            State(state),
            Query(ChannelListFilter {
                q: Some("aurora".to_string()),
                limit: Some(10),
                exclude_home_hidden: Some(true),
            }),
        )
        .await
        .unwrap()
        .0;

        assert!(filtered.iter().all(|channel| channel.id != "aurora-atlas"));
        assert!(filtered.iter().all(|channel| channel.id != "aurora-live"));
    }
}
