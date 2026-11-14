pub mod listenbrainz;
pub mod musicbrainz;

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::web::AppState;

use crate::crypto::AppCrypto;

const SETTINGS_KEY: &str = "listenbrainz_settings";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListenBrainzSettings {
    pub enabled: bool,
    #[serde(default)]
    pub user_token: String,
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub token_hint: Option<String>,
    pub username: Option<String>,
    pub auto_scrobble: bool,
}

impl std::fmt::Debug for ListenBrainzSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let masked = if self.user_token.is_empty() {
            "[EMPTY]"
        } else {
            "[REDACTED]"
        };
        f.debug_struct("ListenBrainzSettings")
            .field("enabled", &self.enabled)
            .field("user_token", &masked)
            .field("has_token", &self.has_token)
            .field("token_hint", &self.token_hint)
            .field("username", &self.username)
            .field("auto_scrobble", &self.auto_scrobble)
            .finish()
    }
}

impl Default for ListenBrainzSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            user_token: String::new(),
            has_token: false,
            token_hint: None,
            username: None,
            auto_scrobble: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct MediaMusicTrack {
    pub media_id: i64,
    pub recording_mbid: String,
    pub artist_name: String,
    pub track_name: String,
    pub release_name: Option<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_scrobbled_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MusicCandidate {
    pub recording_mbid: String,
    pub track_name: String,
    pub artist_name: String,
    pub release_name: Option<String>,
    pub duration_secs: Option<u32>,
    pub score: f32,
    pub source: String,
    #[serde(default)]
    pub disambiguation: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub release_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssociateMusicRequest {
    #[serde(default)]
    pub recording_mbid: Option<String>,
    pub artist_name: String,
    pub track_name: String,
    pub release_name: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchMusicQuery {
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct LookupMbidQuery {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct ScrobbleResponse {
    pub success: bool,
    pub scrobbled_at: String,
    pub message: String,
}

// ==========================================
// DB Helpers
// ==========================================

pub async fn load_listenbrainz_settings(
    pool: &SqlitePool,
    crypto: &AppCrypto,
) -> ListenBrainzSettings {
    let row = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

    if let Some(json_str) = row
        && let Ok(mut settings) = serde_json::from_str::<ListenBrainzSettings>(&json_str)
    {
        // Se o token estiver armazenado com envelope enc:v1:, descriptografa
        if AppCrypto::is_encrypted(&settings.user_token) {
            settings.user_token = crypto
                .decrypt_string(&settings.user_token)
                .unwrap_or_default();
        } else if !settings.user_token.trim().is_empty() {
            tracing::error!("legacy ListenBrainz token rejected after migration period");
            settings.user_token.clear();
        }
        settings.has_token = !settings.user_token.trim().is_empty();
        settings.token_hint = if settings.has_token {
            let t = settings.user_token.trim();
            let last4 = if t.len() >= 4 { &t[t.len() - 4..] } else { t };
            Some(format!("••••••••{last4}"))
        } else {
            None
        };
        return settings;
    }
    ListenBrainzSettings::default()
}

pub async fn save_listenbrainz_settings(
    pool: &SqlitePool,
    crypto: &AppCrypto,
    settings: &ListenBrainzSettings,
) -> Result<(), sqlx::Error> {
    let mut to_save = settings.clone();
    // Criptografa o token do ListenBrainz com AEAD antes de persistir no banco de dados
    if !to_save.user_token.trim().is_empty()
        && !AppCrypto::is_encrypted(&to_save.user_token)
        && let Ok(encrypted) = crypto.encrypt_string(to_save.user_token.trim())
    {
        to_save.user_token = encrypted;
    }
    to_save.has_token = false;
    to_save.token_hint = None;

    let json_str = serde_json::to_string(&to_save).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = datetime('now')",
    )
    .bind(SETTINGS_KEY)
    .bind(json_str)
    .execute(pool)
    .await?;
    Ok(())
}

/// Migra automaticamente tokens existentes em texto puro para o envelope criptografado 'enc:v1:...'
pub async fn migrate_listenbrainz_token_if_needed(
    pool: &SqlitePool,
    crypto: &AppCrypto,
) -> Result<(), crate::error::AppError> {
    let row = sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(pool)
        .await?;

    if let Some(json_str) = row
        && let Ok(mut settings) = serde_json::from_str::<ListenBrainzSettings>(&json_str)
    {
        let clean = settings.user_token.trim().to_owned();
        if !clean.is_empty() && !AppCrypto::is_encrypted(&clean) {
            settings.user_token = crypto.encrypt_string(&clean)?;
            settings.has_token = false;
            settings.token_hint = None;
            let encoded = serde_json::to_string(&settings).map_err(|error| {
                crate::error::AppError::Config(format!(
                    "falha ao serializar migração do ListenBrainz: {error}"
                ))
            })?;
            sqlx::query(
                "UPDATE settings SET value_json = ?, updated_at = datetime('now') WHERE key = ?",
            )
            .bind(encoded)
            .bind(SETTINGS_KEY)
            .execute(pool)
            .await?;
            tracing::info!("ListenBrainz token migrated and verified as enc:v1");
        }
    }
    Ok(())
}

pub async fn get_media_music_track(
    pool: &SqlitePool,
    media_id: i64,
) -> Result<Option<MediaMusicTrack>, sqlx::Error> {
    sqlx::query_as::<_, MediaMusicTrack>(
        "SELECT media_id, recording_mbid, artist_name, track_name, release_name, source, created_at, updated_at, last_scrobbled_at
         FROM media_music_tracks
         WHERE media_id = ?",
    )
    .bind(media_id)
    .fetch_optional(pool)
    .await
}

pub async fn save_media_music_track(
    pool: &SqlitePool,
    media_id: i64,
    req: &AssociateMusicRequest,
) -> Result<MediaMusicTrack, sqlx::Error> {
    let source = req.source.as_deref().unwrap_or("manual_id");
    let mbid = req.recording_mbid.as_deref().map(str::trim).unwrap_or("");
    sqlx::query(
        "INSERT INTO media_music_tracks (media_id, recording_mbid, artist_name, track_name, release_name, source, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
         ON CONFLICT(media_id) DO UPDATE SET
            recording_mbid = excluded.recording_mbid,
            artist_name = excluded.artist_name,
            track_name = excluded.track_name,
            release_name = excluded.release_name,
            source = excluded.source,
            updated_at = datetime('now')",
    )
    .bind(media_id)
    .bind(mbid)
    .bind(req.artist_name.trim())
    .bind(req.track_name.trim())
    .bind(req.release_name.as_deref().map(str::trim))
    .bind(source)
    .execute(pool)
    .await?;

    get_media_music_track(pool, media_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn delete_media_music_track(
    pool: &SqlitePool,
    media_id: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM media_music_tracks WHERE media_id = ?")
        .bind(media_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn record_scrobble(
    pool: &SqlitePool,
    media_id: i64,
    scrobbled_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE media_music_tracks SET last_scrobbled_at = ?, updated_at = datetime('now') WHERE media_id = ?",
    )
    .bind(scrobbled_at)
    .bind(media_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn extract_mbid_from_input(input: &str) -> String {
    let trimmed = input.trim();
    // Se for URL do listenbrainz ou musicbrainz, pega o último segmento que se parece com UUID
    for part in trimmed.split('/') {
        let clean = part.trim();
        if clean.len() == 36 && clean.chars().filter(|c| *c == '-').count() == 4 {
            return clean.to_string();
        }
    }
    trimmed.to_string()
}

// ==========================================
// Axum Handlers
// ==========================================

pub(crate) async fn get_listenbrainz_settings_endpoint(
    State(state): State<AppState>,
) -> Json<ListenBrainzSettings> {
    let settings = load_listenbrainz_settings(&state.pool, &state.crypto).await;
    let mut response = settings.clone();
    response.user_token = String::new(); // Mascara o token: nunca expõe a chave real na API
    Json(response)
}

pub(crate) async fn update_listenbrainz_settings_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<ListenBrainzSettings>,
) -> Result<Json<ListenBrainzSettings>, StatusCode> {
    let mut current = load_listenbrainz_settings(&state.pool, &state.crypto).await;
    current.enabled = payload.enabled;
    current.auto_scrobble = payload.auto_scrobble;
    let new_token = payload.user_token.trim();
    if !new_token.is_empty() && !new_token.starts_with('•') {
        current.user_token = new_token.to_string();
    } else if new_token.is_empty() && payload.username.is_none() {
        current.user_token = String::new();
        current.username = None;
    }
    if let Some(user) = payload.username {
        current.username = if user.trim().is_empty() {
            None
        } else {
            Some(user)
        };
    }

    save_listenbrainz_settings(&state.pool, &state.crypto, &current)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to save ListenBrainz settings");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut response = load_listenbrainz_settings(&state.pool, &state.crypto).await;
    response.user_token = String::new(); // Mascara o token
    Ok(Json(response))
}

pub async fn validate_token_endpoint(
    Json(payload): Json<listenbrainz::TokenValidationResult>,
) -> Json<listenbrainz::TokenValidationResult> {
    // Se passar token vazio ou nulo
    let token = payload.message; // ou campo dedicado
    let res = listenbrainz::validate_user_token(&token)
        .await
        .unwrap_or_else(|e| listenbrainz::TokenValidationResult {
            valid: false,
            username: None,
            message: e,
        });
    Json(res)
}

pub(crate) async fn validate_token_post(
    Json(body): Json<serde_json::Value>,
) -> Json<listenbrainz::TokenValidationResult> {
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");
    let res = listenbrainz::validate_user_token(token)
        .await
        .unwrap_or_else(|e| listenbrainz::TokenValidationResult {
            valid: false,
            username: None,
            message: e,
        });
    Json(res)
}

pub(crate) async fn get_media_music_endpoint(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Result<Json<Option<MediaMusicTrack>>, StatusCode> {
    let track = get_media_music_track(&state.pool, media_id)
        .await
        .map_err(|e| {
            tracing::error!(%e, media_id, "failed to fetch media music association");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(track))
}

pub(crate) async fn save_media_music_endpoint(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
    Json(payload): Json<AssociateMusicRequest>,
) -> Result<Json<MediaMusicTrack>, StatusCode> {
    let saved = save_media_music_track(&state.pool, media_id, &payload)
        .await
        .map_err(|e| {
            tracing::error!(%e, media_id, "failed to associate music with media");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(saved))
}

pub(crate) async fn delete_media_music_endpoint(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Result<StatusCode, StatusCode> {
    delete_media_music_track(&state.pool, media_id)
        .await
        .map_err(|e| {
            tracing::error!(%e, media_id, "failed to unlink music from media");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn search_music_endpoint(
    Query(query): Query<SearchMusicQuery>,
) -> Result<Json<Vec<MusicCandidate>>, (StatusCode, Json<serde_json::Value>)> {
    let clean = query.q.trim();
    if clean.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let candidates = match musicbrainz::search_musicbrainz(clean).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(%e, query = %clean, "failed to search MusicBrainz");
            if e.contains("503") || e.contains("429") {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(crate::error::error_json(
                        "upstream_unavailable",
                        "O servidor do MusicBrainz está temporariamente sobrecarregado (503). Por favor, aguarde alguns segundos e tente novamente.",
                    )),
                ));
            }
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(crate::error::error_json(
                    "upstream_error",
                    format!("Falha na consulta ao MusicBrainz: {e}"),
                )),
            ));
        }
    };
    Ok(Json(candidates))
}

pub(crate) async fn lookup_mbid_endpoint(
    Query(query): Query<LookupMbidQuery>,
) -> Result<Json<Option<MusicCandidate>>, (StatusCode, Json<serde_json::Value>)> {
    let mbid = extract_mbid_from_input(&query.id);
    let candidate = match musicbrainz::lookup_recording_mbid(&mbid).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(%e, mbid, "failed to query MBID");
            if e.contains("503") || e.contains("429") {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(crate::error::error_json(
                        "upstream_unavailable",
                        "O servidor do MusicBrainz está temporariamente sobrecarregado (503). Aguarde alguns instantes.",
                    )),
                ));
            }
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(crate::error::error_json(
                    "upstream_error",
                    format!("Falha na consulta ao MusicBrainz: {e}"),
                )),
            ));
        }
    };
    Ok(Json(candidate))
}

pub(crate) async fn recognize_media_endpoint(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Result<Json<Vec<MusicCandidate>>, (StatusCode, Json<serde_json::Value>)> {
    // Buscar título da mídia e nome do canal (se houver) para limpeza
    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT m.title, c.name
         FROM media m
         LEFT JOIN media_channels mc ON mc.media_id = m.id
         LEFT JOIN channels c ON c.id = mc.channel_id
         WHERE m.id = ?
         LIMIT 1",
    )
    .bind(media_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, media_id, "failed to fetch media for music search");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(crate::error::error_json(
                "internal_error",
                "Erro no banco de dados",
            )),
        )
    })?;

    let (media_title, channel_name) = match row {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(crate::error::error_json(
                    "not_found",
                    "Mídia não encontrada",
                )),
            ));
        }
    };

    let clean_title = clean_video_title(&media_title, channel_name.as_deref());
    if clean_title.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let candidates = match musicbrainz::search_musicbrainz(&clean_title).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(%e, query = %clean_title, "MusicBrainz search failed");
            if e.contains("503") || e.contains("429") {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(crate::error::error_json(
                        "upstream_unavailable",
                        "O servidor do MusicBrainz está temporariamente sobrecarregado (503). Por favor, aguarde alguns segundos e tente novamente.",
                    )),
                ));
            }
            return Err((
                StatusCode::BAD_GATEWAY,
                Json(crate::error::error_json(
                    "upstream_error",
                    format!("Falha na consulta ao MusicBrainz: {e}"),
                )),
            ));
        }
    };

    Ok(Json(candidates))
}

