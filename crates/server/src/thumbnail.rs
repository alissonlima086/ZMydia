use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Path as AxumPath, State},
    http::{Response, StatusCode, header},
    response::IntoResponse,
};
use sqlx::SqlitePool;
use tokio::{
    process::Command,
    sync::{Mutex, mpsc},
    time::timeout,
};

use crate::web::AppState;

const PLACEHOLDER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360" viewBox="0 0 640 360"><rect width="640" height="360" fill="#17191e"/><path d="M285 135v90l80-45z" fill="#555b66"/></svg>"##;
const THUMBNAIL_MEMORY_CACHE_BYTES: usize = 64 * 1024 * 1024;
const THUMBNAIL_PREWARM_LIMIT: i64 = 256;

#[derive(Debug)]
struct ThumbnailMemoryCache {
    entries: HashMap<String, Bytes>,
    order: VecDeque<String>,
    total_bytes: usize,
    max_bytes: usize,
}

impl ThumbnailMemoryCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
        }
    }

    fn get(&mut self, key: &str) -> Option<Bytes> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|candidate| candidate != key);
        self.order.push_back(key.to_owned());
        Some(value)
    }

    fn insert(&mut self, key: String, value: Bytes) {
        if value.len() > self.max_bytes {
            return;
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.len());
        }
        self.order.retain(|candidate| candidate != &key);
        self.total_bytes += value.len();
        self.order.push_back(key.clone());
        self.entries.insert(key, value);

        while self.total_bytes > self.max_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.total_bytes = self.total_bytes.saturating_sub(removed.len());
            }
        }
    }
}

#[derive(Clone)]
pub struct ThumbnailService {
    sender: mpsc::Sender<Job>,
    pending: Arc<Mutex<HashSet<i64>>>,
    cache_dir: Arc<PathBuf>,
    memory_cache: Arc<Mutex<ThumbnailMemoryCache>>,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub media_id: i64,
    pub path: String,
    pub duration_ms: Option<i64>,
    pub cache_key: String,
    pub source_path: Option<String>,
}

impl ThumbnailService {
    pub fn start(pool: SqlitePool, cache_dir: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel(2048);
        let receiver = Arc::new(Mutex::new(receiver));
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let cache_dir = Arc::new(cache_dir);
        let memory_cache = Arc::new(Mutex::new(ThumbnailMemoryCache::new(
            THUMBNAIL_MEMORY_CACHE_BYTES,
        )));
        if let Err(error) = std::fs::create_dir_all(&*cache_dir) {
            tracing::error!(%error, path = %cache_dir.display(), "failed to prepare thumbnail cache");
        }
        for _ in 0..2 {
            tokio::spawn(worker(
                pool.clone(),
                receiver.clone(),
                pending.clone(),
                cache_dir.clone(),
                memory_cache.clone(),
            ));
        }
        let service = Self {
            sender,
            pending,
            cache_dir: cache_dir.clone(),
            memory_cache: memory_cache.clone(),
        };

        let service_clone = service.clone();
        let pool_clone = pool.clone();
        let cache_clone = cache_dir.clone();
        tokio::spawn(async move {
            prune_thumbnail_cache(&cache_clone).await;
            prewarm_thumbnail_cache(&pool_clone, &cache_clone, &memory_cache).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
            let sources = sqlx::query_scalar::<_, i64>(
                "SELECT id FROM media_sources WHERE enabled = 1 AND removed_at IS NULL",
            )
            .fetch_all(&pool_clone)
            .await
            .unwrap_or_default();
            for source_id in sources {
                service_clone.enqueue_source(source_id, &pool_clone).await;
            }
        });

        service
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{key}.jpg"))
    }

    pub async fn enqueue(&self, job: Job) {
        let mut pending = self.pending.lock().await;
        if pending.insert(job.media_id) {
            drop(pending);
            if self.sender.send(job.clone()).await.is_err() {
                self.pending.lock().await.remove(&job.media_id);
            }
        }
    }

