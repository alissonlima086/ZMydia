use std::path::{Path, PathBuf};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, Transaction};

use crate::web::AppState;

#[derive(Debug, Serialize, FromRow)]
pub struct MediaSource {
    id: i64,
    path: String,
    recursive: bool,
    enabled: bool,
    mode: String,
    last_scanned_at: Option<String>,
    status: String,
    error_message: Option<String>,
    created_at: String,
    updated_at: String,
    file_count: i64,
    #[sqlx(default)]
    available: bool,
    #[sqlx(default)]
    readable: bool,
    #[sqlx(default)]
    writable: bool,
    #[sqlx(default)]
    free_space_bytes: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ScanJob {
    id: i64,
    source_id: Option<i64>,
    status: String,
    discovered_files: i64,
    processed_files: i64,
    error_message: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSource {
    path: String,
    #[serde(default = "default_true")]
    recursive: bool,
    #[serde(default)]
    mode: SourceMode,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSource {
    path: Option<String>,
    recursive: Option<bool>,
    enabled: Option<bool>,
    mode: Option<SourceMode>,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    #[default]
    ReadOnly,
    Managed,
}

impl SourceMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Managed => "managed",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SourceMutation {
    source: MediaSource,
    scan_job_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ScanBatch {
    jobs: Vec<ScanJob>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<MediaSource>>, ApiError> {
    let mut sources = sqlx::query_as::<_, MediaSource>(SOURCE_SELECT_ALL)
        .fetch_all(&state.pool)
        .await?;
    for source in &mut sources {
        source.refresh_diagnostics();
    }
    Ok(Json(sources))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateSource>,
) -> Result<(StatusCode, Json<SourceMutation>), ApiError> {
    let path = validate_source_path(&input.path)?;
    let mut transaction = state.pool.begin().await?;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO media_sources (path, recursive, enabled, mode, status)
         VALUES (?, ?, 1, ?, 'pending')
         ON CONFLICT(path) DO UPDATE SET recursive = excluded.recursive, enabled = 1,
            mode = excluded.mode, status = 'pending', error_message = NULL, removed_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE media_sources.removed_at IS NOT NULL
         RETURNING id",
    )
    .bind(&path)
    .bind(input.recursive)
    .bind(input.mode.as_str())
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| ApiError::conflict("essa fonte já está cadastrada"))?;
    let (job_id, created) = insert_or_find_job(&mut transaction, id).await?;
    transaction.commit().await?;
    state.watchers.refresh().await;
    state.refresh_authorized_roots().await;
    if created {
        state
            .queue
            .enqueue(job_id, id)
            .await
            .map_err(ApiError::internal)?;
    }

    let source = fetch_source(&state, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(SourceMutation {
            source,
            scan_job_id: Some(job_id),
        }),
    ))
}

pub(crate) async fn update(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(input): Json<UpdateSource>,
) -> Result<Json<SourceMutation>, ApiError> {
    let current = fetch_source(&state, id).await?;
    let path = input
        .path
        .as_deref()
        .map(validate_source_path)
        .transpose()?
        .unwrap_or(current.path);
    let recursive = input.recursive.unwrap_or(current.recursive);
    let enabled = input.enabled.unwrap_or(current.enabled);
    let mode = input.mode.map(SourceMode::as_str).unwrap_or(&current.mode);
    let status = if enabled { "pending" } else { "disabled" };

    let result = sqlx::query(
        "UPDATE media_sources SET path = ?, recursive = ?, enabled = ?, mode = ?, status = ?,
            error_message = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND removed_at IS NULL",
    )
    .bind(path)
    .bind(recursive)
    .bind(enabled)
    .bind(mode)
    .bind(status)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(map_unique_conflict)?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("fonte não encontrada"));
    }
    state.watchers.refresh().await;
    state.refresh_authorized_roots().await;

