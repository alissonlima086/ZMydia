use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubtitleSettings {
    pub auto_download: bool,
    pub languages: Vec<String>,
    pub include_auto_generated: bool,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            auto_download: true,
            languages: vec!["pt".to_string(), "en".to_string()],
            include_auto_generated: false,
        }
    }
}

pub async fn load_subtitle_settings(pool: &SqlitePool) -> SubtitleSettings {
    let row =
        sqlx::query_as::<_, (String,)>("SELECT value_json FROM settings WHERE key = 'subtitles'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some((json,)) = row {
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        SubtitleSettings::default()
    }
}

pub async fn save_subtitle_settings(
    pool: &SqlitePool,
    settings: &SubtitleSettings,
) -> Result<(), sqlx::Error> {
    let mut normalized_langs: Vec<String> = settings
        .languages
        .iter()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    normalized_langs.sort();
    normalized_langs.dedup();

    let clean = SubtitleSettings {
        auto_download: settings.auto_download,
        languages: normalized_langs,
        include_auto_generated: settings.include_auto_generated,
    };
    let json_str = serde_json::to_string(&clean).unwrap_or_else(|_| "{}".to_string());

    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES ('subtitles', ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = excluded.updated_at",
    )
    .bind(json_str)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SubtitleTrack {
    pub id: i64,
    pub media_id: i64,
    pub lang_code: String,
    pub label: String,
    pub source: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrackSummary {
    pub id: i64,
    pub lang_code: String,
    pub label: String,
    pub source: String,
    pub url: String,
}

pub fn lang_code_to_label(code: &str) -> String {
    match code.to_lowercase().as_str() {
        "pt" | "por" => "Português".to_string(),
        "pt-br" | "pt_br" => "Português (Brasil)".to_string(),
        "pt-pt" | "pt_pt" => "Português (Portugal)".to_string(),
        "en" | "eng" => "English".to_string(),
        "en-us" | "en_us" => "English (US)".to_string(),
        "en-gb" | "en_gb" => "English (UK)".to_string(),
        "es" | "spa" => "Español".to_string(),
        "es-419" => "Español (Latinoamérica)".to_string(),
        "fr" | "fra" | "fre" => "Français".to_string(),
        "de" | "deu" | "ger" => "Deutsch".to_string(),
        "it" | "ita" => "Italiano".to_string(),
        "ja" | "jpn" => "日本語".to_string(),
        "ko" | "kor" => "한국어".to_string(),
        "zh" | "zho" | "chi" => "中文 (Simplificado)".to_string(),
        "zh-hant" | "zh-tw" => "中文 (Tradicional)".to_string(),
        "ru" | "rus" => "Русский".to_string(),
        "id" | "ind" => "Bahasa Indonesia".to_string(),
        "th" | "tha" => "ไทย".to_string(),
        "vi" | "vie" => "Tiếng Việt".to_string(),
        "ar" | "ara" => "العربية".to_string(),
        "hi" | "hin" => "हिन्दी".to_string(),
        "nl" | "nld" | "dut" => "Nederlands".to_string(),
        "pl" | "pol" => "Polski".to_string(),
        "sv" | "swe" => "Svenska".to_string(),
        "tr" | "tur" => "Türkçe".to_string(),
        "uk" | "ukr" => "Українська".to_string(),
        other => other.to_uppercase(),
    }
}

pub async fn list_subtitles_for_media(
    pool: &SqlitePool,
    media_id: i64,
) -> Result<Vec<SubtitleTrackSummary>, sqlx::Error> {
    let rows = sqlx::query_as::<_, SubtitleTrack>(
        "SELECT id, media_id, lang_code, label, source, file_path
         FROM media_subtitles
         WHERE media_id = ?
         ORDER BY CASE WHEN lang_code LIKE 'pt%' THEN 1 WHEN lang_code LIKE 'en%' THEN 2 ELSE 3 END, label ASC",
    )
    .bind(media_id)
    .fetch_all(pool)
    .await?;

    let summaries = rows
        .into_iter()
        .map(|row| SubtitleTrackSummary {
            id: row.id,
            lang_code: row.lang_code.clone(),
            label: row.label,
            source: row.source,
            url: format!("/api/media/{}/subtitles/{}", row.media_id, row.lang_code),
        })
        .collect();

    Ok(summaries)
}

pub async fn get_subtitle_file(
    pool: &SqlitePool,
    media_id: i64,
    lang_code: &str,
) -> Result<Option<PathBuf>, sqlx::Error> {
    let row = sqlx::query_scalar::<_, String>(
        "SELECT file_path FROM media_subtitles WHERE media_id = ? AND lang_code = ? LIMIT 1",
    )
    .bind(media_id)
    .bind(lang_code)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(PathBuf::from))
}

#[allow(clippy::too_many_arguments)]
pub async fn download_youtube_subtitles(
    pool: &SqlitePool,
    media_id: i64,
    video_id: &str,
    subtitles_dir: &Path,
    languages: &[String],
    include_auto: bool,
    use_cookies: bool,
    browser: &str,
) -> Result<usize, String> {
    if languages.is_empty() {
        return Ok(0);
    }

    std::fs::create_dir_all(subtitles_dir)
        .map_err(|e| format!("falha ao criar pasta de legendas: {e}"))?;

    let langs_arg = languages.join(",");
    let mut cmd = tokio::process::Command::new(crate::config::external_command(
        "MYMEDIA_YTDLP_BIN",
        "yt-dlp",
    ));
    cmd.env_remove("LD_LIBRARY_PATH");
    if use_cookies && crate::youtube_worker::is_valid_browser(browser) {
        cmd.args(["--cookies-from-browser", browser.trim()]);
    }

    let output_pattern = subtitles_dir.join(format!("{media_id}_%(id)s.%(ext)s"));
    let mut args = vec![
        "--skip-download".to_string(),
        "--write-sub".to_string(),
        "--convert-subs".to_string(),
        "vtt".to_string(),
        "--sub-langs".to_string(),
        langs_arg,
        "-o".to_string(),
        output_pattern.to_string_lossy().to_string(),
    ];

    if include_auto {
        args.push("--write-auto-sub".to_string());
    }

    args.push(format!("https://youtu.be/{video_id}"));
    cmd.args(&args);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("falha ao executar yt-dlp para legendas: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(media_id, %video_id, stderr = %err, "yt-dlp returned an error while fetching subtitles");
    }

    // Identificar arquivos gerados no diretório
    let prefix = format!("{media_id}_{video_id}.");
    let mut saved_count = 0;

    let mut dir = match tokio::fs::read_dir(subtitles_dir).await {
        Ok(d) => d,
        Err(_) => return Ok(0),
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with(&prefix) && file_name.ends_with(".vtt") {
            // Extrair o código de idioma do nome: {media_id}_{video_id}.{lang}.vtt
            let remainder = &file_name[prefix.len()..file_name.len() - 4];
            let (lang_code, is_auto) = if let Some(stripped) = remainder.strip_suffix(".auto") {
                (stripped, true)
            } else {
                (remainder, false)
            };

            let label = lang_code_to_label(lang_code);
            let source = if is_auto { "youtube_auto" } else { "youtube" };
            let full_path = entry.path().to_string_lossy().to_string();

            let res = sqlx::query(
                "INSERT INTO media_subtitles (media_id, lang_code, label, source, file_path)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(media_id, lang_code, source) DO UPDATE SET
                    label = excluded.label,
                    file_path = excluded.file_path",
            )
            .bind(media_id)
            .bind(lang_code)
            .bind(label)
            .bind(source)
            .bind(&full_path)
            .execute(pool)
            .await;

            if res.is_ok() {
                saved_count += 1;
            }
        }
    }

    Ok(saved_count)
}

pub async fn sync_subtitles_for_media(
    pool: &SqlitePool,
    cache_dir: &Path,
    media_id: i64,
    languages: Option<Vec<String>>,
) -> Result<usize, String> {
    let youtube_id =
        sqlx::query_scalar::<_, Option<String>>("SELECT youtube_id FROM media WHERE id = ?")
            .bind(media_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .flatten();

    let Some(yt_id) = youtube_id else {
        return Err("Vídeo não possui YouTube ID vinculado".to_string());
    };

    let settings = load_subtitle_settings(pool).await;
    let yt_settings = crate::youtube_worker::load_youtube_settings(pool).await;

    let target_langs = languages.unwrap_or(settings.languages);
    let subs_dir = cache_dir.parent().unwrap_or(cache_dir).join("subtitles");

    download_youtube_subtitles(
        pool,
        media_id,
        &yt_id,
        &subs_dir,
        &target_langs,
        settings.include_auto_generated,
        yt_settings.use_browser_cookies,
        &yt_settings.browser,
    )
    .await
}

pub async fn sync_all_pending_subtitles(
    pool: &SqlitePool,
    cache_dir: &Path,
) -> Result<usize, String> {
    let settings = load_subtitle_settings(pool).await;
    if !settings.auto_download || settings.languages.is_empty() {
        return Ok(0);
    }

    let yt_settings = crate::youtube_worker::load_youtube_settings(pool).await;
    let subs_dir = cache_dir.parent().unwrap_or(cache_dir).join("subtitles");

    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT m.id, m.youtube_id
         FROM media m
         WHERE m.is_available = 1 AND m.youtube_id IS NOT NULL AND m.youtube_id != ''",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut total_downloaded = 0;
    for (media_id, yt_id) in rows {
        if let Ok(count) = download_youtube_subtitles(
            pool,
            media_id,
            &yt_id,
            &subs_dir,
            &settings.languages,
            settings.include_auto_generated,
            yt_settings.use_browser_cookies,
            &yt_settings.browser,
        )
        .await
        {
            total_downloaded += count;
        }
    }

    Ok(total_downloaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_code_to_label() {
        assert_eq!(lang_code_to_label("pt"), "Português");
        assert_eq!(lang_code_to_label("pt-BR"), "Português (Brasil)");
        assert_eq!(lang_code_to_label("en"), "English");
        assert_eq!(lang_code_to_label("es"), "Español");
        assert_eq!(lang_code_to_label("ja"), "日本語");
        assert_eq!(lang_code_to_label("xyz"), "XYZ");
    }

    #[tokio::test]
    async fn test_subtitle_settings_persistence() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let default_settings = load_subtitle_settings(&pool).await;
        assert!(default_settings.auto_download);
        assert_eq!(default_settings.languages, vec!["pt", "en"]);

        let custom = SubtitleSettings {
            auto_download: false,
            languages: vec!["PT".to_string(), "es".to_string(), " pt ".to_string()],
            include_auto_generated: true,
        };
        save_subtitle_settings(&pool, &custom).await.unwrap();

        let loaded = load_subtitle_settings(&pool).await;
        assert!(!loaded.auto_download);
        assert_eq!(loaded.languages, vec!["es", "pt"]);
        assert!(loaded.include_auto_generated);
    }

    #[tokio::test]
    async fn test_media_subtitles_crud() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO media (id, title, is_available) VALUES (1, 'Video 1', 1)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO media_subtitles (media_id, lang_code, label, source, file_path)
             VALUES (1, 'pt', 'Português', 'youtube', '/tmp/1_pt.vtt')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let list = list_subtitles_for_media(&pool, 1).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].lang_code, "pt");
        assert_eq!(list[0].label, "Português");
        assert_eq!(list[0].url, "/api/media/1/subtitles/pt");

        let file = get_subtitle_file(&pool, 1, "pt").await.unwrap();
        assert_eq!(file, Some(PathBuf::from("/tmp/1_pt.vtt")));

        let missing = get_subtitle_file(&pool, 1, "en").await.unwrap();
        assert!(missing.is_none());
    }
}
