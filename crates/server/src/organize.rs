use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{MEDIA_SELECT_COLUMNS, MediaDbRow, MediaItem, fetch_tags_for_media},
    web::AppState,
};

fn generate_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("{prefix}_{now:x}")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToggleFavoriteResponse {
    pub media_id: i64,
    pub is_favorite: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistSummary {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub media_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub thumbnail_media_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistDetail {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistRequest {
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlaylistRequest {
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddItemRequest {
    pub media_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct ReorderRequest {
    pub media_ids: Vec<i64>,
}

// ================= FAVORITOS =================

pub(crate) async fn list_favorites(
    State(state): State<AppState>,
) -> Result<Json<Vec<MediaItem>>, StatusCode> {
    let query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS}
         FROM favorites fav
         JOIN media m ON m.id = fav.media_id
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN playback_state ps ON ps.media_id = m.id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE m.is_available = 1
         ORDER BY fav.created_at DESC"
    );
    let rows = sqlx::query_as::<_, MediaDbRow>(sqlx::AssertSqlSafe(query.as_str()))
        .fetch_all(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to query favorites");
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

pub(crate) async fn add_favorite(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query("INSERT INTO favorites (media_id) VALUES (?) ON CONFLICT(media_id) DO NOTHING")
        .bind(media_id)
        .execute(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, media_id, "failed to add favorite");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn remove_favorite(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query("DELETE FROM favorites WHERE media_id = ?")
        .bind(media_id)
        .execute(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, media_id, "failed to remove favorite");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn toggle_favorite(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
) -> Result<Json<ToggleFavoriteResponse>, StatusCode> {
    let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM favorites WHERE media_id = ?")
        .bind(media_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if exists.is_some() {
        sqlx::query("DELETE FROM favorites WHERE media_id = ?")
            .bind(media_id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(ToggleFavoriteResponse {
            media_id,
            is_favorite: false,
        }))
    } else {
        sqlx::query("INSERT INTO favorites (media_id) VALUES (?) ON CONFLICT(media_id) DO NOTHING")
            .bind(media_id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(ToggleFavoriteResponse {
            media_id,
            is_favorite: true,
        }))
    }
}

// ================= PLAYLISTS =================

pub(crate) async fn list_playlists(
    State(state): State<AppState>,
) -> Result<Json<Vec<PlaylistSummary>>, StatusCode> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            i64,
            String,
            String,
            Option<i64>,
        ),
    >(
        "SELECT p.id, p.title, p.description, COUNT(pi.media_id) as media_count,
                p.created_at, p.updated_at,
                (SELECT pi2.media_id FROM playlist_items pi2 WHERE pi2.playlist_id = p.id ORDER BY pi2.position ASC LIMIT 1) as thumb_id
         FROM playlists p
         LEFT JOIN playlist_items pi ON pi.playlist_id = p.id
         GROUP BY p.id
         ORDER BY p.updated_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to list playlists");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let playlists = rows
        .into_iter()
        .map(|row| PlaylistSummary {
            id: row.0,
            title: row.1,
            description: row.2,
            media_count: row.3,
            created_at: row.4,
            updated_at: row.5,
            thumbnail_media_id: row.6,
        })
        .collect();

    Ok(Json(playlists))
}

pub(crate) async fn create_playlist(
    State(state): State<AppState>,
    Json(payload): Json<CreatePlaylistRequest>,
) -> Result<Json<PlaylistSummary>, StatusCode> {
    let title = payload.title.trim();
    if title.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let id = generate_id("pl");

    let row = sqlx::query_as::<_, (String, String, Option<String>, String, String)>(
        "INSERT INTO playlists (id, title, description) VALUES (?, ?, ?)
         RETURNING id, title, description, created_at, updated_at",
    )
    .bind(&id)
    .bind(title)
    .bind(payload.description.as_deref().map(str::trim))
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to create playlist");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(PlaylistSummary {
        id: row.0,
        title: row.1,
        description: row.2,
        media_count: 0,
        created_at: row.3,
        updated_at: row.4,
        thumbnail_media_id: None,
    }))
}

pub(crate) async fn get_playlist(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PlaylistDetail>, StatusCode> {
    let playlist = sqlx::query_as::<_, (String, String, Option<String>, String, String)>(
        "SELECT id, title, description, created_at, updated_at FROM playlists WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let Some(playlist) = playlist else {
        return Err(StatusCode::NOT_FOUND);
    };

    let query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS}
         FROM playlist_items pi
         JOIN media m ON m.id = pi.media_id
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         LEFT JOIN playback_state ps ON ps.media_id = m.id
         LEFT JOIN favorites fav ON fav.media_id = m.id
         LEFT JOIN thumbnails t ON t.media_id = m.id
         WHERE pi.playlist_id = ? AND m.is_available = 1
         ORDER BY pi.position ASC"
    );
    let rows = sqlx::query_as::<_, MediaDbRow>(sqlx::AssertSqlSafe(query.as_str()))
        .bind(&id)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let media_ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids).await?;

    let items = rows
        .into_iter()
        .map(|row| row.into_media_item(&mut tag_map))
        .collect();

    Ok(Json(PlaylistDetail {
        id: playlist.0,
        title: playlist.1,
        description: playlist.2,
        created_at: playlist.3,
        updated_at: playlist.4,
        items,
    }))
}

pub(crate) async fn update_playlist(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdatePlaylistRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut query =
        String::from("UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
    if payload.title.is_some() {
        query.push_str(", title = ?");
    }
    if payload.description.is_some() {
        query.push_str(", description = ?");
    }
    query.push_str(" WHERE id = ?");

    let mut sqlx_query = sqlx::query(sqlx::AssertSqlSafe(query.as_str()));
    if let Some(ref title) = payload.title {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        sqlx_query = sqlx_query.bind(trimmed);
    }
    if let Some(ref desc) = payload.description {
        sqlx_query = sqlx_query.bind(desc.trim());
    }
    sqlx_query = sqlx_query.bind(&id);

    let res = sqlx_query
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

pub(crate) async fn delete_playlist(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let res = sqlx::query("DELETE FROM playlists WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

pub(crate) async fn add_playlist_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<AddItemRequest>,
) -> Result<StatusCode, StatusCode> {
    let next_pos = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query(
        "INSERT INTO playlist_items (playlist_id, media_id, position)
         VALUES (?, ?, ?)
         ON CONFLICT(playlist_id, media_id) DO NOTHING",
    )
    .bind(&id)
    .bind(payload.media_id)
    .bind(next_pos)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query(
        "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(&id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::CREATED)
}

pub(crate) async fn remove_playlist_item(
    State(state): State<AppState>,
    Path((id, media_id)): Path<(String, i64)>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query("DELETE FROM playlist_items WHERE playlist_id = ? AND media_id = ?")
        .bind(&id)
        .bind(media_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query(
        "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(&id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn reorder_playlist_items(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ReorderRequest>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for (pos, media_id) in payload.media_ids.iter().enumerate() {
        sqlx::query(
            "UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND media_id = ?",
        )
        .bind(pos as i64)
        .bind(&id)
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    sqlx::query(
        "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn favorites_and_playlists_lifecycle() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Inserir mídia
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Video 1', 1)")
            .execute(&pool)
            .await
            .unwrap();

        // 1. Toggle favorite
        let res = toggle_favorite(State(state.clone()), Path(1))
            .await
            .unwrap()
            .0;
        assert!(res.is_favorite);

        let favs = list_favorites(State(state.clone())).await.unwrap().0;
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, 1);
        assert!(favs[0].is_favorite);

        let res2 = toggle_favorite(State(state.clone()), Path(1))
            .await
            .unwrap()
            .0;
        assert!(!res2.is_favorite);

        let favs2 = list_favorites(State(state.clone())).await.unwrap().0;
        assert_eq!(favs2.len(), 0);

        // 2. Playlists lifecycle
        let created = create_playlist(
            State(state.clone()),
            Json(CreatePlaylistRequest {
                title: "Minha Playlist".to_owned(),
                description: Some("Teste".to_owned()),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(created.title, "Minha Playlist");

        add_playlist_item(
            State(state.clone()),
            Path(created.id.clone()),
            Json(AddItemRequest { media_id: 1 }),
        )
        .await
        .unwrap();

        let detail = get_playlist(State(state.clone()), Path(created.id.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(detail.items.len(), 1);
        assert_eq!(detail.items[0].id, 1);

        remove_playlist_item(State(state.clone()), Path((created.id.clone(), 1)))
            .await
            .unwrap();

        let detail2 = get_playlist(State(state.clone()), Path(created.id.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(detail2.items.len(), 0);

        delete_playlist(State(state.clone()), Path(created.id.clone()))
            .await
            .unwrap();

        let lists = list_playlists(State(state.clone())).await.unwrap().0;
        assert_eq!(lists.len(), 0);
    }
}
