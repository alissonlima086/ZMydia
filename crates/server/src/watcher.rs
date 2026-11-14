use std::{collections::HashMap, path::Path, time::Duration};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use sqlx::{FromRow, SqlitePool};
use tokio::{
    sync::mpsc,
    time::{Instant, interval},
};

use crate::scanner::ScanQueue;

const DEBOUNCE: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct WatcherService {
    sender: mpsc::Sender<Command>,
}

#[derive(Debug)]
enum Command {
    Refresh,
    FileEvent(i64),
}

#[derive(Debug, FromRow)]
struct WatchedSource {
    id: i64,
    path: String,
    recursive: bool,
}

impl WatcherService {
    pub fn start(pool: SqlitePool, queue: ScanQueue) -> Self {
        let (sender, receiver) = mpsc::channel(256);
        let service = Self {
            sender: sender.clone(),
        };
        tokio::spawn(run(pool, queue, sender, receiver));
        let _ = service.sender.try_send(Command::Refresh);
        service
    }

    pub async fn refresh(&self) {
        if self.sender.send(Command::Refresh).await.is_err() {
            tracing::error!("filesystem watcher service is unavailable");
        }
    }
}

async fn run(
    pool: SqlitePool,
    queue: ScanQueue,
    sender: mpsc::Sender<Command>,
    mut receiver: mpsc::Receiver<Command>,
) {
    let mut watchers: HashMap<i64, RecommendedWatcher> = HashMap::new();
    let mut pending: HashMap<i64, Instant> = HashMap::new();
    let mut ticker = interval(Duration::from_millis(500));

    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else { break };
                match command {
                    Command::Refresh => {
                        pending.clear();
                        match load_watchers(&pool, &sender).await {
                            Ok(next) => watchers = next,
                            Err(error) => tracing::error!(%error, "failed to refresh filesystem watchers"),
                        }
                    }
                    Command::FileEvent(source_id) => {
                        pending.insert(source_id, Instant::now() + DEBOUNCE);
                    }
                }
            }
            _ = ticker.tick() => {
                let now = Instant::now();
                let due = pending
                    .iter()
                    .filter_map(|(source_id, deadline)| (*deadline <= now).then_some(*source_id))
                    .collect::<Vec<_>>();
                for source_id in due {
                    pending.remove(&source_id);
                    if let Err(error) = request_scan(&pool, &queue, source_id).await {
                        tracing::error!(source_id, %error, "failed to enqueue scan from filesystem event");
                    }
                }
            }
        }
    }

    drop(watchers);
}

async fn load_watchers(
    pool: &SqlitePool,
    sender: &mpsc::Sender<Command>,
) -> Result<HashMap<i64, RecommendedWatcher>, sqlx::Error> {
    let sources = sqlx::query_as::<_, WatchedSource>(
        "SELECT id, path, recursive FROM media_sources WHERE enabled = 1 AND removed_at IS NULL ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    let mut watchers = HashMap::with_capacity(sources.len());

    for source in sources {
        let event_sender = sender.clone();
        let source_id = source.id;
        let watcher = RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event) => {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => {
                            if let EventKind::Modify(notify::event::ModifyKind::Metadata(
                                notify::event::MetadataKind::AccessTime,
                            )) = event.kind
                            {
                                return;
                            }
                            if event_sender
                                .try_send(Command::FileEvent(source_id))
                                .is_err()
                            {
                                tracing::warn!(source_id, "filesystem event queue is full");
                            }
                        }
                        _ => {}
                    }
                }
                Err(error) => tracing::warn!(source_id, %error, "filesystem watcher error"),
            },
            Config::default(),
        );
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(source_id, %error, "failed to create filesystem watcher");
                continue;
            }
        };
        let mode = if source.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(error) = watcher.watch(Path::new(&source.path), mode) {
            tracing::warn!(source_id, path = %source.path, %error, "failed to watch media source");
            continue;
        }
        watchers.insert(source_id, watcher);
    }

    tracing::info!(sources = watchers.len(), "filesystem watchers refreshed");
    Ok(watchers)
}

async fn request_scan(
    pool: &SqlitePool,
    queue: &ScanQueue,
    source_id: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut transaction = pool.begin().await?;
    let active_job = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM scan_jobs WHERE source_id = ? AND status IN ('queued', 'running') ORDER BY id LIMIT 1",
    )
    .bind(source_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if active_job.is_some() {
        transaction.commit().await?;
        return Ok(());
    }

    let enabled = sqlx::query_scalar::<_, bool>(
        "SELECT enabled AND removed_at IS NULL FROM media_sources WHERE id = ?",
    )
    .bind(source_id)
    .fetch_optional(&mut *transaction)
    .await?
    .unwrap_or(false);
    if !enabled {
        transaction.commit().await?;
        return Ok(());
    }

    let job_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO scan_jobs (source_id, status) VALUES (?, 'queued') RETURNING id",
    )
    .bind(source_id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("UPDATE media_sources SET status = 'pending', error_message = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(source_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    queue.enqueue(job_id, source_id).await?;
    Ok(())
}
