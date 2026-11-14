use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{MEDIA_SELECT_COLUMNS, MediaDbRow, MediaItem, fetch_tags_for_media},
    web::AppState,
};

#[derive(Debug, Deserialize, Default, Clone)]
pub struct FeedFilter {
    pub seed: Option<i64>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
    pub tag: Option<String>,
    pub channel: Option<String>,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub is_short: Option<bool>,
    pub exclude_home_hidden: Option<bool>,
    pub mode: Option<String>,
    /// Até três grades recentes, da mais nova para a mais antiga.
    /// Formato: `1,2,3|4,5,6|7,8,9`.
    pub recent_ids: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeedResponse {
    pub items: Vec<MediaItem>,
    pub next_cursor: Option<String>,
    pub seed: i64,
}

#[derive(sqlx::FromRow, Debug)]
pub struct FeedDbRow {
    #[sqlx(flatten)]
    pub media_row: MediaDbRow,
    pub feed_order: i64,
}

impl std::ops::Deref for FeedDbRow {
    type Target = MediaDbRow;
    fn deref(&self) -> &Self::Target {
        &self.media_row
    }
}

impl FeedDbRow {
    pub fn into_media_item(
        self,
        tag_map: &mut std::collections::HashMap<i64, Vec<crate::catalog::TagRef>>,
    ) -> MediaItem {
        self.media_row.into_media_item(tag_map)
    }
}

static FEED_SEED_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn mix_u64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

pub(crate) fn generate_feed_seed() -> i64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(42);
    let counter = FEED_SEED_COUNTER.fetch_add(1, Ordering::Relaxed);
    // Mantém o valor no intervalo inteiro seguro do JavaScript, pois a seed
    // retorna ao backend nas requisições de paginação.
    (mix_u64(timestamp ^ counter.rotate_left(23)) & ((1u64 << 53) - 1)) as i64
}

pub(crate) fn seeded_unit_interval(seed: i64, media_id: i64, step: usize) -> f64 {
    let mixed = mix_u64(
        (seed as u64)
            ^ (media_id as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93)
            ^ (step as u64).wrapping_mul(0xA076_1D64_78BD_642F),
    );
    let mantissa = mixed >> 11;
    (mantissa as f64 + 0.5) / ((1u64 << 53) as f64)
}

pub fn encode_cursor(feed_order: i64, id: i64) -> String {
    format!("{:x}_{:x}", feed_order, id)
}

pub fn decode_cursor(cursor: &str) -> Option<(i64, i64)> {
    let mut parts = cursor.split('_');
    let order_hex = parts.next()?;
    let id_hex = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let order_val = i64::from_str_radix(order_hex, 16).ok()?;
    let id = i64::from_str_radix(id_hex, 16).ok()?;
    Some((order_val, id))
}

