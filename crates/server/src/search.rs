use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    catalog::{MEDIA_SELECT_COLUMNS, MediaDbRow, MediaItem, fetch_tags_for_media},
    web::AppState,
};

#[derive(Debug, Deserialize, Default)]
pub struct SearchFilter {
    pub q: Option<String>,
    pub tag: Option<String>,
    pub channel: Option<String>,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub is_short: Option<bool>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub exclude_home_hidden: Option<bool>,
}

pub fn extract_search_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for c in query.chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(current);
            current = String::new();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub fn sanitize_fts5_query(query: &str) -> String {
    let tokens = extract_search_tokens(query);
    tokens
        .into_iter()
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) async fn search_media(
    State(state): State<AppState>,
    Query(filter): Query<SearchFilter>,
) -> Result<Json<Vec<MediaItem>>, StatusCode> {
    let limit = filter.limit.unwrap_or(30).clamp(1, 100);
    let offset = filter.offset.unwrap_or(0).max(0);

    let raw_q = filter.q.as_deref().unwrap_or("").trim();
    let tokens = extract_search_tokens(raw_q);
    let sanitized_query = if !tokens.is_empty() {
        Some(sanitize_fts5_query(raw_q))
    } else {
        None
    };
    let has_fts = sanitized_query.as_ref().is_some_and(|q| !q.is_empty());

    let is_short_filter = match (filter.is_short, filter.media_type.as_deref()) {
        (Some(val), _) => Some(val),
        (None, Some("short")) => Some(true),
        (None, Some("video")) => Some(false),
        _ => None,
    };

    let hidden_tags = if filter.exclude_home_hidden == Some(true) {
        crate::catalog::load_home_settings(&state.pool)
            .await
            .hidden_tags
    } else {
        Vec::new()
    };

    let mut query = if has_fts {
        format!(
            "SELECT {MEDIA_SELECT_COLUMNS}
             FROM media_fts fts
             JOIN media m ON m.id = fts.media_id
             LEFT JOIN media_channels mc ON mc.media_id = m.id
             LEFT JOIN channels c ON c.id = mc.channel_id
             LEFT JOIN playback_state ps ON ps.media_id = m.id
             LEFT JOIN favorites fav ON fav.media_id = m.id
             LEFT JOIN thumbnails t ON t.media_id = m.id
             WHERE fts.media_fts MATCH ? AND m.is_available = 1"
        )
    } else {
        format!(
            "SELECT {MEDIA_SELECT_COLUMNS}
             FROM media m
             LEFT JOIN media_channels mc ON mc.media_id = m.id
             LEFT JOIN channels c ON c.id = mc.channel_id
             LEFT JOIN playback_state ps ON ps.media_id = m.id
             LEFT JOIN favorites fav ON fav.media_id = m.id
             LEFT JOIN thumbnails t ON t.media_id = m.id
             WHERE m.is_available = 1"
        )
    };

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

    if let Some(is_short) = is_short_filter {
        if is_short {
            query.push_str(" AND m.is_short = 1");
        } else {
            query.push_str(" AND m.is_short = 0");
        }
    }

    let lowercase_tokens: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
    let uses_relevance =
        has_fts && matches!(filter.sort.as_deref(), None | Some("") | Some("relevance"));

    let (tier_clause, tier_binds) = if uses_relevance && !lowercase_tokens.is_empty() {
        if lowercase_tokens.len() == 1 {
            let pat = format!("%{}%", lowercase_tokens[0]);
            (
                "(CASE \
                    WHEN lower(m.title) LIKE ? THEN 1 \
                    WHEN lower(c.name) LIKE ? THEN 2 \
                    ELSE 3 \
                END)"
                    .to_string(),
                vec![pat.clone(), pat],
            )
        } else {
            let title_all = lowercase_tokens
                .iter()
                .map(|_| "lower(m.title) LIKE ?")
                .collect::<Vec<_>>()
                .join(" AND ");
            let title_any = lowercase_tokens
                .iter()
                .map(|_| "lower(m.title) LIKE ?")
                .collect::<Vec<_>>()
                .join(" OR ");
            let chan_any = lowercase_tokens
                .iter()
                .map(|_| "lower(c.name) LIKE ?")
                .collect::<Vec<_>>()
                .join(" OR ");

            let clause = format!(
                "(CASE \
                    WHEN {title_all} THEN 1 \
                    WHEN {title_any} THEN 2 \
                    WHEN {chan_any} THEN 3 \
                    ELSE 4 \
                END)"
            );

            let mut binds = Vec::with_capacity(lowercase_tokens.len() * 3);
            for t in &lowercase_tokens {
                binds.push(format!("%{t}%"));
            }
            for t in &lowercase_tokens {
                binds.push(format!("%{t}%"));
            }
            for t in &lowercase_tokens {
                binds.push(format!("%{t}%"));
            }
            (clause, binds)
        }
    } else {
        (String::new(), Vec::new())
    };

    let sort_mode = filter
        .sort
        .as_deref()
        .unwrap_or(if has_fts { "relevance" } else { "recent" });

    match sort_mode {
        "relevance" if has_fts => {
            query.push_str(&format!(
                " ORDER BY {tier_clause} ASC, bm25(media_fts, 15.0, 3.0, 1.0, 0.2) ASC, m.id DESC"
            ));
        }
        "recent" => query.push_str(" ORDER BY m.id DESC"),
        "title" => query.push_str(" ORDER BY m.title COLLATE NOCASE ASC, m.id DESC"),
        "duration" => query.push_str(" ORDER BY m.duration_ms DESC NULLS LAST, m.id DESC"),
        _ if has_fts => {
            query.push_str(&format!(
                " ORDER BY {tier_clause} ASC, bm25(media_fts, 15.0, 3.0, 1.0, 0.2) ASC, m.id DESC"
            ));
        }
        _ => query.push_str(" ORDER BY m.id DESC"),
    }

    query.push_str(" LIMIT ? OFFSET ?");

    let mut sqlx_query = sqlx::query_as::<_, MediaDbRow>(sqlx::AssertSqlSafe(query.as_str()));

    if let Some(q) = sanitized_query.as_deref().filter(|s| !s.is_empty()) {
        sqlx_query = sqlx_query.bind(q);
    }

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

    for bind in tier_binds {
        sqlx_query = sqlx_query.bind(bind);
    }

    sqlx_query = sqlx_query.bind(limit).bind(offset);

    let rows = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to execute search query");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_fts5_queries_correctly() {
        assert_eq!(sanitize_fts5_query("hello world"), "\"hello\"* \"world\"*");
        assert_eq!(sanitize_fts5_query("  "), "");
        assert_eq!(
            sanitize_fts5_query("bright horizon's journey"),
            "\"bright\"* \"horizon\"* \"s\"* \"journey\"*"
        );
        assert_eq!(sanitize_fts5_query("星空の旅"), "\"星空の旅\"*");
        assert_eq!(
            sanitize_fts5_query("Aurora/Starlight"),
            "\"Aurora\"* \"Starlight\"*"
        );
    }

    #[tokio::test]
    async fn fts5_search_matches_unicode_and_tags() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Inserir canal
        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_example', 'Example Channel')")
            .execute(&pool)
            .await
            .unwrap();

        // Inserir tag
        sqlx::query(
            "INSERT INTO tags (id, normalized_name, display_name) VALUES (1, '90s', '90s')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Inserir mídias
        sqlx::query(
            "INSERT INTO media (id, title, is_available) VALUES (1, 'Bright Horizon Journey', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO media_channels (media_id, channel_id, origin) VALUES (1, 'ch_example', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (2, '星空の旅', 1)")
            .execute(&pool)
            .await
            .unwrap();

        // Sincronizar FTS
        let mut conn = pool.acquire().await.unwrap();
        crate::catalog::sync_media_fts(&mut conn).await.unwrap();

        // 1. Busca por termo ocidental com prefixo
        let res_latin = search_media(
            State(state.clone()),
            Query(SearchFilter {
                q: Some("bright".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res_latin.len(), 1);
        assert_eq!(res_latin[0].id, 1);

        // 2. Busca por Unicode / japonês
        let res_jp = search_media(
            State(state.clone()),
            Query(SearchFilter {
                q: Some("星空".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res_jp.len(), 1);
        assert_eq!(res_jp[0].id, 2);

        // 3. Busca por tag indexada no FTS
        let res_tag = search_media(
            State(state.clone()),
            Query(SearchFilter {
                q: Some("90s".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(res_tag.len(), 1);
        assert_eq!(res_tag[0].id, 1);
    }

    #[tokio::test]
    async fn search_prioritizes_title_over_tags() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        // Video 1: title does not have the query, but its tag does.
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Unrelated synthetic title', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tags (id, normalized_name, display_name) VALUES (1, 'aurora', 'aurora')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        // Video 2: title has the query and must rank first.
        sqlx::query(
            "INSERT INTO media (id, title, is_available) VALUES (2, 'Aurora - Starlight', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut conn = pool.acquire().await.unwrap();
        crate::catalog::sync_media_fts(&mut conn).await.unwrap();

        let res = search_media(
            State(state.clone()),
            Query(SearchFilter {
                q: Some("Aurora".to_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(res.len(), 2);
        // Video 2 MUST be first because it matches in the title!
        assert_eq!(res[0].id, 2);
        assert_eq!(res[1].id, 1);
    }
}
