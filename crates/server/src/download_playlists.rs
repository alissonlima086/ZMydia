use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

use crate::{
    auth::AuthenticatedUser,
    downloader::{DOWNLOAD_STALLED_ERROR, LEGACY_DOWNLOAD_STALLED_ERROR, load_download_settings},
    web::AppState,
    youtube_worker::load_youtube_settings,
};

const MAX_PLAYLIST_ITEMS: usize = 10_000;
const PLAYLIST_SETTINGS_KEY: &str = "playlist_download_settings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistDownloadSettings {
    pub active_profile: String,
    pub min_cooldown_seconds: u32,
    pub max_cooldown_seconds: u32,
    pub dynamic_duration_cooldown: bool,
    pub daily_video_limit: Option<u32>,
    pub daily_gb_limit: Option<u32>,
    pub daily_limit_action: String,
}

impl Default for PlaylistDownloadSettings {
    fn default() -> Self {
        Self::preset("conservative")
    }
}

impl PlaylistDownloadSettings {
    fn preset(profile: &str) -> Self {
        match profile {
            "balanced" => Self {
                active_profile: profile.into(),
                min_cooldown_seconds: 45,
                max_cooldown_seconds: 120,
                dynamic_duration_cooldown: false,
                daily_video_limit: None,
                daily_gb_limit: None,
                daily_limit_action: "ignore".into(),
            },
            "fast" => Self {
                active_profile: profile.into(),
                min_cooldown_seconds: 5,
                max_cooldown_seconds: 15,
                dynamic_duration_cooldown: false,
                daily_video_limit: None,
                daily_gb_limit: None,
                daily_limit_action: "ignore".into(),
            },
            _ => Self {
                active_profile: "conservative".into(),
                min_cooldown_seconds: 180,
                max_cooldown_seconds: 600,
                dynamic_duration_cooldown: true,
                daily_video_limit: Some(30),
                daily_gb_limit: Some(50),
                daily_limit_action: "pause_and_notify".into(),
            },
        }
    }

    fn normalized(mut self) -> Self {
        if !matches!(
            self.active_profile.as_str(),
            "conservative" | "balanced" | "fast" | "custom"
        ) {
            self.active_profile = "conservative".into();
        }
        self.min_cooldown_seconds = self.min_cooldown_seconds.min(86_400);
        self.max_cooldown_seconds = self
            .max_cooldown_seconds
            .clamp(self.min_cooldown_seconds, 86_400);
        self.daily_video_limit = self
            .daily_video_limit
            .filter(|value| *value > 0)
            .map(|value| value.min(10_000));
        self.daily_gb_limit = self
            .daily_gb_limit
            .filter(|value| *value > 0)
            .map(|value| value.min(100_000));
        if !matches!(
            self.daily_limit_action.as_str(),
            "pause_and_notify" | "ignore"
        ) {
            self.daily_limit_action = "pause_and_notify".into();
        }
        self
    }
}

pub async fn load_playlist_download_settings(pool: &SqlitePool) -> PlaylistDownloadSettings {
    match sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key = ?")
        .bind(PLAYLIST_SETTINGS_KEY)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(raw)) => serde_json::from_str::<PlaylistDownloadSettings>(&raw)
            .unwrap_or_default()
            .normalized(),
        _ => PlaylistDownloadSettings::default(),
    }
}

async fn save_playlist_download_settings(
    pool: &SqlitePool,
    settings: PlaylistDownloadSettings,
) -> Result<PlaylistDownloadSettings, sqlx::Error> {
    let clean = if settings.active_profile == "custom" {
        settings.normalized()
    } else {
        PlaylistDownloadSettings::preset(&settings.active_profile)
    };
    let json = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".into());
    sqlx::query("INSERT INTO settings (key, value_json, updated_at) VALUES (?, ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at")
        .bind(PLAYLIST_SETTINGS_KEY).bind(json).execute(pool).await?;
    Ok(clean)
}

