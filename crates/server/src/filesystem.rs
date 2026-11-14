use std::{
    io::Read,
    path::{Path, PathBuf},
};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::StatusCode,
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::web::AppState;

type ApiResult<T> = Result<T, (StatusCode, String)>;

#[derive(Debug, Serialize)]
pub struct DuplicateCandidateView {
    id: i64,
    status: String,
    score: i64,
    reason: Value,
    winner_path: String,
    winner_size_bytes: i64,
    duplicate_path: String,
    duplicate_size_bytes: i64,
    created_at: String,
    decided_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FilesystemOperationView {
    id: i64,
    operation_type: String,
    state: String,
    from_path: String,
    to_path: String,
    quarantine_path: Option<String>,
    size_bytes: Option<i64>,
    checksum_sha256: Option<String>,
    error_message: Option<String>,
    retention_until: Option<String>,
    created_at: String,
    completed_at: Option<String>,
    can_undo: bool,
    can_discard: bool,
}

#[derive(Debug, Serialize)]
pub struct RecoverySummary {
    pub inspected: u64,
    pub completed: u64,
    pub failed: u64,
}

#[derive(Debug, sqlx::FromRow)]
struct OperationRecord {
    id: i64,
    operation_type: String,
    source_id: i64,
    media_file_id: Option<i64>,
    from_path: String,
    to_path: String,
    quarantine_path: Option<String>,
    state: String,
    checksum_sha256: Option<String>,
}

pub(crate) async fn list_duplicates(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<DuplicateCandidateView>>> {
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            i64,
            String,
            String,
            i64,
            String,
            i64,
            String,
            Option<String>,
        ),
    >(
        "SELECT dc.id, dc.status, dc.score, dc.reason_json,
                winner.path, winner.size_bytes, duplicate.path, duplicate.size_bytes,
                dc.created_at, dc.decided_at
         FROM duplicate_candidates dc
         JOIN media_files winner ON winner.id = dc.winner_media_file_id
         JOIN media_files duplicate ON duplicate.id = dc.duplicate_media_file_id
         ORDER BY CASE dc.status WHEN 'pending' THEN 0 WHEN 'quarantined' THEN 1 ELSE 2 END,
                  dc.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    status,
                    score,
                    reason,
                    winner_path,
                    winner_size_bytes,
                    duplicate_path,
                    duplicate_size_bytes,
                    created_at,
                    decided_at,
                )| DuplicateCandidateView {
                    id,
                    status,
                    score,
                    reason: serde_json::from_str(&reason).unwrap_or(Value::Null),
                    winner_path,
                    winner_size_bytes,
                    duplicate_path,
                    duplicate_size_bytes,
                    created_at,
                    decided_at,
                },
            )
            .collect(),
    ))
}

pub(crate) async fn list_operations(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<FilesystemOperationView>>> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, Option<String>, Option<i64>, Option<String>, Option<String>, Option<String>, String, Option<String>, bool, bool)>(
        "SELECT id, operation_type, state, from_path, to_path, quarantine_path,
                size_bytes, checksum_sha256, error_message, retention_until, created_at, completed_at,
                state = 'done' AND (retention_until IS NULL OR retention_until > datetime('now')) AS can_undo,
                state = 'done' AND quarantine_path IS NOT NULL AND retention_until <= datetime('now') AS can_discard
         FROM filesystem_operations
         ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(
                    id,
                    operation_type,
                    state,
                    from_path,
                    to_path,
                    quarantine_path,
                    size_bytes,
                    checksum_sha256,
                    error_message,
                    retention_until,
                    created_at,
                    completed_at,
                    can_undo,
                    can_discard,
                )| FilesystemOperationView {
                    id,
                    operation_type,
                    state,
                    from_path,
                    to_path,
                    quarantine_path,
                    size_bytes,
                    checksum_sha256,
                    error_message,
                    retention_until,
                    created_at,
                    completed_at,
                    can_undo,
                    can_discard,
                },
            )
            .collect(),
    ))
}