pub(crate) async fn get_feed(
    State(state): State<AppState>,
    Query(filter): Query<FeedFilter>,
) -> Result<Json<FeedResponse>, StatusCode> {
    let rec_settings = crate::recommender::load_recommendation_settings(&state.pool).await;
    let wants_recommended = filter.mode.as_deref() == Some("recommended")
        || (rec_settings.enabled
            && filter.mode.as_deref() != Some("random")
            && filter.tag.is_none()
            && filter.channel.is_none());

    if wants_recommended {
        return crate::recommender::get_recommended_feed(&state, &filter, &rec_settings)
            .await
            .map(Json);
    }

    let seed = filter.seed.unwrap_or_else(generate_feed_seed);
    let limit = filter.limit.unwrap_or(24).clamp(1, 100);

    let decoded_cursor = match &filter.cursor {
        Some(cursor_str) => match decode_cursor(cursor_str) {
            Some(decoded) => Some(decoded),
            None => return Err(StatusCode::BAD_REQUEST),
        },
        None => None,
    };

    // Resolução do filtro de short vs vídeo padrão (Home nunca deve exibir shorts por padrão)
    let is_short_filter = match (filter.is_short, filter.media_type.as_deref()) {
        (Some(val), _) => Some(val),
        (None, Some("short")) => Some(true),
        (None, Some("video")) => Some(false),
        (None, None) if filter.exclude_home_hidden == Some(true) => Some(false),
        _ => None,
    };

    let hidden_tags = if filter.exclude_home_hidden == Some(true) {
        crate::catalog::load_home_settings(&state.pool)
            .await
            .hidden_tags
    } else {
        Vec::new()
    };

    let mut query = format!(
        "WITH feed_cte AS (
            SELECT
                {MEDIA_SELECT_COLUMNS},
                ((((((m.id | ?) - (m.id & ?)) * 2654435761 + 1013904223) & 2147483647) * 1597334677 + 2246822519) & 2147483647) AS feed_order
            FROM media m
            LEFT JOIN media_channels mc ON mc.media_id = m.id
            LEFT JOIN channels c ON c.id = mc.channel_id
            LEFT JOIN playback_state ps ON ps.media_id = m.id
            LEFT JOIN favorites fav ON fav.media_id = m.id
            LEFT JOIN thumbnails t ON t.media_id = m.id
            WHERE m.is_available = 1",
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

    if let Some(is_short) = is_short_filter {
        if is_short {
            query.push_str(" AND m.is_short = 1");
        } else {
            query.push_str(" AND m.is_short = 0");
        }
    }

    query.push(')'); // encerra WITH feed_cte

    query.push_str(
        " SELECT id, title, duration_ms, width, height, rotation, is_short, is_available,
                 channel_id, channel_name, channel_avatar_url, position_ms, completed, is_favorite,
                 is_official_thumbnail, thumbnail_version, description, user_notes, youtube_id, published_at, feed_order
          FROM feed_cte",
    );

    if decoded_cursor.is_some() {
        query.push_str(" WHERE (feed_order > ?) OR (feed_order = ? AND id > ?)");
    }

    query.push_str(" ORDER BY feed_order ASC, id ASC LIMIT ?");

    let mut sqlx_query = sqlx::query_as::<_, FeedDbRow>(sqlx::AssertSqlSafe(query.as_str()));

    // Bind para feed_order no CTE: seed aparece duas vezes (| seed e & seed)
    sqlx_query = sqlx_query.bind(seed).bind(seed);

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

    if let Some(tag) = &filter.tag {
        sqlx_query = sqlx_query.bind(tag).bind(tag);
    }

    if let Some(channel) = &filter.channel {
        sqlx_query = sqlx_query.bind(channel).bind(channel);
    }

    if let Some((last_order, last_id)) = decoded_cursor {
        sqlx_query = sqlx_query.bind(last_order).bind(last_order).bind(last_id);
    }

    sqlx_query = sqlx_query.bind(limit);

    let rows = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query feed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if rows.is_empty() {
        return Ok(Json(FeedResponse {
            items: Vec::new(),
            next_cursor: None,
            seed,
        }));
    }

    let media_ids: Vec<i64> = rows.iter().map(|row| row.media_row.id).collect();
    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids).await?;

    let next_cursor = if (rows.len() as i64) == limit {
        rows.last()
            .map(|last_row| encode_cursor(last_row.feed_order, last_row.media_row.id))
    } else {
        None
    };

    let items = rows
        .into_iter()
        .map(|row| row.into_media_item(&mut tag_map))
        .collect();

    Ok(Json(FeedResponse {
        items,
        next_cursor,
        seed,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_encode_decode_roundtrip() {
        let order = 123456789;
        let id = 42;
        let encoded = encode_cursor(order, id);
        assert_eq!(encoded, format!("{:x}_{:x}", order, id));

        let (decoded_order, decoded_id) = decode_cursor(&encoded).expect("should decode");
        assert_eq!(decoded_order, order);
        assert_eq!(decoded_id, id);
    }

    #[test]
    fn invalid_cursor_returns_none() {
        assert!(decode_cursor("invalid").is_none());
        assert!(decode_cursor("12_zz_extra").is_none());
        assert!(decode_cursor("not_hex").is_none());
    }

    #[test]
    fn deterministic_order_formula_matches_rust() {
        let id: i64 = 42;
        let seed: i64 = 987654321;
        let h1 = ((id ^ seed)
            .wrapping_mul(2654435761)
            .wrapping_add(1013904223))
            & 0x7FFFFFFF;
        let expected = (h1.wrapping_mul(1597334677).wrapping_add(2246822519)) & 0x7FFFFFFF;
        assert_eq!(expected, 2130100169);
    }

    #[test]
    fn generated_seeds_and_mixed_samples_rotate() {
        let seeds = (0..16)
            .map(|_| generate_feed_seed())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(seeds.len(), 16);

        let samples = (1..=16)
            .map(|seed| seeded_unit_interval(seed, 525, 5).to_bits())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(samples.len(), 16);
    }

    #[tokio::test]
    async fn feed_pagination_and_stability() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        for i in 1..=5 {
            sqlx::query("INSERT INTO media (id, title, is_available) VALUES (?, ?, 1)")
                .bind(i)
                .bind(format!("Video {i}"))
                .execute(&pool)
                .await
                .unwrap();
        }

        let seed = 12345;
        let res1 = get_feed(
            State(state.clone()),
            Query(FeedFilter {
                seed: Some(seed),
                limit: Some(2),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(res1.items.len(), 2);
        assert!(res1.next_cursor.is_some());

        let res2 = get_feed(
            State(state.clone()),
            Query(FeedFilter {
                seed: Some(seed),
                cursor: res1.next_cursor,
                limit: Some(2),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(res2.items.len(), 2);
        let ids1: Vec<i64> = res1.items.iter().map(|item| item.id).collect();
        let ids2: Vec<i64> = res2.items.iter().map(|item| item.id).collect();
        for id in &ids2 {
            assert!(
                !ids1.contains(id),
                "id {id} não deve se repetir entre páginas"
            );
        }
    }

    #[tokio::test]
    async fn feed_respects_exclude_home_hidden() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query(
            "INSERT INTO media (id, title, is_available) VALUES (1, 'Vlog', 1), (2, 'Game', 1), (3, 'Clean Title', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'Vlog', 'vlog'), (2, 'Game', 'game')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual'), (2, 2, 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_vlog', 'Super Vlog Channel')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (3, 'ch_vlog', 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        crate::catalog::save_home_settings(
            &pool,
            &crate::catalog::HomeSettings {
                hidden_tags: vec!["vlog".to_string()],
            },
        )
        .await
        .unwrap();

        // Feed normal (without exclude_home_hidden) -> 3 items
        let res_all = get_feed(
            State(state.clone()),
            Query(FeedFilter {
                seed: Some(42),
                limit: Some(10),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res_all.items.len(), 3);

        // Feed with exclude_home_hidden: true -> only Video 2 (Game). Video 3 is excluded by channel!
        let res_home = get_feed(
            State(state.clone()),
            Query(FeedFilter {
                seed: Some(42),
                limit: Some(10),
                exclude_home_hidden: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res_home.items.len(), 1);
        assert_eq!(res_home.items[0].id, 2);
    }
}
