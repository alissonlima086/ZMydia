use std::{
    io::Read,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Serialize, Default, Clone)]
pub struct OrganizeSummary {
    pub total_candidates: u64,
    pub moved: u64,
    pub already_organized: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct OrganizePreviewItem {
    pub media_id: i64,
    pub current_path: String,
    pub target_path: String,
    pub size_bytes: i64,
    pub collision: bool,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct OrganizePreview {
    pub moves: Vec<OrganizePreviewItem>,
    pub already_organized: u64,
    pub collisions: u64,
    pub required_bytes: u64,
}

pub struct OrganizeMediaRequest<'a> {
    pub source_path: Option<&'a str>,
    pub channel_name: &'a str,
    pub youtube_id: &'a str,
    pub is_short: bool,
    pub current_path: &'a str,
}

pub fn media_mutations_enabled() -> bool {
    std::env::var("MYMEDIA_ALLOW_MEDIA_MUTATIONS")
        .is_ok_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

/// Sanitizes a string for use as a directory or file name segment.
pub fn sanitize_path_segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push(' '),
            '⧸' | '／' | '＼' | '：' | '＂' | '｜' | '？' | '＊' | '＜' | '＞' => {
                out.push(' ')
            }
            other => out.push(other),
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == ' ' || c == '-');
    if trimmed.is_empty() {
        "Desconhecido".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Safely truncates a string to fit within a byte limit without splitting UTF-8 characters.
pub fn truncate_utf8_to_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut limit = max_bytes;
    while limit > 0 && !s.is_char_boundary(limit) {
        limit -= 1;
    }
    s[..limit].trim_end()
}

/// Computes the rigid canonical path for a media item.
/// Format: `{source_root}/{Channel_Name}/[{Shorts}/]{Title} [{youtube_id}].{ext}`
pub fn compute_canonical_path(
    source_root: &Path,
    channel_name: &str,
    title: &str,
    youtube_id: &str,
    is_short: bool,
    ext: &str,
) -> PathBuf {
    let clean_channel = sanitize_path_segment(channel_name);
    let target_dir = if is_short {
        source_root.join(&clean_channel).join("Shorts")
    } else {
        source_root.join(&clean_channel)
    };

    let clean_title = sanitize_path_segment(title);
    let id_bracket = format!("[{youtube_id}]");

    let clean_ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let extension = if clean_ext.is_empty() {
        "mp4"
    } else {
        &clean_ext
    };

    let suffix = format!(" [{youtube_id}].{extension}");
    let max_filename_bytes: usize = 250;

    let filename = if clean_title.contains(&id_bracket) {
        // Already contains the bracketed ID, sanitize and ensure bounds
        let base_suffix = format!(".{extension}");
        let allowed = max_filename_bytes.saturating_sub(base_suffix.len());
        let truncated_base = truncate_utf8_to_bytes(&clean_title, allowed);
        format!("{truncated_base}.{extension}")
    } else {
        let allowed = max_filename_bytes.saturating_sub(suffix.len());
        let truncated_base = truncate_utf8_to_bytes(&clean_title, allowed);
        format!("{truncated_base}{suffix}")
    };

    target_dir.join(filename)
}

/// Organizes a single media file to its canonical channel path.
/// Moves a video and its companion subtitles, then updates SQLite.
pub async fn organize_single_media(
    pool: &SqlitePool,
    media_id: i64,
    request: OrganizeMediaRequest<'_>,
) -> Result<bool, String> {
    if !media_mutations_enabled() {
        return Err("reorganização desabilitada; defina MYMEDIA_ALLOW_MEDIA_MUTATIONS=1 para autorizá-la explicitamente".to_owned());
    }
    let current = Path::new(request.current_path);
    if !current.is_file() {
        return Err(format!(
            "arquivo atual não encontrado no disco: {}",
            request.current_path
        ));
    }

    let source_record = sqlx::query_as::<_, (i64, i64, String, String)>(
        "SELECT mf.id, s.id, s.path, s.mode
         FROM media_files mf
         JOIN source_files sf ON sf.media_file_id = mf.id
         JOIN media_sources s ON s.id = sf.source_id
         WHERE mf.media_id = ? AND mf.path = ? AND s.enabled = 1 AND s.removed_at IS NULL
         LIMIT 1",
    )
    .bind(media_id)
    .bind(request.current_path)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "arquivo não pertence a uma fonte ativa".to_owned())?;
    let (media_file_id, source_id, managed_root, source_mode) = source_record;
    if source_mode != "managed" {
        return Err("fonte em modo somente leitura; altere-a explicitamente para gerenciada antes de organizar".to_owned());
    }