pub(crate) async fn approve_duplicate(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<Json<Value>> {
    let row = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            String,
            i64,
            Option<String>,
            Option<String>,
            String,
            i64,
            Option<String>,
            Option<String>,
            i64,
            String,
            String,
        ),
    >(
        "SELECT dc.winner_media_file_id, dc.duplicate_media_file_id,
                winner.path, winner.size_bytes, winner.video_codec, winner.probe_json,
                duplicate.path, duplicate.size_bytes, duplicate.video_codec, duplicate.probe_json,
                source.id, source.path, source.mode
         FROM duplicate_candidates dc
         JOIN media_files winner ON winner.id = dc.winner_media_file_id
         JOIN media_files duplicate ON duplicate.id = dc.duplicate_media_file_id
         JOIN source_files sf ON sf.media_file_id = duplicate.id AND sf.is_available = 1
         JOIN media_sources source ON source.id = sf.source_id AND source.removed_at IS NULL
         WHERE dc.id = ? AND dc.status = 'pending'
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?
    .ok_or((
        StatusCode::NOT_FOUND,
        "candidato pendente não encontrado".to_owned(),
    ))?;

    let (
        winner_file_id,
        duplicate_file_id,
        winner_path,
        winner_size,
        winner_codec,
        winner_probe,
        duplicate_path,
        duplicate_size,
        duplicate_codec,
        duplicate_probe,
        source_id,
        source_root,
        source_mode,
    ) = row;
    if source_mode != "managed" {
        return Err((
            StatusCode::CONFLICT,
            "a cópia está em fonte somente leitura; nenhuma alteração foi feita".to_owned(),
        ));
    }

    let duplicate = Path::new(&duplicate_path);
    let winner = Path::new(&winner_path);
    if !duplicate.is_file() || !winner.is_file() {
        return Err((
            StatusCode::CONFLICT,
            "uma das cópias não está disponível no caminho indexado".to_owned(),
        ));
    }
    if !duplicate.starts_with(Path::new(&source_root)) {
        return Err((
            StatusCode::CONFLICT,
            "a cópia não pertence à raiz gerenciada registrada".to_owned(),
        ));
    }

    let winner_checksum = sha256_file(winner).await.map_err(conflict)?;
    let duplicate_checksum = sha256_file(duplicate).await.map_err(conflict)?;
    let stream_comparison = compare_streams(
        winner_probe.as_deref(),
        duplicate_probe.as_deref(),
        winner_codec.as_deref(),
        duplicate_codec.as_deref(),
    );
    let exact_match = winner_checksum == duplicate_checksum;
    let reason = json!({
        "match": "youtube_id",
        "winner_size_bytes": winner_size,
        "duplicate_size_bytes": duplicate_size,
        "exact_checksum_match": exact_match,
        "stream_comparison": stream_comparison,
        "decision": "user_approved"
    });

    let operation_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO filesystem_operations
            (operation_type, source_id, media_file_id, from_path, to_path, state,
             size_bytes, checksum_sha256, retention_until)
         VALUES ('deduplicate', ?, ?, ?, ?, 'planned', ?, ?, datetime('now', '+30 days'))
         RETURNING id",
    )
    .bind(source_id)
    .bind(duplicate_file_id)
    .bind(&duplicate_path)
    .bind(&winner_path)
    .bind(duplicate_size)
    .bind(&duplicate_checksum)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'verified',
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(operation_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let quarantine_dir = Path::new(&source_root)
        .join(".mymedia-quarantine")
        .join("duplicates")
        .join(id.to_string());
    tokio::fs::create_dir_all(&quarantine_dir)
        .await
        .map_err(|error| conflict(format!("falha ao criar quarentena: {error}")))?;
    let quarantine_path = quarantine_dir.join(
        duplicate
            .file_name()
            .ok_or_else(|| conflict("nome da cópia inválido"))?,
    );
    sqlx::query(
        "UPDATE filesystem_operations SET quarantine_path = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(quarantine_path.to_string_lossy().as_ref())
    .bind(operation_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    tokio::fs::rename(duplicate, &quarantine_path)
        .await
        .map_err(|error| conflict(format!("falha ao mover cópia para quarentena: {error}")))?;
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'quarantined',
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(operation_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "UPDATE media_files SET path = ?, is_available = 0,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(quarantine_path.to_string_lossy().as_ref())
    .bind(duplicate_file_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE source_files SET is_available = 0 WHERE media_file_id = ?")
        .bind(duplicate_file_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query(
        "UPDATE duplicate_candidates SET status = 'quarantined', reason_json = ?,
            winner_checksum_sha256 = ?, duplicate_checksum_sha256 = ?,
            decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(reason.to_string())
    .bind(&winner_checksum)
    .bind(&duplicate_checksum)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'database_committed', quarantine_path = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(quarantine_path.to_string_lossy().as_ref())
    .bind(operation_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    refresh_media_availability(&mut tx, winner_file_id, duplicate_file_id).await?;
    tx.commit().await.map_err(internal)?;
    set_done(&state.pool, operation_id)
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "operation_id": operation_id,
        "status": "quarantined",
        "exact_checksum_match": exact_match,
        "stream_comparison": stream_comparison
    })))
}

pub(crate) async fn reject_duplicate(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<StatusCode> {
    let result = sqlx::query(
        "UPDATE duplicate_candidates SET status = 'rejected',
            decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND status = 'pending'",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            "candidato pendente não encontrado".to_owned(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn undo_operation(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<Json<Value>> {
    undo(&state.pool, id).await?;
    Ok(Json(json!({ "status": "undone" })))
}

pub(crate) async fn discard_operation(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> ApiResult<StatusCode> {
    let row = sqlx::query_as::<_, (String, Option<String>, Option<i64>)>(
        "SELECT operation_type, quarantine_path, media_file_id
         FROM filesystem_operations
         WHERE id = ? AND state = 'done' AND quarantine_path IS NOT NULL
           AND retention_until <= datetime('now')",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?
    .ok_or((
        StatusCode::CONFLICT,
        "operação ainda está em retenção ou não possui item em quarentena".to_owned(),
    ))?;
    let (operation_type, quarantine_path, media_file_id) = row;
    let quarantine_path = quarantine_path.expect("query requires quarantine_path");
    match tokio::fs::remove_file(&quarantine_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(conflict(format!(
                "não foi possível descartar o arquivo em quarentena: {error}"
            )));
        }
    }
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'done', quarantine_path = NULL,
            error_message = 'quarentena descartada definitivamente',
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    if operation_type == "deduplicate"
        && let Some(media_file_id) = media_file_id
    {
        sqlx::query(
            "UPDATE duplicate_candidates SET status = 'discarded',
                    decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE duplicate_media_file_id = ? AND status = 'quarantined'",
        )
        .bind(media_file_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn recover_incomplete_operations(
    pool: &SqlitePool,
) -> Result<RecoverySummary, sqlx::Error> {
    let operations = sqlx::query_as::<_, OperationRecord>(
        "SELECT id, operation_type, source_id, media_file_id, from_path, to_path,
                quarantine_path, state, checksum_sha256
         FROM filesystem_operations
         WHERE state NOT IN ('done', 'undone', 'failed') ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    let mut summary = RecoverySummary {
        inspected: operations.len() as u64,
        completed: 0,
        failed: 0,
    };
    for operation in operations {
        match recover_one(pool, &operation).await {
            Ok(true) => summary.completed += 1,
            Ok(false) => summary.failed += 1,
            Err(error) => {
                summary.failed += 1;
                let _result = mark_failed(pool, operation.id, &error.to_string()).await;
            }
        }
    }
    Ok(summary)
}

async fn recover_one(pool: &SqlitePool, operation: &OperationRecord) -> Result<bool, sqlx::Error> {
    let source_mode = sqlx::query_scalar::<_, String>(
        "SELECT mode FROM media_sources WHERE id = ? AND removed_at IS NULL",
    )
    .bind(operation.source_id)
    .fetch_optional(pool)
    .await?;
    if source_mode.as_deref() != Some("managed") {
        mark_failed(
            pool,
            operation.id,
            "recuperação bloqueada: fonte ausente ou somente leitura",
        )
        .await?;
        return Ok(false);
    }
    let from_exists = Path::new(&operation.from_path).is_file();
    let to_exists = Path::new(&operation.to_path).is_file();
    let quarantine_exists = operation
        .quarantine_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file());

    match operation.operation_type.as_str() {
        "organize" => match operation.state.as_str() {
            "planned" if from_exists && !to_exists => {
                mark_failed(
                    pool,
                    operation.id,
                    "operação planejada não iniciou; origem preservada",
                )
                .await?;
                Ok(false)
            }
            "planned" | "copied" | "verified" if to_exists => {
                if !path_matches_checksum(&operation.to_path, operation.checksum_sha256.as_deref())
                    .await
                {
                    mark_failed(
                        pool,
                        operation.id,
                        "destino não passou na verificação de checksum",
                    )
                    .await?;
                    return Ok(false);
                }
                if let Err(error) =
                    crate::organizer::recover_companion_subtitles(pool, operation.id).await
                {
                    mark_failed(pool, operation.id, &error).await?;
                    return Ok(false);
                }
                complete_organize_recovery(pool, operation, from_exists).await?;
                Ok(true)
            }
            "database_committed" if to_exists => {
                if let Err(error) =
                    crate::organizer::recover_companion_subtitles(pool, operation.id).await
                {
                    mark_failed(pool, operation.id, &error).await?;
                    return Ok(false);
                }
                complete_organize_recovery(pool, operation, from_exists).await?;
                Ok(true)
            }
            "quarantined" if to_exists && quarantine_exists => {
                if let Err(error) =
                    crate::organizer::recover_companion_subtitles(pool, operation.id).await
                {
                    mark_failed(pool, operation.id, &error).await?;
                    return Ok(false);
                }
                set_done(pool, operation.id).await?;
                Ok(true)
            }
            _ => {
                mark_failed(
                    pool,
                    operation.id,
                    "estado do journal diverge dos arquivos encontrados",
                )
                .await?;
                Ok(false)
            }
        },
        "deduplicate" => {
            if quarantine_exists {
                complete_deduplicate_recovery(pool, operation).await?;
                Ok(true)
            } else {
                mark_failed(
                    pool,
                    operation.id,
                    "deduplicação incompleta; cópia original preservada",
                )
                .await?;
                Ok(false)
            }
        }
        _ => {
            mark_failed(
                pool,
                operation.id,
                "tipo de operação sem recuperação automática",
            )
            .await?;
            Ok(false)
        }
    }
}

async fn complete_deduplicate_recovery(
    pool: &SqlitePool,
    operation: &OperationRecord,
) -> Result<(), sqlx::Error> {
    let Some(media_file_id) = operation.media_file_id else {
        return mark_failed(
            pool,
            operation.id,
            "journal de deduplicação sem arquivo associado",
        )
        .await;
    };
    let quarantine_path = operation.quarantine_path.as_deref().unwrap_or_default();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE media_files SET path = ?, is_available = 0,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(quarantine_path)
    .bind(media_file_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE source_files SET is_available = 0 WHERE media_file_id = ?")
        .bind(media_file_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE duplicate_candidates SET status = 'quarantined',
            decided_at = COALESCE(decided_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         WHERE duplicate_media_file_id = ? AND status IN ('pending', 'approved')",
    )
    .bind(media_file_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'done',
            completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(operation.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

async fn complete_organize_recovery(
    pool: &SqlitePool,
    operation: &OperationRecord,
    source_still_exists: bool,
) -> Result<(), sqlx::Error> {
    let mut quarantine_path = operation.quarantine_path.clone();
    if source_still_exists {
        let source_root = sqlx::query_scalar::<_, String>(
            "SELECT path FROM media_sources WHERE id = ? AND mode = 'managed'",
        )
        .bind(operation.source_id)
        .fetch_one(pool)
        .await?;
        let target_dir = Path::new(&source_root)
            .join(".mymedia-quarantine")
            .join("operations")
            .join(operation.id.to_string());
        tokio::fs::create_dir_all(&target_dir).await?;
        let target = target_dir.join(
            Path::new(&operation.from_path)
                .file_name()
                .unwrap_or_default(),
        );
        tokio::fs::rename(&operation.from_path, &target).await?;
        quarantine_path = Some(target.to_string_lossy().into_owned());
    }
    let mut tx = pool.begin().await?;
    if let Some(media_file_id) = operation.media_file_id {
        sqlx::query(
            "UPDATE media_files SET path = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(&operation.to_path)
        .bind(media_file_id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'done', quarantine_path = ?,
            completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(quarantine_path)
    .bind(operation.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

pub async fn undo(pool: &SqlitePool, id: i64) -> ApiResult<()> {
    let operation = sqlx::query_as::<_, OperationRecord>(
        "SELECT id, operation_type, source_id, media_file_id, from_path, to_path,
                quarantine_path, state, checksum_sha256
         FROM filesystem_operations
         WHERE id = ? AND state = 'done'
           AND (retention_until IS NULL OR retention_until > datetime('now'))",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    .ok_or((
        StatusCode::CONFLICT,
        "operação não pode mais ser desfeita".to_owned(),
    ))?;
    let mode = sqlx::query_scalar::<_, String>("SELECT mode FROM media_sources WHERE id = ?")
        .bind(operation.source_id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if mode != "managed" {
        return Err(conflict("a fonte deixou de ser gerenciada"));
    }
    let restore_source = operation
        .quarantine_path
        .as_deref()
        .unwrap_or(&operation.to_path);
    if !path_matches_checksum(restore_source, operation.checksum_sha256.as_deref()).await {
        return Err(conflict(
            "o arquivo de restauração não corresponde ao checksum registrado",
        ));
    }
    if Path::new(&operation.from_path).exists() {
        return Err(conflict("o caminho original está ocupado; undo cancelado"));
    }
    if operation.operation_type == "organize" {
        crate::organizer::undo_companion_subtitles(pool, id)
            .await
            .map_err(conflict)?;
    }
    if let Some(parent) = Path::new(&operation.from_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| conflict(format!("falha ao recriar diretório original: {error}")))?;
    }
    tokio::fs::rename(restore_source, &operation.from_path)
        .await
        .map_err(|error| conflict(format!("falha ao restaurar arquivo: {error}")))?;

    let mut tx = pool.begin().await.map_err(internal)?;
    if let Some(media_file_id) = operation.media_file_id {
        sqlx::query(
            "UPDATE media_files SET path = ?, is_available = 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(&operation.from_path)
        .bind(media_file_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query("UPDATE source_files SET is_available = 1 WHERE media_file_id = ?")
            .bind(media_file_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        if operation.operation_type == "deduplicate" {
            sqlx::query(
                "UPDATE duplicate_candidates SET status = 'restored',
                    decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE duplicate_media_file_id = ? AND status = 'quarantined'",
            )
            .bind(media_file_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        }
    }
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'undone', quarantine_path = NULL,
            completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    if operation.operation_type == "organize"
        && operation.quarantine_path.is_some()
        && Path::new(&operation.to_path).is_file()
    {
        tokio::fs::remove_file(&operation.to_path)
            .await
            .map_err(|error| {
                conflict(format!(
                    "origem restaurada, mas a cópia verificada não pôde ser removida: {error}"
                ))
            })?;
    }
    Ok(())
}

fn compare_streams(
    winner_probe: Option<&str>,
    duplicate_probe: Option<&str>,
    winner_codec: Option<&str>,
    duplicate_codec: Option<&str>,
) -> Value {
    let winner = stream_signature(winner_probe, winner_codec);
    let duplicate = stream_signature(duplicate_probe, duplicate_codec);
    json!({
        "winner": winner,
        "duplicate": duplicate,
        "equivalent": winner == duplicate
    })
}

fn stream_signature(probe: Option<&str>, fallback_codec: Option<&str>) -> Value {
    let parsed = probe.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let mut streams = parsed
        .as_ref()
        .and_then(|value| value.get("streams"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|stream| {
                    let kind = stream.get("codec_type")?.as_str()?;
                    matches!(kind, "video" | "audio" | "subtitle").then(|| {
                        json!({
                            "type": kind,
                            "codec": stream.get("codec_name").and_then(Value::as_str),
                            "width": stream.get("width").and_then(Value::as_i64),
                            "height": stream.get("height").and_then(Value::as_i64),
                            "channels": stream.get("channels").and_then(Value::as_i64),
                            "sample_rate": stream.get("sample_rate").and_then(Value::as_str)
                        })
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if streams.is_empty() {
        streams.push(json!({ "type": "video", "codec": fallback_codec }));
    }
    Value::Array(streams)
}

pub(crate) async fn sha256_file(path: &Path) -> Result<String, String> {
    let path = PathBuf::from(path);
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)
            .map_err(|error| format!("falha ao abrir {}: {error}", path.display()))?;
        let mut context = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("falha ao ler {}: {error}", path.display()))?;
            if read == 0 {
                break;
            }
            context.update(&buffer[..read]);
        }
        Ok(context
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    })
    .await
    .map_err(|error| format!("tarefa de checksum falhou: {error}"))?
}

/// Moves a newly-created file, verifying bytes before removing the source when
/// the rename crosses filesystems. Existing destinations are never replaced.
pub(crate) async fn move_verified(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err(format!(
            "destino já existe; nenhuma cópia foi alterada: {}",
            destination.display()
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "destino sem diretório pai".to_owned())?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("falha ao criar diretório destino: {error}"))?;
    let source_checksum = sha256_file(source).await?;
    if tokio::fs::rename(source, destination).await.is_ok() {
        let destination_checksum = sha256_file(destination).await?;
        if destination_checksum != source_checksum {
            return Err("arquivo renomeado não corresponde ao checksum de origem".to_owned());
        }
        return Ok(());
    }
    tokio::fs::copy(source, destination)
        .await
        .map_err(|error| format!("falha ao copiar arquivo: {error}"))?;
    let destination_checksum = sha256_file(destination).await?;
    if destination_checksum != source_checksum {
        return Err("cópia não corresponde ao checksum de origem; origem preservada".to_owned());
    }
    tokio::fs::remove_file(source).await.map_err(|error| {
        format!("cópia verificada, mas a origem temporária não pôde ser removida: {error}")
    })?;
    Ok(())
}

async fn path_matches_checksum(path: &str, checksum: Option<&str>) -> bool {
    let Some(expected) = checksum else {
        return false;
    };
    sha256_file(Path::new(path))
        .await
        .is_ok_and(|actual| actual == expected)
}

async fn refresh_media_availability(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    winner_file_id: i64,
    duplicate_file_id: i64,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE media SET is_available = EXISTS (
            SELECT 1 FROM media_files WHERE media_files.media_id = media.id AND media_files.is_available = 1
         ) WHERE id IN (
            SELECT media_id FROM media_files WHERE id IN (?, ?)
         )",
    )
    .bind(winner_file_id)
    .bind(duplicate_file_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn mark_failed(pool: &SqlitePool, id: i64, message: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'failed', error_message = ?,
            completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(message)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_done(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE filesystem_operations SET state = 'done',
            completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    tracing::error!(%error, "filesystem operation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "erro interno do servidor".to_owned(),
    )
}

fn conflict(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::CONFLICT, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mymedia_sprint3_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    async fn setup_operation(
        pool: &SqlitePool,
        root: &Path,
        state: &str,
        from_exists: bool,
        to_exists: bool,
        quarantine_exists: bool,
    ) -> (i64, i64, PathBuf, PathBuf, Option<PathBuf>) {
        std::fs::create_dir_all(root).unwrap();
        let from = root.join(format!("from-{state}.mp4"));
        let to = root.join("organized").join(format!("to-{state}.mp4"));
        let quarantine = quarantine_exists.then(|| {
            root.join(".mymedia-quarantine")
                .join(format!("q-{state}.mp4"))
        });
        if from_exists {
            std::fs::write(&from, b"verified-media").unwrap();
        }
        if to_exists {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(&to, b"verified-media").unwrap();
        }
        if let Some(path) = &quarantine {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"verified-media").unwrap();
        }
        let source_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_sources (path, mode, status) VALUES (?, 'managed', 'ready') RETURNING id",
        )
        .bind(root.to_string_lossy().as_ref())
        .fetch_one(pool)
        .await
        .unwrap();
        let media_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media (title, is_available) VALUES (?, 1) RETURNING id",
        )
        .bind(format!("test-{state}"))
        .fetch_one(pool)
        .await
        .unwrap();
        let indexed_path = if matches!(state, "database_committed" | "quarantined") {
            &to
        } else {
            &from
        };
        let file_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_files
                (path, title, extension, size_bytes, modified_at_unix, media_id)
             VALUES (?, ?, 'mp4', 14, 1, ?) RETURNING id",
        )
        .bind(indexed_path.to_string_lossy().as_ref())
        .bind(format!("test-{state}"))
        .bind(media_id)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO source_files (source_id, media_file_id) VALUES (?, ?)")
            .bind(source_id)
            .bind(file_id)
            .execute(pool)
            .await
            .unwrap();
        let checksum = sha256_file(if from_exists {
            &from
        } else if to_exists {
            &to
        } else {
            quarantine.as_ref().unwrap()
        })
        .await
        .unwrap();
        let operation_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO filesystem_operations
                (operation_type, source_id, media_file_id, from_path, to_path,
                 quarantine_path, state, size_bytes, checksum_sha256, retention_until)
             VALUES ('organize', ?, ?, ?, ?, ?, ?, 14, ?, datetime('now', '+30 days'))
             RETURNING id",
        )
        .bind(source_id)
        .bind(file_id)
        .bind(from.to_string_lossy().as_ref())
        .bind(to.to_string_lossy().as_ref())
        .bind(
            quarantine
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
        )
        .bind(state)
        .bind(checksum)
        .fetch_one(pool)
        .await
        .unwrap();
        (operation_id, file_id, from, to, quarantine)
    }

    async fn setup_duplicate(
        pool: &SqlitePool,
        root: &Path,
        mode: &str,
    ) -> (i64, PathBuf, PathBuf) {
        std::fs::create_dir_all(root).unwrap();
        let winner = root.join("winner.mp4");
        let duplicate = root.join("duplicate.mp4");
        std::fs::write(&winner, b"same-stream-content").unwrap();
        std::fs::write(&duplicate, b"same-stream-content").unwrap();
        let source_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_sources (path, mode, status) VALUES (?, ?, 'ready') RETURNING id",
        )
        .bind(root.to_string_lossy().as_ref())
        .bind(mode)
        .fetch_one(pool)
        .await
        .unwrap();
        let media_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media (title, youtube_id) VALUES ('duplicate-test', 'abcdefghijk') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let winner_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_files
                (path, title, extension, size_bytes, modified_at_unix, media_id, video_codec)
             VALUES (?, 'winner', 'mp4', 19, 1, ?, 'h264') RETURNING id",
        )
        .bind(winner.to_string_lossy().as_ref())
        .bind(media_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let duplicate_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_files
                (path, title, extension, size_bytes, modified_at_unix, media_id, video_codec)
             VALUES (?, 'duplicate', 'mp4', 19, 1, ?, 'h264') RETURNING id",
        )
        .bind(duplicate.to_string_lossy().as_ref())
        .bind(media_id)
        .fetch_one(pool)
        .await
        .unwrap();
        for file_id in [winner_id, duplicate_id] {
            sqlx::query("INSERT INTO source_files (source_id, media_file_id) VALUES (?, ?)")
                .bind(source_id)
                .bind(file_id)
                .execute(pool)
                .await
                .unwrap();
        }
        let candidate_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO duplicate_candidates
                (winner_media_file_id, duplicate_media_file_id, score, reason_json)
             VALUES (?, ?, 10, '{\"match\":\"youtube_id\"}') RETURNING id",
        )
        .bind(winner_id)
        .bind(duplicate_id)
        .fetch_one(pool)
        .await
        .unwrap();
        (candidate_id, winner, duplicate)
    }

    #[tokio::test]
    async fn recovery_reconciles_every_organize_stage_without_losing_media() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = temp_root("recovery_stages");

        let cases = [
            ("planned", true, false, false, "failed"),
            ("copied", true, true, false, "done"),
            ("verified", true, true, false, "done"),
            ("database_committed", true, true, false, "done"),
            ("quarantined", false, true, true, "done"),
        ];
        let mut records = Vec::new();
        for (state, from_exists, to_exists, quarantine_exists, expected) in cases {
            let case_root = root.join(state);
            let record = setup_operation(
                &pool,
                &case_root,
                state,
                from_exists,
                to_exists,
                quarantine_exists,
            )
            .await;
            records.push((record, expected));
        }

        let summary = recover_incomplete_operations(&pool).await.unwrap();
        assert_eq!(summary.inspected, 5);
        assert_eq!(summary.completed, 4);
        assert_eq!(summary.failed, 1);

        for ((operation_id, file_id, from, to, quarantine), expected) in records {
            let state = sqlx::query_scalar::<_, String>(
                "SELECT state FROM filesystem_operations WHERE id = ?",
            )
            .bind(operation_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(state, expected);
            let db_path =
                sqlx::query_scalar::<_, String>("SELECT path FROM media_files WHERE id = ?")
                    .bind(file_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert!(
                Path::new(&db_path).is_file(),
                "DB path must exist: {db_path}"
            );
            assert!(
                from.is_file()
                    || to.is_file()
                    || quarantine.as_ref().is_some_and(|path| path.is_file()),
                "at least one verified copy must survive"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn undo_restores_an_organize_operation_after_restart() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = temp_root("undo_restart");
        let (operation_id, file_id, from, to, _) =
            setup_operation(&pool, &root, "database_committed", false, true, false).await;
        sqlx::query("UPDATE filesystem_operations SET state = 'done' WHERE id = ?")
            .bind(operation_id)
            .execute(&pool)
            .await
            .unwrap();

        undo(&pool, operation_id).await.unwrap();

        assert!(from.is_file());
        assert!(!to.exists());
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT mf.path, operation.state FROM media_files mf
             JOIN filesystem_operations operation ON operation.media_file_id = mf.id
             WHERE mf.id = ?",
        )
        .bind(file_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, from.to_string_lossy());
        assert_eq!(row.1, "undone");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn planned_operation_in_read_only_source_is_never_completed() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = temp_root("readonly");
        let (operation_id, _, from, to, _) =
            setup_operation(&pool, &root, "copied", true, true, false).await;
        sqlx::query(
            "UPDATE media_sources SET mode = 'read_only' WHERE id = (
                SELECT source_id FROM filesystem_operations WHERE id = ?)",
        )
        .bind(operation_id)
        .execute(&pool)
        .await
        .unwrap();

        let summary = recover_incomplete_operations(&pool).await.unwrap();
        assert_eq!(summary.completed, 0);
        assert!(from.is_file());
        assert!(to.is_file());
        let state =
            sqlx::query_scalar::<_, String>("SELECT state FROM filesystem_operations WHERE id = ?")
                .bind(operation_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "failed");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn duplicate_quarantine_and_undo_survive_state_reload() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = temp_root("duplicate_undo");
        let (candidate_id, winner, duplicate) = setup_duplicate(&pool, &root, "managed").await;
        let state = AppState::for_test(pool.clone());

        let response = approve_duplicate(State(state), AxumPath(candidate_id))
            .await
            .unwrap();
        let operation_id = response.0["operation_id"].as_i64().unwrap();
        assert!(winner.is_file());
        assert!(!duplicate.exists());
        let quarantine = sqlx::query_scalar::<_, String>(
            "SELECT quarantine_path FROM filesystem_operations WHERE id = ?",
        )
        .bind(operation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(Path::new(&quarantine).is_file());

        undo(&pool, operation_id).await.unwrap();
        assert!(winner.is_file());
        assert!(duplicate.is_file());
        assert!(!Path::new(&quarantine).exists());
        let status =
            sqlx::query_scalar::<_, String>("SELECT status FROM duplicate_candidates WHERE id = ?")
                .bind(candidate_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "restored");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn read_only_source_rejects_deduplication_without_touching_files() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = temp_root("duplicate_readonly");
        let (candidate_id, winner, duplicate) = setup_duplicate(&pool, &root, "read_only").await;
        let state = AppState::for_test(pool.clone());

        let error = approve_duplicate(State(state), AxumPath(candidate_id))
            .await
            .unwrap_err();
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert!(winner.is_file());
        assert!(duplicate.is_file());
        let operations = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM filesystem_operations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(operations, 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn permanent_discard_is_blocked_until_retention_expires() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = temp_root("retention");
        let (candidate_id, winner, duplicate) = setup_duplicate(&pool, &root, "managed").await;
        let state = AppState::for_test(pool.clone());
        let response = approve_duplicate(State(state.clone()), AxumPath(candidate_id))
            .await
            .unwrap();
        let operation_id = response.0["operation_id"].as_i64().unwrap();
        let quarantine = sqlx::query_scalar::<_, String>(
            "SELECT quarantine_path FROM filesystem_operations WHERE id = ?",
        )
        .bind(operation_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        let early = discard_operation(State(state.clone()), AxumPath(operation_id)).await;
        assert_eq!(early.unwrap_err().0, StatusCode::CONFLICT);
        assert!(Path::new(&quarantine).is_file());
        sqlx::query(
            "UPDATE filesystem_operations SET retention_until = datetime('now', '-1 day') WHERE id = ?",
        )
        .bind(operation_id)
        .execute(&pool)
        .await
        .unwrap();
        discard_operation(State(state), AxumPath(operation_id))
            .await
            .unwrap();
        assert!(winner.is_file());
        assert!(!duplicate.exists());
        assert!(!Path::new(&quarantine).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn verified_move_never_overwrites_a_collision() {
        let root = temp_root("verified_move_collision");
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.mp4");
        let destination = root.join("destination.mp4");
        std::fs::write(&source, b"source-bytes").unwrap();
        std::fs::write(&destination, b"existing-bytes").unwrap();

        let error = move_verified(&source, &destination).await.unwrap_err();
        assert!(error.contains("destino já existe"));
        assert_eq!(std::fs::read(&source).unwrap(), b"source-bytes");
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing-bytes");
        std::fs::remove_dir_all(root).unwrap();
    }
}