    pub async fn enqueue_source(&self, source_id: i64, pool: &SqlitePool) {
        let rows = sqlx::query_as::<_, (i64, String, i64, i64, Option<i64>, String)>(
            "SELECT m.id, f.path, f.size_bytes, f.modified_at_unix, m.duration_ms, s.path as source_path
             FROM media m
             JOIN media_files f ON f.media_id = m.id
             JOIN source_files sf ON sf.media_file_id = f.id
             JOIN media_sources s ON s.id = sf.source_id
             WHERE s.id = ? AND m.is_available = 1 AND f.is_available = 1
             ORDER BY m.id DESC",
        )
        .bind(source_id)
        .fetch_all(pool)
        .await;

        let Ok(rows) = rows else { return };

        for (media_id, path, size, mtime, duration_ms, source_path) in rows {
            let cache_key = format!("{media_id}-{size}-{mtime}");
            if find_existing_thumbnail(&path, Some(&source_path), &cache_key, &self.cache_dir)
                .is_some()
            {
                continue;
            }
            self.enqueue(Job {
                media_id,
                path,
                duration_ms,
                cache_key,
                source_path: Some(source_path),
            })
            .await;
        }
    }
}

pub fn resolve_thumbnail_dir(_job: &Job, cache_dir: &Path) -> PathBuf {
    let _ = std::fs::create_dir_all(cache_dir);
    cache_dir.to_path_buf()
}

