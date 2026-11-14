use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

use sqlx::{FromRow, SqlitePool};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use walkdir::WalkDir;

use crate::{
    probe::{self, TechnicalMetadata},
    thumbnail::ThumbnailService,
    youtube_worker::YoutubeWorkerService,
};

#[derive(Clone)]
pub struct ScanQueue {
    sender: mpsc::Sender<ScanCommand>,
}

#[derive(Clone, Copy, Debug)]
struct ScanCommand {
    job_id: i64,
    source_id: i64,
}

#[derive(Debug, FromRow)]
struct SourceConfig {
    path: String,
    recursive: bool,
    enabled: bool,
    removed_at: Option<String>,
}

#[derive(Debug)]
struct DiscoveredFile {
    path: String,
    title: String,
    extension: String,
    size_bytes: i64,
    modified_at_unix: i64,
    technical: Option<Result<TechnicalMetadata, String>>,
    inferred_channel: Option<InferredChannel>,
}

#[derive(Debug)]
struct InferredChannel {
    id: String,
    name: String,
}

#[derive(Debug)]
struct DiscoveryResult {
    files: Vec<DiscoveredFile>,
    warnings: Vec<String>,
    reconciliation_safe: bool,
}

#[derive(Debug, FromRow)]
struct ExistingFile {
    path: String,
    size_bytes: i64,
    modified_at_unix: i64,
    inspected_at: Option<String>,
    inspection_error: Option<String>,
}

struct ExistingFileState {
    size_bytes: i64,
    modified_at_unix: i64,
    inspected_at: Option<String>,
    inspection_error: Option<String>,
}

impl ScanQueue {
    pub fn start(
        pool: SqlitePool,
        thumbnails: ThumbnailService,
        youtube: YoutubeWorkerService,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(64);
        tokio::spawn(run_worker(pool, receiver, thumbnails, youtube));
        Self { sender }
    }

    pub async fn enqueue(&self, job_id: i64, source_id: i64) -> Result<(), String> {
        self.sender
            .try_send(ScanCommand { job_id, source_id })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    "fila de descoberta cheia; tente novamente em instantes".to_owned()
                }
                mpsc::error::TrySendError::Closed(_) => {
                    "fila de descoberta indisponível".to_owned()
                }
            })
    }
}

async fn run_worker(
    pool: SqlitePool,
    mut receiver: mpsc::Receiver<ScanCommand>,
    thumbnails: ThumbnailService,
    youtube: YoutubeWorkerService,
) {
    if let Err(error) = recover_pending_jobs(&pool, &thumbnails, &youtube).await {
        tracing::error!(%error, "failed to recover scan jobs");
    }

    while let Some(command) = receiver.recv().await {
        if let Err(error) = process_job(&pool, command, &thumbnails, &youtube).await {
            tracing::error!(job_id = command.job_id, source_id = command.source_id, %error, "scan failed");
            fail_job(&pool, command, &error.to_string()).await;
        }
    }
}

async fn recover_pending_jobs(
    pool: &SqlitePool,
    thumbnails: &ThumbnailService,
    youtube: &YoutubeWorkerService,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE scan_jobs SET status = 'queued', started_at = NULL WHERE status = 'running'",
    )
    .execute(pool)
    .await?;
    let jobs = sqlx::query_as::<_, (i64, i64)>(
        "SELECT id, source_id FROM scan_jobs WHERE status = 'queued' AND source_id IS NOT NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    for (job_id, source_id) in jobs {
        if let Err(error) =
            process_job(pool, ScanCommand { job_id, source_id }, thumbnails, youtube).await
        {
            tracing::error!(job_id, source_id, %error, "recovered scan failed");
            fail_job(pool, ScanCommand { job_id, source_id }, &error.to_string()).await;
        }
    }

    Ok(())
}