#[derive(Debug, Deserialize)]
pub struct ImportPlaylistRequest {
    pub url: String,
    pub quality: Option<String>,
    pub container: Option<String>,
    pub create_local_playlist: Option<bool>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DownloadPlaylistSummary {
    pub id: String,
    pub title: String,
    pub channel_name: Option<String>,
    pub thumbnail_url: Option<String>,
    pub status: String,
    pub quality: String,
    pub format_container: String,
    pub create_local_playlist: bool,
    pub local_playlist_id: Option<String>,
    pub download_profile: String,
    pub cooldown_until: Option<String>,
    pub protection_reason: Option<String>,
    pub cooldown_reason: Option<String>,
    pub item_count: i64,
    pub selected_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub unavailable_count: i64,
    pub pending_count: i64,
    pub active_count: i64,
    pub skipped_existing_count: i64,
    pub cancelled_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct DownloadPlaylistItem {
    pub position: i64,
    pub youtube_id: Option<String>,
    pub title: String,
    pub channel_name: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub availability: String,
    pub selected: bool,
    pub status: String,
    pub download_job_id: Option<String>,
    pub media_id: Option<i64>,
    pub error_message: Option<String>,
    pub progress_percent: f64,
    pub speed_text: Option<String>,
    pub eta_text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DownloadPlaylistDetail {
    #[serde(flatten)]
    pub summary: DownloadPlaylistSummary,
    pub items: Vec<DownloadPlaylistItem>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistItemsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSelectionRequest {
    pub selected: bool,
    pub positions: Option<Vec<i64>>,
}

#[derive(Debug)]
struct PlaylistUrl {
    id: String,
    canonical: String,
}

#[derive(Debug)]
struct ImportedPlaylist {
    title: String,
    channel_name: Option<String>,
    thumbnail_url: Option<String>,
    items: Vec<ImportedPlaylistItem>,
}

#[derive(Debug)]
struct ImportedPlaylistItem {
    position: i64,
    youtube_id: Option<String>,
    url: Option<String>,
    title: String,
    channel_name: Option<String>,
    thumbnail_url: Option<String>,
    duration_ms: Option<i64>,
    availability: String,
    status: String,
    selected: bool,
}

fn generate_id() -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("dlp_{micros:x}_{}", std::process::id())
}

fn parse_playlist_url(raw: &str) -> Result<PlaylistUrl, String> {
    let url =
        crate::validation::youtube_url(raw).ok_or_else(|| "URL inválida do YouTube".to_owned())?;
    if url.path() != "/playlist" {
        return Err("Use um link direto de playlist do YouTube".to_owned());
    }
    let id = url
        .query_pairs()
        .find_map(|(key, value)| (key == "list").then(|| value.into_owned()))
        .filter(|value| {
            (10..=100).contains(&value.len())
                && value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
        })
        .ok_or_else(|| "O link não contém um identificador de playlist válido".to_owned())?;

    Ok(PlaylistUrl {
        canonical: format!("https://www.youtube.com/playlist?list={id}"),
        id,
    })
}

fn valid_youtube_id(value: &str) -> bool {
    value.len() == 11
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn first_thumbnail(value: &serde_json::Value) -> Option<String> {
    value
        .get("thumbnails")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|thumbnail| thumbnail.get("url")?.as_str().map(str::to_owned))
        .filter(|url| crate::validation::youtube_image_url(url).is_some())
}

fn normalize_availability(raw: Option<&str>, has_id: bool) -> (&'static str, &'static str, bool) {
    match raw.unwrap_or("unknown") {
        "public" => ("public", "pending", has_id),
        "unlisted" => ("unlisted", "pending", has_id),
        "private" => ("private", "unavailable", false),
        "premium_only" => ("premium_only", "unavailable", false),
        "subscriber_only" => ("subscriber_only", "unavailable", false),
        "needs_auth" => ("needs_auth", "pending", has_id),
        "unavailable" => ("unavailable", "unavailable", false),
        _ if has_id => ("unknown", "pending", true),
        _ => ("unavailable", "unavailable", false),
    }
}

fn parse_playlist_json(bytes: &[u8]) -> Result<ImportedPlaylist, String> {
    let root: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "O YouTube retornou metadados de playlist inválidos".to_owned())?;
    let title = root
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Playlist do YouTube")
        .to_owned();
    let channel_name = root
        .get("channel")
        .or_else(|| root.get("uploader"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let entries = root
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "A playlist não contém uma lista de vídeos legível".to_owned())?;
    if entries.len() > MAX_PLAYLIST_ITEMS {
        return Err(format!(
            "A playlist excede o limite de {MAX_PLAYLIST_ITEMS} itens"
        ));
    }

    let mut items = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let youtube_id = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| valid_youtube_id(value))
            .map(str::to_owned);
        let (availability, status, selected) = normalize_availability(
            entry
                .get("availability")
                .and_then(serde_json::Value::as_str),
            youtube_id.is_some(),
        );
        let title = entry
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Vídeo indisponível")
            .to_owned();
        let duration_ms = entry
            .get("duration")
            .and_then(serde_json::Value::as_f64)
            .filter(|duration| *duration >= 0.0)
            .map(|duration| (duration * 1000.0).round() as i64);
        let url = youtube_id
            .as_ref()
            .map(|id| format!("https://www.youtube.com/watch?v={id}"));

        items.push(ImportedPlaylistItem {
            position: index as i64,
            youtube_id,
            url,
            title,
            channel_name: entry
                .get("channel")
                .or_else(|| entry.get("uploader"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            thumbnail_url: first_thumbnail(entry),
            duration_ms,
            availability: availability.to_owned(),
            status: status.to_owned(),
            selected,
        });
    }

    Ok(ImportedPlaylist {
        title,
        channel_name,
        thumbnail_url: first_thumbnail(&root)
            .or_else(|| items.iter().find_map(|item| item.thumbnail_url.clone())),
        items,
    })
}

async fn extract_playlist(url: &str, state: &AppState) -> Result<ImportedPlaylist, String> {
    let youtube_settings = load_youtube_settings(&state.pool).await;
    let mut command = tokio::process::Command::new(crate::config::external_command(
        "MYMEDIA_YTDLP_BIN",
        "yt-dlp",
    ));
    crate::downloader::configure_ytdlp_js_runtime(&mut command);
    command
        .env_remove("LD_LIBRARY_PATH")
        .kill_on_drop(true)
        .arg("--ignore-config")
        .arg("--dump-single-json")
        .arg("--flat-playlist")
        .arg("--ignore-errors")
        .arg("--no-warnings");
    if youtube_settings.use_browser_cookies
        && crate::youtube_worker::is_valid_browser(&youtube_settings.browser)
    {
        command
            .arg("--cookies-from-browser")
            .arg(youtube_settings.browser.trim());
    }
    command.arg(url);

    let output = tokio::time::timeout(Duration::from_secs(180), command.output())
        .await
        .map_err(|_| "A leitura da playlist excedeu três minutos".to_owned())?
        .map_err(|error| format!("Não foi possível iniciar o yt-dlp: {error}"))?;
    if !output.status.success() {
        // Do not log stdout/stderr here: playlist titles and source URLs are private input.
        return Err("Não foi possível consultar a playlist no YouTube".to_owned());
    }
    parse_playlist_json(&output.stdout)
}

async fn insert_items(
    transaction: &mut Transaction<'_, Sqlite>,
    playlist_id: &str,
    items: &[ImportedPlaylistItem],
) -> Result<(), sqlx::Error> {
    for item in items {
        sqlx::query(
            "INSERT INTO download_playlist_items (
                download_playlist_id, position, youtube_id, url, title, channel_name,
                thumbnail_url, duration_ms, availability, selected, status
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(playlist_id)
        .bind(item.position)
        .bind(&item.youtube_id)
        .bind(&item.url)
        .bind(&item.title)
        .bind(&item.channel_name)
        .bind(&item.thumbnail_url)
        .bind(item.duration_ms)
        .bind(&item.availability)
        .bind(item.selected)
        .bind(&item.status)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

const SUMMARY_QUERY: &str =
    "SELECT p.id, p.title, p.channel_name, p.thumbnail_url, p.status, p.quality,
            p.format_container, p.create_local_playlist, p.local_playlist_id,
            p.download_profile, p.cooldown_until, p.protection_reason, p.cooldown_reason,
            COUNT(i.position) AS item_count,
            COALESCE(SUM(CASE WHEN i.selected = 1 THEN 1 ELSE 0 END), 0) AS selected_count,
            COALESCE(SUM(CASE WHEN i.status = 'completed' THEN 1 ELSE 0 END), 0) AS completed_count,
            COALESCE(SUM(CASE WHEN i.status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
            COALESCE(SUM(CASE WHEN i.status = 'unavailable' THEN 1 ELSE 0 END), 0) AS unavailable_count,
            COALESCE(SUM(CASE WHEN i.status = 'pending' THEN 1 ELSE 0 END), 0) AS pending_count,
            COALESCE(SUM(CASE WHEN i.status IN ('queued', 'downloading', 'processing') THEN 1 ELSE 0 END), 0) AS active_count,
            COALESCE(SUM(CASE WHEN i.status = 'skipped_existing' THEN 1 ELSE 0 END), 0) AS skipped_existing_count,
            COALESCE(SUM(CASE WHEN i.status = 'cancelled' THEN 1 ELSE 0 END), 0) AS cancelled_count,
            p.created_at, p.updated_at
     FROM download_playlists p
     LEFT JOIN download_playlist_items i ON i.download_playlist_id = p.id";

async fn fetch_summary(
    state: &AppState,
    owner_key: &str,
    id: &str,
) -> Result<DownloadPlaylistSummary, StatusCode> {
    let query = format!("{SUMMARY_QUERY} WHERE p.id = ? AND p.owner_key = ? GROUP BY p.id");
    sqlx::query_as::<_, DownloadPlaylistSummary>(sqlx::AssertSqlSafe(query.as_str()))
        .bind(id)
        .bind(owner_key)
        .fetch_optional(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to fetch download playlist");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)
}

pub(crate) async fn import_playlist_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<ImportPlaylistRequest>,
) -> Result<(StatusCode, Json<DownloadPlaylistSummary>), (StatusCode, String)> {
    let source =
        parse_playlist_url(&payload.url).map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let imported = extract_playlist(&source.canonical, &state)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if imported.items.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "A playlist não contém vídeos".to_owned(),
        ));
    }

    let settings = load_download_settings(&state.pool).await;
    let playlist_settings = load_playlist_download_settings(&state.pool).await;
    let id = generate_id();
    let quality = payload.quality.unwrap_or_else(|| "best".to_owned());
    let container = match payload.container.as_deref() {
        Some("mp4") => "mp4",
        _ => "mkv",
    };
    let create_local_playlist = payload.create_local_playlist.unwrap_or(true);
    let mut transaction = state.pool.begin().await.map_err(internal_error)?;
    sqlx::query(
        "INSERT INTO download_playlists (
            id, source_url, youtube_playlist_id, title, channel_name, thumbnail_url,
            quality, format_container, target_source_id, owner_key, create_local_playlist,
            download_profile
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&source.canonical)
    .bind(&source.id)
    .bind(&imported.title)
    .bind(&imported.channel_name)
    .bind(&imported.thumbnail_url)
    .bind(&quality)
    .bind(container)
    .bind(settings.target_source_id)
    .bind(&user.principal_key)
    .bind(create_local_playlist)
    .bind(&playlist_settings.active_profile)
    .execute(&mut *transaction)
    .await
    .map_err(internal_error)?;
    insert_items(&mut transaction, &id, &imported.items)
        .await
        .map_err(internal_error)?;
    transaction.commit().await.map_err(internal_error)?;

    let summary = fetch_summary(&state, &user.principal_key, &id)
        .await
        .map_err(|status| {
            (
                status,
                "Não foi possível carregar a playlist importada".to_owned(),
            )
        })?;
    Ok((StatusCode::CREATED, Json(summary)))
}

fn internal_error(error: sqlx::Error) -> (StatusCode, String) {
    tracing::error!(%error, "download playlist persistence failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Não foi possível salvar a playlist".to_owned(),
    )
}

pub(crate) async fn list_playlists_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<DownloadPlaylistSummary>>, StatusCode> {
    let query = format!(
        "{SUMMARY_QUERY} WHERE p.owner_key = ? GROUP BY p.id ORDER BY p.created_at DESC LIMIT 100"
    );
    let playlists =
        sqlx::query_as::<_, DownloadPlaylistSummary>(sqlx::AssertSqlSafe(query.as_str()))
            .bind(&user.principal_key)
            .fetch_all(&state.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "failed to list download playlists");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    Ok(Json(playlists))
}

pub(crate) async fn get_playlist_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Query(query): Query<PlaylistItemsQuery>,
) -> Result<Json<DownloadPlaylistDetail>, StatusCode> {
    let summary = fetch_summary(&state, &user.principal_key, &id).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 250);
    let offset = query.offset.unwrap_or(0).max(0);
    let items = sqlx::query_as::<_, DownloadPlaylistItem>(
        "SELECT i.position, i.youtube_id, i.title, i.channel_name, i.thumbnail_url, i.duration_ms,
                i.availability, i.selected, i.status, i.download_job_id, i.media_id,
                i.error_message, COALESCE(j.progress_percent, 0.0) AS progress_percent,
                j.speed_text, j.eta_text
         FROM download_playlist_items i
         LEFT JOIN download_jobs j ON j.id = i.download_job_id
         WHERE i.download_playlist_id = ?
         ORDER BY i.position LIMIT ? OFFSET ?",
    )
    .bind(&id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to list download playlist items");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(DownloadPlaylistDetail {
        summary,
        items,
        limit,
        offset,
    }))
}

#[derive(Clone)]
pub struct PlaylistDownloadScheduler {
    pool: SqlitePool,
    downloader: crate::downloader::DownloadService,
}

impl PlaylistDownloadScheduler {
    pub fn start(pool: SqlitePool, downloader: crate::downloader::DownloadService) -> Self {
        let scheduler = Self { pool, downloader };
        let worker = scheduler.clone();
        tokio::spawn(async move {
            if let Err(error) = worker.recover_interrupted_items().await {
                tracing::error!(%error, "failed to recover playlist download items");
            }
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                if let Err(error) = worker.tick().await {
                    tracing::error!(%error, "playlist download scheduler tick failed");
                }
            }
        });
        scheduler
    }