    let scan_job_id = if enabled {
        Some(enqueue_source_after_update(&state, id).await?)
    } else {
        disable_source_files(&state, id).await?;
        None
    };
    let source = fetch_source(&state, id).await?;
    Ok(Json(SourceMutation {
        source,
        scan_job_id,
    }))
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let mut transaction = state.pool.begin().await?;
    let result = sqlx::query(
        "UPDATE media_sources SET enabled = 0, status = 'disabled', removed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND removed_at IS NULL",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("fonte não encontrada"));
    }
    sqlx::query("UPDATE source_files SET is_available = 0 WHERE source_id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE scan_jobs SET status = 'cancelled', error_message = 'fonte removida', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE source_id = ? AND status = 'queued'")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    refresh_availability(&mut transaction).await?;
    transaction.commit().await?;
    state.watchers.refresh().await;
    state.refresh_authorized_roots().await;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn scan_source(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<(StatusCode, Json<ScanJob>), ApiError> {
    let source = fetch_source(&state, id).await?;
    if !source.enabled {
        return Err(ApiError::conflict("ative a fonte antes de iniciar um scan"));
    }
    let job_id = enqueue_source(&state, id).await?;
    let job = fetch_job(&state, job_id).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub(crate) async fn scan_all(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<ScanBatch>), ApiError> {
    let source_ids = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM media_sources WHERE enabled = 1 AND removed_at IS NULL ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut jobs = Vec::with_capacity(source_ids.len());
    for source_id in source_ids {
        let job_id = enqueue_source(&state, source_id).await?;
        jobs.push(fetch_job(&state, job_id).await?);
    }
    Ok((StatusCode::ACCEPTED, Json(ScanBatch { jobs })))
}

pub(crate) async fn get_job(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<ScanJob>, ApiError> {
    Ok(Json(fetch_job(&state, id).await?))
}

async fn fetch_source(state: &AppState, id: i64) -> Result<MediaSource, ApiError> {
    let mut source = sqlx::query_as::<_, MediaSource>(SOURCE_SELECT_ONE)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("fonte não encontrada"))?;
    source.refresh_diagnostics();
    Ok(source)
}

impl MediaSource {
    fn refresh_diagnostics(&mut self) {
        let path = Path::new(&self.path);
        self.available = path.is_dir();
        self.readable = self.available && std::fs::read_dir(path).is_ok();
        self.writable = self.available
            && std::fs::metadata(path)
                .map(|metadata| !metadata.permissions().readonly())
                .unwrap_or(false);
        self.free_space_bytes = self
            .available
            .then(|| {
                fs2::available_space(path)
                    .ok()
                    .and_then(|bytes| i64::try_from(bytes).ok())
            })
            .flatten();
    }
}

async fn fetch_job(state: &AppState, id: i64) -> Result<ScanJob, ApiError> {
    sqlx::query_as::<_, ScanJob>(
        "SELECT id, source_id, status, discovered_files, processed_files, error_message,
            created_at, started_at, finished_at FROM scan_jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("scan não encontrado"))
}

async fn enqueue_source(state: &AppState, source_id: i64) -> Result<i64, ApiError> {
    let mut transaction = state.pool.begin().await?;
    let (job_id, created) = insert_or_find_job(&mut transaction, source_id).await?;
    if created {
        sqlx::query("UPDATE media_sources SET status = 'pending', error_message = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(source_id)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    if created {
        state
            .queue
            .enqueue(job_id, source_id)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(job_id)
}

async fn enqueue_source_after_update(state: &AppState, source_id: i64) -> Result<i64, ApiError> {
    let mut transaction = state.pool.begin().await?;
    let existing_queued = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM scan_jobs WHERE source_id = ? AND status = 'queued' ORDER BY id LIMIT 1",
    )
    .bind(source_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let (job_id, created) = if let Some(id) = existing_queued {
        (id, false)
    } else {
        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO scan_jobs (source_id, status) VALUES (?, 'queued') RETURNING id",
        )
        .bind(source_id)
        .fetch_one(&mut *transaction)
        .await?;
        (id, true)
    };
    transaction.commit().await?;
    if created {
        state
            .queue
            .enqueue(job_id, source_id)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(job_id)
}

async fn insert_or_find_job(
    transaction: &mut Transaction<'_, Sqlite>,
    source_id: i64,
) -> Result<(i64, bool), sqlx::Error> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM scan_jobs WHERE source_id = ? AND status IN ('queued', 'running') ORDER BY id LIMIT 1",
    )
    .bind(source_id)
    .fetch_optional(&mut **transaction)
    .await?
    {
        return Ok((id, false));
    }
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO scan_jobs (source_id, status) VALUES (?, 'queued') RETURNING id",
    )
    .bind(source_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok((id, true))
}

async fn disable_source_files(state: &AppState, source_id: i64) -> Result<(), ApiError> {
    let mut transaction = state.pool.begin().await?;
    sqlx::query("UPDATE source_files SET is_available = 0 WHERE source_id = ?")
        .bind(source_id)
        .execute(&mut *transaction)
        .await?;
    refresh_availability(&mut transaction).await?;
    transaction.commit().await?;
    Ok(())
}

async fn refresh_availability(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE media_files SET is_available = EXISTS (
            SELECT 1 FROM source_files WHERE source_files.media_file_id = media_files.id AND source_files.is_available = 1
        )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE media SET is_available = EXISTS (
            SELECT 1 FROM media_files WHERE media_files.media_id = media.id AND media_files.is_available = 1
        )",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_source_path(input: &str) -> Result<String, ApiError> {
    if input.trim().is_empty() {
        return Err(ApiError::validation("informe o caminho da fonte"));
    }
    let path = PathBuf::from(input);
    if !path.is_absolute() {
        return Err(ApiError::validation("o caminho precisa ser absoluto"));
    }
    let symlink_metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        ApiError::validation(format!("não foi possível acessar a fonte: {error}"))
    })?;
    if symlink_metadata.file_type().is_symlink() {
        return Err(ApiError::validation(
            "a raiz da fonte não pode ser um link simbólico",
        ));
    }
    if !symlink_metadata.is_dir() {
        return Err(ApiError::validation("a fonte precisa ser um diretório"));
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        ApiError::validation(format!("não foi possível validar a fonte: {error}"))
    })?;
    path_to_string(&canonical)
}