async fn process_job(
    pool: &SqlitePool,
    command: ScanCommand,
    thumbnails: &ThumbnailService,
    youtube: &YoutubeWorkerService,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source = sqlx::query_as::<_, SourceConfig>(
        "SELECT path, recursive, enabled, removed_at FROM media_sources WHERE id = ?",
    )
    .bind(command.source_id)
    .fetch_optional(pool)
    .await?;

    let Some(source) = source else {
        cancel_job(pool, command.job_id, "fonte removida").await?;
        return Ok(());
    };
    if !source.enabled || source.removed_at.is_some() {
        cancel_job(pool, command.job_id, "fonte desabilitada").await?;
        return Ok(());
    }

    sqlx::query("UPDATE scan_jobs SET status = 'running', started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), error_message = NULL WHERE id = ? AND status = 'queued'")
        .bind(command.job_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE media_sources SET status = 'scanning', error_message = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(command.source_id)
        .execute(pool)
        .await?;

    let recursive = source.recursive;
    let root = PathBuf::from(source.path);
    let mut discovery = tokio::task::spawn_blocking(move || discover(&root, recursive)).await??;
    inspect_changed_files(pool, &mut discovery).await?;
    let discovered_count = i64::try_from(discovery.files.len())?;

    sqlx::query("UPDATE scan_jobs SET discovered_files = ? WHERE id = ?")
        .bind(discovered_count)
        .bind(command.job_id)
        .execute(pool)
        .await?;

    persist_discovery(pool, command, discovery).await?;
    thumbnails.enqueue_source(command.source_id, pool).await;
    youtube.trigger();
    Ok(())
}

fn discover(root: &Path, recursive: bool) -> Result<DiscoveryResult, std::io::Error> {
    let metadata = std::fs::metadata(root)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a fonte não é um diretório",
        ));
    }

    let mut walker = WalkDir::new(root).follow_links(false);
    if !recursive {
        walker = walker.max_depth(1);
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    for entry in walker.into_iter().filter_entry(|entry| {
        if entry.depth() == 0 {
            return true;
        }
        if entry.file_type().is_symlink() {
            return false;
        }
        if entry.file_type().is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with('.'))
        {
            return false;
        }
        true
    }) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_file() || !is_video_file(entry.path()) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                warnings.push(format!("{}: {error}", entry.path().display()));
                continue;
            }
        };
        let Some(path) = entry.path().to_str().map(str::to_owned) else {
            warnings.push(format!(
                "caminho não-UTF-8 ignorado: {}",
                entry.path().display()
            ));
            continue;
        };
        let extension = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let title = entry
            .path()
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Vídeo sem título")
            .to_owned();
        let modified_at_unix = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .and_then(|value| i64::try_from(value.as_nanos()).ok())
            .unwrap_or(0);
        let size_bytes = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
        let inferred_channel = infer_channel(root, entry.path());

        files.push(DiscoveredFile {
            path,
            title,
            extension,
            size_bytes,
            modified_at_unix,
            technical: None,
            inferred_channel,
        });
    }
    let reconciliation_safe = warnings.is_empty();
    Ok(DiscoveryResult {
        files,
        warnings,
        reconciliation_safe,
    })
}

async fn inspect_changed_files(
    pool: &SqlitePool,
    discovery: &mut DiscoveryResult,
) -> Result<(), sqlx::Error> {
    let existing_rows = sqlx::query_as::<_, ExistingFile>(
        "SELECT f.path, f.size_bytes, f.modified_at_unix, m.inspected_at, m.inspection_error
         FROM media_files f LEFT JOIN media m ON m.id = f.media_id",
    )
    .fetch_all(pool)
    .await?;
    let mut existing: HashMap<String, ExistingFileState> =
        HashMap::with_capacity(existing_rows.len());
    for file in existing_rows {
        existing.insert(
            file.path,
            ExistingFileState {
                size_bytes: file.size_bytes,
                modified_at_unix: file.modified_at_unix,
                inspected_at: file.inspected_at,
                inspection_error: file.inspection_error,
            },
        );
    }
    let semaphore = Arc::new(Semaphore::new(2));
    let mut tasks = JoinSet::new();

    for (index, file) in discovery.files.iter().enumerate() {
        let needs_probe = existing.get(&file.path).is_none_or(|known| {
            known.size_bytes != file.size_bytes
                || known.modified_at_unix != file.modified_at_unix
                || known.inspected_at.is_none()
                || known.inspection_error.is_some()
        });
        if !needs_probe {
            continue;
        }
        let path = file.path.clone();
        let semaphore = semaphore.clone();
        tasks.spawn(async move {
            let permit = semaphore.acquire_owned().await;
            let result = match permit {
                Ok(_permit) => probe::inspect(&path).await,
                Err(_) => Err("controle de concorrência do ffprobe indisponível".to_owned()),
            };
            (index, result)
        });
    }

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((index, technical)) => discovery.files[index].technical = Some(technical),
            Err(error) => tracing::error!(%error, "ffprobe task failed"),
        }
    }
    Ok(())
}