    async fn recover_interrupted_items(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE download_playlist_items
             SET status = 'pending', updated_at = datetime('now')
             WHERE status IN ('queued', 'downloading', 'processing')
               AND download_job_id IS NULL",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn tick(&self) -> Result<(), sqlx::Error> {
        self.reclassify_existing_items().await?;
        self.reconcile_jobs().await?;
        reconcile_existing_playlist_items(&self.pool).await?;
        self.finish_complete_playlists().await?;

        let active = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM download_playlist_items
             WHERE status IN ('queued', 'downloading', 'processing')",
        )
        .fetch_one(&self.pool)
        .await?;
        if active > 0 {
            return Ok(());
        }

        let playlist = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, owner_key, quality, format_container
             FROM download_playlists
             WHERE status = 'running'
               AND (cooldown_until IS NULL OR cooldown_until <= datetime('now'))
               AND EXISTS (
                   SELECT 1 FROM download_playlist_items i
                   WHERE i.download_playlist_id = download_playlists.id
                     AND i.selected = 1 AND i.status = 'pending'
               )
             ORDER BY COALESCE(started_at, created_at), created_at
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some((playlist_id, owner_key, quality, container)) = playlist else {
            return Ok(());
        };

        let bypass_budget = sqlx::query_scalar::<_, bool>(
            "SELECT bypass_budget_once FROM download_playlists WHERE id = ?",
        )
        .bind(&playlist_id)
        .fetch_one(&self.pool)
        .await?;
        if !bypass_budget && self.apply_daily_budget_if_needed(&playlist_id).await? {
            return Ok(());
        }
        if bypass_budget {
            sqlx::query("UPDATE download_playlists SET bypass_budget_once = 0, cooldown_reason = NULL WHERE id = ?")
                .bind(&playlist_id).execute(&self.pool).await?;
        }