pub(crate) async fn scrobble_media_endpoint(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Result<Json<ScrobbleResponse>, (StatusCode, Json<ScrobbleResponse>)> {
    let settings = load_listenbrainz_settings(&state.pool, &state.crypto).await;
    if !settings.enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ScrobbleResponse {
                success: false,
                scrobbled_at: String::new(),
                message: "Módulo ListenBrainz está desativado nas configurações".to_string(),
            }),
        ));
    }

    if settings.user_token.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ScrobbleResponse {
                success: false,
                scrobbled_at: String::new(),
                message: "Token do ListenBrainz não configurado".to_string(),
            }),
        ));
    }

    let track = match get_media_music_track(&state.pool, media_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ScrobbleResponse {
                    success: false,
                    scrobbled_at: String::new(),
                    message: "Nenhuma música associada a esta mídia".to_string(),
                }),
            ));
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ScrobbleResponse {
                    success: false,
                    scrobbled_at: String::new(),
                    message: format!("Erro no banco de dados: {e}"),
                }),
            ));
        }
    };

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let now_iso = chrono_iso_now();

    if let Err(err) =
        listenbrainz::submit_single_listen(&settings.user_token, &track, now_unix).await
    {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(ScrobbleResponse {
                success: false,
                scrobbled_at: String::new(),
                message: format!("Falha ao submeter ao ListenBrainz: {err}"),
            }),
        ));
    }

    let _ = record_scrobble(&state.pool, media_id, &now_iso).await;

    Ok(Json(ScrobbleResponse {
        success: true,
        scrobbled_at: now_iso,
        message: format!("Scrobble de '{}' registrado com sucesso", track.track_name),
    }))
}