async fn persist_discovery(
    pool: &SqlitePool,
    command: ScanCommand,
    discovery: DiscoveryResult,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let scan_is_complete = discovery.reconciliation_safe;
    let source_enabled = sqlx::query_scalar::<_, bool>(
        "SELECT enabled AND removed_at IS NULL FROM media_sources WHERE id = ?",
    )
    .bind(command.source_id)
    .fetch_optional(&mut *transaction)
    .await?
    .unwrap_or(false);
    if !source_enabled {
        sqlx::query("UPDATE scan_jobs SET status = 'cancelled', error_message = 'fonte desabilitada', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(command.job_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        return Ok(());
    }
    if scan_is_complete {
        sqlx::query("UPDATE source_files SET is_available = 0 WHERE source_id = ?")
            .bind(command.source_id)
            .execute(&mut *transaction)
            .await?;
    }

    let processed_count = discovery.files.len() as i64;
    for file in discovery.files {
        let (media_file_id, current_media_id) = sqlx::query_as::<_, (i64, Option<i64>)>(
            "INSERT INTO media_files (path, title, extension, size_bytes, modified_at_unix, is_available)
             VALUES (?, ?, ?, ?, ?, 1)
             ON CONFLICT(path) DO UPDATE SET title = excluded.title, extension = excluded.extension,
                size_bytes = excluded.size_bytes, modified_at_unix = excluded.modified_at_unix,
                is_available = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             RETURNING id, media_id",
        )
        .bind(&file.path)
        .bind(&file.title)
        .bind(&file.extension)
        .bind(file.size_bytes)
        .bind(file.modified_at_unix)
        .fetch_one(&mut *transaction)
        .await?;

        let yt_id = crate::youtube_worker::extract_youtube_id(&file.path);
        let (media_id, update_media_technical) = if let Some(media_id) = current_media_id {
            (media_id, true)
        } else if let Some(ref yid) = yt_id {
            // Verifica se já existe uma mídia com este youtube_id na biblioteca
            let existing_yt = sqlx::query_as::<
                _,
                (
                    i64,
                    Option<i64>,
                    Option<i64>,
                    Option<i64>,
                    Option<i64>,
                    Option<String>,
                ),
            >(
                "SELECT m.id, mf.id, m.width, m.height, mf.size_bytes, mf.video_codec
                 FROM media m
                 LEFT JOIN media_files mf ON mf.media_id = m.id
                 WHERE m.youtube_id = ?
                 ORDER BY mf.id
                 LIMIT 1",
            )
            .bind(yid)
            .fetch_optional(&mut *transaction)
            .await?;

            if let Some((existing_id, existing_file_id, ex_w, ex_h, ex_size, ex_codec)) =
                existing_yt
            {
                let new_tech = file.technical.as_ref().and_then(|t| t.as_ref().ok());
                let new_score = crate::dedup::compute_quality_score(
                    new_tech.and_then(|t| t.width),
                    new_tech.and_then(|t| t.height),
                    file.size_bytes,
                    new_tech.and_then(|t| t.video_codec.as_deref()),
                );
                let existing_score = crate::dedup::compute_quality_score(
                    ex_w,
                    ex_h,
                    ex_size.unwrap_or(0),
                    ex_codec.as_deref(),
                );

                sqlx::query("UPDATE media_files SET media_id = ? WHERE id = ?")
                    .bind(existing_id)
                    .bind(media_file_id)
                    .execute(&mut *transaction)
                    .await?;
                if let Some(existing_file_id) = existing_file_id {
                    let (winner_file_id, duplicate_file_id, winner_score, duplicate_score) =
                        if new_score > existing_score {
                            (media_file_id, existing_file_id, new_score, existing_score)
                        } else {
                            (existing_file_id, media_file_id, existing_score, new_score)
                        };
                    let reason = serde_json::json!({
                        "match": "youtube_id",
                        "youtube_id": yid,
                        "winner_score": winner_score,
                        "duplicate_score": duplicate_score,
                        "rule": "resolution_codec_size"
                    });
                    sqlx::query(
                        "INSERT INTO duplicate_candidates
                            (winner_media_file_id, duplicate_media_file_id, score, reason_json)
                         VALUES (?, ?, ?, ?)
                         ON CONFLICT(winner_media_file_id, duplicate_media_file_id) DO UPDATE SET
                            score = excluded.score, reason_json = excluded.reason_json",
                    )
                    .bind(winner_file_id)
                    .bind(duplicate_file_id)
                    .bind(winner_score.saturating_sub(duplicate_score))
                    .bind(reason.to_string())
                    .execute(&mut *transaction)
                    .await?;
                }
                tracing::info!(media_id = existing_id, path = %file.path, "duplicate file linked without deleting either physical file");
                (existing_id, new_score > existing_score)
            } else {
                let new_id = sqlx::query_scalar::<_, i64>(
                    "INSERT INTO media (title, youtube_id, is_available) VALUES (?, ?, 1) RETURNING id",
                )
                .bind(&file.title)
                .bind(yid)
                .fetch_one(&mut *transaction)
                .await?;
                sqlx::query("UPDATE media_files SET media_id = ? WHERE id = ?")
                    .bind(new_id)
                    .bind(media_file_id)
                    .execute(&mut *transaction)
                    .await?;
                (new_id, true)
            }
        } else {
            let media_id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO media (title, is_available) VALUES (?, 1) RETURNING id",
            )
            .bind(&file.title)
            .fetch_one(&mut *transaction)
            .await?;
            sqlx::query("UPDATE media_files SET media_id = ? WHERE id = ?")
                .bind(media_id)
                .bind(media_file_id)
                .execute(&mut *transaction)
                .await?;
            (media_id, true)
        };
        sqlx::query("UPDATE media SET title = ?, youtube_id = COALESCE(youtube_id, ?), is_available = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(&file.title)
            .bind(&yt_id)
            .bind(media_id)
            .execute(&mut *transaction)
            .await?;

        if let Some(technical) = file.technical {
            match technical {
                Ok(technical) => {
                    let is_short = technical.is_short();
                    let subtitle_codecs = serde_json::to_string(&technical.subtitle_codecs)
                        .unwrap_or_else(|_| "[]".to_owned());
                    let published_at = extract_date_from_probe_json(&technical.raw_json);
                    sqlx::query(
                        "UPDATE media_files SET container = ?, video_codec = ?, audio_codec = ?,
                            subtitle_codecs_json = ?, probe_json = ? WHERE id = ?",
                    )
                    .bind(technical.container)
                    .bind(technical.video_codec)
                    .bind(technical.audio_codec)
                    .bind(subtitle_codecs)
                    .bind(technical.raw_json)
                    .bind(media_file_id)
                    .execute(&mut *transaction)
                    .await?;
                    if update_media_technical {
                        sqlx::query(
                            "UPDATE media SET duration_ms = ?, width = ?, height = ?, rotation = ?,
                            is_short = ?, published_at = COALESCE(?, published_at),
                            inspected_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                            inspection_error = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
                        )
                        .bind(technical.duration_ms)
                        .bind(technical.width)
                        .bind(technical.height)
                        .bind(technical.rotation)
                        .bind(is_short)
                        .bind(published_at)
                        .bind(media_id)
                        .execute(&mut *transaction)
                        .await?;
                    }
                }
                Err(error) => {
                    sqlx::query(
                        "UPDATE media SET inspected_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                            inspection_error = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
                    )
                    .bind(truncate(&error, 1000))
                    .bind(media_id)
                    .execute(&mut *transaction)
                    .await?;
                }
            }
        }

        if let Some(channel) = file.inferred_channel {
            let matched_id = sqlx::query_scalar::<_, String>(
                "SELECT id FROM channels WHERE lower(trim(name)) = lower(trim(?)) OR id = ? LIMIT 1",
            )
            .bind(&channel.name)
            .bind(&channel.id)
            .fetch_optional(&mut *transaction)
            .await?;

            let final_id = if let Some(existing_id) = matched_id {
                sqlx::query(
                    "UPDATE channels SET
                        name = trim(?),
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?",
                )
                .bind(&channel.name)
                .bind(&existing_id)
                .execute(&mut *transaction)
                .await?;
                existing_id
            } else {
                sqlx::query(
                    "INSERT INTO channels (id, name) VALUES (?, trim(?))
                     ON CONFLICT(id) DO UPDATE SET
                        name = trim(excluded.name),
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                )
                .bind(&channel.id)
                .bind(&channel.name)
                .execute(&mut *transaction)
                .await?;
                channel.id.clone()
            };

            sqlx::query(
                "INSERT INTO media_channels (media_id, channel_id, origin) VALUES (?, ?, 'inferred')
                 ON CONFLICT(media_id) DO NOTHING",
            )
            .bind(media_id)
            .bind(&final_id)
            .execute(&mut *transaction)
            .await?;
        }

        sqlx::query(
            "INSERT INTO source_files (source_id, media_file_id, is_available, last_seen_scan_id)
             VALUES (?, ?, 1, ?)
             ON CONFLICT(source_id, media_file_id) DO UPDATE SET is_available = 1, last_seen_scan_id = excluded.last_seen_scan_id",
        )
        .bind(command.source_id)
        .bind(media_file_id)
        .bind(command.job_id)
        .execute(&mut *transaction)
        .await?;
    }

    sqlx::query(
        "UPDATE media_files SET is_available = EXISTS (
            SELECT 1 FROM source_files WHERE source_files.media_file_id = media_files.id AND source_files.is_available = 1
        )",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE media SET is_available = EXISTS (
            SELECT 1 FROM media_files WHERE media_files.media_id = media.id AND media_files.is_available = 1
        )",
    )
    .execute(&mut *transaction)
    .await?;

    // Clean up duplicate channels where 0 media channel exists but another with same name has media
    sqlx::query(
        "DELETE FROM channels
         WHERE id IN (
             SELECT c1.id FROM channels c1
             JOIN channels c2 ON lower(trim(c1.name)) = lower(trim(c2.name)) AND c1.id <> c2.id
             WHERE (SELECT COUNT(*) FROM media_channels WHERE channel_id = c1.id) = 0
               AND (SELECT COUNT(*) FROM media_channels WHERE channel_id = c2.id) > 0
         )",
    )
    .execute(&mut *transaction)
    .await?;

    sqlx::query("UPDATE channels SET name = trim(name) WHERE name <> trim(name)")
        .execute(&mut *transaction)
        .await?;

    crate::catalog::sync_media_fts(&mut transaction).await?;

    let warning_message = (!discovery.warnings.is_empty()).then(|| {
        let omitted = discovery.warnings.len().saturating_sub(5);
        let mut message = discovery
            .warnings
            .into_iter()
            .take(5)
            .collect::<Vec<_>>()
            .join("; ");
        if omitted > 0 {
            message.push_str(&format!("; e mais {omitted} aviso(s)"));
        }
        message
    });
    sqlx::query(
        "UPDATE scan_jobs SET status = 'completed', processed_files = ?, error_message = ?,
            finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(processed_count)
    .bind(&warning_message)
    .bind(command.job_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE media_sources SET status = 'ready', error_message = ?,
            last_scanned_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(warning_message)
    .bind(command.source_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let pool_clone = pool.clone();
    tokio::spawn(async move {
        crate::youtube_worker::sync_pending_channels(&pool_clone).await;
    });

    Ok(())
}

async fn cancel_job(pool: &SqlitePool, job_id: i64, reason: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE scan_jobs SET status = 'cancelled', error_message = ?, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(reason)
        .bind(job_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn fail_job(pool: &SqlitePool, command: ScanCommand, message: &str) {
    let message = truncate(message, 1000);
    if let Err(error) = sqlx::query("UPDATE scan_jobs SET status = 'failed', error_message = ?, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(&message)
        .bind(command.job_id)
        .execute(pool)
        .await {
        tracing::error!(%error, job_id = command.job_id, "failed to persist scan error");
    }
    if let Err(error) = sqlx::query("UPDATE media_sources SET status = 'error', error_message = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND enabled = 1 AND removed_at IS NULL")
        .bind(message)
        .bind(command.source_id)
        .execute(pool)
        .await {
        tracing::error!(%error, source_id = command.source_id, "failed to persist source error");
    }
}

fn extract_date_from_probe_json(raw_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_json).ok()?;
    let tags = v.get("format")?.get("tags")?;
    let raw_date = tags
        .get("date")
        .or_else(|| tags.get("DATE"))
        .or_else(|| tags.get("year"))
        .or_else(|| tags.get("YEAR"))
        .or_else(|| tags.get("creation_time"))
        .and_then(|d| d.as_str())?;

    let s = raw_date.trim();
    if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8]))
    } else if s.len() >= 10 && s.chars().nth(4) == Some('-') && s.chars().nth(7) == Some('-') {
        Some(s[0..10].to_string())
    } else if s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("{s}-01-01"))
    } else {
        None
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn infer_channel(root: &Path, video_path: &Path) -> Option<InferredChannel> {
    let relative_parent = video_path.parent()?.strip_prefix(root).ok()?;
    let name = relative_parent
        .components()
        .next()?
        .as_os_str()
        .to_str()?
        .trim();
    if name.is_empty()
        || matches!(
            name.to_ascii_lowercase().as_str(),
            "shorts" | "videos" | "lives" | "live" | "uploads"
        )
    {
        return None;
    }

    let mut slug = String::new();
    let mut separator_pending = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('_');
            }
            slug.push(character);
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    let id = format!("ch_{slug}");
    crate::validation::is_valid_channel_id(&id).then(|| InferredChannel {
        id,
        name: name.to_owned(),
    })
}

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "mpeg" | "mpg" | "ogv"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_video_extensions_case_insensitively() {
        assert!(is_video_file(Path::new("clip.MP4")));
        assert!(is_video_file(Path::new("filme.mkv")));
        assert!(!is_video_file(Path::new("capa.jpg")));
        assert!(!is_video_file(Path::new(".mediatags.json")));
    }

    #[test]
    fn infers_only_a_first_level_channel_folder() {
        let root = Path::new("/library");
        let inferred = infer_channel(root, Path::new("/library/Example Creator/Shorts/a.mp4"))
            .expect("channel should be inferred");
        assert_eq!(inferred.id, "ch_example_creator");
        assert_eq!(inferred.name, "Example Creator");
        assert!(infer_channel(root, Path::new("/library/video.mp4")).is_none());
        assert!(infer_channel(root, Path::new("/library/Shorts/a.mp4")).is_none());
    }

    #[tokio::test]
    async fn scan_queue_returns_predictable_backpressure_when_full() {
        let (sender, _receiver) = mpsc::channel(1);
        let queue = ScanQueue { sender };
        queue.enqueue(1, 10).await.unwrap();
        let error = queue.enqueue(2, 10).await.unwrap_err();
        assert!(error.contains("cheia"));
    }

    #[tokio::test]
    async fn duplicate_scan_never_deletes_physical_copies() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let temp_dir = std::env::temp_dir().join(format!(
            "mymedia_scan_duplicate_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let first = temp_dir.join("first [abcdefghijk].mp4");
        let second = temp_dir.join("second [abcdefghijk].mp4");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();

        let source_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_sources (path, recursive) VALUES (?, 1) RETURNING id",
        )
        .bind(temp_dir.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        let job_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO scan_jobs (source_id, status) VALUES (?, 'running') RETURNING id",
        )
        .bind(source_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let file = |path: &Path, title: &str| DiscoveredFile {
            path: path.to_string_lossy().into_owned(),
            title: title.to_owned(),
            extension: "mp4".to_owned(),
            size_bytes: std::fs::metadata(path).unwrap().len() as i64,
            modified_at_unix: 1,
            technical: None,
            inferred_channel: None,
        };
        let discovery = DiscoveryResult {
            files: vec![file(&first, "first"), file(&second, "second")],
            warnings: Vec::new(),
            reconciliation_safe: true,
        };

        persist_discovery(&pool, ScanCommand { job_id, source_id }, discovery)
            .await
            .unwrap();

        assert!(first.is_file());
        assert!(second.is_file());
        let linked = sqlx::query_scalar::<_, i64>(
            "SELECT count(DISTINCT mf.id) FROM media_files mf
             JOIN media m ON m.id = mf.media_id WHERE m.youtube_id = 'abcdefghijk'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(linked, 2);
        let proposal =
            sqlx::query_as::<_, (String, i64)>("SELECT status, score FROM duplicate_candidates")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(proposal.0, "pending");
        assert!(proposal.1 >= 0);
        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