        let item = sqlx::query_as::<_, (i64, String)>(
            "SELECT position, url FROM download_playlist_items
             WHERE download_playlist_id = ? AND selected = 1 AND status = 'pending'
               AND url IS NOT NULL
             ORDER BY position LIMIT 1",
        )
        .bind(&playlist_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((position, url)) = item else {
            return Ok(());
        };

        sqlx::query(
            "UPDATE download_playlist_items SET status = 'queued', updated_at = datetime('now')
             WHERE download_playlist_id = ? AND position = ? AND status = 'pending'",
        )
        .bind(&playlist_id)
        .bind(position)
        .execute(&self.pool)
        .await?;

        match self
            .downloader
            .enqueue_playlist_item(&owner_key, url, Some(quality), Some(container))
            .await
        {
            Ok(job) => {
                sqlx::query(
                    "UPDATE download_playlist_items
                     SET download_job_id = ?, status = 'queued', error_message = NULL,
                         updated_at = datetime('now')
                     WHERE download_playlist_id = ? AND position = ?",
                )
                .bind(job.id)
                .bind(&playlist_id)
                .bind(position)
                .execute(&self.pool)
                .await?;
            }
            Err(message) => {
                let duplicate = is_existing_item_error(&message);
                let protected = is_rate_limit_error(&message);
                sqlx::query(
                    "UPDATE download_playlist_items
                     SET status = ?, error_message = ?, updated_at = datetime('now')
                     WHERE download_playlist_id = ? AND position = ?",
                )
                .bind(if duplicate {
                    "skipped_existing"
                } else {
                    "failed"
                })
                .bind(&message)
                .bind(&playlist_id)
                .bind(position)
                .execute(&self.pool)
                .await?;
                if duplicate {
                    self.attach_existing_media(&playlist_id, position).await?;
                }
                if protected {
                    self.pause_for_protection(&playlist_id, &message).await?;
                } else if !duplicate {
                    self.set_cooldown(&playlist_id, 15 * 60).await?;
                }
            }
        }
        Ok(())
    }

