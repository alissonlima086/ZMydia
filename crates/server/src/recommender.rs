use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{
        MEDIA_SELECT_COLUMNS, MediaDbRow, MediaItem, TagRef, fetch_tags_for_media,
        load_home_settings,
    },
    feed::{FeedFilter, FeedResponse},
    playback::NOISE_TAGS,
    web::AppState,
};

/// Configurações do algoritmo de recomendação modular.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct RecommendationSettings {
    /// Se o algoritmo inteligente está ativado (se false, usa o comportamento padrão/antigo).
    pub enabled: bool,
    /// Limite máximo de vídeos do mesmo canal por página no feed (0 = sem limite).
    pub max_videos_per_channel: i64,
    /// Se deve impulsionar vídeos nunca assistidos na biblioteca.
    pub boost_unwatched: bool,
    /// Bônus de pontuação para vídeos nunca assistidos.
    pub unwatched_bonus: f64,
    /// Se deve penalizar vídeos longos já concluídos recentemente (fadiga).
    pub fatigue_filter: bool,
    /// Percentual de vídeos de serendipidade / exploração aleatória no feed (0 a 50).
    pub serendipity_percent: i64,
}

impl Default for RecommendationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            max_videos_per_channel: 3,
            boost_unwatched: true,
            unwatched_bonus: 5.0,
            fatigue_filter: true,
            serendipity_percent: 15,
        }
    }
}

pub async fn load_recommendation_settings(pool: &sqlx::SqlitePool) -> RecommendationSettings {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT value_json FROM settings WHERE key = 'recommendation'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some((json,)) = row {
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        RecommendationSettings::default()
    }
}

pub async fn save_recommendation_settings(
    pool: &sqlx::SqlitePool,
    settings: &RecommendationSettings,
) -> Result<(), sqlx::Error> {
    let clean = RecommendationSettings {
        enabled: settings.enabled,
        max_videos_per_channel: settings.max_videos_per_channel.clamp(0, 20),
        boost_unwatched: settings.boost_unwatched,
        unwatched_bonus: settings.unwatched_bonus.clamp(0.0, 50.0),
        fatigue_filter: settings.fatigue_filter,
        serendipity_percent: settings.serendipity_percent.clamp(0, 50),
    };

    let json = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at) VALUES ('recommendation', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at"
    )
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn get_recommendation_settings_endpoint(
    State(state): State<AppState>,
) -> Json<RecommendationSettings> {
    let settings = load_recommendation_settings(&state.pool).await;
    Json(settings)
}

pub(crate) async fn update_recommendation_settings_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<RecommendationSettings>,
) -> Result<Json<RecommendationSettings>, StatusCode> {
    save_recommendation_settings(&state.pool, &payload)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to save recommendation settings");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(load_recommendation_settings(&state.pool).await))
}

#[derive(sqlx::FromRow, Debug)]
pub struct ScoredMediaDbRow {
    #[sqlx(flatten)]
    pub media_row: MediaDbRow,
    pub score: f64,
}

impl std::ops::Deref for ScoredMediaDbRow {
    type Target = MediaDbRow;
    fn deref(&self) -> &Self::Target {
        &self.media_row
    }
}

impl ScoredMediaDbRow {
    pub fn into_media_item(self, tag_map: &mut HashMap<i64, Vec<TagRef>>) -> MediaItem {
        self.media_row.into_media_item(tag_map)
    }
}

/// Extrai palavras-chave significativas do título para correlação léxica trans-canal.
pub fn extract_significant_keywords(title: &str) -> Vec<String> {
    let stopwords = [
        "official",
        "video",
        "audio",
        "music",
        "visualizer",
        "lyric",
        "lyrics",
        "remastered",
        "remaster",
        "records",
        "napalm",
        "clip",
        "full",
        "live",
        "album",
        "version",
        "hd",
        "4k",
        "hq",
        "feat",
        "with",
        "from",
        "part",
        "para",
        "com",
        "uma",
        "dos",
        "das",
        "que",
        "show",
        "clipes",
    ];

    let mut words = Vec::new();
    for token in title.split(|c: char| !c.is_alphanumeric()) {
        let lower = token.trim().to_lowercase();
        if lower.len() >= 3
            && !stopwords.contains(&lower.as_str())
            && !lower.chars().all(|c| c.is_numeric())
            && !words.contains(&lower)
        {
            words.push(lower);
        }
    }
    words
}