pub fn find_existing_thumbnail(
    media_path: &str,
    source_path: Option<&str>,
    cache_key: &str,
    cache_dir: &Path,
) -> Option<PathBuf> {
    let filename = format!("{cache_key}.jpg");
    // 1. Check in fast local cache_dir first
    let local = cache_dir.join(&filename);
    if local.is_file() {
        return Some(local);
    }
    // 2. Check in source_path/.mymedia/thumbnails/
    if let Some(src) = source_path {
        let p = Path::new(src)
            .join(".mymedia")
            .join("thumbnails")
            .join(&filename);
        if p.is_file() {
            return Some(p);
        }
    }
    // 3. Check in file's parent/.mymedia/thumbnails/
    if let Some(parent) = Path::new(media_path).parent() {
        let p = parent.join(".mymedia").join("thumbnails").join(&filename);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub(crate) async fn get_thumbnail(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Response<Body> {
    let row = sqlx::query_as::<_, (String, i64, i64, Option<i64>, Option<String>)>(
        "SELECT f.path, f.size_bytes, f.modified_at_unix, m.duration_ms, s.path as source_path
         FROM media m
         JOIN media_files f ON f.media_id = m.id
         LEFT JOIN source_files sf ON sf.media_file_id = f.id
         LEFT JOIN media_sources s ON s.id = sf.source_id
         WHERE m.id = ? AND f.is_available = 1
         ORDER BY f.id LIMIT 1",
    )
    .bind(media_id)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((path, size, mtime, duration_ms, source_path))) = row else {
        return (StatusCode::NOT_FOUND, "Miniatura indisponível").into_response();
    };

    let cache_key = format!("{media_id}-{size}-{mtime}");
    if let Some(bytes) = state.thumbnails.memory_cache.lock().await.get(&cache_key) {
        return thumbnail_response(bytes);
    }
    if let Some(thumb_path) = find_existing_thumbnail(
        &path,
        source_path.as_deref(),
        &cache_key,
        &state.thumbnails.cache_dir,
    ) && let Ok(bytes) = tokio::fs::read(&thumb_path).await
    {
        let bytes = Bytes::from(bytes);
        state
            .thumbnails
            .memory_cache
            .lock()
            .await
            .insert(cache_key.clone(), bytes.clone());
        return thumbnail_response(bytes);
    }

    state
        .thumbnails
        .enqueue(Job {
            media_id,
            path,
            duration_ms,
            cache_key,
            source_path,
        })
        .await;

    Response::builder()
        .status(StatusCode::ACCEPTED)
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .header(header::RETRY_AFTER, "2")
        .body(Body::from(PLACEHOLDER))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn thumbnail_response(bytes: Bytes) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(
            header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn worker(
    pool: SqlitePool,
    receiver: Arc<Mutex<mpsc::Receiver<Job>>>,
    pending: Arc<Mutex<HashSet<i64>>>,
    cache_dir: Arc<PathBuf>,
    memory_cache: Arc<Mutex<ThumbnailMemoryCache>>,
) {
    loop {
        let job = { receiver.lock().await.recv().await };
        let Some(job) = job else { break };
        let result = generate(&job, &cache_dir).await;
        if result.is_ok()
            && let Ok(bytes) =
                tokio::fs::read(cache_dir.join(format!("{}.jpg", job.cache_key))).await
        {
            memory_cache
                .lock()
                .await
                .insert(job.cache_key.clone(), Bytes::from(bytes));
        }
        let (status, error) = match result {
            Ok(()) => ("ready", None),
            Err(error) => ("error", Some(error)),
        };
        if let Err(db_error) = sqlx::query(
            "INSERT INTO thumbnails (media_id, cache_key, status, error_message, is_official, source) VALUES (?, ?, ?, ?, 0, 'generated')
             ON CONFLICT(media_id) DO UPDATE SET cache_key=excluded.cache_key, status=excluded.status,
                error_message=excluded.error_message, is_official=0, source='generated', updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        )
        .bind(job.media_id)
        .bind(&job.cache_key)
        .bind(status)
        .bind(error)
        .execute(&pool)
        .await {
            tracing::error!(%db_error, media_id = job.media_id, "failed to persist thumbnail state");
        }
        pending.lock().await.remove(&job.media_id);
    }
}

async fn generate(job: &Job, cache_dir: &Path) -> Result<(), String> {
    let canonical_path = std::fs::canonicalize(&job.path)
        .map_err(|e| format!("arquivo inacessível para miniatura: {e}"))?;
    if !canonical_path.is_file() {
        return Err("caminho de mídia não é um arquivo regular".to_owned());
    }
    let output_dir = resolve_thumbnail_dir(job, cache_dir);
    let output = output_dir.join(format!("{}.jpg", job.cache_key));
    let cache_path = cache_dir.join(format!("{}.jpg", job.cache_key));
    let timestamps = candidate_seek_points(job.duration_ms);

    let mut last_error = None;
    let mut generated_at_least_one = false;

    for seek_sec in timestamps {
        let seek_str = format!("{seek_sec:.3}");
        let process = Command::new(crate::config::external_command(
            "MYMEDIA_FFMPEG_BIN",
            "ffmpeg",
        ))
        .args(["-v", "info", "-ss", &seek_str, "-threads", "1", "-i"])
        .arg(&canonical_path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=640:-2,blackframe=amount=95:threshold=32",
            "-q:v",
            "3",
            "-y",
        ])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();

        let result = timeout(Duration::from_secs(10), process)
            .await
            .map_err(|_| "timeout do FFmpeg".to_owned())?
            .map_err(|error| error.to_string())?;

        if result.status.success() {
            generated_at_least_one = true;
            let stderr_str = String::from_utf8_lossy(&result.stderr);
            if is_black_frame(&stderr_str) {
                tracing::debug!(
                    media_id = job.media_id,
                    seek = %seek_str,
                    "frame escuro/preto detectado na miniatura; tentando ponto alternativo"
                );
                continue;
            }
            if output != cache_path {
                tokio::fs::create_dir_all(cache_dir)
                    .await
                    .map_err(|error| format!("falha ao preparar cache de miniaturas: {error}"))?;
                tokio::fs::copy(&output, &cache_path)
                    .await
                    .map_err(|error| format!("falha ao copiar miniatura para o cache: {error}"))?;
            }
            return Ok(());
        } else {
            last_error = Some(String::from_utf8_lossy(&result.stderr).trim().to_owned());
        }
    }

    if generated_at_least_one && output.is_file() {
        if output != cache_path {
            tokio::fs::create_dir_all(cache_dir)
                .await
                .map_err(|error| format!("falha ao preparar cache de miniaturas: {error}"))?;
            tokio::fs::copy(&output, &cache_path)
                .await
                .map_err(|error| format!("falha ao copiar miniatura para o cache: {error}"))?;
        }
        Ok(())
    } else {
        Err(last_error.unwrap_or_else(|| "falha ao gerar miniatura com FFmpeg".to_owned()))
    }
}

async fn prune_thumbnail_cache(cache_dir: &Path) {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(Duration::from_secs(30 * 24 * 60 * 60))
        .unwrap_or(std::time::UNIX_EPOCH);
    let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await else {
        return;
    };
    let mut removed = 0_u64;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jpg") {
            continue;
        }
        let stale = entry
            .metadata()
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .is_some_and(|modified| modified < cutoff);
        if stale && tokio::fs::remove_file(&path).await.is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(removed, "expired thumbnails removed from cache");
    }
}

async fn prewarm_thumbnail_cache(
    pool: &SqlitePool,
    cache_dir: &Path,
    memory_cache: &Arc<Mutex<ThumbnailMemoryCache>>,
) {
    let rows = sqlx::query_as::<_, (i64, String, i64, i64, Option<String>)>(
        "SELECT m.id, f.path, f.size_bytes, f.modified_at_unix, s.path AS source_path
         FROM media m
         JOIN media_files f ON f.media_id = m.id
         LEFT JOIN source_files sf ON sf.media_file_id = f.id
         LEFT JOIN media_sources s ON s.id = sf.source_id
         LEFT JOIN favorites fav ON fav.media_id = m.id
         LEFT JOIN playback_state ps ON ps.media_id = m.id
         WHERE m.is_available = 1 AND f.is_available = 1
         GROUP BY m.id
         ORDER BY (fav.media_id IS NOT NULL) DESC,
                  COALESCE(ps.updated_at, '') DESC,
                  m.id DESC
         LIMIT ?",
    )
    .bind(THUMBNAIL_PREWARM_LIMIT)
    .fetch_all(pool)
    .await;

    let Ok(rows) = rows else {
        return;
    };
    let mut loaded = 0_u64;
    for (media_id, media_path, size, mtime, source_path) in rows {
        let cache_key = format!("{media_id}-{size}-{mtime}");
        let Some(path) =
            find_existing_thumbnail(&media_path, source_path.as_deref(), &cache_key, cache_dir)
        else {
            continue;
        };
        let Ok(bytes) = tokio::fs::read(path).await else {
            continue;
        };
        memory_cache
            .lock()
            .await
            .insert(cache_key, Bytes::from(bytes));
        loaded += 1;
    }
    if loaded > 0 {
        tracing::info!(loaded, "thumbnail cache warmed in memory");
    }
}

pub(crate) fn candidate_seek_points(duration_ms: Option<i64>) -> Vec<f64> {
    match duration_ms {
        Some(ms) if ms > 0 => {
            let dur = ms as f64 / 1000.0;
            if dur <= 3.0 {
                vec![dur * 0.3, dur * 0.6, dur * 0.1]
            } else {
                vec![dur * 0.30, dur * 0.15, dur * 0.50]
            }
        }
        _ => vec![1.0, 0.5, 2.0],
    }
}

pub(crate) fn is_black_frame(stderr: &str) -> bool {
    stderr.contains("Parsed_blackframe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_candidate_seek_points() {
        let points = candidate_seek_points(Some(100_000));
        assert_eq!(points.len(), 3);
        assert!((points[0] - 30.0).abs() < f64::EPSILON);
        assert!((points[1] - 15.0).abs() < f64::EPSILON);
        assert!((points[2] - 50.0).abs() < f64::EPSILON);

        let fallback = candidate_seek_points(None);
        assert_eq!(fallback, vec![1.0, 0.5, 2.0]);
    }

    #[test]
    fn detects_black_frame_from_ffmpeg_output() {
        let sample_black =
            "[Parsed_blackframe_1 @ 0x76f8fc010cc0] frame:0 pblack:100 pts:0 t:0.000000";
        assert!(is_black_frame(sample_black));

        let sample_normal = "frame=    1 fps=0.0 q=3.0 Lsize=N/A time=00:00:00.04 bitrate=N/A";
        assert!(!is_black_frame(sample_normal));
    }

    #[test]
    fn resolves_thumbnail_dir_and_finds_existing() {
        let temp_dir = std::env::temp_dir().join(format!("mymedia_test_{}", std::process::id()));
        let source_dir = temp_dir.join("source");
        let cache_dir = temp_dir.join("cache");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&cache_dir).unwrap();

        let video_path = source_dir.join("clip.mp4");
        std::fs::write(&video_path, b"test").unwrap();

        let job = Job {
            media_id: 1,
            path: video_path.to_str().unwrap().to_owned(),
            duration_ms: Some(10_000),
            cache_key: "1-100-200".to_owned(),
            source_path: Some(source_dir.to_str().unwrap().to_owned()),
        };

        let resolved_dir = resolve_thumbnail_dir(&job, &cache_dir);
        assert_eq!(resolved_dir, cache_dir);
        assert!(resolved_dir.is_dir());

        let thumb_file = resolved_dir.join("1-100-200.jpg");
        std::fs::write(&thumb_file, b"jpgdata").unwrap();

        let found = find_existing_thumbnail(
            video_path.to_str().unwrap(),
            Some(source_dir.to_str().unwrap()),
            "1-100-200",
            &cache_dir,
        );
        assert_eq!(found, Some(thumb_file));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn memory_cache_evicts_oldest_entries_within_budget() {
        let mut cache = ThumbnailMemoryCache::new(6);
        cache.insert("first".to_owned(), Bytes::from_static(b"123"));
        cache.insert("second".to_owned(), Bytes::from_static(b"456"));
        assert!(cache.get("first").is_some());

        cache.insert("third".to_owned(), Bytes::from_static(b"789"));

        assert!(cache.get("first").is_some());
        assert!(cache.get("second").is_none());
        assert!(cache.get("third").is_some());
        assert!(cache.total_bytes <= cache.max_bytes);
    }
}