fn path_to_string(path: &Path) -> Result<String, ApiError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ApiError::validation("caminhos não-UTF-8 ainda não são suportados"))
}

fn map_unique_conflict(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|database_error| database_error.is_unique_violation())
    {
        ApiError::conflict("já existe uma fonte com esse caminho")
    } else {
        ApiError::from(error)
    }
}

const SOURCE_SELECT_ALL: &str =
    "SELECT s.id, s.path, s.recursive, s.enabled, s.mode, s.last_scanned_at, s.status, s.error_message,
        s.created_at, s.updated_at,
        (SELECT COUNT(*) FROM source_files sf WHERE sf.source_id = s.id AND sf.is_available = 1) AS file_count
     FROM media_sources s WHERE s.removed_at IS NULL ORDER BY s.path COLLATE NOCASE";

const SOURCE_SELECT_ONE: &str =
    "SELECT s.id, s.path, s.recursive, s.enabled, s.mode, s.last_scanned_at, s.status, s.error_message,
        s.created_at, s.updated_at,
        (SELECT COUNT(*) FROM source_files sf WHERE sf.source_id = s.id AND sf.is_available = 1) AS file_count
     FROM media_sources s WHERE s.id = ? AND s.removed_at IS NULL";

const fn default_true() -> bool {
    true
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        let message = message.into();
        tracing::error!(%message, "internal API error");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "erro interno do servidor".to_owned(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "erro interno do servidor".to_owned(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(crate::error::error_json(
                if self.status == StatusCode::NOT_FOUND {
                    "not_found"
                } else if self.status == StatusCode::CONFLICT {
                    "conflict"
                } else if self.status.is_client_error() {
                    "invalid_request"
                } else {
                    "internal_error"
                },
                self.message,
            )),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_source_paths() {
        let error = validate_source_path("videos").expect_err("relative path should fail");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn new_sources_default_to_read_only() {
        let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mode = sqlx::query_scalar::<_, String>(
            "INSERT INTO media_sources (path) VALUES ('/tmp/read-only-source') RETURNING mode",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mode, "read_only");
    }

    #[test]
    fn source_diagnostics_report_access_and_space() {
        let path =
            std::env::temp_dir().join(format!("mymedia-source-diagnostics-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let mut source = MediaSource {
            id: 1,
            path: path.to_string_lossy().into_owned(),
            recursive: true,
            enabled: true,
            mode: "managed".to_owned(),
            last_scanned_at: None,
            status: "ready".to_owned(),
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
            file_count: 0,
            available: false,
            readable: false,
            writable: false,
            free_space_bytes: None,
        };

        source.refresh_diagnostics();

        assert!(source.available);
        assert!(source.readable);
        assert!(source.writable);
        assert!(source.free_space_bytes.is_some_and(|bytes| bytes > 0));
        std::fs::remove_dir(path).unwrap();
    }
}