pub(crate) async fn playing_now_media_endpoint(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Result<StatusCode, StatusCode> {
    let settings = load_listenbrainz_settings(&state.pool, &state.crypto).await;
    if !settings.enabled || settings.user_token.trim().is_empty() {
        return Ok(StatusCode::OK);
    }

    if let Ok(Some(track)) = get_media_music_track(&state.pool, media_id).await {
        let _ = listenbrainz::submit_playing_now(&settings.user_token, &track).await;
    }

    Ok(StatusCode::OK)
}

fn chrono_iso_now() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = (secs / 86400) as i64;
    let time_of_day = (secs % 86400) as u32;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1024 + doe / 1461 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn clean_video_title(title: &str, channel_name: Option<&str>) -> String {
    let mut clean = title.to_string();

    // 1. Remove bracketed 11-char YouTube ID like [QBGEahKfeco]
    if let Some(start) = clean.rfind('[')
        && let Some(end) = clean.rfind(']')
        && end > start
        && end - start == 12
    {
        clean.replace_range(start..=end, "");
    }

    // 2. Remove common video/music noise
    let patterns = [
        "(Official Music Video)",
        "(Official Video)",
        "[Official Music Video]",
        "[Official Video]",
        "(Official Audio)",
        "[Official Audio]",
        "(Audio)",
        "(Videoclipe Oficial)",
        "[Videoclipe Oficial]",
        "(Clipe Oficial)",
        "[Clipe Oficial]",
        "(HD)",
        "(4K)",
        "(Lyrics)",
        "[Lyrics]",
        "(Letra)",
        "[Letra]",
        "(MV)",
        "[MV]",
    ];

    for pat in patterns {
        clean = clean.replace(pat, "");
        clean = clean.replace(&pat.to_lowercase(), "");
        clean = clean.replace(&pat.to_uppercase(), "");
    }

    // 3. Remove channel name if present at the end of title
    if let Some(ch) = channel_name {
        let ch_trimmed = ch.trim();
        if !ch_trimmed.is_empty() {
            let lower_clean = clean.to_lowercase();
            let lower_ch = ch_trimmed.to_lowercase();
            if let Some(pos) = lower_clean.rfind(&lower_ch)
                && pos > 0
                && pos + lower_ch.len() >= lower_clean.trim_end().len().saturating_sub(2)
            {
                let before = &clean[..pos];
                let trimmed_before = before.trim_end_matches(|c: char| {
                    c.is_whitespace()
                        || c == '|'
                        || c == '｜'
                        || c == '-'
                        || c == '–'
                        || c == '—'
                        || c == ':'
                });
                clean = trimmed_before.to_string();
            }
        }
    }

    // 4. Clean trailing and leading separators
    let trimmed = clean
        .trim()
        .trim_matches(|c: char| {
            c == '|' || c == '｜' || c == '-' || c == '–' || c == '—' || c == ':'
        })
        .trim();

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_video_title() {
        let original = "Example Artist - Sample Track (Official Video)";
        assert_eq!(
            clean_video_title(original, Some("Example Artist")),
            "Example Artist - Sample Track"
        );

        let original2 = "Artista - Música [Clipe Oficial]";
        assert_eq!(clean_video_title(original2, None), "Artista - Música");

        let original3 = "Sample EP - \"demo.exe\" ｜ Example Channel";
        assert_eq!(
            clean_video_title(original3, Some("Example Channel")),
            "Sample EP - \"demo.exe\""
        );

        let original4 = "Sample EP - demo.exe Example Channel [abcdefghijk]";
        assert_eq!(
            clean_video_title(original4, Some("Example Channel")),
            "Sample EP - demo.exe"
        );
    }

    #[test]
    fn test_extract_mbid_from_url() {
        let url1 = "https://listenbrainz.org/track/00000000-0000-0000-0000-000000000001/";
        assert_eq!(
            extract_mbid_from_input(url1),
            "00000000-0000-0000-0000-000000000001"
        );

        let url2 = "https://musicbrainz.org/recording/00000000-0000-0000-0000-000000000001";
        assert_eq!(
            extract_mbid_from_input(url2),
            "00000000-0000-0000-0000-000000000001"
        );

        let plain = "00000000-0000-0000-0000-000000000001";
        assert_eq!(
            extract_mbid_from_input(plain),
            "00000000-0000-0000-0000-000000000001"
        );
    }

    #[tokio::test]
    async fn test_listenbrainz_settings_persistence() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let crypto = AppCrypto::from_key_bytes(&[55u8; 32]).unwrap();

        let initial = load_listenbrainz_settings(&pool, &crypto).await;
        assert!(!initial.enabled);
        assert_eq!(initial.user_token, "");
        assert_eq!(initial.username, None);

        let mut updated = initial.clone();
        updated.enabled = true;
        updated.user_token = "my_token_123".to_string();
        updated.username = Some("test_user".to_string());
        save_listenbrainz_settings(&pool, &crypto, &updated)
            .await
            .unwrap();

        // 1. Verifica no SQLite bruto: o JSON deve conter enc:v1: e NÃO o texto plano
        let raw_json =
            sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key = ?")
                .bind(SETTINGS_KEY)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            raw_json.contains("enc:v1:"),
            "Token deve estar cifrado no banco"
        );
        assert!(
            !raw_json.contains("my_token_123"),
            "Token em texto plano NUNCA deve ser gravado no banco"
        );

        // 2. Leitura com AppCrypto deve decifrar transparentemente
        let loaded = load_listenbrainz_settings(&pool, &crypto).await;
        assert!(loaded.enabled);
        assert_eq!(loaded.user_token, "my_token_123");
        assert_eq!(loaded.username.as_deref(), Some("test_user"));
    }

    #[tokio::test]
    async fn test_media_music_track_crud_lifecycle() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();

        // Inserir mídia mock
        sqlx::query("INSERT INTO media (id, title) VALUES (100, 'Test Video')")
            .execute(&pool)
            .await
            .unwrap();

        let req = AssociateMusicRequest {
            recording_mbid: Some("00000000-0000-0000-0000-000000000001".to_string()),
            artist_name: "Example Artist".to_string(),
            track_name: "Sample Track".to_string(),
            release_name: Some("Sample Release".to_string()),
            source: Some("musicbrainz".to_string()),
        };

        let saved = save_media_music_track(&pool, 100, &req).await.unwrap();
        assert_eq!(saved.media_id, 100);
        assert_eq!(saved.track_name, "Sample Track");
        assert_eq!(saved.artist_name, "Example Artist");
        assert_eq!(saved.last_scrobbled_at, None);

        // Record scrobble
        record_scrobble(&pool, 100, "2026-11-10T12:00:00Z")
            .await
            .unwrap();
        let fetched = get_media_music_track(&pool, 100).await.unwrap().unwrap();
        assert_eq!(
            fetched.last_scrobbled_at.as_deref(),
            Some("2026-11-10T12:00:00Z")
        );

        // Delete
        let deleted = delete_media_music_track(&pool, 100).await.unwrap();
        assert!(deleted);
        assert!(get_media_music_track(&pool, 100).await.unwrap().is_none());
    }
}