/// Gera vídeos relacionados com inteligência trans-canal (entidades no título, tags sem ruído, artista MusicBrainz e mesmo canal).
pub(crate) async fn get_recommended_related_media(
    state: &AppState,
    media_id: i64,
    exclude_hidden: bool,
    settings: &RecommendationSettings,
) -> Result<Vec<MediaItem>, StatusCode> {
    let limit = 20i64;
    let candidate_limit = 100i64;
    let hidden_tags = if exclude_hidden {
        load_home_settings(&state.pool).await.hidden_tags
    } else {
        Vec::new()
    };

    // 1. Obter informações da mídia alvo
    let target = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "SELECT m.title, mc.channel_id, mmt.artist_name
         FROM media m
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN media_music_tracks mmt ON mmt.media_id = m.id
         WHERE m.id = ?",
    )
    .bind(media_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, "failed to fetch target media for recommendations");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let (target_title, target_channel_id, target_artist_name) = match target {
        Some(t) => t,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let title_keywords = extract_significant_keywords(&target_title);

    // 2. Montar lista de ruído para exclusão de tags genéricas
    let noise_in_clause = NOISE_TAGS
        .iter()
        .map(|t| format!("'{}'", t.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    // 3. Montar query SQL trans-canal com pesos combinados
    let mut title_match_expressions = Vec::new();
    for kw in title_keywords.iter().take(3) {
        let escaped = kw.replace('\'', "''");
        title_match_expressions.push(format!(
            "CASE WHEN lower(m.title) LIKE '%{}%' THEN 6.0 ELSE 0.0 END",
            escaped
        ));
    }
    let title_bonus_sql = if title_match_expressions.is_empty() {
        "0.0".to_string()
    } else {
        title_match_expressions.join(" + ")
    };

    let mut query = format!(
        "SELECT {MEDIA_SELECT_COLUMNS},
               (
                   -- Bônus de mesmo canal
                   COALESCE((
                       SELECT CASE WHEN mc.channel_id = ? THEN 5.0 ELSE 0.0 END
                   ), 0.0)
                   +
                   -- Bônus de tags limpas em comum (ignorando ruídos)
                   COALESCE((
                       SELECT COUNT(*) * 4.0
                       FROM media_tags mt
                       JOIN tags t ON t.id = mt.tag_id
                       WHERE mt.media_id = m.id
                         AND t.normalized_name NOT IN ({noise_in_clause})
                         AND mt.tag_id IN (
                             SELECT tag_id FROM media_tags WHERE media_id = ?
                         )
                   ), 0.0)
                   +
                   -- Bônus de mesmo artista (MusicBrainz)
                   COALESCE((
                       SELECT CASE WHEN ? IS NOT NULL AND lower(mmt.artist_name) = lower(?) THEN 8.0 ELSE 0.0 END
                       FROM media_music_tracks mmt
                       WHERE mmt.media_id = m.id
                   ), 0.0)
                   +
                   -- Bônus trans-canal por palavras-chave do título
                   ({title_bonus_sql})
               ) as score
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

    query.push_str(" ORDER BY score DESC, m.id DESC LIMIT ?");

    let mut sqlx_query = sqlx::query_as::<_, ScoredMediaDbRow>(sqlx::AssertSqlSafe(query.as_str()))
        .bind(&target_channel_id)
        .bind(media_id)
        .bind(&target_artist_name)
        .bind(&target_artist_name)
        .bind(media_id);

    if !hidden_tags.is_empty() {
        for htag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{htag}%"));
        }
        for htag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{htag}%"));
        }
        for htag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{htag}%"));
        }
    }

    sqlx_query = sqlx_query.bind(candidate_limit);

    let rows = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query recommended related media");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut channel_counts: HashMap<String, i64> = HashMap::new();
    let mut selected = Vec::with_capacity(limit as usize);
    for row in rows.into_iter().filter(|row| row.score > 0.0) {
        let channel_id = row.media_row.channel_id.clone().unwrap_or_default();
        let count = channel_counts.get(&channel_id).copied().unwrap_or(0);
        if settings.max_videos_per_channel > 0
            && !channel_id.is_empty()
            && count >= settings.max_videos_per_channel
        {
            continue;
        }
        if !channel_id.is_empty() {
            *channel_counts.entry(channel_id).or_insert(0) += 1;
        }
        selected.push(row);
        if selected.len() == limit as usize {
            break;
        }
    }

    let media_ids: Vec<i64> = selected.iter().map(|row| row.media_row.id).collect();
    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to fetch tags for recommended related media");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let items = selected
        .into_iter()
        .map(|row| row.into_media_item(&mut tag_map))
        .collect();

    Ok(items)
}

/// Codifica o cursor do feed recomendado: `offset_{n}`
pub fn encode_recommended_cursor(offset: usize) -> String {
    format!("rec_{:x}", offset)
}

