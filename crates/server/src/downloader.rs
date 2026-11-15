use std::{collections::HashMap, env, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Extension, Json,
    body::Body,
    extract::{Path as AxumPath, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::{Mutex, Semaphore, oneshot},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{organizer, scanner::ScanQueue, web::AppState, youtube_worker::load_youtube_settings};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct DownloadSettings {
    pub target_source_id: Option<i64>,
    pub preferred_container: String,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            target_source_id: None,
            preferred_container: "mkv".to_string(),
        }
    }
}

pub async fn load_download_settings(pool: &SqlitePool) -> DownloadSettings {
    match sqlx::query_scalar::<_, String>(
        "SELECT value_json FROM settings WHERE key = 'download_settings'",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(raw)) => serde_json::from_str::<DownloadSettings>(&raw).unwrap_or_default(),
        _ => DownloadSettings::default(),
    }
}

pub async fn save_download_settings(
    pool: &SqlitePool,
    settings: &DownloadSettings,
) -> Result<(), sqlx::Error> {
    let clean = DownloadSettings {
        target_source_id: settings.target_source_id,
        preferred_container: if settings.preferred_container == "mp4" {
            "mp4".to_string()
        } else {
            "mkv".to_string()
        },
    };
    let json_str = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES ('download_settings', ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at",
    )
    .bind(json_str)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct VideoProbeRequest {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExistingLibraryMedia {
    pub media_id: i64,
    pub title: String,
    pub resolution: String,
    pub height: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoProbeResponse {
    pub youtube_id: String,
    pub title: String,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub is_short: bool,
    pub available_qualities: Vec<String>,
    pub existing_media: Option<ExistingLibraryMedia>,
}

pub fn parse_quality_height(quality: &str) -> i64 {
    match quality {
        "2160p" | "4k" => 2160,
        "1440p" | "2k" => 1440,
        "1080p" => 1080,
        "720p" => 720,
        "480p" => 480,
        "360p" => 360,
        "audio" => 0,
        "best" => 1080,
        _ => {
            if let Some(num_str) = quality.strip_suffix('p') {
                num_str.parse::<i64>().unwrap_or(1080)
            } else {
                1080
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateDownloadRequest {
    pub url: String,
    pub quality: Option<String>,
    pub container: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
pub struct DownloadJob {
    pub id: String,
    pub url: String,
    pub youtube_id: Option<String>,
    pub title: Option<String>,
    pub channel_name: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub format_container: String,
    pub quality: String,
    pub status: String,
    pub progress_percent: f64,
    pub speed_text: Option<String>,
    pub eta_text: Option<String>,
    pub output_path: Option<String>,
    pub media_id: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn generate_download_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut rand_bytes = [0u8; 6];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut rand_bytes);
    }
    format!(
        "dl_{:x}_{:x}",
        nanos,
        u64::from_be_bytes([
            0,
            0,
            rand_bytes[0],
            rand_bytes[1],
            rand_bytes[2],
            rand_bytes[3],
            rand_bytes[4],
            rand_bytes[5]
        ])
    )
}

pub fn sanitize_youtube_url(raw: &str) -> Option<String> {
    crate::validation::youtube_url(raw).map(Into::into)
}

fn extract_youtube_id_from_url(raw: &str) -> Option<String> {
    let url = crate::validation::youtube_url(raw)?;
    let host = url.host_str()?.to_ascii_lowercase();
    let candidate = if host == "youtu.be" || host.ends_with(".youtu.be") {
        url.path_segments()?.next().map(str::to_owned)
    } else if url.path() == "/watch" {
        url.query_pairs()
            .find_map(|(key, value)| (key == "v").then(|| value.into_owned()))
    } else {
        let mut segments = url.path_segments()?;
        match segments.next() {
            Some("shorts" | "embed" | "live") => segments.next().map(str::to_owned),
            _ => None,
        }
    }?;

    (candidate.len() == 11
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')))
    .then_some(candidate)
}

pub async fn probe_youtube_video(
    url: &str,
    use_cookies: bool,
    browser: &str,
) -> Result<VideoProbeResponse, String> {
    let clean_url =
        sanitize_youtube_url(url).ok_or_else(|| "URL inválida do YouTube".to_string())?;

    let mut cmd = tokio::process::Command::new(crate::config::external_command(
        "MYMEDIA_YTDLP_BIN",
        "yt-dlp",
    ));
    cmd.env_remove("LD_LIBRARY_PATH");
    cmd.arg("--ignore-config")
        .arg("--dump-single-json")
        .arg("--no-warnings")
        .arg("--no-playlist")
        .arg("--flat-playlist")
        .arg("--ignore-no-formats-error")
        .arg("--format")
        .arg("all");

    if use_cookies && crate::youtube_worker::is_valid_browser(browser) {
        cmd.arg("--cookies-from-browser").arg(browser.trim());
    }

    cmd.arg(&clean_url);

    cmd.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .map_err(|_| "A consulta ao yt-dlp excedeu 60 segundos".to_string())?
        .map_err(|e| format!("Falha ao executar yt-dlp: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Erro ao consultar vídeo: {err}"));
    }

    let json_val: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Falha ao decodificar dados do vídeo: {e}"))?;

    let youtube_id = json_val["id"].as_str().unwrap_or_default().to_string();
    if youtube_id.is_empty() {
        return Err("ID do vídeo não encontrado".to_string());
    }

    let title = json_val["title"]
        .as_str()
        .unwrap_or("Sem título")
        .to_string();

    let channel_name = json_val["uploader"]
        .as_str()
        .or_else(|| json_val["channel"].as_str())
        .unwrap_or("Desconhecido")
        .to_string();

    let thumbnail_url = json_val["thumbnail"].as_str().map(|s| s.to_string());
    let duration_ms = json_val["duration"].as_f64().map(|s| (s * 1000.0) as i64);

    let width = json_val["width"].as_i64();
    let height = json_val["height"].as_i64();
    let is_short_form = crate::probe::is_short_form_video(duration_ms, width, height, 0);

    let is_short = clean_url.contains("/shorts/")
        || json_val["is_short"].as_bool().unwrap_or(false)
        || is_short_form;

    // Extract available quality heights from formats
    let mut max_height: u32 = 0;
    if let Some(formats) = json_val["formats"].as_array() {
        for f in formats {
            if let Some(h) = f["height"].as_u64() {
                let h_u32 = h as u32;
                if h_u32 > max_height {
                    max_height = h_u32;
                }
            }
        }
    }

    let mut available_qualities = vec!["best".to_string()];
    if max_height >= 1440 {
        available_qualities.push("1440p".to_string());
    }
    if max_height >= 1080 {
        available_qualities.push("1080p".to_string());
    }
    if max_height >= 720 {
        available_qualities.push("720p".to_string());
    }
    if max_height >= 480 {
        available_qualities.push("480p".to_string());
    }
    if max_height >= 360 {
        available_qualities.push("360p".to_string());
    }

    Ok(VideoProbeResponse {
        youtube_id,
        title,
        channel_name,
        thumbnail_url,
        duration_ms,
        is_short,
        available_qualities,
        existing_media: None,
    })
}

#[derive(Clone)]
pub struct DownloadService {
    pool: SqlitePool,
    cache_dir: PathBuf,
    queue: ScanQueue,
    cancel_handles: Arc<Mutex<HashMap<String, oneshot::Sender<CancelReason>>>>,
    concurrency: Arc<Semaphore>,
    probe_concurrency: Arc<Semaphore>,
    runtime: DownloadRuntimeConfig,
}

#[derive(Clone, Copy)]
enum CancelReason {
    User,
    Shutdown,
}

#[derive(Debug, Clone)]
struct DownloadRuntimeConfig {
    concurrency: usize,
    probe_concurrency: usize,
    max_queued: i64,
    max_per_owner: i64,
    min_free_bytes: u64,
    idle_timeout: Duration,
}

impl DownloadRuntimeConfig {
    fn from_env() -> Self {
        Self {
            concurrency: env_usize("MYMEDIA_DOWNLOAD_CONCURRENCY", 2, 1, 8),
            probe_concurrency: env_usize("MYMEDIA_PROBE_CONCURRENCY", 4, 1, 16),
            max_queued: env_usize("MYMEDIA_DOWNLOAD_MAX_QUEUE", 32, 1, 256) as i64,
            max_per_owner: env_usize("MYMEDIA_DOWNLOAD_MAX_PER_OWNER", 8, 1, 64) as i64,
            min_free_bytes: env_u64("MYMEDIA_DOWNLOAD_MIN_FREE_MIB", 1024, 0, 1_048_576)
                * 1024
                * 1024,
            idle_timeout: Duration::from_secs(env_u64(
                "MYMEDIA_DOWNLOAD_IDLE_TIMEOUT_SECONDS",
                300,
                30,
                86_400,
            )),
        }
    }
}

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

#[derive(Debug, Clone)]
struct DownloadTarget {
    source_id: i64,
    path: PathBuf,
}

struct DownloadTask {
    job_id: String,
    url: String,
    quality: String,
    container: String,
    probe: Option<VideoProbeResponse>,
    target: DownloadTarget,
    cancel_rx: oneshot::Receiver<CancelReason>,
}

async fn resolve_download_target(
    pool: &SqlitePool,
    settings: &DownloadSettings,
) -> Result<DownloadTarget, String> {
    let selected = if let Some(source_id) = settings.target_source_id {
        sqlx::query_as::<_, (i64, String)>(
            "SELECT id, path FROM media_sources
             WHERE id = ? AND enabled = 1 AND mode = 'managed' AND removed_at IS NULL",
        )
        .bind(source_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("Falha ao consultar a pasta de downloads: {error}"))?
    } else {
        None
    };

    let (source_id, path) = match selected {
        Some(target) => target,
        None => sqlx::query_as::<_, (i64, String)>(
            "SELECT id, path FROM media_sources
             WHERE enabled = 1 AND mode = 'managed' AND removed_at IS NULL
             ORDER BY id LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("Falha ao consultar as pastas gerenciadas: {error}"))?
        .ok_or_else(|| {
            "Nenhuma pasta gerenciada está configurada para receber downloads. Abra Configurações → Pastas e habilite uma pasta gerenciada."
                .to_owned()
        })?,
    };

    if settings.target_source_id != Some(source_id) {
        let mut reconciled = settings.clone();
        reconciled.target_source_id = Some(source_id);
        save_download_settings(pool, &reconciled)
            .await
            .map_err(|error| format!("Falha ao salvar a pasta de downloads: {error}"))?;
    }

    Ok(DownloadTarget {
        source_id,
        path: PathBuf::from(path),
    })
}

async fn mark_index_failure(pool: &SqlitePool, job_id: &str, message: &str) {
    if let Err(error) = sqlx::query(
        "UPDATE download_jobs SET status = 'failed', process_id = NULL, error_message = ?,
         finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
    )
    .bind(message)
    .bind(job_id)
    .execute(pool)
    .await
    {
        tracing::error!(%error, %job_id, "failed to persist indexing error");
    }
}

async fn persist_interruption(
    pool: &SqlitePool,
    job_id: &str,
    reason: CancelReason,
) -> Result<(), sqlx::Error> {
    let shutting_down = matches!(reason, CancelReason::Shutdown);
    let next_status = if shutting_down { "queued" } else { "cancelled" };
    let message = shutting_down.then_some("Pausado durante shutdown coordenado");
    sqlx::query(
        "UPDATE download_jobs SET status = ?, process_id = NULL, error_message = ?,
         finished_at = CASE WHEN ? = 'cancelled' THEN datetime('now') ELSE NULL END,
         updated_at = datetime('now') WHERE id = ?",
    )
    .bind(next_status)
    .bind(message)
    .bind(next_status)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn wait_for_index(pool: &SqlitePool, job_id: &str, path: &str, scan_job_id: i64) {
    for _ in 0..480 {
        if let Ok(Some(media_id)) = sqlx::query_scalar::<_, i64>(
            "SELECT media_id FROM media_files WHERE path = ? AND is_available = 1",
        )
        .bind(path)
        .fetch_optional(pool)
        .await
        {
            if let Err(error) = sqlx::query(
                "UPDATE media
                 SET youtube_id = COALESCE(
                         youtube_id,
                         (SELECT youtube_id FROM download_jobs WHERE id = ?)
                     ),
                     updated_at = CASE
                         WHEN youtube_id IS NULL
                              AND (SELECT youtube_id FROM download_jobs WHERE id = ?) IS NOT NULL
                         THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         ELSE updated_at
                     END
                 WHERE id = ?",
            )
            .bind(job_id)
            .bind(job_id)
            .bind(media_id)
            .execute(pool)
            .await
            {
                tracing::warn!(%error, %job_id, media_id, "download youtube_id could not be propagated");
            }
            if let Err(error) = sqlx::query(
                "UPDATE download_jobs SET status = 'completed', media_id = ?, error_message = NULL,
                    process_id = NULL, finished_at = datetime('now'), updated_at = datetime('now')
                  WHERE id = ?",
            )
            .bind(media_id)
            .bind(job_id)
            .execute(pool)
            .await
            {
                tracing::error!(%error, %job_id, "failed to complete indexed download");
            }
            return;
        }

        let scan_state = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status, error_message FROM scan_jobs WHERE id = ?",
        )
        .bind(scan_job_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        if let Some((status, error)) = scan_state
            && matches!(status.as_str(), "failed" | "cancelled")
        {
            mark_index_failure(
                pool,
                job_id,
                error.as_deref().unwrap_or("A indexação do download falhou"),
            )
            .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    mark_index_failure(
        pool,
        job_id,
        "O arquivo foi salvo, mas a indexação excedeu o tempo esperado",
    )
    .await;
}

async fn recover_awaiting_index(pool: &SqlitePool, queue: &ScanQueue) {
    let jobs = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, output_path, target_source_id FROM download_jobs
         WHERE status = 'awaiting_index' AND output_path IS NOT NULL AND target_source_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (job_id, path, source_id) in jobs {
        let scan_job_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO scan_jobs (source_id, status) VALUES (?, 'queued') RETURNING id",
        )
        .bind(source_id)
        .fetch_one(pool)
        .await;
        let Ok(scan_job_id) = scan_job_id else {
            mark_index_failure(pool, &job_id, "Não foi possível retomar a indexação").await;
            continue;
        };
        if queue.enqueue(scan_job_id, source_id).await.is_err() {
            mark_index_failure(pool, &job_id, "Fila de indexação indisponível").await;
            continue;
        }
        let pool = pool.clone();
        tokio::spawn(async move {
            wait_for_index(&pool, &job_id, &path, scan_job_id).await;
        });
    }
}

async fn normalize_interrupted_downloads(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE download_jobs
         SET status = 'queued', process_id = NULL, speed_text = NULL, eta_text = NULL,
             error_message = 'Retomado após reinício do servidor', updated_at = datetime('now')
         WHERE status IN ('probing', 'downloading')",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE download_jobs
         SET status = 'failed', process_id = NULL, finished_at = datetime('now'),
             error_message = 'Reinício durante organização; verifique o arquivo de destino',
             updated_at = datetime('now')
         WHERE status = 'organizing'",
    )
    .execute(pool)
    .await?;
    sqlx::query("UPDATE download_jobs SET process_id = NULL WHERE status != 'downloading'")
        .execute(pool)
        .await?;
    Ok(())
}

async fn maintain_download_history(pool: &SqlitePool) {
    match sqlx::query(
        "DELETE FROM download_jobs
         WHERE status IN ('completed', 'failed', 'cancelled')
           AND COALESCE(finished_at, updated_at) < datetime('now', '-30 days')",
    )
    .execute(pool)
    .await
    {
        Ok(result) if result.rows_affected() > 0 => {
            tracing::info!(
                removed = result.rows_affected(),
                "old download history removed"
            );
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "download history retention failed"),
    }
}

impl DownloadService {
    pub fn start(pool: SqlitePool, cache_dir: PathBuf, queue: ScanQueue) -> Self {
        let runtime = DownloadRuntimeConfig::from_env();
        let service = Self {
            pool: pool.clone(),
            cache_dir,
            queue,
            cancel_handles: Arc::new(Mutex::new(HashMap::new())),
            concurrency: Arc::new(Semaphore::new(runtime.concurrency)),
            probe_concurrency: Arc::new(Semaphore::new(runtime.probe_concurrency)),
            runtime,
        };

        let recovery_service = service.clone();
        tokio::spawn(async move {
            if let Err(error) = normalize_interrupted_downloads(&recovery_service.pool).await {
                tracing::error!(%error, "failed to normalize downloads after restart");
            }
            maintain_download_history(&recovery_service.pool).await;

            recovery_service.recover_queued().await;
            recover_awaiting_index(&recovery_service.pool, &recovery_service.queue).await;
        });

        service
    }

    pub async fn enqueue(
        &self,
        owner_key: &str,
        url: String,
        quality: Option<String>,
        container: Option<String>,
    ) -> Result<DownloadJob, String> {
        let clean_url =
            sanitize_youtube_url(&url).ok_or_else(|| "URL inválida do YouTube".to_string())?;

        let settings = load_download_settings(&self.pool).await;
        let target = resolve_download_target(&self.pool, &settings).await?;
        self.ensure_disk_reserve(&target.path)?;

        let queued = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM download_jobs WHERE status = 'queued'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| format!("Falha ao consultar a fila de downloads: {error}"))?;
        if queued >= self.runtime.max_queued {
            return Err(format!(
                "Fila de downloads cheia (limite de {} itens)",
                self.runtime.max_queued
            ));
        }
        let owned = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM download_jobs
             WHERE owner_key = ?
               AND status IN ('queued', 'downloading', 'organizing', 'awaiting_index')",
        )
        .bind(owner_key)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| format!("Falha ao consultar o limite da credencial: {error}"))?;
        if owned >= self.runtime.max_per_owner {
            return Err(format!(
                "Limite de {} downloads ativos por credencial atingido",
                self.runtime.max_per_owner
            ));
        }
        let chosen_container = container
            .filter(|c| c == "mp4" || c == "mkv")
            .unwrap_or(settings.preferred_container);

        let chosen_quality = quality.unwrap_or_else(|| "best".to_string());

        let job_id = generate_download_id();

        // Perform probe first to collect title, channel, duration, thumbnail
        let probe = match self.probe(&clean_url).await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(%e, "probe failed; queuing with minimal metadata");
                extract_youtube_id_from_url(&clean_url).map(|youtube_id| VideoProbeResponse {
                    youtube_id,
                    title: "Video".to_owned(),
                    channel_name: "YouTube".to_owned(),
                    thumbnail_url: None,
                    duration_ms: None,
                    is_short: clean_url.contains("/shorts/"),
                    available_qualities: Vec::new(),
                    existing_media: None,
                })
            }
        };

        let youtube_id = probe.as_ref().map(|p| p.youtube_id.clone());
        let title = probe.as_ref().map(|p| p.title.clone());
        let channel_name = probe.as_ref().map(|p| p.channel_name.clone());
        let thumbnail_url = probe.as_ref().and_then(|p| p.thumbnail_url.clone());
        let duration_ms = probe.as_ref().and_then(|p| p.duration_ms);

        // Bloqueia download se o vídeo já existir na biblioteca com qualidade igual ou superior
        if let Some(ref yid) = youtube_id {
            let existing = sqlx::query_as::<_, (i64, String, Option<i64>, Option<i64>)>(
                "SELECT m.id, m.title, m.width, m.height
                 FROM media m
                 WHERE m.youtube_id = ? AND m.is_available = 1
                 LIMIT 1",
            )
            .bind(yid)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);

            if let Some((_ex_id, ex_title, _w, h)) = existing {
                let existing_height = h.unwrap_or(0);
                let requested_height = parse_quality_height(&chosen_quality);
                if requested_height > 0 && requested_height <= existing_height {
                    return Err(format!(
                        "Download bloqueado: este vídeo já existe na biblioteca com qualidade igual ou superior ({}p - \"{}\"). O sistema preserva a melhor cópia para evitar duplicatas.",
                        existing_height, ex_title
                    ));
                }
            }
        }

        let inserted = sqlx::query(
            "INSERT INTO download_jobs (
                id, url, youtube_id, title, channel_name, thumbnail_url, duration_ms,
                format_container, quality, status, progress_percent, target_source_id,
                owner_key, created_at, updated_at
            )
            SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', 0.0, ?, ?, datetime('now'), datetime('now')
            WHERE (SELECT COUNT(*) FROM download_jobs WHERE status = 'queued') < ?
              AND (SELECT COUNT(*) FROM download_jobs
                   WHERE owner_key = ?
                     AND status IN ('queued', 'downloading', 'organizing', 'awaiting_index')) < ?",
        )
        .bind(&job_id)
        .bind(&clean_url)
        .bind(&youtube_id)
        .bind(&title)
        .bind(&channel_name)
        .bind(&thumbnail_url)
        .bind(duration_ms)
        .bind(&chosen_container)
        .bind(&chosen_quality)
        .bind(target.source_id)
        .bind(owner_key)
        .bind(self.runtime.max_queued)
        .bind(owner_key)
        .bind(self.runtime.max_per_owner)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Erro ao salvar job no banco: {e}"))?;
        if inserted.rows_affected() == 0 {
            return Err(format!(
                "Fila de downloads cheia (limite de {} itens)",
                self.runtime.max_queued
            ));
        }

        let job = sqlx::query_as::<_, DownloadJob>(
            "SELECT id, url, youtube_id, title, channel_name, thumbnail_url, duration_ms,
                    format_container, quality, status, progress_percent, speed_text, eta_text,
                    output_path, media_id, error_message, created_at, updated_at
             FROM download_jobs WHERE id = ?",
        )
        .bind(&job_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Erro ao buscar job inserido: {e}"))?;

        self.spawn_task(
            job_id,
            clean_url,
            chosen_quality,
            chosen_container,
            probe,
            target,
        )
        .await;

        Ok(job)
    }

    async fn probe(&self, url: &str) -> Result<VideoProbeResponse, String> {
        let _permit = self
            .probe_concurrency
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                "Limite de consultas de vídeo atingido; tente novamente em instantes".to_owned()
            })?;
        let settings = load_youtube_settings(&self.pool).await;
        probe_youtube_video(url, settings.use_browser_cookies, &settings.browser).await
    }

    fn ensure_disk_reserve(&self, path: &std::path::Path) -> Result<(), String> {
        let available = fs2::available_space(path)
            .map_err(|error| format!("Não foi possível verificar o espaço livre: {error}"))?;
        if available < self.runtime.min_free_bytes {
            return Err(format!(
                "Espaço livre insuficiente: {} MiB disponíveis; a reserva mínima é {} MiB",
                available / 1024 / 1024,
                self.runtime.min_free_bytes / 1024 / 1024
            ));
        }
        Ok(())
    }

    async fn recover_queued(&self) {
        let jobs = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<i64>,
                i64,
                String,
            ),
        >(
            "SELECT d.id, d.url, d.quality, d.format_container, d.youtube_id, d.title,
                    d.channel_name, d.thumbnail_url, d.duration_ms, d.target_source_id, s.path
             FROM download_jobs d
             JOIN media_sources s ON s.id = d.target_source_id
             WHERE d.status = 'queued' AND s.enabled = 1 AND s.mode = 'managed'
             ORDER BY d.created_at",
        )
        .fetch_all(&self.pool)
        .await;
        let Ok(jobs) = jobs else {
            tracing::error!(error = ?jobs.err(), "failed to recover persistent queue");
            return;
        };

        if let Err(error) = sqlx::query(
            "UPDATE download_jobs
             SET status = 'failed', finished_at = datetime('now'),
                 error_message = 'Destino gerenciado indisponível após reinício',
                 updated_at = datetime('now')
             WHERE status = 'queued' AND NOT EXISTS (
                 SELECT 1 FROM media_sources s
                 WHERE s.id = download_jobs.target_source_id
                   AND s.enabled = 1 AND s.mode = 'managed' AND s.removed_at IS NULL
             )",
        )
        .execute(&self.pool)
        .await
        {
            tracing::error!(%error, "failed to stop downloads without a valid destination");
        }

        for (
            id,
            url,
            quality,
            container,
            youtube_id,
            title,
            channel_name,
            thumbnail_url,
            duration_ms,
            source_id,
            path,
        ) in jobs
        {
            let probe = youtube_id.map(|youtube_id| VideoProbeResponse {
                youtube_id,
                title: title.unwrap_or_else(|| "Video".to_owned()),
                channel_name: channel_name.unwrap_or_else(|| "YouTube".to_owned()),
                thumbnail_url,
                duration_ms,
                is_short: false,
                available_qualities: Vec::new(),
                existing_media: None,
            });
            self.spawn_task(
                id,
                url,
                quality,
                container,
                probe,
                DownloadTarget {
                    source_id,
                    path: PathBuf::from(path),
                },
            )
            .await;
        }
    }

    async fn spawn_task(
        &self,
        job_id: String,
        url: String,
        quality: String,
        container: String,
        probe: Option<VideoProbeResponse>,
        target: DownloadTarget,
    ) {
        let (cancel_tx, cancel_rx) = oneshot::channel();
        self.cancel_handles
            .lock()
            .await
            .insert(job_id.clone(), cancel_tx);
        let service = self.clone();
        tokio::spawn(async move {
            service
                .run_download_task(DownloadTask {
                    job_id,
                    url,
                    quality,
                    container,
                    probe,
                    target,
                    cancel_rx,
                })
                .await;
        });
    }

    async fn fail_job(&self, job_id: &str, message: &str) {
        if let Err(error) = sqlx::query(
            "UPDATE download_jobs
             SET status = 'failed', process_id = NULL, error_message = ?,
                 finished_at = datetime('now'), updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(message)
        .bind(job_id)
        .execute(&self.pool)
        .await
        {
            tracing::error!(%error, %job_id, "failed to persist download error");
        }
    }

    pub async fn cancel_or_remove(&self, job_id: &str) -> Result<bool, String> {
        // Send cancel if currently running
        {
            let mut handles = self.cancel_handles.lock().await;
            if let Some(tx) = handles.remove(job_id) {
                let _ = tx.send(CancelReason::User);
            }
        }

        let res = sqlx::query("DELETE FROM download_jobs WHERE id = ?")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Erro ao remover job: {e}"))?;

        Ok(res.rows_affected() > 0)
    }

    pub async fn shutdown(&self) {
        let handles = {
            let mut handles = self.cancel_handles.lock().await;
            handles
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>()
        };
        let requested = handles.len();
        for sender in handles {
            let _ = sender.send(CancelReason::Shutdown);
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let active = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM download_jobs WHERE status = 'downloading'",
            )
            .fetch_one(&self.pool)
            .await
            .unwrap_or_default();
            if active == 0 || tokio::time::Instant::now() >= deadline {
                if active > 0 {
                    tracing::error!(
                        active,
                        "downloads did not stop within the shutdown deadline"
                    );
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tracing::info!(requested, "coordinated download shutdown completed");
    }

    async fn run_download_task(&self, task: DownloadTask) {
        let DownloadTask {
            job_id,
            url,
            quality,
            container,
            probe,
            target,
            mut cancel_rx,
        } = task;
        let permit = tokio::select! {
            permit = self.concurrency.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => return,
            },
            _ = &mut cancel_rx => {
                self.cancel_handles.lock().await.remove(&job_id);
                return;
            }
        };
        let claimed = sqlx::query(
            "UPDATE download_jobs
             SET status = 'downloading', attempt_count = attempt_count + 1,
                 started_at = datetime('now'), last_progress_at = datetime('now'),
                 error_message = NULL, updated_at = datetime('now')
             WHERE id = ? AND status = 'queued'",
        )
        .bind(&job_id)
        .execute(&self.pool)
        .await;
        match claimed {
            Ok(result) if result.rows_affected() == 1 => {}
            Ok(_) => {
                self.cancel_handles.lock().await.remove(&job_id);
                return;
            }
            Err(error) => {
                tracing::error!(%error, %job_id, "failed to start queued download");
                self.cancel_handles.lock().await.remove(&job_id);
                return;
            }
        }
        let _permit = permit;
        let temp_dir = self.cache_dir.join("downloads_temp").join(&job_id);
        if let Err(error) = tokio::fs::create_dir_all(&temp_dir).await {
            self.fail_job(
                &job_id,
                &format!("Falha ao preparar diretório temporário: {error}"),
            )
            .await;
            return;
        }

        let output_template = temp_dir.join("video.%(ext)s");

        let yt_settings = load_youtube_settings(&self.pool).await;

        let format_arg = match quality.as_str() {
            "1440p" => "bv*[height<=1440]+ba/b[height<=1440]",
            "1080p" => "bv*[height<=1080]+ba/b[height<=1080]",
            "720p" => "bv*[height<=720]+ba/b[height<=720]",
            "480p" => "bv*[height<=480]+ba/b[height<=480]",
            "360p" => "bv*[height<=360]+ba/b[height<=360]",
            _ => "bv*+ba/b",
        };

        let mut cmd = tokio::process::Command::new(crate::config::external_command(
            "MYMEDIA_YTDLP_BIN",
            "yt-dlp",
        ));
        cmd.env_remove("LD_LIBRARY_PATH");
        cmd.kill_on_drop(true);
        cmd.arg("--ignore-config")
            .arg("--no-playlist")
            .arg("-f")
            .arg(format_arg)
            .arg("--remux-video")
            .arg(&container)
            .arg("--newline")
            .arg("--progress-template")
            .arg("download:%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s")
            .arg("-o")
            .arg(output_template)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if yt_settings.use_browser_cookies
            && crate::youtube_worker::is_valid_browser(&yt_settings.browser)
        {
            cmd.arg("--cookies-from-browser")
                .arg(yt_settings.browser.trim());
        }

        cmd.arg(&url);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("Falha ao iniciar processo yt-dlp: {e}");
                self.fail_job(&job_id, &err_msg).await;
                return;
            }
        };

        let process_id = child.id().map(i64::from);
        if let Err(error) = sqlx::query(
            "UPDATE download_jobs SET process_id = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(process_id)
        .bind(&job_id)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, %job_id, "failed to persist download PID");
        }

        let stdout = child.stdout.take();
        let mut reader = stdout.map(|s| BufReader::new(s).lines());
        let stderr_task = child.stderr.take().map(|stderr| {
            tokio::spawn(async move {
                let mut stderr = stderr;
                let mut bytes = Vec::new();
                let mut chunk = [0_u8; 8192];
                loop {
                    match stderr.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(read) => {
                            bytes.extend_from_slice(&chunk[..read]);
                            if bytes.len() > 65_536 {
                                bytes.drain(..bytes.len() - 65_536);
                            }
                        }
                        Err(_) => break,
                    }
                }
                String::from_utf8_lossy(&bytes).trim().to_owned()
            })
        });

        let mut last_db_update = tokio::time::Instant::now();
        let mut last_activity = tokio::time::Instant::now();
        let mut last_percent = 0.0;
        let mut watchdog = tokio::time::interval(Duration::from_secs(5));
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let exit_status = loop {
            tokio::select! {
                reason = &mut cancel_rx => {
                    let reason = reason.unwrap_or(CancelReason::Shutdown);
                    let shutting_down = matches!(reason, CancelReason::Shutdown);
                    tracing::info!(job_id = %job_id, shutting_down, "download interrupted");
                    let _ = child.kill().await;
                    if let Err(error) = persist_interruption(&self.pool, &job_id, reason).await {
                        tracing::error!(%error, %job_id, "failed to persist cancellation");
                    }
                    if let Err(error) = tokio::fs::remove_dir_all(&temp_dir).await {
                        tracing::warn!(%error, %job_id, "failed to clean up cancelled download");
                    }
                    return;
                }
                _ = watchdog.tick() => {
                    if last_activity.elapsed() >= self.runtime.idle_timeout {
                        let _ = child.kill().await;
                        self.fail_job(&job_id, "Download encerrado pelo watchdog por falta de progresso").await;
                        if let Err(error) = tokio::fs::remove_dir_all(&temp_dir).await {
                            tracing::warn!(%error, %job_id, "failed to clean up expired download");
                        }
                        return;
                    }
                }
                status_res = child.wait() => {
                    break status_res;
                }
                line_res = async {
                    if let Some(r) = &mut reader {
                        r.next_line().await
                    } else {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        Ok(None)
                    }
                } => {
                    if let Ok(Some(line)) = line_res
                        && let Some(progress) = line.strip_prefix("download:")
                    {
                            last_activity = tokio::time::Instant::now();
                            let parts: Vec<&str> = progress.split('|').collect();
                            if !parts.is_empty() {
                                let percent_str = parts[0].trim().trim_end_matches('%');
                                if let Ok(parsed_p) = percent_str.parse::<f64>() {
                                    let speed_text = parts.get(1).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                                    let eta_text = parts.get(2).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

                                    let should_update = last_db_update.elapsed() >= Duration::from_millis(800)
                                        || (parsed_p - last_percent).abs() >= 2.0;

                                    if should_update {
                                        last_db_update = tokio::time::Instant::now();
                                        last_percent = parsed_p;
                                        if let Err(error) = sqlx::query(
                                            "UPDATE download_jobs
                                             SET progress_percent = ?, speed_text = ?, eta_text = ?,
                                                 last_progress_at = datetime('now'), updated_at = datetime('now')
                                             WHERE id = ?",
                                        )
                                        .bind(parsed_p)
                                        .bind(&speed_text)
                                        .bind(&eta_text)
                                        .bind(&job_id)
                                        .execute(&self.pool)
                                        .await {
                                            tracing::warn!(%error, %job_id, "failed to persist progress");
                                        }
                                    }
                                }
                            }
                    }
                }
            }
        };

        // Remove from cancel handles
        {
            let mut handles = self.cancel_handles.lock().await;
            handles.remove(&job_id);
        }

        let stderr_text = match stderr_task {
            Some(task) => task.await.unwrap_or_default(),
            None => String::new(),
        };

        match exit_status {
            Ok(status) if status.success() => {
                // Find downloaded file in temp_dir
                let mut downloaded_file: Option<PathBuf> = None;
                if let Ok(mut entries) = tokio::fs::read_dir(&temp_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if path.is_file() {
                            let ext = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase();
                            if ext == container || ext == "mkv" || ext == "mp4" {
                                downloaded_file = Some(path);
                                break;
                            }
                        }
                    }
                }

                let Some(source_file) = downloaded_file else {
                    self.fail_job(&job_id, "Arquivo final não encontrado").await;
                    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
                    return;
                };

                {
                    // Update status to organizing
                    if let Err(error) = sqlx::query(
                        "UPDATE download_jobs SET status = 'organizing', process_id = NULL,
                         updated_at = datetime('now') WHERE id = ?",
                    )
                    .bind(&job_id)
                    .execute(&self.pool)
                    .await
                    {
                        self.fail_job(&job_id, &format!("Falha ao persistir organização: {error}"))
                            .await;
                        return;
                    }

                    let source_root = &target.path;
                    let title = probe
                        .as_ref()
                        .map(|p| p.title.clone())
                        .unwrap_or_else(|| "Video".to_string());
                    let channel = probe
                        .as_ref()
                        .map(|p| p.channel_name.clone())
                        .unwrap_or_else(|| "YouTube".to_string());
                    let ytid = probe
                        .as_ref()
                        .map(|p| p.youtube_id.clone())
                        .unwrap_or_else(|| "unknown".to_string());

                    // Determine if the actual downloaded file is vertical (is_short)
                    let probed_meta = crate::probe::inspect(&source_file.display().to_string())
                        .await
                        .ok();
                    let is_short = if let Some(ref meta) = probed_meta {
                        meta.is_short()
                    } else {
                        probe.as_ref().map(|p| p.is_short).unwrap_or(false)
                    };

                    let canonical = organizer::compute_canonical_path(
                        source_root,
                        &channel,
                        &title,
                        &ytid,
                        is_short,
                        &container,
                    );

                    let move_result = match self.ensure_disk_reserve(&target.path) {
                        Ok(()) => crate::filesystem::move_verified(&source_file, &canonical).await,
                        Err(error) => Err(error),
                    };

                    match move_result {
                        Ok(()) => {
                            let canon_str = canonical.display().to_string();

                            // Enqueue scan to index the new file
                            let scan_job_res = sqlx::query_scalar::<_, i64>(
                                "INSERT INTO scan_jobs (source_id, status) VALUES (?, 'queued') RETURNING id",
                            )
                            .bind(target.source_id)
                            .fetch_one(&self.pool)
                            .await;

                            let Ok(scan_job_id) = scan_job_res else {
                                mark_index_failure(
                                    &self.pool,
                                    &job_id,
                                    "Não foi possível criar o scan do download",
                                )
                                .await;
                                return;
                            };
                            if self
                                .queue
                                .enqueue(scan_job_id, target.source_id)
                                .await
                                .is_err()
                            {
                                mark_index_failure(
                                    &self.pool,
                                    &job_id,
                                    "Fila de indexação indisponível",
                                )
                                .await;
                                return;
                            }

                            if let Err(error) = sqlx::query(
                            "UPDATE download_jobs
                             SET status = 'awaiting_index', progress_percent = 100.0, output_path = ?,
                                 target_source_id = ?, media_id = NULL, error_message = NULL,
                                 updated_at = datetime('now')
                             WHERE id = ?",
                        )
                        .bind(&canon_str)
                        .bind(target.source_id)
                        .bind(&job_id)
                        .execute(&self.pool)
                        .await {
                                mark_index_failure(
                                    &self.pool,
                                    &job_id,
                                    &format!("Falha ao persistir indexação: {error}"),
                                )
                                .await;
                                return;
                            }
                            wait_for_index(&self.pool, &job_id, &canon_str, scan_job_id).await;
                        }
                        Err(error) => {
                            tracing::error!(
                                %error,
                                destination = %canonical.display(),
                                "download could not be moved with verification"
                            );
                            self.fail_job(
                                &job_id,
                                &format!("Falha ao mover arquivo para a pasta canônica: {error}"),
                            )
                            .await;
                        }
                    }
                }

                let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            }
            Ok(status) => {
                let mut err_msg = format!("yt-dlp encerrou com código {status}");
                if !stderr_text.is_empty() {
                    err_msg = stderr_text;
                }
                self.fail_job(&job_id, &err_msg).await;
                let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            }
            Err(e) => {
                let err_msg = format!("Erro no processo de download: {e}");
                self.fail_job(&job_id, &err_msg).await;
                let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            }
        }
    }
}