    // Determine the source root
    let root_path: PathBuf = if let Some(src) = request.source_path {
        if src != managed_root {
            return Err(
                "a raiz informada não corresponde à fonte gerenciada do arquivo".to_owned(),
            );
        }
        PathBuf::from(src)
    } else {
        // Lookup from media_sources
        let sources = sqlx::query_scalar::<_, String>(
            "SELECT path FROM media_sources WHERE enabled = 1 AND mode = 'managed' AND removed_at IS NULL",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut matched_source = None;
        for s in sources {
            let p = Path::new(&s);
            if current.starts_with(p) {
                matched_source = Some(p.to_path_buf());
                break;
            }
        }
        matched_source
            .or_else(|| current.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let ext = current
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");

    // Fetch the current title from media table
    let title: String = sqlx::query_scalar("SELECT title FROM media WHERE id = ?")
        .bind(media_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| {
            current
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("video")
                .to_string()
        });

    let target = compute_canonical_path(
        &root_path,
        request.channel_name,
        &title,
        request.youtube_id,
        request.is_short,
        ext,
    );

    if current == target {
        return Ok(false); // Already in canonical location
    }

    let target_dir = target
        .parent()
        .ok_or_else(|| "diretório de destino inválido".to_string())?;

    let final_target = target.clone();
    if final_target.exists() && final_target != current {
        return Err(format!(
            "colisão detectada em {}; nenhuma cópia foi alterada",
            final_target.display()
        ));
    }

    tokio::fs::create_dir_all(target_dir)
        .await
        .map_err(|e| format!("erro ao criar pasta destino {}: {e}", target_dir.display()))?;

    let size_bytes = std::fs::metadata(current)
        .map_err(|e| format!("não foi possível ler o arquivo de origem: {e}"))?
        .len();
    let checksum = sha256_file(current).await?;
    let final_target_str = final_target.to_string_lossy().to_string();
    let operation_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO filesystem_operations
            (operation_type, source_id, media_file_id, from_path, to_path, state, size_bytes, checksum_sha256, retention_until)
         VALUES ('organize', ?, ?, ?, ?, 'planned', ?, ?, datetime('now', '+30 days'))
         RETURNING id",
    )
    .bind(source_id)
    .bind(media_file_id)
    .bind(request.current_path)
    .bind(&final_target_str)
    .bind(i64::try_from(size_bytes).unwrap_or(i64::MAX))
    .bind(&checksum)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    plan_companion_subtitles(pool, operation_id, media_id, &final_target).await?;

    let renamed = tokio::fs::rename(current, &final_target).await.is_ok();
    if !renamed {
        tokio::fs::copy(current, &final_target)
            .await
            .map_err(|e| format!("falha ao copiar arquivo: {e}"))?;
    }
    set_operation_state(pool, operation_id, "copied", None).await?;

    let target_checksum = sha256_file(&final_target).await?;
    if target_checksum != checksum {
        set_operation_state(
            pool,
            operation_id,
            "failed",
            Some("checksum do destino diverge da origem"),
        )
        .await?;
        return Err("verificação da cópia falhou; a origem foi preservada".to_owned());
    }
    set_operation_state(pool, operation_id, "verified", None).await?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE media_files SET path = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
    )
    .bind(&final_target_str)
    .bind(media_file_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE filesystem_operations SET state = 'database_committed', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(operation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    if !renamed {
        let quarantine_dir = root_path
            .join(".mymedia-quarantine")
            .join("operations")
            .join(operation_id.to_string());
        tokio::fs::create_dir_all(&quarantine_dir)
            .await
            .map_err(|e| format!("falha ao criar quarentena: {e}"))?;
        let quarantine_path = quarantine_dir.join(
            current
                .file_name()
                .ok_or_else(|| "nome do arquivo de origem inválido".to_owned())?,
        );
        sqlx::query("UPDATE filesystem_operations SET quarantine_path = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(quarantine_path.to_string_lossy().as_ref())
            .bind(operation_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(current, &quarantine_path)
            .await
            .map_err(|e| {
                format!("cópia verificada, mas a origem não pôde ser movida para quarentena: {e}")
            })?;
        sqlx::query("UPDATE filesystem_operations SET state = 'quarantined', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(operation_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }

    recover_companion_subtitles(pool, operation_id).await?;
    set_operation_state(pool, operation_id, "done", None).await?;

    // Safely cleanup empty old directory if it's not the root
    if let Some(old_dir) = current.parent()
        && old_dir != root_path
        && old_dir.starts_with(&root_path)
    {
        clean_empty_dir_recursive(old_dir, &root_path);
    }

    Ok(true)
}

async fn plan_companion_subtitles(
    pool: &SqlitePool,
    operation_id: i64,
    media_id: i64,
    new_video: &Path,
) -> Result<(), String> {
    let subs = match sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, lang_code, file_path FROM media_subtitles WHERE media_id = ?",
    )
    .bind(media_id)
    .fetch_all(pool)
    .await
    {
        Ok(s) => s,
        Err(error) => return Err(format!("falha ao consultar legendas companheiras: {error}")),
    };

    let new_dir = match new_video.parent() {
        Some(d) => d,
        None => return Err("destino da mídia não possui diretório".to_owned()),
    };
    let new_stem = match new_video.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return Err("destino da mídia não possui nome válido".to_owned()),
    };

    for (sub_id, lang, sub_path_str) in subs {
        let old_sub = Path::new(&sub_path_str);
        if !old_sub.is_file() {
            continue;
        }
        let sub_ext = old_sub
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("vtt");
        let new_sub = new_dir.join(format!("{new_stem}.{lang}.{sub_ext}"));

        if old_sub != new_sub {
            if new_sub.exists() {
                return Err(format!(
                    "colisão de legenda detectada em {}; nenhuma mídia foi alterada",
                    new_sub.display()
                ));
            }
            let checksum = sha256_file(old_sub).await?;
            sqlx::query(
                "INSERT INTO filesystem_operation_companions
                    (operation_id, subtitle_id, from_path, to_path, checksum_sha256)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(operation_id)
            .bind(sub_id)
            .bind(old_sub.to_string_lossy().as_ref())
            .bind(new_sub.to_string_lossy().as_ref())
            .bind(checksum)
            .execute(pool)
            .await
            .map_err(|e| format!("falha ao registrar legenda no journal: {e}"))?;
        }
    }
    Ok(())
}

pub(crate) async fn recover_companion_subtitles(
    pool: &SqlitePool,
    operation_id: i64,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, (i64, Option<i64>, String, String, String, String)>(
        "SELECT id, subtitle_id, from_path, to_path, checksum_sha256, state
         FROM filesystem_operation_companions
         WHERE operation_id = ? AND state NOT IN ('done', 'undone') ORDER BY id",
    )
    .bind(operation_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    for (id, subtitle_id, from_path, to_path, checksum, _state) in rows {
        let from = Path::new(&from_path);
        let to = Path::new(&to_path);
        if from.is_file() && !to.exists() {
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("falha ao criar diretório de legenda: {e}"))?;
            }
            tokio::fs::rename(from, to)
                .await
                .map_err(|e| format!("falha ao mover legenda {}: {e}", from.display()))?;
            sqlx::query("UPDATE filesystem_operation_companions SET state = 'moved', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        } else if from.exists() && to.exists() {
            return Err(format!(
                "ambos os caminhos da legenda existem; reconciliação manual necessária: {}",
                to.display()
            ));
        }
        if !to.is_file() || sha256_file(to).await? != checksum {
            sqlx::query("UPDATE filesystem_operation_companions SET state = 'failed', error_message = 'checksum da legenda diverge', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            return Err(format!(
                "legenda {} não passou na verificação",
                to.display()
            ));
        }
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        if let Some(subtitle_id) = subtitle_id {
            sqlx::query("UPDATE media_subtitles SET file_path = ? WHERE id = ?")
                .bind(&to_path)
                .bind(subtitle_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        sqlx::query("UPDATE filesystem_operation_companions SET state = 'done', error_message = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) async fn undo_companion_subtitles(
    pool: &SqlitePool,
    operation_id: i64,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, (i64, Option<i64>, String, String, String)>(
        "SELECT id, subtitle_id, from_path, to_path, checksum_sha256
         FROM filesystem_operation_companions
         WHERE operation_id = ? AND state = 'done' ORDER BY id DESC",
    )
    .bind(operation_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for (id, subtitle_id, from_path, to_path, checksum) in rows {
        let from = Path::new(&from_path);
        let to = Path::new(&to_path);
        if from.exists() {
            return Err(format!(
                "caminho original da legenda está ocupado: {}",
                from.display()
            ));
        }
        if !to.is_file() || sha256_file(to).await? != checksum {
            return Err(format!(
                "legenda alterada desde a organização: {}",
                to.display()
            ));
        }
        if let Some(parent) = from.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        tokio::fs::rename(to, from)
            .await
            .map_err(|e| e.to_string())?;
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        if let Some(subtitle_id) = subtitle_id {
            sqlx::query("UPDATE media_subtitles SET file_path = ? WHERE id = ?")
                .bind(&from_path)
                .bind(subtitle_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        sqlx::query("UPDATE filesystem_operation_companions SET state = 'undone', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn sha256_file(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)
            .map_err(|e| format!("falha ao abrir {} para checksum: {e}", path.display()))?;
        let mut context = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|e| format!("falha ao ler {} para checksum: {e}", path.display()))?;
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
    .map_err(|e| format!("tarefa de checksum falhou: {e}"))?
}

async fn set_operation_state(
    pool: &SqlitePool,
    operation_id: i64,
    state: &str,
    error: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE filesystem_operations SET state = ?, error_message = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            completed_at = CASE WHEN ? IN ('done', 'undone', 'failed') THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE completed_at END
         WHERE id = ?",
    )
    .bind(state)
    .bind(error)
    .bind(state)
    .bind(operation_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Recursively removes empty directories up to the source root.
fn clean_empty_dir_recursive(dir: &Path, root: &Path) {
    if dir == root || !dir.starts_with(root) {
        return;
    }
    // Only remove a truly empty directory. Never delete ignored companion files.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let remaining = entries.filter_map(|entry| entry.ok()).count();

    if remaining == 0
        && std::fs::remove_dir(dir).is_ok()
        && let Some(parent) = dir.parent()
    {
        clean_empty_dir_recursive(parent, root);
    }
}

/// Runs reorganization for all official media files in the library.
pub async fn organize_all_official_media(
    pool: &SqlitePool,
) -> Result<OrganizeSummary, sqlx::Error> {
    let query = "
        SELECT m.id, mf.path, s.path AS source_path,
               c.name AS channel_name, m.youtube_id, m.is_short
        FROM media m
        JOIN media_files mf ON mf.media_id = m.id
        LEFT JOIN source_files sf ON sf.media_file_id = mf.id
        LEFT JOIN media_sources s ON s.id = sf.source_id
        JOIN media_channels mc ON mc.media_id = m.id
        JOIN channels c ON c.id = mc.channel_id
        WHERE m.is_available = 1 AND mf.is_available = 1
          AND s.mode = 'managed' AND s.enabled = 1 AND s.removed_at IS NULL
          AND m.youtube_id IS NOT NULL
        ORDER BY m.id ASC
    ";

    let rows =
        sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, bool)>(query)
            .fetch_all(pool)
            .await?;

    let mut summary = OrganizeSummary {
        total_candidates: rows.len() as u64,
        moved: 0,
        already_organized: 0,
        errors: Vec::new(),
    };

    for (media_id, current_path, source_path, channel_name, youtube_id, is_short) in rows {
        let Some(yt_id) = youtube_id else { continue };
        match organize_single_media(
            pool,
            media_id,
            OrganizeMediaRequest {
                source_path: source_path.as_deref(),
                channel_name: &channel_name,
                youtube_id: &yt_id,
                is_short,
                current_path: &current_path,
            },
        )
        .await
        {
            Ok(true) => summary.moved += 1,
            Ok(false) => summary.already_organized += 1,
            Err(e) => {
                tracing::warn!(media_id, %current_path, %e, "failed to organize media");
                summary.errors.push(format!("ID {media_id}: {e}"));
            }
        }
    }

    Ok(summary)
}

/// Builds a read-only plan for bulk organization. No directory or file is changed.
pub async fn preview_all_official_media(pool: &SqlitePool) -> Result<OrganizePreview, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, Option<String>, bool, i64)>(
        "SELECT m.id, mf.path, s.path, c.name, m.youtube_id, m.is_short, mf.size_bytes
         FROM media m
         JOIN media_files mf ON mf.media_id = m.id
         JOIN source_files sf ON sf.media_file_id = mf.id
         JOIN media_sources s ON s.id = sf.source_id
         JOIN media_channels mc ON mc.media_id = m.id
         JOIN channels c ON c.id = mc.channel_id
         WHERE m.is_available = 1 AND mf.is_available = 1
           AND s.enabled = 1 AND s.mode = 'managed' AND s.removed_at IS NULL
           AND m.youtube_id IS NOT NULL
         ORDER BY m.id ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut preview = OrganizePreview::default();
    for (media_id, current_path, source_path, channel_name, youtube_id, is_short, size_bytes) in
        rows
    {
        let current = Path::new(&current_path);
        let Some(youtube_id) = youtube_id else {
            continue;
        };
        let extension = current
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("mp4");
        let title = sqlx::query_scalar::<_, String>("SELECT title FROM media WHERE id = ?")
            .bind(media_id)
            .fetch_one(pool)
            .await?;
        let target = compute_canonical_path(
            Path::new(&source_path),
            &channel_name,
            &title,
            &youtube_id,
            is_short,
            extension,
        );
        if current == target {
            preview.already_organized += 1;
            continue;
        }
        let collision = target.exists();
        if collision {
            preview.collisions += 1;
        }
        preview.required_bytes = preview
            .required_bytes
            .saturating_add(u64::try_from(size_bytes).unwrap_or(0));
        preview.moves.push(OrganizePreviewItem {
            media_id,
            current_path,
            target_path: target.to_string_lossy().into_owned(),
            size_bytes,
            collision,
        });
    }
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_path_segment() {
        assert_eq!(sanitize_path_segment("Example Creator"), "Example Creator");
        assert_eq!(
            sanitize_path_segment("Example Creator / Ambient"),
            "Example Creator Ambient"
        );
        assert_eq!(
            sanitize_path_segment("  ...Special: Name?*  "),
            "Special Name"
        );
        assert_eq!(sanitize_path_segment("   "), "Desconhecido");
    }

    #[test]
    fn test_compute_canonical_path_regular() {
        let root = Path::new("/media/videos");
        let path = compute_canonical_path(
            root,
            "Example Creator",
            "【星空】夜の旅",
            "abcdefghijk",
            false,
            "mp4",
        );
        assert_eq!(
            path,
            PathBuf::from("/media/videos/Example Creator/【星空】夜の旅 [abcdefghijk].mp4")
        );
    }

    #[test]
    fn test_compute_canonical_path_shorts() {
        let root = Path::new("/media/videos");
        let path = compute_canonical_path(
            root,
            "Example Creator",
            "Short Demo Clip",
            "abc12345678",
            true,
            "webm",
        );
        assert_eq!(
            path,
            PathBuf::from(
                "/media/videos/Example Creator/Shorts/Short Demo Clip [abc12345678].webm"
            )
        );
    }

    #[test]
    fn test_compute_canonical_path_does_not_duplicate_id() {
        let root = Path::new("/media/videos");
        let path = compute_canonical_path(
            root,
            "Example Creator",
            "Night Journey [abcdefghijk]",
            "abcdefghijk",
            false,
            "mp4",
        );
        assert_eq!(
            path,
            PathBuf::from("/media/videos/Example Creator/Night Journey [abcdefghijk].mp4")
        );
    }

    #[tokio::test]
    async fn companion_subtitle_is_journaled_verified_and_undoable() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let root = std::env::temp_dir().join(format!(
            "mymedia_subtitle_journal_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let video = root.join("video.mp4");
        let subtitle = root.join("video.pt.vtt");
        let target = root.join("Channel").join("video [abcdefghijk].mp4");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(&subtitle, b"WEBVTT\n").unwrap();
        let source_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_sources (path, mode) VALUES (?, 'managed') RETURNING id",
        )
        .bind(root.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        let media_id =
            sqlx::query_scalar::<_, i64>("INSERT INTO media (title) VALUES ('video') RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        let file_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_files
                (path, title, extension, size_bytes, modified_at_unix, media_id)
             VALUES (?, 'video', 'mp4', 5, 1, ?) RETURNING id",
        )
        .bind(video.to_string_lossy().as_ref())
        .bind(media_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let subtitle_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO media_subtitles (media_id, lang_code, label, source, file_path)
             VALUES (?, 'pt', 'Português', 'local', ?) RETURNING id",
        )
        .bind(media_id)
        .bind(subtitle.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        let operation_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO filesystem_operations
                (operation_type, source_id, media_file_id, from_path, to_path, state)
             VALUES ('organize', ?, ?, ?, ?, 'planned') RETURNING id",
        )
        .bind(source_id)
        .bind(file_id)
        .bind(video.to_string_lossy().as_ref())
        .bind(target.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();

        plan_companion_subtitles(&pool, operation_id, media_id, &target)
            .await
            .unwrap();
        recover_companion_subtitles(&pool, operation_id)
            .await
            .unwrap();
        let moved_path = target.parent().unwrap().join("video [abcdefghijk].pt.vtt");
        assert!(moved_path.is_file());
        let db_path =
            sqlx::query_scalar::<_, String>("SELECT file_path FROM media_subtitles WHERE id = ?")
                .bind(subtitle_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(db_path, moved_path.to_string_lossy());

        undo_companion_subtitles(&pool, operation_id).await.unwrap();
        assert!(subtitle.is_file());
        assert!(!moved_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