    async fn reclassify_existing_items(&self) -> Result<(), sqlx::Error> {
        let duplicates = sqlx::query_as::<_, (String, i64)>(
            "SELECT download_playlist_id, position
             FROM download_playlist_items
             WHERE status = 'failed' AND (
                 error_message = 'already_exists'
                 OR lower(error_message) LIKE '%destino já existe%'
                 OR lower(error_message) LIKE '%já existe na biblioteca%'
             )",
        )
        .fetch_all(&self.pool)
        .await?;
        for (playlist_id, position) in duplicates {
            sqlx::query(
                "UPDATE download_playlist_items
                 SET status = 'skipped_existing', error_message = NULL,
                     updated_at = datetime('now')
                 WHERE download_playlist_id = ? AND position = ? AND status = 'failed'",
            )
            .bind(&playlist_id)
            .bind(position)
            .execute(&self.pool)
            .await?;
            self.attach_existing_media(&playlist_id, position).await?;
            sqlx::query(
                "UPDATE download_playlists
                 SET cooldown_until = NULL, protection_reason = NULL,
                     updated_at = datetime('now')
                 WHERE id = ? AND status = 'running'",
            )
            .bind(&playlist_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn reconcile_jobs(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE download_playlist_items
             SET status = 'failed', error_message = 'O job de download não está mais disponível',
                 updated_at = datetime('now')
             WHERE status IN ('queued', 'downloading', 'processing')
               AND download_job_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM download_jobs j WHERE j.id = download_playlist_items.download_job_id
               )",
        )
        .execute(&self.pool)
        .await?;

        let jobs = sqlx::query_as::<
            _,
            (
                String,
                i64,
                String,
                String,
                Option<i64>,
                Option<String>,
                Option<i64>,
            ),
        >(
            "SELECT i.download_playlist_id, i.position, i.download_job_id, j.status,
                    j.media_id, j.error_message, i.duration_ms
             FROM download_playlist_items i
             JOIN download_jobs j ON j.id = i.download_job_id
             WHERE i.status IN ('queued', 'downloading', 'processing')",
        )
        .fetch_all(&self.pool)
        .await?;

        for (playlist_id, position, _job_id, job_status, media_id, error, duration_ms) in jobs {
            let duplicate = error.as_deref().is_some_and(is_existing_item_error);
            let item_status = match job_status.as_str() {
                "queued" | "probing" => "queued",
                "downloading" => "downloading",
                "organizing" | "awaiting_index" => "processing",
                "completed" => "completed",
                "failed" if duplicate => "skipped_existing",
                "failed" => "failed",
                "cancelled" => "cancelled",
                _ => continue,
            };
            sqlx::query(
                "UPDATE download_playlist_items
                 SET status = ?, media_id = COALESCE(?, media_id), error_message = ?,
                     updated_at = datetime('now')
                 WHERE download_playlist_id = ? AND position = ?",
            )
            .bind(item_status)
            .bind(media_id)
            .bind(&error)
            .bind(&playlist_id)
            .bind(position)
            .execute(&self.pool)
            .await?;

            if item_status == "completed" {
                if let Some(media_id) = media_id {
                    self.add_media_to_local_playlist(&playlist_id, position, media_id)
                        .await?;
                }
                let settings = load_playlist_download_settings(&self.pool).await;
                self.set_cooldown(
                    &playlist_id,
                    configured_cooldown_seconds(&settings, duration_ms),
                )
                .await?;
            } else if item_status == "skipped_existing" {
                self.attach_existing_media(&playlist_id, position).await?;
                self.set_cooldown(&playlist_id, 0).await?;
            } else if item_status == "failed" {
                if error.as_deref().is_some_and(is_rate_limit_error) {
                    self.pause_for_protection(
                        &playlist_id,
                        error.as_deref().unwrap_or("Proteção do YouTube acionada"),
                    )
                    .await?;
                } else {
                    self.set_cooldown(&playlist_id, 15 * 60).await?;
                }
            }
        }
        Ok(())
    }

    async fn attach_existing_media(
        &self,
        playlist_id: &str,
        position: i64,
    ) -> Result<(), sqlx::Error> {
        let media_id = sqlx::query_scalar::<_, i64>(
            "SELECT m.id
             FROM download_playlist_items i
             JOIN media m ON m.youtube_id = i.youtube_id AND m.is_available = 1
             WHERE i.download_playlist_id = ? AND i.position = ?
             LIMIT 1",
        )
        .bind(playlist_id)
        .bind(position)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(media_id) = media_id {
            sqlx::query(
                "UPDATE download_playlist_items SET media_id = ?, updated_at = datetime('now')
                 WHERE download_playlist_id = ? AND position = ?",
            )
            .bind(media_id)
            .bind(playlist_id)
            .bind(position)
            .execute(&self.pool)
            .await?;
            self.add_media_to_local_playlist(playlist_id, position, media_id)
                .await?;
        }
        Ok(())
    }

    async fn add_media_to_local_playlist(
        &self,
        download_playlist_id: &str,
        position: i64,
        media_id: i64,
    ) -> Result<(), sqlx::Error> {
        link_media_to_local_playlist(&self.pool, download_playlist_id, position, media_id).await
    }

    async fn set_cooldown(&self, playlist_id: &str, seconds: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE download_playlists
             SET cooldown_until = datetime('now', '+' || ? || ' seconds'),
                 cooldown_reason = CASE WHEN ? > 0 THEN 'interval' ELSE NULL END,
                 updated_at = datetime('now')
             WHERE id = ? AND status = 'running'",
        )
        .bind(seconds)
        .bind(seconds)
        .bind(playlist_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn pause_for_protection(
        &self,
        playlist_id: &str,
        reason: &str,
    ) -> Result<(), sqlx::Error> {
        let safe_reason = if reason.to_ascii_lowercase().contains("429") {
            "O YouTube limitou temporariamente as requisições (HTTP 429)"
        } else {
            "O YouTube solicitou uma pausa de proteção ou verificação manual"
        };
        sqlx::query(
            "UPDATE download_playlists
             SET status = 'paused_rate_limit', protection_reason = ?,
                 cooldown_until = datetime('now', '+6 hours'), updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(safe_reason)
        .bind(playlist_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn apply_daily_budget_if_needed(&self, playlist_id: &str) -> Result<bool, sqlx::Error> {
        let settings = load_playlist_download_settings(&self.pool).await;
        if settings.daily_limit_action == "ignore"
            || (settings.daily_video_limit.is_none() && settings.daily_gb_limit.is_none())
        {
            return Ok(false);
        }
        let started = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM download_playlist_items i
             JOIN download_jobs j ON j.id = i.download_job_id
             WHERE j.started_at >= datetime('now', '-24 hours')
               AND NOT (
                   j.status = 'failed'
                   AND (
                       j.error_message IN (?, ?, 'already_exists')
                       OR lower(j.error_message) LIKE '%destino já existe%'
                       OR lower(j.error_message) LIKE '%já existe na biblioteca%'
                   )
               )",
        )
        .bind(DOWNLOAD_STALLED_ERROR)
        .bind(LEGACY_DOWNLOAD_STALLED_ERROR)
        .fetch_one(&self.pool)
        .await?;
        let recent_paths = sqlx::query_scalar::<_, String>(
            "SELECT j.output_path
             FROM download_playlist_items i
             JOIN download_jobs j ON j.id = i.download_job_id
             WHERE j.output_path IS NOT NULL
               AND COALESCE(j.finished_at, j.updated_at) >= datetime('now', '-24 hours')",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut downloaded_bytes = 0u64;
        for path in recent_paths {
            if let Ok(metadata) = tokio::fs::metadata(path).await {
                downloaded_bytes = downloaded_bytes.saturating_add(metadata.len());
            }
        }
        let video_limit_hit = settings
            .daily_video_limit
            .is_some_and(|limit| started >= i64::from(limit));
        let bytes_limit = settings
            .daily_gb_limit
            .map(|limit| u64::from(limit) * 1024 * 1024 * 1024);
        let bytes_limit_hit = bytes_limit.is_some_and(|limit| downloaded_bytes >= limit);
        if !video_limit_hit && !bytes_limit_hit {
            return Ok(false);
        }
        let reason = if bytes_limit_hit {
            format!(
                "daily_bytes:{}",
                settings.daily_gb_limit.unwrap_or_default()
            )
        } else {
            format!(
                "daily_videos:{}",
                settings.daily_video_limit.unwrap_or_default()
            )
        };
        sqlx::query(
            "UPDATE download_playlists
             SET cooldown_until = (
                    SELECT COALESCE(datetime(MAX(j.started_at), '+24 hours'), datetime('now', '+24 hours'))
                    FROM download_playlist_items i
                    JOIN download_jobs j ON j.id = i.download_job_id
                    WHERE j.started_at >= datetime('now', '-24 hours')
                      AND NOT (
                          j.status = 'failed'
                          AND (
                              j.error_message IN (?, ?, 'already_exists')
                              OR lower(j.error_message) LIKE '%destino já existe%'
                              OR lower(j.error_message) LIKE '%já existe na biblioteca%'
                          )
                      )
                 ),
                 cooldown_reason = ?,
                 updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(DOWNLOAD_STALLED_ERROR)
        .bind(LEGACY_DOWNLOAD_STALLED_ERROR)
        .bind(reason)
        .bind(playlist_id)
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    async fn finish_complete_playlists(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE download_playlists
             SET status = CASE
                    WHEN EXISTS (
                        SELECT 1 FROM download_playlist_items i
                        WHERE i.download_playlist_id = download_playlists.id
                          AND i.status = 'failed'
                    ) THEN 'completed_with_errors'
                    ELSE 'completed'
                 END,
                 cooldown_until = NULL, finished_at = datetime('now'), updated_at = datetime('now')
             WHERE status IN ('running', 'paused')
               AND NOT EXISTS (
                   SELECT 1 FROM download_playlist_items i
                   WHERE i.download_playlist_id = download_playlists.id
                     AND i.selected = 1
                     AND i.status IN ('pending', 'queued', 'downloading', 'processing')
               )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn configured_cooldown_seconds(
    settings: &PlaylistDownloadSettings,
    duration_ms: Option<i64>,
) -> i64 {
    let mut minimum = i64::from(settings.min_cooldown_seconds);
    let mut maximum = i64::from(settings.max_cooldown_seconds);
    if settings.dynamic_duration_cooldown {
        let multiplier = match duration_ms.unwrap_or_default() / 60_000 {
            ..=30 => 1,
            31..=120 => 2,
            121..=240 => 3,
            _ => 6,
        };
        minimum = (minimum * multiplier).min(86_400);
        maximum = (maximum * multiplier).min(86_400).max(minimum);
    }
    let spread = maximum - minimum + 1;
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as i64;
    minimum + seed.rem_euclid(spread)
}

fn is_rate_limit_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    [
        "http error 429",
        "too many requests",
        "confirm you're not a bot",
        "confirm you’re not a bot",
        "captcha",
        "automated queries",
    ]
    .iter()
    .any(|pattern| normalized.contains(pattern))
}

fn is_existing_item_error(message: &str) -> bool {
    let normalized = message.to_lowercase();
    normalized == "already_exists"
        || normalized.contains("destino já existe")
        || normalized.contains("já existe na biblioteca")
}

async fn update_playlist_state(
    state: &AppState,
    owner_key: &str,
    id: &str,
    action: &str,
) -> Result<DownloadPlaylistSummary, StatusCode> {
    let affected = match action {
        "start" => {
            sqlx::query(
                "UPDATE download_playlists
                 SET status = 'running', started_at = COALESCE(started_at, datetime('now')),
                     protection_reason = NULL, updated_at = datetime('now')
                 WHERE id = ? AND owner_key = ?
                   AND (
                       status IN ('ready', 'paused')
                       OR (status = 'paused_rate_limit' AND cooldown_until <= datetime('now'))
                   )",
            )
            .bind(id)
            .bind(owner_key)
            .execute(&state.pool)
            .await
        }
        "pause" => {
            sqlx::query(
                "UPDATE download_playlists SET status = 'paused', updated_at = datetime('now')
                 WHERE id = ? AND owner_key = ? AND status = 'running'",
            )
            .bind(id)
            .bind(owner_key)
            .execute(&state.pool)
            .await
        }
        "cancel" => {
            let mut transaction = state.pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let result = sqlx::query(
                "UPDATE download_playlists
                 SET status = 'cancelled', finished_at = datetime('now'), updated_at = datetime('now')
                 WHERE id = ? AND owner_key = ?
                   AND status IN ('ready', 'running', 'paused', 'paused_rate_limit')",
            )
            .bind(id)
            .bind(owner_key)
            .execute(&mut *transaction)
            .await;
            if result.as_ref().is_ok_and(|value| value.rows_affected() > 0) {
                sqlx::query(
                    "UPDATE download_playlist_items SET status = 'cancelled', updated_at = datetime('now')
                     WHERE download_playlist_id = ? AND status = 'pending'",
                )
                .bind(id)
                .execute(&mut *transaction)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            transaction.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            result
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    .map_err(|error| {
        tracing::error!(%error, "failed to update download playlist state");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if affected.rows_affected() == 0 {
        fetch_summary(state, owner_key, id).await?;
        return Err(StatusCode::CONFLICT);
    }
    fetch_summary(state, owner_key, id).await
}

async fn ensure_local_playlist_for_media(
    pool: &SqlitePool,
    download_playlist_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let Some((title, youtube_playlist_id, create_local_playlist)) =
        sqlx::query_as::<_, (String, String, bool)>(
            "SELECT title, youtube_playlist_id, create_local_playlist
             FROM download_playlists WHERE id = ?",
        )
        .bind(download_playlist_id)
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };
    if !create_local_playlist {
        return Ok(None);
    }

    // Reuse the local playlist linked by any previous run of the same YouTube
    // playlist. New imports use a deterministic fallback ID, so repeated runs
    // cannot create duplicate local playlists.
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT dp.local_playlist_id
         FROM download_playlists dp
         JOIN playlists p ON p.id = dp.local_playlist_id
         WHERE dp.youtube_playlist_id = ? AND dp.local_playlist_id IS NOT NULL
         ORDER BY dp.created_at, dp.id
         LIMIT 1",
    )
    .bind(&youtube_playlist_id)
    .fetch_optional(pool)
    .await?;
    let local_playlist_id = existing.unwrap_or_else(|| format!("pl_youtube_{youtube_playlist_id}"));

    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO playlists (id, title, description) VALUES (?, ?, NULL)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(&local_playlist_id)
    .bind(&title)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE download_playlists
         SET local_playlist_id = ?, updated_at = datetime('now')
         WHERE youtube_playlist_id = ? AND create_local_playlist = 1",
    )
    .bind(&local_playlist_id)
    .bind(&youtube_playlist_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Some(local_playlist_id))
}

async fn link_media_to_local_playlist(
    pool: &SqlitePool,
    download_playlist_id: &str,
    position: i64,
    media_id: i64,
) -> Result<(), sqlx::Error> {
    let Some(local_playlist_id) =
        ensure_local_playlist_for_media(pool, download_playlist_id).await?
    else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO playlist_items (playlist_id, media_id, position)
         VALUES (?, ?, ?)
         ON CONFLICT(playlist_id, media_id) DO UPDATE SET position = excluded.position",
    )
    .bind(&local_playlist_id)
    .bind(media_id)
    .bind(position)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE playlists SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(local_playlist_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn reconcile_existing_playlist_items(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    // A filesystem preflight can mark an item as existing before the source
    // scan has indexed that file. Revisit such items on every scheduler tick,
    // including after the download playlist itself has already completed.
    let items = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT i.download_playlist_id, i.position, m.id
         FROM download_playlist_items i
         JOIN download_playlists dp ON dp.id = i.download_playlist_id
         JOIN media m ON m.youtube_id = i.youtube_id AND m.is_available = 1
         WHERE i.status = 'skipped_existing'
           AND dp.create_local_playlist = 1
           AND (
               i.media_id IS NULL
               OR dp.local_playlist_id IS NULL
               OR NOT EXISTS (
                   SELECT 1 FROM playlist_items pi
                   WHERE pi.playlist_id = dp.local_playlist_id AND pi.media_id = m.id
               )
           )",
    )
    .fetch_all(pool)
    .await?;

    let mut reconciled = 0_u64;
    for (download_playlist_id, position, media_id) in items {
        sqlx::query(
            "UPDATE download_playlist_items SET media_id = ?, updated_at = datetime('now')
             WHERE download_playlist_id = ? AND position = ?",
        )
        .bind(media_id)
        .bind(&download_playlist_id)
        .bind(position)
        .execute(pool)
        .await?;
        link_media_to_local_playlist(pool, &download_playlist_id, position, media_id).await?;
        reconciled += 1;
    }
    Ok(reconciled)
}

pub(crate) async fn update_selection_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateSelectionRequest>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    if payload.positions.as_ref().is_some_and(|positions| {
        positions.len() > 1_000 || positions.iter().any(|position| *position < 0)
    }) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let editable = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM download_playlists
            WHERE id = ? AND owner_key = ? AND status IN ('ready', 'paused')
         )",
    )
    .bind(&id)
    .bind(&user.principal_key)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !editable {
        return Err(StatusCode::CONFLICT);
    }

    let mut transaction = state
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match payload.positions {
        Some(positions) => {
            for position in positions {
                sqlx::query(
                    "UPDATE download_playlist_items SET selected = ?, updated_at = datetime('now')
                     WHERE download_playlist_id = ? AND position = ? AND status = 'pending'",
                )
                .bind(payload.selected)
                .bind(&id)
                .bind(position)
                .execute(&mut *transaction)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        None => {
            sqlx::query(
                "UPDATE download_playlist_items SET selected = ?, updated_at = datetime('now')
                 WHERE download_playlist_id = ? AND status = 'pending'",
            )
            .bind(payload.selected)
            .bind(&id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    fetch_summary(&state, &user.principal_key, &id)
        .await
        .map(Json)
}

pub(crate) async fn start_playlist_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    update_playlist_state(&state, &user.principal_key, &id, "start")
        .await
        .map(Json)
}

pub(crate) async fn pause_playlist_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    update_playlist_state(&state, &user.principal_key, &id, "pause")
        .await
        .map(Json)
}

pub(crate) async fn skip_playlist_cooldown_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    payload: Option<Json<SkipCooldownRequest>>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    let override_limits = payload
        .map(|Json(value)| value.override_limits)
        .unwrap_or(false);
    let updated = sqlx::query(
        "UPDATE download_playlists
         SET cooldown_until = NULL,
             bypass_budget_once = CASE WHEN cooldown_reason LIKE 'daily_%' AND ? THEN 1 ELSE bypass_budget_once END,
             cooldown_reason = CASE WHEN cooldown_reason LIKE 'daily_%' AND NOT ? THEN cooldown_reason ELSE NULL END,
             updated_at = datetime('now')
         WHERE id = ? AND owner_key = ? AND status = 'running'
           AND cooldown_until > datetime('now')
           AND (? OR cooldown_reason IS NULL OR cooldown_reason NOT LIKE 'daily_%')
           AND EXISTS (
               SELECT 1 FROM download_playlist_items i
               WHERE i.download_playlist_id = download_playlists.id
                 AND i.selected = 1 AND i.status = 'pending'
           )",
    )
    .bind(override_limits)
    .bind(override_limits)
    .bind(&id)
    .bind(&user.principal_key)
    .bind(override_limits)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if updated.rows_affected() == 0 {
        return Err(StatusCode::CONFLICT);
    }
    fetch_summary(&state, &user.principal_key, &id)
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct SkipCooldownRequest {
    #[serde(default)]
    pub override_limits: bool,
}

pub(crate) async fn get_playlist_download_settings_endpoint(
    State(state): State<AppState>,
) -> Json<PlaylistDownloadSettings> {
    Json(load_playlist_download_settings(&state.pool).await)
}

pub(crate) async fn update_playlist_download_settings_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<PlaylistDownloadSettings>,
) -> Result<Json<PlaylistDownloadSettings>, StatusCode> {
    save_playlist_download_settings(&state.pool, payload)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn cancel_playlist_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    update_playlist_state(&state, &user.principal_key, &id, "cancel").await?;
    let active_jobs = sqlx::query_scalar::<_, String>(
        "SELECT i.download_job_id
         FROM download_playlist_items i
         JOIN download_playlists p ON p.id = i.download_playlist_id
         WHERE i.download_playlist_id = ? AND p.owner_key = ?
           AND i.status IN ('queued', 'downloading')
           AND i.download_job_id IS NOT NULL",
    )
    .bind(&id)
    .bind(&user.principal_key)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query(
        "UPDATE download_playlist_items SET status = 'cancelled', updated_at = datetime('now')
         WHERE download_playlist_id = ? AND status IN ('queued', 'downloading')",
    )
    .bind(&id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for job_id in active_jobs {
        state
            .downloader
            .cancel(&job_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    fetch_summary(&state, &user.principal_key, &id)
        .await
        .map(Json)
}

pub(crate) async fn retry_failed_playlist_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    let mut transaction = state
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let owned = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM download_playlists WHERE id = ? AND owner_key = ?)",
    )
    .bind(&id)
    .bind(&user.principal_key)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !owned {
        return Err(StatusCode::NOT_FOUND);
    }

    let retried = sqlx::query(
        "UPDATE download_playlist_items
         SET status = 'pending', selected = 1, download_job_id = NULL,
             error_message = NULL, updated_at = datetime('now')
         WHERE download_playlist_id = ? AND status IN ('failed', 'cancelled')
           AND EXISTS (
               SELECT 1 FROM download_playlists p
               WHERE p.id = download_playlist_items.download_playlist_id
                 AND p.owner_key = ? AND p.status != 'paused_rate_limit'
           )",
    )
    .bind(&id)
    .bind(&user.principal_key)
    .execute(&mut *transaction)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if retried.rows_affected() == 0 {
        return Err(StatusCode::CONFLICT);
    }
    sqlx::query(
        "UPDATE download_playlists
         SET status = 'running', cooldown_until = NULL, protection_reason = NULL,
             finished_at = NULL, updated_at = datetime('now')
         WHERE id = ? AND owner_key = ? AND status != 'paused_rate_limit'",
    )
    .bind(&id)
    .bind(&user.principal_key)
    .execute(&mut *transaction)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    fetch_summary(&state, &user.principal_key, &id)
        .await
        .map(Json)
}

pub(crate) async fn retry_playlist_item_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, position)): Path<(String, i64)>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    let updated = sqlx::query(
        "UPDATE download_playlist_items
         SET status = 'pending', selected = 1, download_job_id = NULL,
             media_id = NULL, error_message = NULL, updated_at = datetime('now')
         WHERE download_playlist_id = ? AND position = ?
           AND status IN ('failed', 'cancelled')
           AND EXISTS (
               SELECT 1 FROM download_playlists p
               WHERE p.id = download_playlist_items.download_playlist_id
                 AND p.owner_key = ? AND p.status != 'paused_rate_limit'
           )",
    )
    .bind(&id)
    .bind(position)
    .bind(&user.principal_key)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if updated.rows_affected() == 0 {
        return Err(StatusCode::CONFLICT);
    }
    sqlx::query(
        "UPDATE download_playlists
         SET status = 'running', cooldown_until = NULL, protection_reason = NULL,
             finished_at = NULL, updated_at = datetime('now')
         WHERE id = ? AND owner_key = ?",
    )
    .bind(&id)
    .bind(&user.principal_key)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    fetch_summary(&state, &user.principal_key, &id)
        .await
        .map(Json)
}

pub(crate) async fn cancel_playlist_item_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, position)): Path<(String, i64)>,
) -> Result<Json<DownloadPlaylistSummary>, StatusCode> {
    let item = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT i.status, i.download_job_id
         FROM download_playlist_items i
         JOIN download_playlists p ON p.id = i.download_playlist_id
         WHERE i.download_playlist_id = ? AND i.position = ? AND p.owner_key = ?",
    )
    .bind(&id)
    .bind(position)
    .bind(&user.principal_key)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    if !matches!(item.0.as_str(), "pending" | "queued" | "downloading") {
        return Err(StatusCode::CONFLICT);
    }
    sqlx::query(
        "UPDATE download_playlist_items
         SET status = 'cancelled', error_message = NULL, updated_at = datetime('now')
         WHERE download_playlist_id = ? AND position = ?",
    )
    .bind(&id)
    .bind(position)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(job_id) = item.1 {
        state
            .downloader
            .cancel(&job_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    fetch_summary(&state, &user.principal_key, &id)
        .await
        .map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_playlist_urls_without_share_tracking() {
        let parsed = parse_playlist_url(
            "https://youtube.com/playlist?list=PLsynthetic_1234567890&si=not-persisted",
        )
        .expect("synthetic playlist URL should parse");
        assert_eq!(parsed.id, "PLsynthetic_1234567890");
        assert_eq!(
            parsed.canonical,
            "https://www.youtube.com/playlist?list=PLsynthetic_1234567890"
        );
    }

    #[test]
    fn rejects_video_urls_until_list_from_video_is_supported() {
        let error = parse_playlist_url(
            "https://www.youtube.com/watch?v=synthetic01&list=PLsynthetic_1234567890",
        )
        .expect_err("watch URLs are intentionally out of scope");
        assert!(error.contains("link direto"));
    }

    #[test]
    fn parses_flat_playlist_metadata_without_network_access() {
        let json = br#"{
            "title":"Synthetic course",
            "channel":"Example channel",
            "entries":[
                {"id":"abc123DEF_0","title":"Long lesson","duration":18000,"availability":"public"},
                {"title":"Private video","availability":"private"}
            ]
        }"#;
        let playlist = parse_playlist_json(json).expect("synthetic metadata should parse");
        assert_eq!(playlist.items.len(), 2);
        assert_eq!(playlist.items[0].duration_ms, Some(18_000_000));
        assert!(playlist.items[0].selected);
        assert_eq!(playlist.items[1].status, "unavailable");
        assert!(!playlist.items[1].selected);
    }

    #[test]
    fn long_videos_receive_a_longer_conservative_cooldown() {
        let settings = PlaylistDownloadSettings::default();
        let short = configured_cooldown_seconds(&settings, Some(20 * 60_000));
        let very_long = configured_cooldown_seconds(&settings, Some(5 * 60 * 60_000));
        assert!((180..=600).contains(&short));
        assert!((1080..=3600).contains(&very_long));
    }

    #[test]
    fn playlist_profiles_use_their_documented_cooldown_ranges() {
        let conservative = PlaylistDownloadSettings::preset("conservative");
        let balanced = PlaylistDownloadSettings::preset("balanced");
        let fast = PlaylistDownloadSettings::preset("fast");
        let custom = PlaylistDownloadSettings {
            active_profile: "custom".into(),
            min_cooldown_seconds: 20,
            max_cooldown_seconds: 25,
            dynamic_duration_cooldown: false,
            daily_video_limit: None,
            daily_gb_limit: None,
            daily_limit_action: "ignore".into(),
        };

        assert!((180..=600).contains(&configured_cooldown_seconds(&conservative, None)));
        assert!((45..=120).contains(&configured_cooldown_seconds(&balanced, None)));
        assert!((5..=15).contains(&configured_cooldown_seconds(&fast, None)));
        assert!((20..=25).contains(&configured_cooldown_seconds(&custom, None)));
    }

    #[tokio::test]
    async fn playlist_settings_are_persisted_and_normalized() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let custom = PlaylistDownloadSettings {
            active_profile: "custom".into(),
            min_cooldown_seconds: 30,
            max_cooldown_seconds: 10,
            dynamic_duration_cooldown: false,
            daily_video_limit: Some(0),
            daily_gb_limit: Some(75),
            daily_limit_action: "pause_and_notify".into(),
        };
        let saved = save_playlist_download_settings(&pool, custom)
            .await
            .unwrap();
        assert_eq!(saved.max_cooldown_seconds, 30);
        assert_eq!(saved.daily_video_limit, None);
        assert_eq!(load_playlist_download_settings(&pool).await, saved);
    }

    #[test]
    fn recognizes_only_explicit_protection_errors() {
        assert!(is_rate_limit_error("HTTP Error 429: Too Many Requests"));
        assert!(is_rate_limit_error("Sign in to confirm you're not a bot"));
        assert!(!is_rate_limit_error("Video unavailable"));
    }

    #[test]
    fn recognizes_existing_files_as_idempotent_playlist_results() {
        assert!(is_existing_item_error("already_exists"));
        assert!(is_existing_item_error(
            "Falha ao mover arquivo: destino já existe; nenhuma cópia foi alterada"
        ));
        assert!(is_existing_item_error(
            "Download bloqueado: este vídeo já existe na biblioteca"
        ));
        assert!(!is_existing_item_error("HTTP Error 500"));
    }

    #[tokio::test]
    async fn local_playlist_is_reused_by_youtube_playlist_id_without_description() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        for id in ["run-one", "run-two"] {
            sqlx::query(
                "INSERT INTO download_playlists
                 (id, source_url, youtube_playlist_id, title, owner_key)
                 VALUES (?, 'https://youtube.test/playlist', 'PLstable123456',
                         'Stable playlist', 'test-owner')",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }

        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM playlists")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0,
            "importing or starting a run must not create an empty local playlist"
        );

        let first = ensure_local_playlist_for_media(&pool, "run-one")
            .await
            .unwrap()
            .unwrap();
        let second = ensure_local_playlist_for_media(&pool, "run-two")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, "pl_youtube_PLstable123456");
        assert_eq!(second, first);

        let local = sqlx::query_as::<_, (i64, Option<String>)>(
            "SELECT COUNT(*), MAX(description) FROM playlists",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(local, (1, None));
        let linked = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM download_playlists WHERE local_playlist_id = ?",
        )
        .bind(first)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(linked, 2);
    }

    #[tokio::test]
    async fn indexed_existing_media_is_later_added_to_the_local_playlist() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "INSERT INTO download_playlists
             (id, source_url, youtube_playlist_id, title, owner_key, status)
             VALUES ('existing-run', 'https://youtube.test/playlist', 'PLexisting1234',
                     'Existing course', 'test-owner', 'completed')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO download_playlist_items
             (download_playlist_id, position, youtube_id, title, status)
             VALUES ('existing-run', 3, 'abc123DEF_0', 'Existing lesson', 'skipped_existing')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO media (id, title, youtube_id, is_available)
             VALUES (902, 'Existing lesson', 'abc123DEF_0', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(reconcile_existing_playlist_items(&pool).await.unwrap(), 1);
        assert_eq!(reconcile_existing_playlist_items(&pool).await.unwrap(), 0);

        let media_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT media_id FROM download_playlist_items
             WHERE download_playlist_id = 'existing-run' AND position = 3",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(media_id, Some(902));
        let linked = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM download_playlists dp
             JOIN playlist_items pi ON pi.playlist_id = dp.local_playlist_id
             WHERE dp.id = 'existing-run' AND pi.media_id = 902 AND pi.position = 3",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(linked, 1);
    }

    #[tokio::test]
    async fn legacy_playlist_merge_preserves_items_and_removes_empty_duplicate() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO media (id, title) VALUES (901, 'Preserved video')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO playlists (id, title, description) VALUES
             ('pl_youtube_dlp_empty', 'Course', 'Playlist criada durante um download do YouTube.'),
             ('pl_youtube_dlp_populated', 'Course', 'Playlist criada durante um download do YouTube.'),
             ('pl_youtube_dlp_only_empty', 'Empty course', 'Playlist criada durante um download do YouTube.')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO playlist_items (playlist_id, media_id, position)
             VALUES ('pl_youtube_dlp_populated', 901, 4)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (id, local_id) in [
            ("legacy-run-1", "pl_youtube_dlp_empty"),
            ("legacy-run-2", "pl_youtube_dlp_populated"),
        ] {
            sqlx::query(
                "INSERT INTO download_playlists
                 (id, source_url, youtube_playlist_id, title, owner_key, local_playlist_id)
                 VALUES (?, 'https://youtube.test/playlist', 'PLlegacy123456',
                         'Course', 'test-owner', ?)",
            )
            .bind(id)
            .bind(local_id)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO download_playlists
             (id, source_url, youtube_playlist_id, title, owner_key, local_playlist_id)
             VALUES ('empty-run', 'https://youtube.test/empty', 'PLempty1234567',
                     'Empty course', 'test-owner', 'pl_youtube_dlp_only_empty')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::raw_sql(include_str!(
            "../migrations/202611250001_merge_youtube_playlists.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let playlists = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT id, description FROM playlists WHERE title = 'Course'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            playlists,
            vec![("pl_youtube_dlp_populated".to_owned(), None)]
        );
        let linked = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM download_playlists
             WHERE local_playlist_id = 'pl_youtube_dlp_populated'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(linked, 2);
        let items = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM playlist_items
             WHERE playlist_id = 'pl_youtube_dlp_populated' AND media_id = 901",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(items, 1);
        let empty_local_id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT local_playlist_id FROM download_playlists WHERE id = 'empty-run'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(empty_local_id, None);
        let empty_playlist_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = 'pl_youtube_dlp_only_empty')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!empty_playlist_exists);
    }
}