// REST Handlers

pub(crate) async fn probe_video_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<VideoProbeRequest>,
) -> Result<Json<VideoProbeResponse>, (StatusCode, String)> {
    let mut probe = state
        .downloader
        .probe(&payload.url)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // Verifica se este vídeo já existe na biblioteca
    let existing = sqlx::query_as::<_, (i64, String, Option<i64>, Option<i64>)>(
        "SELECT m.id, m.title, m.width, m.height
         FROM media m
         WHERE m.youtube_id = ? AND m.is_available = 1
         LIMIT 1",
    )
    .bind(&probe.youtube_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    if let Some((media_id, title, _w, h)) = existing {
        let res_str = match h {
            Some(height) => format!("{height}p"),
            None => "Qualidade padrão".to_string(),
        };
        probe.existing_media = Some(ExistingLibraryMedia {
            media_id,
            title,
            resolution: res_str,
            height: h,
        });
    }

    Ok(Json(probe))
}

pub(crate) async fn create_download_endpoint(
    State(state): State<AppState>,
    Extension(user): Extension<crate::auth::AuthenticatedUser>,
    Json(payload): Json<CreateDownloadRequest>,
) -> Result<Json<DownloadJob>, (StatusCode, String)> {
    state
        .downloader
        .enqueue(
            &user.principal_key,
            payload.url,
            payload.quality,
            payload.container,
        )
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

pub(crate) async fn list_downloads_endpoint(
    State(state): State<AppState>,
) -> Result<Json<Vec<DownloadJob>>, StatusCode> {
    let jobs = sqlx::query_as::<_, DownloadJob>(
        "SELECT id, url, youtube_id, title, channel_name, thumbnail_url, duration_ms,
                format_container, quality, status, progress_percent, speed_text, eta_text,
                output_path, media_id, error_message, created_at, updated_at
         FROM download_jobs
         ORDER BY created_at DESC
         LIMIT 5",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(%e, "failed to fetch download jobs");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(jobs))
}

pub(crate) async fn delete_download_endpoint(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .downloader
        .cancel_or_remove(&id)
        .await
        .map(|ok| {
            if ok {
                StatusCode::NO_CONTENT
            } else {
                StatusCode::NOT_FOUND
            }
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub(crate) async fn get_download_file_endpoint(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    request: Request<Body>,
) -> Response {
    let job_opt = sqlx::query_as::<_, (String, Option<String>, String)>(
        "SELECT title, output_path, format_container FROM download_jobs WHERE id = ? AND status = 'completed'",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((title, Some(output_path), container))) = job_opt else {
        return (StatusCode::NOT_FOUND, "Arquivo não encontrado").into_response();
    };

    let path = PathBuf::from(&output_path);
    if !path.is_file() {
        return (StatusCode::NOT_FOUND, "Arquivo físico não existe mais").into_response();
    }

    let clean_title = organizer::sanitize_path_segment(&title);
    let filename = format!("{clean_title}.{container}");
    let mime = if container == "mp4" {
        "video/mp4"
    } else {
        "video/x-matroska"
    };

    let mut response = match ServeFile::new(&path).oneshot(request).await {
        Ok(res) => res.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| {
                header::HeaderValue::from_static("attachment; filename=\"video.mkv\"")
            }),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, header::HeaderValue::from_static(mime));

    response
}

pub(crate) async fn get_download_settings_endpoint(
    State(state): State<AppState>,
) -> Json<DownloadSettings> {
    Json(load_download_settings(&state.pool).await)
}

pub(crate) async fn update_download_settings_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<DownloadSettings>,
) -> Result<Json<DownloadSettings>, StatusCode> {
    if let Some(source_id) = payload.target_source_id {
        let managed = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM media_sources
                WHERE id = ? AND enabled = 1 AND mode = 'managed' AND removed_at IS NULL
            )",
        )
        .bind(source_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to validate download source");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        if !managed {
            return Err(StatusCode::CONFLICT);
        }
    }
    save_download_settings(&state.pool, &payload)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to save download settings");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(load_download_settings(&state.pool).await))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_youtube_url() {
        assert_eq!(
            sanitize_youtube_url("  https://www.youtube.com/watch?v=abc12345678  "),
            Some("https://www.youtube.com/watch?v=abc12345678".to_string())
        );
        assert_eq!(
            sanitize_youtube_url("https://youtu.be/abc12345678"),
            Some("https://youtu.be/abc12345678".to_string())
        );
        assert_eq!(
            sanitize_youtube_url("https://youtube.com/shorts/abc12345678"),
            Some("https://youtube.com/shorts/abc12345678".to_string())
        );
        assert_eq!(sanitize_youtube_url("ftp://example.com"), None);
        assert_eq!(sanitize_youtube_url("https://vimeo.com/12345"), None);
    }

    #[test]
    fn extracts_youtube_id_from_supported_video_urls() {
        assert_eq!(
            extract_youtube_id_from_url("https://www.youtube.com/watch?v=abc123_DEF-"),
            Some("abc123_DEF-".to_owned())
        );
        assert_eq!(
            extract_youtube_id_from_url("https://youtu.be/abc123_DEF-?feature=share"),
            Some("abc123_DEF-".to_owned())
        );
        assert_eq!(
            extract_youtube_id_from_url("https://youtube.com/shorts/abc123_DEF-?si=token"),
            Some("abc123_DEF-".to_owned())
        );
        assert_eq!(
            extract_youtube_id_from_url("https://www.youtube.com/embed/abc123_DEF-"),
            Some("abc123_DEF-".to_owned())
        );
        assert_eq!(
            extract_youtube_id_from_url("https://youtube.com/shorts/too-short"),
            None
        );
    }

    #[test]
    fn test_generate_download_id() {
        let id1 = generate_download_id();
        let id2 = generate_download_id();
        assert!(id1.starts_with("dl_"));
        assert!(id2.starts_with("dl_"));
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn download_target_requires_and_reconciles_managed_source() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let empty = resolve_download_target(&pool, &DownloadSettings::default()).await;
        assert!(empty.unwrap_err().contains("Nenhuma pasta gerenciada"));

        sqlx::query(
            "INSERT INTO media_sources (id, path, mode, status) VALUES
             (1, '/library/read-only', 'read_only', 'ready'),
             (2, '/library/managed', 'managed', 'ready')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let settings = DownloadSettings {
            target_source_id: Some(1),
            preferred_container: "mkv".to_owned(),
        };
        let target = resolve_download_target(&pool, &settings).await.unwrap();
        assert_eq!(target.source_id, 2);
        assert_eq!(target.path, PathBuf::from("/library/managed"));
        assert_eq!(
            load_download_settings(&pool).await.target_source_id,
            Some(2)
        );
    }

    #[tokio::test]
    async fn test_download_settings_crud() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();

        let default_s = load_download_settings(&pool).await;
        assert_eq!(default_s.target_source_id, None);
        assert_eq!(default_s.preferred_container, "mkv");

        let custom = DownloadSettings {
            target_source_id: Some(42),
            preferred_container: "mp4".to_string(),
        };
        save_download_settings(&pool, &custom).await.unwrap();

        let loaded = load_download_settings(&pool).await;
        assert_eq!(loaded.target_source_id, Some(42));
        assert_eq!(loaded.preferred_container, "mp4");
    }

    #[tokio::test]
    async fn restart_normalizes_every_download_state() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let states = [
            "queued",
            "probing",
            "downloading",
            "organizing",
            "awaiting_index",
            "completed",
            "failed",
            "cancelled",
        ];
        for state in states {
            sqlx::query(
                "INSERT INTO download_jobs
                 (id, url, format_container, quality, status, process_id)
                 VALUES (?, 'https://youtu.be/abc12345678', 'mkv', 'best', ?, 4242)",
            )
            .bind(state)
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
        }

        normalize_interrupted_downloads(&pool).await.unwrap();

        let actual = sqlx::query_as::<_, (String, String, Option<i64>)>(
            "SELECT id, status, process_id FROM download_jobs ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|(id, status, pid)| (id, (status, pid)))
        .collect::<HashMap<_, _>>();

        assert_eq!(actual["queued"].0, "queued");
        assert_eq!(actual["probing"], ("queued".to_owned(), None));
        assert_eq!(actual["downloading"], ("queued".to_owned(), None));
        assert_eq!(actual["organizing"], ("failed".to_owned(), None));
        for unchanged in ["awaiting_index", "completed", "failed", "cancelled"] {
            assert_eq!(actual[unchanged], (unchanged.to_owned(), None));
        }
    }

    #[tokio::test]
    async fn coordinated_shutdown_requeues_while_user_cancel_is_terminal() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        for id in ["shutdown", "user"] {
            sqlx::query(
                "INSERT INTO download_jobs
                 (id, url, format_container, quality, status, process_id)
                 VALUES (?, 'https://youtu.be/abc12345678', 'mkv', 'best', 'downloading', 4242)",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }

        persist_interruption(&pool, "shutdown", CancelReason::Shutdown)
            .await
            .unwrap();
        persist_interruption(&pool, "user", CancelReason::User)
            .await
            .unwrap();

        let shutdown = sqlx::query_as::<_, (String, Option<i64>, Option<String>)>(
            "SELECT status, process_id, finished_at FROM download_jobs WHERE id = 'shutdown'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(shutdown, ("queued".to_owned(), None, None));
        let user = sqlx::query_as::<_, (String, Option<i64>, Option<String>)>(
            "SELECT status, process_id, finished_at FROM download_jobs WHERE id = 'user'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(user.0, "cancelled");
        assert_eq!(user.1, None);
        assert!(user.2.is_some());
    }
}