pub fn decode_recommended_cursor(cursor: &str) -> Option<usize> {
    let hex = cursor.strip_prefix("rec_")?;
    usize::from_str_radix(hex, 16).ok()
}

fn parse_recent_exposure_factors(raw: Option<&str>) -> HashMap<i64, f64> {
    let mut factors = HashMap::new();
    let generation_factors = [0.15, 0.35, 0.65];
    let Some(raw) = raw else {
        return factors;
    };
    for (generation, ids) in raw.split('|').take(3).enumerate() {
        for id in ids.split(',').filter_map(|value| value.parse::<i64>().ok()) {
            factors
                .entry(id)
                .and_modify(|current| *current = f64::min(*current, generation_factors[generation]))
                .or_insert(generation_factors[generation]);
        }
    }
    factors
}

/// Gera o Feed da Home ("Para Você") com afinidade multi-fator, anti-bolha (MMR) e respeito a tags ocultas.
pub(crate) async fn get_recommended_feed(
    state: &AppState,
    filter: &FeedFilter,
    settings: &RecommendationSettings,
) -> Result<FeedResponse, StatusCode> {
    let limit = filter.limit.unwrap_or(24).clamp(1, 100);
    // Cursores inválidos ou no formato legado reiniciam a paginação.
    let offset = filter
        .cursor
        .as_deref()
        .and_then(decode_recommended_cursor)
        .unwrap_or_default();

    let seed = filter.seed.unwrap_or_else(crate::feed::generate_feed_seed);

    let hidden_tags = if filter.exclude_home_hidden == Some(true) {
        load_home_settings(&state.pool).await.hidden_tags
    } else {
        Vec::new()
    };

    let is_short_filter = match (filter.is_short, filter.media_type.as_deref()) {
        (Some(val), _) => Some(val),
        (None, Some("short")) => Some(true),
        (None, Some("video")) => Some(false),
        (None, None) if filter.exclude_home_hidden == Some(true) => Some(false),
        _ => None,
    };

    let noise_in_clause = NOISE_TAGS
        .iter()
        .map(|t| format!("'{}'", t.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    let mut query = format!(
        "WITH channel_signals AS (
            SELECT mc.channel_id,
                   SUM(5.0 / (1.0 + MAX(julianday('now') - julianday(s.watched_at), 0.0) / 30.0)) AS value
            FROM playback_scrobbles s
            JOIN media_channels mc ON mc.media_id = s.media_id
            GROUP BY mc.channel_id
            UNION ALL
            SELECT mc.channel_id, COUNT(*) * 2.0 AS value
            FROM favorites f
            JOIN media_channels mc ON mc.media_id = f.media_id
            GROUP BY mc.channel_id
            UNION ALL
            SELECT mc.channel_id,
                   SUM(
                       (0.2 + 1.8 * MIN(CAST(e.watched_ms AS REAL) / e.duration_ms, 1.0))
                       / (1.0 + MAX(julianday('now') - julianday(e.updated_at), 0.0) / 30.0)
                   ) AS value
            FROM playback_engagement_sessions e
            JOIN media_channels mc ON mc.media_id = e.media_id
            GROUP BY mc.channel_id
        ),
        raw_channel_affinity AS (
            SELECT channel_id, SUM(value) AS raw_aff
            FROM channel_signals
            GROUP BY channel_id
        ),
        user_channel_affinity AS (
            SELECT channel_id, 30.0 * raw_aff / (raw_aff + 20.0) AS affinity
            FROM raw_channel_affinity
        ),
        tag_signals AS (
            SELECT mt.tag_id,
                   SUM(2.0 / (1.0 + MAX(julianday('now') - julianday(s.watched_at), 0.0) / 30.0)) AS value
            FROM playback_scrobbles s
            JOIN media_tags mt ON mt.media_id = s.media_id
            JOIN tags t ON t.id = mt.tag_id
            WHERE t.normalized_name NOT IN ({noise_in_clause})
            GROUP BY mt.tag_id
            UNION ALL
            SELECT mt.tag_id,
                   SUM(
                       (0.1 + 0.9 * MIN(CAST(e.watched_ms AS REAL) / e.duration_ms, 1.0))
                       / (1.0 + MAX(julianday('now') - julianday(e.updated_at), 0.0) / 30.0)
                   ) AS value
            FROM playback_engagement_sessions e
            JOIN media_tags mt ON mt.media_id = e.media_id
            JOIN tags t ON t.id = mt.tag_id
            WHERE t.normalized_name NOT IN ({noise_in_clause})
            GROUP BY mt.tag_id
        ),
        raw_tag_affinity AS (
            SELECT tag_id, SUM(value) AS raw_aff
            FROM tag_signals
            GROUP BY tag_id
        ),
        user_tag_affinity AS (
            SELECT tag_id, 15.0 * raw_aff / (raw_aff + 10.0) AS tag_affinity
            FROM raw_tag_affinity
        ),
        media_retention AS (
            SELECT media_id,
                   COUNT(*) AS session_count,
                   MIN(SUM(CAST(watched_ms AS REAL)) / SUM(duration_ms), 1.0) AS retention_ratio
            FROM playback_engagement_sessions
            GROUP BY media_id
        ),
        media_consumption AS (
            SELECT media_id, COUNT(*) AS play_count
            FROM playback_scrobbles
            GROUP BY media_id
        )
        SELECT 
            {MEDIA_SELECT_COLUMNS},
            (
                COALESCE(uca.affinity, 0.0) * 1.5 +
                CASE 
                    WHEN COALESCE((
                        SELECT SUM(uta.tag_affinity)
                        FROM media_tags mt2
                        JOIN user_tag_affinity uta ON uta.tag_id = mt2.tag_id
                        WHERE mt2.media_id = m.id
                    ), 0.0) > 20.0 THEN 20.0
                    ELSE COALESCE((
                        SELECT SUM(uta.tag_affinity)
                        FROM media_tags mt2
                        JOIN user_tag_affinity uta ON uta.tag_id = mt2.tag_id
                        WHERE mt2.media_id = m.id
                    ), 0.0)
                END +
                -- Bônus de novidade / não assistido
                (CASE WHEN ps.completed IS NULL AND ? THEN ? ELSE 0.0 END) +
                -- A Home prioriza descoberta em vez de repetir itens já iniciados.
                (CASE WHEN ps.position_ms > 0 THEN -8.0 ELSE 0.0 END) +
                -- Fadiga também vale para música, mas com janela menor.
                (CASE
                    WHEN NOT ? OR ps.completed <> 1 THEN 0.0
                    WHEN mmt.media_id IS NOT NULL AND ps.completed_at >= datetime('now', '-3 days') THEN -15.0
                    WHEN mmt.media_id IS NULL AND ps.completed_at >= datetime('now', '-14 days') THEN -25.0
                    ELSE 0.0
                END) +
                -- Repetir o mesmo item muitas vezes reduz sua utilidade como recomendação.
                (-MIN(COALESCE(mco.play_count, 0) * 2.0, 30.0)) +
                -- Retenção geral só afeta diretamente o item após mais de uma sessão.
                (CASE WHEN mr.session_count >= 2 THEN (mr.retention_ratio - 0.5) * 10.0 ELSE 0.0 END)
            ) AS score
        FROM media m
        LEFT JOIN media_channels mc ON mc.media_id = m.id
        LEFT JOIN channels c ON c.id = mc.channel_id
        LEFT JOIN user_channel_affinity uca ON uca.channel_id = mc.channel_id
        LEFT JOIN playback_state ps ON ps.media_id = m.id
        LEFT JOIN favorites fav ON fav.media_id = m.id
        LEFT JOIN thumbnails t ON t.media_id = m.id
        LEFT JOIN media_music_tracks mmt ON mmt.media_id = m.id
        LEFT JOIN media_retention mr ON mr.media_id = m.id
        LEFT JOIN media_consumption mco ON mco.media_id = m.id
        WHERE m.is_available = 1",
    );

    if let Some(is_short) = is_short_filter {
        query.push_str(&format!(
            " AND m.is_short = {}",
            if is_short { 1 } else { 0 }
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

    query.push_str(" ORDER BY score DESC, m.id DESC");

    let mut sqlx_query = sqlx::query_as::<_, ScoredMediaDbRow>(sqlx::AssertSqlSafe(query.as_str()))
        .bind(settings.boost_unwatched)
        .bind(settings.unwatched_bonus)
        .bind(settings.fatigue_filter);

    if let Some(tag) = &filter.tag {
        sqlx_query = sqlx_query.bind(tag).bind(tag);
    }

    if let Some(channel) = &filter.channel {
        sqlx_query = sqlx_query.bind(channel).bind(channel);
    }

    if !hidden_tags.is_empty() {
        for htag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{htag}%"));
        }
        for htag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{htag}%"));
        }
        for htag in &hidden_tags {
            sqlx_query = sqlx_query.bind(format!("%{htag}%"));
        }
    }

    let candidates = sqlx_query.fetch_all(&state.pool).await.map_err(|error| {
        tracing::error!(%error, "failed to query recommended feed candidates");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // O score define chance, não uma ordem obrigatória. A seleção usa uma corrida
    // exponencial ponderada, determinística para a mesma seed e realmente distinta
    // entre seeds diferentes.
    let max_per_channel = settings.max_videos_per_channel;
    let target_count = offset.saturating_add(limit as usize);
    let mut selected_rows: Vec<ScoredMediaDbRow> = Vec::with_capacity(target_count);
    let mut channel_counts: HashMap<String, i64> = HashMap::new();

    let mut pool: Vec<ScoredMediaDbRow> = candidates;
    let min_score = pool
        .iter()
        .map(|candidate| candidate.score)
        .fold(f64::INFINITY, f64::min);
    let max_score = pool
        .iter()
        .map(|candidate| candidate.score)
        .fold(f64::NEG_INFINITY, f64::max);
    let score_span = (max_score - min_score).max(0.000_001);
    let relevance_span = 4.0 - settings.serendipity_percent.clamp(0, 50) as f64 * 0.04;
    let recent_factors = parse_recent_exposure_factors(filter.recent_ids.as_deref());

    while selected_rows.len() < target_count && !pool.is_empty() {
        let page_position = selected_rows.len() % limit as usize;
        if page_position == 0 {
            channel_counts.clear();
        }
        let last_ch = selected_rows
            .last()
            .and_then(|r| r.media_row.channel_id.as_deref())
            .unwrap_or("");
        let last_kw = selected_rows
            .last()
            .and_then(|r| {
                extract_significant_keywords(&r.media_row.title)
                    .into_iter()
                    .next()
            })
            .unwrap_or_default();

        let step_idx = selected_rows.len();
        // Encontrar o vencedor probabilístico elegível. `-ln(U) / peso` é uma
        // corrida exponencial: pesos maiores vencem mais, mas nunca sempre.
        let mut best_idx: Option<usize> = None;
        let mut best_race_key = f64::INFINITY;

        for (idx, cand) in pool.iter().enumerate() {
            let ch_id = cand.media_row.channel_id.as_deref().unwrap_or("");
            let kws = extract_significant_keywords(&cand.media_row.title);
            let first_kw = kws.first().map(|s| s.as_str()).unwrap_or("");

            let ch_cnt = if !ch_id.is_empty() {
                channel_counts.get(ch_id).copied().unwrap_or(0)
            } else {
                0
            };
            if max_per_channel > 0 && !ch_id.is_empty() && ch_cnt >= max_per_channel {
                continue;
            }

            let normalized_score = ((cand.score - min_score) / score_span).clamp(0.0, 1.0);
            let mut weight = 1.0 + relevance_span * normalized_score.powi(2);
            if cand.media_row.position_ms.is_none() {
                weight *= 1.4;
            }
            weight *= recent_factors
                .get(&cand.media_row.id)
                .copied()
                .unwrap_or(1.0);
            weight *= match ch_cnt {
                0 => 1.0,
                1 => 0.5,
                _ => 0.25,
            };
            if !ch_id.is_empty() && ch_id == last_ch {
                weight *= 0.25;
            } else if !first_kw.is_empty() && first_kw == last_kw {
                weight *= 0.5;
            }

            let random = crate::feed::seeded_unit_interval(seed, cand.media_row.id, step_idx);
            let race_key = -random.ln() / weight.max(0.000_001);
            if race_key < best_race_key {
                best_race_key = race_key;
                best_idx = Some(idx);
            }
        }

        // Se a cota esgotar o pool, completa a página probabilisticamente sem
        // deixar espaços vazios. A penalidade por canal continua aplicada.
        let chosen_idx = match best_idx {
            Some(idx) => idx,
            None => {
                let mut fallback_best_idx = 0;
                let mut fallback_best_key = f64::INFINITY;
                for (idx, cand) in pool.iter().enumerate() {
                    let channel_id = cand.media_row.channel_id.as_deref().unwrap_or("");
                    let channel_count = channel_counts.get(channel_id).copied().unwrap_or(0);
                    let normalized_score = ((cand.score - min_score) / score_span).clamp(0.0, 1.0);
                    let mut weight = 1.0 + relevance_span * normalized_score.powi(2);
                    weight *= recent_factors
                        .get(&cand.media_row.id)
                        .copied()
                        .unwrap_or(1.0);
                    weight *= match channel_count {
                        0 => 1.0,
                        1 => 0.5,
                        _ => 0.25,
                    };
                    if !channel_id.is_empty() && channel_id == last_ch {
                        weight *= 0.25;
                    }
                    let random =
                        crate::feed::seeded_unit_interval(seed, cand.media_row.id, step_idx);
                    let race_key = -random.ln() / weight.max(0.000_001);
                    if race_key < fallback_best_key {
                        fallback_best_key = race_key;
                        fallback_best_idx = idx;
                    }
                }
                fallback_best_idx
            }
        };

        let chosen = pool.remove(chosen_idx);
        let ch_id = chosen.media_row.channel_id.clone().unwrap_or_default();

        if !ch_id.is_empty() {
            *channel_counts.entry(ch_id).or_insert(0) += 1;
        }

        selected_rows.push(chosen);
    }

    let has_more = !pool.is_empty();
    let page_rows = selected_rows
        .into_iter()
        .skip(offset)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let media_ids: Vec<i64> = page_rows.iter().map(|r| r.media_row.id).collect();
    let mut tag_map = fetch_tags_for_media(&state.pool, &media_ids)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to fetch tags for recommended feed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let items = page_rows
        .into_iter()
        .map(|row| row.into_media_item(&mut tag_map))
        .collect::<Vec<_>>();

    let next_cursor = if has_more {
        Some(encode_recommended_cursor(offset + items.len()))
    } else {
        None
    };

    Ok(FeedResponse {
        items,
        next_cursor,
        seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_significant_keywords() {
        let title = "AURORA ATLAS - Starlight (HD - 4K Remastered) Official Music Video";
        let words = extract_significant_keywords(title);
        assert!(words.contains(&"aurora".to_string()));
        assert!(words.contains(&"starlight".to_string()));
        assert!(!words.contains(&"official".to_string()));
        assert!(!words.contains(&"video".to_string()));
        assert!(!words.contains(&"remastered".to_string()));
    }

    #[test]
    fn test_cursor_roundtrip() {
        let offset = 48;
        let encoded = encode_recommended_cursor(offset);
        let decoded = decode_recommended_cursor(&encoded).expect("should decode");
        assert_eq!(decoded, offset);
    }

    #[test]
    fn recent_exposure_penalty_decays_across_generations() {
        let factors = parse_recent_exposure_factors(Some("10,11|11,12|13"));
        assert_eq!(factors.get(&10), Some(&0.15));
        assert_eq!(factors.get(&11), Some(&0.15));
        assert_eq!(factors.get(&12), Some(&0.35));
        assert_eq!(factors.get(&13), Some(&0.65));
    }

    #[tokio::test]
    async fn test_recommendation_settings_crud() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();

        let initial = load_recommendation_settings(&pool).await;
        assert!(!initial.enabled);
        assert_eq!(initial.max_videos_per_channel, 3);

        let modified = RecommendationSettings {
            enabled: true,
            max_videos_per_channel: 5,
            boost_unwatched: true,
            unwatched_bonus: 7.5,
            fatigue_filter: false,
            serendipity_percent: 20,
        };
        save_recommendation_settings(&pool, &modified)
            .await
            .unwrap();

        let reloaded = load_recommendation_settings(&pool).await;
        assert!(reloaded.enabled);
        assert_eq!(reloaded.max_videos_per_channel, 5);
        assert_eq!(reloaded.unwatched_bonus, 7.5);
        assert!(!reloaded.fatigue_filter);
    }

    #[tokio::test]
    async fn test_recommended_related_media_trans_channel() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_official', 'Aurora Atlas Official'), ('ch_fan', 'Fan Remasters'), ('ch_other', 'Different Creator')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO media (id, title, is_available, is_short) VALUES (1, 'AURORA ATLAS - Starlight Official', 1, 0), (2, 'AURORA ATLAS - Dawn Echo 4K Remaster', 1, 0), (3, 'Unrelated Tutorial Episode', 1, 0), (4, 'AURORA ATLAS - Starlight Short', 1, 1)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (1, 'ch_official', 'manual'), (2, 'ch_fan', 'manual'), (3, 'ch_other', 'manual'), (4, 'ch_fan', 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'Symphonic Metal', 'symphonic metal')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (1, 1, 'manual'), (2, 1, 'manual'), (4, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        let settings = RecommendationSettings {
            enabled: true,
            ..Default::default()
        };

        let related = get_recommended_related_media(&state, 1, true, &settings)
            .await
            .unwrap();
        // Candidatos sem qualquer relação deixam de completar artificialmente a lista.
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].id, 2);
        assert_eq!(related[0].channel.as_ref().unwrap().id, "ch_fan");
    }

    #[tokio::test]
    async fn test_recommended_feed_diversity_and_hidden_tags() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_mono', 'Monopoly Channel'), ('ch_div', 'Diverse Channel')")
            .execute(&pool)
            .await
            .unwrap();

        // 4 videos from Monopoly Channel, 1 from Diverse, 1 with hidden tag
        for i in 1..=4 {
            sqlx::query("INSERT INTO media (id, title, is_available) VALUES (?, ?, 1)")
                .bind(i)
                .bind(format!("Mono Video {i}"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (?, 'ch_mono', 'manual')")
                .bind(i)
                .execute(&pool)
                .await
                .unwrap();
        }

        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (5, 'Diverse Video', 1), (6, 'Hidden Spam Video', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (5, 'ch_div', 'manual'), (6, 'ch_div', 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO tags (id, display_name, normalized_name) VALUES (1, 'SpamTag', 'spamtag')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO media_tags (media_id, tag_id, origin) VALUES (6, 1, 'manual')")
            .execute(&pool)
            .await
            .unwrap();

        crate::catalog::save_home_settings(
            &pool,
            &crate::catalog::HomeSettings {
                hidden_tags: vec!["spamtag".to_string()],
            },
        )
        .await
        .unwrap();

        // Limite de 2 videos por canal na página
        let settings = RecommendationSettings {
            enabled: true,
            max_videos_per_channel: 2,
            ..Default::default()
        };

        let filter = FeedFilter {
            limit: Some(3),
            exclude_home_hidden: Some(true),
            ..Default::default()
        };

        let res = get_recommended_feed(&state, &filter, &settings)
            .await
            .unwrap();

        // Video 6 (SpamTag) JAMAIS deve aparecer
        assert!(!res.items.iter().any(|item| item.id == 6));

        // Conta quantos são do Monopoly Channel na primeira página de 3 itens
        let mono_count = res
            .items
            .iter()
            .filter(|item| item.channel.as_ref().map(|c| c.id.as_str()) == Some("ch_mono"))
            .count();
        // Não pode ultrapassar max_videos_per_channel (2)
        assert!(
            mono_count <= 2,
            "Monopoly channel excedeu limite de diversidade: {mono_count}"
        );
        // O video 5 (Diverse) deve ter sido incluído
        assert!(res.items.iter().any(|item| item.id == 5));
    }

    #[tokio::test]
    async fn test_recommended_feed_soft_interleaving() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query("INSERT INTO channels (id, name) VALUES ('ch_alpha', 'Channel Alpha'), ('ch_beta', 'Channel Beta')")
            .execute(&pool)
            .await
            .unwrap();

        for i in 1..=3 {
            sqlx::query("INSERT INTO media (id, title, is_available) VALUES (?, ?, 1)")
                .bind(10 + i)
                .bind(format!("Alpha Item {i}"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (?, 'ch_alpha', 'manual')")
                .bind(10 + i)
                .execute(&pool)
                .await
                .unwrap();

            sqlx::query("INSERT INTO media (id, title, is_available) VALUES (?, ?, 1)")
                .bind(20 + i)
                .bind(format!("Beta Item {i}"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (?, 'ch_beta', 'manual')")
                .bind(20 + i)
                .execute(&pool)
                .await
                .unwrap();
        }

        let settings = RecommendationSettings {
            enabled: true,
            max_videos_per_channel: 3,
            ..Default::default()
        };

        let filter = FeedFilter {
            limit: Some(6),
            seed: Some(12345),
            ..Default::default()
        };

        let res = get_recommended_feed(&state, &filter, &settings)
            .await
            .unwrap();
        assert_eq!(res.items.len(), 6);

        let channels: Vec<String> = res
            .items
            .iter()
            .map(|it| it.channel.as_ref().unwrap().id.clone())
            .collect();

        // Não deve ter 3 vídeos seguidos do mesmo canal
        for window in channels.windows(3) {
            let all_same = window[0] == window[1] && window[1] == window[2];
            assert!(
                !all_same,
                "Detectada trinca consecutiva do mesmo canal: {:?}",
                window
            );
        }
    }

    #[tokio::test]
    async fn high_score_increases_probability_without_becoming_mandatory() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        sqlx::query("INSERT INTO channels (id, name) VALUES ('favored', 'Favored Channel')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Favored Candidate', 1), (100, 'Affinity Signal', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (1, 'favored', 'manual'), (100, 'favored', 'manual')")
            .execute(&pool)
            .await
            .unwrap();
        for id in 2..=11 {
            let channel_id = format!("channel-{id}");
            sqlx::query("INSERT INTO channels (id, name) VALUES (?, ?)")
                .bind(&channel_id)
                .bind(format!("Channel {id}"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO media (id, title, is_available) VALUES (?, ?, 1)")
                .bind(id)
                .bind(format!("Candidate {id}"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO media_channels (media_id, channel_id, origin) VALUES (?, ?, 'manual')",
            )
            .bind(id)
            .bind(channel_id)
            .execute(&pool)
            .await
            .unwrap();
        }
        for _ in 0..10 {
            sqlx::query(
                "INSERT INTO playback_scrobbles (media_id, duration_ms) VALUES (100, 60000)",
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let settings = RecommendationSettings {
            enabled: true,
            ..Default::default()
        };
        let mut normal_frequency = 0;
        let mut recent_frequency = 0;
        let mut other_frequencies = HashMap::<i64, usize>::new();
        for seed in 1..=40 {
            let normal = get_recommended_feed(
                &state,
                &FeedFilter {
                    seed: Some(seed),
                    limit: Some(1),
                    ..Default::default()
                },
                &settings,
            )
            .await
            .unwrap();
            let selected_id = normal.items[0].id;
            if selected_id == 1 {
                normal_frequency += 1;
            } else {
                *other_frequencies.entry(selected_id).or_insert(0) += 1;
            }

            let with_recent_penalty = get_recommended_feed(
                &state,
                &FeedFilter {
                    seed: Some(seed),
                    limit: Some(1),
                    recent_ids: Some("1".to_owned()),
                    ..Default::default()
                },
                &settings,
            )
            .await
            .unwrap();
            if with_recent_penalty.items[0].id == 1 {
                recent_frequency += 1;
            }
        }

        let most_common_other = other_frequencies.values().copied().max().unwrap_or(0);
        assert!(normal_frequency > most_common_other);
        assert!(
            normal_frequency < 40,
            "score alto não pode tornar o item obrigatório"
        );
        assert!(recent_frequency < normal_frequency);
    }

    #[tokio::test]
    async fn recommended_feed_pages_never_repeat_selected_candidates() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::for_test(pool.clone());

        for channel in 1..=8 {
            sqlx::query("INSERT INTO channels (id, name) VALUES (?, ?)")
                .bind(format!("ch_{channel}"))
                .bind(format!("Channel {channel}"))
                .execute(&pool)
                .await
                .unwrap();
            for item in 1..=10 {
                let id = channel * 100 + item;
                sqlx::query("INSERT INTO media (id, title, is_available) VALUES (?, ?, 1)")
                    .bind(id)
                    .bind(format!("Channel {channel} Item {item}"))
                    .execute(&pool)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO media_channels (media_id, channel_id, origin) VALUES (?, ?, 'manual')")
                    .bind(id)
                    .bind(format!("ch_{channel}"))
                    .execute(&pool)
                    .await
                    .unwrap();
            }
        }

        let settings = RecommendationSettings {
            enabled: true,
            serendipity_percent: 15,
            ..Default::default()
        };
        let first = get_recommended_feed(
            &state,
            &FeedFilter {
                seed: Some(42),
                limit: Some(24),
                ..Default::default()
            },
            &settings,
        )
        .await
        .unwrap();
        let second = get_recommended_feed(
            &state,
            &FeedFilter {
                seed: Some(42),
                cursor: first.next_cursor.clone(),
                limit: Some(24),
                ..Default::default()
            },
            &settings,
        )
        .await
        .unwrap();

        assert_eq!(first.items.len(), 24);
        assert_eq!(second.items.len(), 24);
        let mut channel_counts = std::collections::HashMap::new();
        for channel_id in first
            .items
            .iter()
            .filter_map(|item| item.channel.as_ref().map(|channel| channel.id.as_str()))
        {
            *channel_counts.entry(channel_id).or_insert(0) += 1;
        }
        assert!(
            channel_counts.values().all(|count| *count <= 3),
            "a cota configurada deve valer para a página inteira: {channel_counts:?}"
        );
        let first_ids = first.items.iter().map(|item| item.id).collect::<Vec<_>>();
        assert!(
            second
                .items
                .iter()
                .all(|item| !first_ids.contains(&item.id))
        );

        let repeated = get_recommended_feed(
            &state,
            &FeedFilter {
                seed: Some(42),
                limit: Some(24),
                ..Default::default()
            },
            &settings,
        )
        .await
        .unwrap();
        assert_eq!(
            first_ids,
            repeated
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>()
        );

        let mut sixth_position_ids = std::collections::HashSet::new();
        for seed in 1..=10 {
            let response = get_recommended_feed(
                &state,
                &FeedFilter {
                    seed: Some(seed),
                    limit: Some(24),
                    ..Default::default()
                },
                &settings,
            )
            .await
            .unwrap();
            sixth_position_ids.insert(response.items[5].id);
        }
        assert!(
            sixth_position_ids.len() >= 5,
            "seeds diferentes devem rotacionar materialmente o sexto card: {sixth_position_ids:?}"
        );
    }
}
