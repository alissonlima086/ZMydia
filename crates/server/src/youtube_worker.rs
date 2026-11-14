use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, Notify};

#[derive(Clone)]
pub struct YoutubeWorkerService {
    #[allow(dead_code)]
    pool: SqlitePool,
    #[allow(dead_code)]
    cache_dir: Arc<PathBuf>,
    notify: Arc<Notify>,
    is_running: Arc<AtomicBool>,
    status: Arc<Mutex<YoutubeWorkerStatus>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct YoutubeWorkerStatus {
    pub is_running: bool,
    pub total_candidates: u64,
    pub processed: u64,
    pub successful: u64,
    pub failed: u64,
    pub bot_blocked: bool,
    pub current_title: Option<String>,
    pub last_run_at: Option<String>,
}

pub const MAX_SYNC_ATTEMPTS: i64 = 2;

fn youtube_base_url() -> String {
    crate::config::external_base_url("MYMEDIA_YOUTUBE_BASE_URL", "https://www.youtube.com")
}

fn youtube_image_base_url() -> String {
    crate::config::external_base_url("MYMEDIA_YOUTUBE_IMAGE_BASE_URL", "https://img.youtube.com")
}

pub async fn record_sync_failure(pool: &SqlitePool, media_id: i64, status: &str) {
    if let Err(error) = sqlx::query(
        "INSERT INTO youtube_sync_attempts (media_id, attempts, last_status, last_attempt_at)
         VALUES (?, 1, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(media_id) DO UPDATE SET
            attempts = youtube_sync_attempts.attempts + 1,
            last_status = excluded.last_status,
            last_attempt_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(media_id)
    .bind(status)
    .execute(pool)
    .await
    {
        tracing::error!(%error, media_id, "failed to persist YouTube synchronization error");
    }
}

pub async fn reset_failed_attempts(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM youtube_sync_attempts")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

pub async fn reset_unofficial_failed_attempts(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM youtube_sync_attempts
         WHERE media_id IN (
             SELECT m.id FROM media m
             LEFT JOIN thumbnails t ON t.media_id = m.id
             WHERE (t.is_official IS NULL OR t.is_official = 0)
         )",
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub const ALLOWED_BROWSERS: &[&str] = &[
    "brave", "chrome", "chromium", "edge", "firefox", "opera", "safari", "vivaldi", "whale",
];

pub fn is_valid_browser(browser: &str) -> bool {
    ALLOWED_BROWSERS.contains(&browser.trim().to_ascii_lowercase().as_str())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct YoutubeSettings {
    pub use_browser_cookies: bool,
    pub browser: String,
}

impl Default for YoutubeSettings {
    fn default() -> Self {
        Self {
            use_browser_cookies: false,
            browser: "firefox".to_string(),
        }
    }
}

pub async fn load_youtube_settings(pool: &SqlitePool) -> YoutubeSettings {
    let mut settings = match sqlx::query_scalar::<_, String>(
        "SELECT value_json FROM settings WHERE key = 'youtube_cookies'",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(raw)) => serde_json::from_str::<YoutubeSettings>(&raw).unwrap_or_default(),
        _ => YoutubeSettings::default(),
    };
    if !is_valid_browser(&settings.browser) {
        settings.browser = "firefox".to_string();
    }
    settings
}

pub async fn save_youtube_settings(
    pool: &SqlitePool,
    settings: &YoutubeSettings,
) -> Result<(), sqlx::Error> {
    let mut sanitized = settings.clone();
    sanitized.browser = sanitized.browser.trim().to_ascii_lowercase();
    if !is_valid_browser(&sanitized.browser) {
        sanitized.browser = "firefox".to_string();
    }
    let json_str = serde_json::to_string(&sanitized).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_at)
         VALUES ('youtube_cookies', ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET
            value_json = excluded.value_json,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(json_str)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug)]
pub struct YoutubeMetadata {
    pub youtube_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub handle: Option<String>,
    pub avatar_url: Option<String>,
    pub published_at: Option<String>,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
}

impl YoutubeWorkerService {
    pub fn start(pool: SqlitePool, cache_dir: PathBuf) -> Self {
        let notify = Arc::new(Notify::new());
        let is_running = Arc::new(AtomicBool::new(false));
        let cache_dir = Arc::new(cache_dir);
        let status = Arc::new(Mutex::new(YoutubeWorkerStatus {
            is_running: false,
            total_candidates: 0,
            processed: 0,
            successful: 0,
            failed: 0,
            bot_blocked: false,
            current_title: None,
            last_run_at: None,
        }));

        let service = Self {
            pool: pool.clone(),
            cache_dir: cache_dir.clone(),
            notify: notify.clone(),
            is_running: is_running.clone(),
            status: status.clone(),
        };

        let worker_pool = pool;
        let worker_cache_dir = cache_dir;
        let worker_notify = notify;
        let worker_is_running = is_running;
        let worker_status = status;

        tokio::spawn(async move {
            loop {
                worker_notify.notified().await;
                if worker_is_running.swap(true, Ordering::SeqCst) {
                    continue;
                }

                {
                    let mut st = worker_status.lock().await;
                    st.is_running = true;
                    st.processed = 0;
                    st.successful = 0;
                    st.failed = 0;
                    st.bot_blocked = false;
                    st.current_title = None;
                }

                run_sync_cycle(&worker_pool, &worker_cache_dir, &worker_status).await;

                {
                    let mut st = worker_status.lock().await;
                    st.is_running = false;
                    st.current_title = None;
                    st.last_run_at = Some(chrono_now());
                }
                worker_is_running.store(false, Ordering::SeqCst);
            }
        });

        // Delay trigger on startup
        let startup_service = service.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            startup_service.trigger();
        });

        service
    }

    pub fn trigger(&self) {
        self.notify.notify_one();
    }

    pub async fn get_status(&self) -> YoutubeWorkerStatus {
        let mut st = self.status.lock().await.clone();
        st.is_running = self.is_running.load(Ordering::SeqCst);
        st
    }
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

pub fn extract_youtube_id(filename: &str) -> Option<String> {
    let dot_pos = filename.rfind('.')?;
    let base = &filename[..dot_pos];
    if !base.ends_with(']') {
        return None;
    }
    let open_bracket = base.rfind('[')?;
    let candidate = &base[open_bracket + 1..base.len() - 1];
    if candidate.len() == 11
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(candidate.to_string())
    } else {
        None
    }
}

pub fn extract_short_description(html: &str) -> Option<String> {
    let key = "\"shortDescription\":\"";
    let start_idx = html.find(key)? + key.len();
    let rest = &html[start_idx..];
    let mut end_idx = 0;
    let mut escaped = false;
    for (i, b) in rest.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if b == b'\\' {
            escaped = true;
        } else if b == b'"' {
            end_idx = i;
            break;
        }
    }
    if end_idx == 0 {
        return None;
    }
    let raw = &rest[..end_idx];
    serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
}

pub fn extract_avatar_url(html: &str) -> Option<String> {
    for prefix in &[
        "https://yt3.googleusercontent.com/",
        "https://yt3.ggpht.com/",
    ] {
        if let Some(start_idx) = html.find(prefix) {
            let rest = &html[start_idx..];
            if let Some(end_idx) = rest.find('"').or_else(|| rest.find('\'')) {
                let url = &rest[..end_idx];
                let base = url.split('=').next().unwrap_or(url);
                return Some(format!("{base}=s176-c-k-c0x00ffffff-no-rj"));
            }
        }
    }
    None
}

pub fn extract_publish_date(html: &str) -> Option<String> {
    if let Some(pos) = html.find("<meta itemprop=\"datePublished\" content=\"") {
        let after = &html[pos + 40..];
        if let Some(end) = after.find('"') {
            let val = &after[..end];
            if val.len() >= 10 && val.chars().nth(4) == Some('-') {
                return Some(val[..10].to_string());
            }
        }
    }
    if let Some(pos) = html.find("\"publishDate\":\"") {
        let after = &html[pos + 15..];
        if let Some(end) = after.find('"') {
            let val = &after[..end];
            if val.len() >= 10 && val.chars().nth(4) == Some('-') {
                return Some(val[..10].to_string());
            }
        }
    }
    None
}

pub fn extract_author(html: &str) -> Option<String> {
    let key = "\"author\":\"";
    let start_idx = html.find(key)? + key.len();
    let rest = &html[start_idx..];
    let end_idx = rest.find('"')?;
    serde_json::from_str::<String>(&format!("\"{}\"", &rest[..end_idx])).ok()
}

pub fn extract_channel_id(html: &str) -> Option<String> {
    for key in &[
        "\"channelId\":\"",
        "\"browseId\":\"",
        "\"externalChannelId\":\"",
    ] {
        let mut search_from = 0;
        while let Some(rel_pos) = html[search_from..].find(key) {
            let start = search_from + rel_pos + key.len();
            let rest = &html[start..];
            if let Some(end) = rest.find('"') {
                let id = &rest[..end];
                if id.starts_with("UC") && id.len() == 24 {
                    return Some(id.to_string());
                }
                search_from = start + end;
            } else {
                break;
            }
        }
    }
    let key = "\"channelId\":\"";
    let start_idx = html.find(key)? + key.len();
    let rest = &html[start_idx..];
    let end_idx = rest.find('"')?;
    Some(rest[..end_idx].to_string())
}

pub fn extract_handle(html: &str) -> Option<String> {
    for key in &[
        "\"canonicalBaseUrl\":\"/@",
        "\"commandMetadata\":{\"webCommandMetadata\":{\"url\":\"/@",
    ] {
        if let Some(pos) = html.find(key) {
            let after = &html[pos + key.len() - 1..]; // keep '@'
            if let Some(end) = after.find('"') {
                let handle = &after[..end];
                if !handle.is_empty() {
                    return Some(handle.to_string());
                }
            }
        }
    }
    None
}

pub fn extract_channel_from_search(html: &str) -> (Option<String>, Option<String>) {
    let mut channel_id = None;
    let mut handle = None;

    if let Some(pos) = html.find("\"channelRenderer\":") {
        let section = &html[pos..pos + 5000.min(html.len() - pos)];
        if let Some(id_pos) = section.find("\"channelId\":\"") {
            let after = &section[id_pos + 13..];
            if let Some(end) = after.find('"') {
                let id = &after[..end];
                if id.starts_with("UC") && id.len() == 24 {
                    channel_id = Some(id.to_string());
                }
            }
        }
        if let Some(h_pos) = section.find("\"canonicalBaseUrl\":\"/@") {
            let after = &section[h_pos + 21..]; // keep '@'
            if let Some(end) = after.find('"') {
                let h = &after[..end];
                if !h.is_empty() {
                    handle = Some(h.to_string());
                }
            }
        }
    }

    if channel_id.is_none() {
        channel_id = extract_channel_id(html);
    }
    if handle.is_none() {
        handle = extract_handle(html);
    }

    (channel_id, handle)
}

pub fn url_encode_query(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(encoded, "%{:02X}", b);
            }
        }
    }
    encoded
}

pub async fn fetch_url_html(url: &str) -> Option<String> {
    let mut command =
        tokio::process::Command::new(crate::config::external_command("MYMEDIA_CURL_BIN", "curl"));
    command
        .kill_on_drop(true)
        .args([
            "-s",
            "-L",
            "-A",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "--max-time",
            "10",
            url,
        ]);
    let output = tokio::time::timeout(Duration::from_secs(12), command.output())
        .await
        .ok()?
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

pub async fn download_and_save_channel_avatar(
    channel_id: &str,
    avatar_url: &str,
    _sample_file_path: Option<&str>,
    _source_path: Option<&str>,
) -> bool {
    if !crate::validation::is_valid_channel_id(channel_id) {
        return false;
    }
    let mut command =
        tokio::process::Command::new(crate::config::external_command("MYMEDIA_CURL_BIN", "curl"));
    command
        .kill_on_drop(true)
        .args([
            "-s",
            "-L",
            "-A",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "--max-time",
            "10",
            avatar_url,
        ]);
    let output = tokio::time::timeout(Duration::from_secs(12), command.output())
        .await
        .ok()
        .and_then(Result::ok);

    let Some(out) = output else {
        return false;
    };
    if !out.status.success() || out.stdout.is_empty() {
        return false;
    }

    true
}

pub async fn sync_single_channel_metadata(pool: &sqlx::SqlitePool, channel_id: &str) -> bool {
    if !crate::validation::is_valid_channel_id(channel_id) {
        return false;
    }
    let query = "
        SELECT c.id, c.name, c.youtube_channel_id, c.handle, c.avatar_url,
               (SELECT m.youtube_id FROM media m
                JOIN media_channels mc ON mc.media_id = m.id
                WHERE mc.channel_id = c.id AND m.youtube_id IS NOT NULL
                LIMIT 1) AS sample_yt_id,
               (SELECT mf.path FROM media_files mf
                JOIN media_channels mc ON mc.media_id = mf.media_id
                WHERE mc.channel_id = c.id AND mf.is_available = 1
                LIMIT 1) AS sample_file_path,
               (SELECT s.path FROM media_sources s WHERE s.enabled = 1 AND s.removed_at IS NULL LIMIT 1) AS source_path
        FROM channels c
        WHERE c.id = ?
    ";
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(query)
    .bind(channel_id)
    .fetch_optional(pool)
    .await;

    let Ok(Some((
        id,
        name,
        mut yt_channel_id,
        mut handle,
        mut avatar_url,
        sample_yt_id,
        sample_file_path,
        source_path,
    ))) = row
    else {
        return false;
    };

    let mut resolved_video_id = sample_yt_id;
    if resolved_video_id.is_none() {
        // Look through media files for this channel to extract a YouTube ID from filename
        let file_paths = sqlx::query_scalar::<_, String>(
            "SELECT mf.path FROM media_files mf
             JOIN media_channels mc ON mc.media_id = mf.media_id
             WHERE mc.channel_id = ? AND mf.is_available = 1
             LIMIT 10",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for fp in file_paths {
            if let Some(yid) = extract_youtube_id(&fp) {
                resolved_video_id = Some(yid);
                break;
            }
        }
    }

    if let Some(vid) = &resolved_video_id {
        let watch_url = format!("{}/watch?v={vid}", youtube_base_url());
        if let Some(html) = fetch_url_html(&watch_url).await {
            if yt_channel_id.is_none() {
                yt_channel_id = extract_channel_id(&html);
            }
            if handle.is_none() {
                handle = extract_handle(&html);
            }
            if avatar_url.is_none() {
                avatar_url = extract_avatar_url(&html);
            }
        }
    }

    if avatar_url.is_none() || yt_channel_id.is_none() {
        if let Some(ref cid) = yt_channel_id {
            let channel_url = format!("{}/channel/{cid}", youtube_base_url());
            if let Some(html) = fetch_url_html(&channel_url).await {
                if avatar_url.is_none() {
                    avatar_url = extract_avatar_url(&html);
                }
                if handle.is_none() {
                    handle = extract_handle(&html);
                }
            }
        } else if let Some(ref h) = handle {
            let handle_str = h.strip_prefix('@').unwrap_or(h);
            let handle_url = format!("{}/@{handle_str}", youtube_base_url());
            if let Some(html) = fetch_url_html(&handle_url).await {
                if avatar_url.is_none() {
                    avatar_url = extract_avatar_url(&html);
                }
                if yt_channel_id.is_none() {
                    yt_channel_id = extract_channel_id(&html);
                }
            }
        } else {
            // Search by channel name
            let search_url = format!(
                "{}/results?search_query={}&sp=EgIQAg%253D%253D",
                youtube_base_url(),
                url_encode_query(&name)
            );
            if let Some(search_html) = fetch_url_html(&search_url).await {
                let (found_cid, found_handle) = extract_channel_from_search(&search_html);
                if yt_channel_id.is_none() {
                    yt_channel_id = found_cid;
                }
                if handle.is_none() {
                    handle = found_handle;
                }
                if let Some(ref cid) = yt_channel_id {
                    let channel_url = format!("{}/channel/{cid}", youtube_base_url());
                    if let Some(html) = fetch_url_html(&channel_url).await
                        && avatar_url.is_none()
                    {
                        avatar_url = extract_avatar_url(&html);
                    }
                }
            }
        }
    }

    if let Err(error) = sqlx::query(
        "UPDATE channels SET
            youtube_channel_id = COALESCE(?, youtube_channel_id),
            handle = COALESCE(?, handle),
            avatar_url = COALESCE(?, avatar_url),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(yt_channel_id.as_deref())
    .bind(handle.as_deref())
    .bind(avatar_url.as_deref())
    .bind(&id)
    .execute(pool)
    .await
    {
        tracing::error!(%error, channel_id = %id, "failed to persist channel metadata");
        return false;
    }

    if let Some(ref a_url) = avatar_url {
        download_and_save_channel_avatar(
            &id,
            a_url,
            sample_file_path.as_deref(),
            source_path.as_deref(),
        )
        .await;
    }

    avatar_url.is_some() || yt_channel_id.is_some()
}

pub async fn sync_pending_channels(pool: &sqlx::SqlitePool) {
    let query = "
        SELECT c.id
        FROM channels c
        JOIN media_channels mc ON mc.channel_id = c.id
        WHERE (c.avatar_url IS NULL OR c.handle IS NULL OR c.youtube_channel_id IS NULL)
        GROUP BY c.id
        ORDER BY count(mc.media_id) DESC
    ";
    let Ok(channel_ids) = sqlx::query_scalar::<_, String>(query).fetch_all(pool).await else {
        return;
    };

    if channel_ids.is_empty() {
        return;
    }

    tracing::info!(
        pending_channels = channel_ids.len(),
        "starting synchronization of pending channels"
    );

    for ch_id in channel_ids {
        if !sync_single_channel_metadata(pool, &ch_id).await {
            tracing::warn!(channel_id = %ch_id, "channel synchronization remained pending");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

pub fn sanitize_tag_name(s: &str) -> String {
    let unescaped = s
        .replace(r"\u0026", "&")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    unescaped.trim().to_string()
}

pub fn extract_keywords(html: &str) -> Vec<String> {
    if let Some(pos) = html.find("<meta name=\"keywords\" content=\"") {
        let after = &html[pos + 31..];
        if let Some(end) = after.find('"') {
            return after[..end]
                .split(',')
                .map(sanitize_tag_name)
                .filter(|s| {
                    let low = s.to_lowercase();
                    !s.is_empty()
                        && s.len() <= 80
                        && !low.starts_with("yt:")
                        && low != "vídeo"
                        && low != "compartilhamento"
                        && low != "celular com câmera"
                        && low != "video"
                        && low != "sharing"
                        && low != "camera phone"
                })
                .collect();
        }
    }
    Vec::new()
}

pub fn extract_category(html: &str) -> Option<String> {
    if let Some(pos) = html.find("\"category\":\"") {
        let after = &html[pos + 12..];
        if let Some(end) = after.find('"') {
            let cat = serde_json::from_str::<String>(&format!("\"{}\"", &after[..end])).ok()?;
            let cleaned = sanitize_tag_name(&cat);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

pub fn fold_char(c: char) -> char {
    match c {
        'a'..='z' | '0'..='9' => c,
        'A'..='Z' => c.to_ascii_lowercase(),
        'á' | 'à' | 'ã' | 'â' | 'ä' | 'Á' | 'À' | 'Ã' | 'Â' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'õ' | 'ô' | 'ö' | 'Ó' | 'Ò' | 'Õ' | 'Ô' | 'Ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        other => {
            let mut lower_chars = other.to_lowercase();
            let first = lower_chars.next().unwrap_or(other);
            match first {
                'á' | 'à' | 'ã' | 'â' | 'ä' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'õ' | 'ô' | 'ö' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                'ñ' => 'n',
                _ => first,
            }
        }
    }
}

pub fn normalize_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        let folded = fold_char(c);
        if folded.is_alphanumeric() {
            out.push(folded);
        } else {
            out.push(' ');
        }
    }
    out
}

pub fn extract_significant_tokens(raw: &str) -> Vec<String> {
    let clean = raw
        .trim_end_matches(".mp4")
        .trim_end_matches(".webm")
        .trim_end_matches(".mkv")
        .trim_end_matches(".avi")
        .trim_end_matches(".mov");

    let normalized = normalize_string(clean);
    normalized
        .split_whitespace()
        .filter(|w| {
            let s = *w;
            s != "1080p"
                && s != "720p"
                && s != "4k"
                && s != "60fps"
                && s != "mp4"
                && s != "mkv"
                && s != "webm"
        })
        .map(|w| w.to_string())
        .collect()
}

pub fn titles_match_flexibly(local_title: &str, yt_title: &str) -> bool {
    let local_tokens = extract_significant_tokens(local_title);
    let yt_tokens = extract_significant_tokens(yt_title);

    if local_tokens.is_empty() || yt_tokens.is_empty() {
        return true;
    }

    let local_norm = local_tokens.join(" ");
    let yt_norm = yt_tokens.join(" ");

    // Substring containment
    if yt_norm.contains(&local_norm) || local_norm.contains(&yt_norm) {
        return true;
    }

    // Significant token overlap: >= 70% of local tokens present in YouTube title
    let mut local_matches = 0;
    for token in &local_tokens {
        if yt_tokens.iter().any(|yt_t| {
            yt_t == token || (token.len() >= 4 && (yt_t.contains(token) || token.contains(yt_t)))
        }) {
            local_matches += 1;
        }
    }
    let local_ratio = (local_matches as f64) / (local_tokens.len() as f64);

    let mut yt_matches = 0;
    for token in &yt_tokens {
        if local_tokens.iter().any(|loc_t| {
            loc_t == token || (token.len() >= 4 && (loc_t.contains(token) || token.contains(loc_t)))
        }) {
            yt_matches += 1;
        }
    }
    let yt_ratio = (yt_matches as f64) / (yt_tokens.len() as f64);

    local_ratio >= 0.70 || yt_ratio >= 0.75
}

pub fn duration_matches_closely(expected_duration_ms: Option<i64>, yt_duration_s: f64) -> bool {
    let Some(dur_ms) = expected_duration_ms else {
        return true;
    };
    let dur_s = (dur_ms as f64) / 1000.0;
    let diff = (dur_s - yt_duration_s).abs();
    // Leeway: within 15 seconds or within 5% of total duration (accommodates intros/sponsorblock)
    diff <= 15.0 || (diff / dur_s.max(1.0)) <= 0.05
}

pub fn is_cjk_char(c: char) -> bool {
    matches!(
        c,
        '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{4E00}'..='\u{9FFF}' // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
    )
}

pub fn is_generic_channel_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "music" | "música" | "videos" | "video" | "happy" | "shitpost" | "others"
    )
}

pub fn build_channel_search_query(channel: &str, clean_title: &str) -> String {
    let mut words = Vec::new();
    for w in channel.split_whitespace() {
        words.push(w);
    }
    for w in clean_title.split_whitespace() {
        if let Some(last) = words.last()
            && last.eq_ignore_ascii_case(w)
        {
            continue;
        }
        words.push(w);
    }
    words.join(" ")
}

pub fn sanitize_search_query(raw: &str) -> String {
    let mut clean = raw;
    for ext in [".mp4", ".webm", ".mkv", ".avi", ".mov", ".flv"] {
        if clean.ends_with(ext) {
            clean = &clean[..clean.len() - ext.len()];
        }
    }

    // Strip trailing "- NA" or "_NA" (yt-dlp artifact when uploader was missing)
    let clean = clean.trim();
    let clean = if let Some(stripped) = clean.strip_suffix("- NA") {
        stripped.trim()
    } else if let Some(stripped) = clean.strip_suffix("-NA") {
        stripped.trim()
    } else if let Some(stripped) = clean.strip_suffix("_NA") {
        stripped.trim()
    } else {
        clean
    };

    let mut out = String::with_capacity(clean.len());
    for c in clean.chars() {
        let cp = c as u32;
        // Strip emojis, pictographs, dingbats, variation selectors, zero-width joiners, and kaomoji scripts
        if (0x1F000..=0x1FAFF).contains(&cp)
            || (0x2600..=0x27BF).contains(&cp)
            || (0x2B00..=0x2BFF).contains(&cp)
            || (0xFE00..=0xFE0F).contains(&cp)
            || (0x1400..=0x167F).contains(&cp) // Canadian Aboriginal Syllabics
            || (0x18B0..=0x18FF).contains(&cp)
            || (0xA000..=0xA4CF).contains(&cp) // Yi Syllables & Radicals
            || (0x13A0..=0x13FF).contains(&cp) // Cherokee
            || (0x2800..=0x28FF).contains(&cp) // Braille
            || matches!(c, '\u{200D}' | '\u{200B}' | '™' | '®' | '©')
        {
            out.push(' ');
            continue;
        }

        match c {
            // Slashes & fullwidth division slashes
            '⧸' | '／' | '＼' | '⁄' => out.push(' '),
            // Colons
            '：' | ':' => out.push(' '),
            // Quotes
            '＂' | '"' | '“' | '”' | '‘' | '’' => out.push(' '),
            // Pipes
            '｜' | '|' => out.push(' '),
            // Question & asterisk
            '？' | '?' | '＊' | '*' => out.push(' '),
            // Angle brackets
            '＜' | '<' | '＞' | '>' => out.push(' '),
            // Cross product, tildes, wave dash, underscores
            '⨯' | '〜' | '~' | '_' => out.push(' '),
            // Lenticular brackets, square brackets, quotes, Japanese brackets
            '【' | '】' | '[' | ']' | '『' | '』' | '「' | '」' => out.push(' '),
            other => out.push(other),
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn titles_match_multilingual_or_duration(
    local_title: &str,
    yt_title: &str,
    expected_duration_ms: Option<i64>,
    yt_duration_s: f64,
) -> bool {
    // 1. Direct flexible title matching
    if titles_match_flexibly(local_title, yt_title) {
        return true;
    }

    // 2. Multilingual / Translated title matching with strong duration correlation
    let Some(dur_ms) = expected_duration_ms else {
        return false;
    };
    let local_dur_s = (dur_ms as f64) / 1000.0;
    let diff = (local_dur_s - yt_duration_s).abs();

    // High confidence duration match (diff <= 2.5s and video >= 20s)
    if local_dur_s >= 20.0 && (diff <= 2.5 || (diff / local_dur_s) <= 0.015) {
        let local_tokens = extract_significant_tokens(local_title);
        let yt_tokens = extract_significant_tokens(yt_title);

        // Check if there is at least one shared non-trivial token
        let has_common_anchor = local_tokens.iter().any(|loc| {
            yt_tokens
                .iter()
                .any(|yt| loc == yt || (loc.len() >= 4 && (yt.contains(loc) || loc.contains(yt))))
        });

        if has_common_anchor {
            return true;
        }

        // Cross-script translation detection:
        // If one title has CJK and the other is Latin / Romanized, or vice-versa
        let local_has_cjk = local_title.chars().any(is_cjk_char);
        let yt_has_cjk = yt_title.chars().any(is_cjk_char);
        if local_has_cjk != yt_has_cjk {
            // E.g. Japanese local title vs auto-translated Portuguese YouTube title
            let shared_any_token = local_tokens.iter().any(|loc| {
                loc.len() >= 2
                    && yt_tokens
                        .iter()
                        .any(|yt| yt == loc || yt.contains(loc) || loc.contains(yt))
            });
            if shared_any_token {
                return true;
            }
        }
    }

    false
}

async fn fetch_youtube_watch_details(
    video_id: &str,
    use_cookies: bool,
    browser: &str,
) -> Option<YoutubeMetadata> {
    // 1. Try yt-dlp first
    let mut cmd = tokio::process::Command::new(crate::config::external_command(
        "MYMEDIA_YTDLP_BIN",
        "yt-dlp",
    ));
    cmd.env_remove("LD_LIBRARY_PATH");
    if use_cookies && is_valid_browser(browser) {
        cmd.args(["--cookies-from-browser", browser.trim()]);
    }
    cmd.args([
        "--dump-json",
        "--skip-download",
        "--no-warnings",
        &format!("{}/watch?v={video_id}", youtube_base_url()),
    ]);

    cmd.kill_on_drop(true);
    if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(60), cmd.output()).await
        && output.status.success()
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
    {
        let title = json
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let description = json
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let channel_name = json
            .get("channel")
            .or_else(|| json.get("uploader"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let channel_id = json
            .get("channel_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let handle = json
            .get("uploader_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let tags = json
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let categories = json
            .get("categories")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let published_at = json
            .get("upload_date")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
                    Some(format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8]))
                } else {
                    None
                }
            });

        return Some(YoutubeMetadata {
            youtube_id: video_id.to_string(),
            title,
            description,
            channel_id,
            channel_name,
            handle,
            avatar_url: None,
            published_at,
            tags,
            categories,
        });
    }

    // 2. Fallback to curl on watch page
    let mut fallback =
        tokio::process::Command::new(crate::config::external_command("MYMEDIA_CURL_BIN", "curl"));
    fallback.kill_on_drop(true).args([
            "-s",
            "-L",
            "--max-time",
            "10",
            "-A",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            &format!("{}/watch?v={video_id}", youtube_base_url()),
        ]);
    let output = tokio::time::timeout(Duration::from_secs(12), fallback.output())
        .await
        .ok()?
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let html = String::from_utf8_lossy(&output.stdout);
    let description = extract_short_description(&html);
    let channel_name = extract_author(&html);
    let channel_id = extract_channel_id(&html);
    let avatar_url = extract_avatar_url(&html);
    let published_at = extract_publish_date(&html);
    let tags = extract_keywords(&html);
    let mut categories = Vec::new();
    if let Some(cat) = extract_category(&html) {
        categories.push(cat);
    }

    Some(YoutubeMetadata {
        youtube_id: video_id.to_string(),
        title: None,
        description,
        channel_id,
        channel_name,
        handle: None,
        avatar_url,
        published_at,
        tags,
        categories,
    })
}

async fn search_youtube(
    query: &str,
    local_title: &str,
    expected_duration_ms: Option<i64>,
    use_cookies: bool,
    browser: &str,
) -> Option<YoutubeMetadata> {
    let mut cmd = tokio::process::Command::new(crate::config::external_command(
        "MYMEDIA_YTDLP_BIN",
        "yt-dlp",
    ));
    cmd.env_remove("LD_LIBRARY_PATH");
    if use_cookies && is_valid_browser(browser) {
        cmd.args(["--cookies-from-browser", browser.trim()]);
    }
    cmd.args([
        "--flat-playlist",
        "--dump-json",
        "--no-warnings",
        &format!("ytsearch1:{query}"),
    ]);

    cmd.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let yt_id = json.get("id").and_then(|v| v.as_str())?.to_string();
    let yt_duration = json.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let channel_name = json
        .get("channel")
        .or_else(|| json.get("uploader"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let channel_id = json
        .get("channel_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let handle = json
        .get("uploader_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let snippet_desc = json
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let tags = json
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let categories = json
        .get("categories")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // Flexible duration check
    if !duration_matches_closely(expected_duration_ms, yt_duration) {
        tracing::debug!(
            query,
            yt_id,
            yt_duration,
            "result duration differs from local media beyond tolerance; discarding"
        );
        return None;
    }

    // Flexible title check with multilingual & duration support
    if let Some(yt_t) = &title
        && !titles_match_multilingual_or_duration(
            local_title,
            yt_t,
            expected_duration_ms,
            yt_duration,
        )
    {
        tracing::debug!(
            local_title,
            yt_t,
            "YouTube title differs from local title beyond flexible tolerance; discarding"
        );
        return None;
    }

    Some(YoutubeMetadata {
        youtube_id: yt_id,
        title,
        description: snippet_desc,
        channel_id,
        channel_name,
        handle,
        avatar_url: None,
        published_at: None,
        tags,
        categories,
    })
}

async fn download_youtube_thumbnail(video_id: &str, output_path: &Path) -> Result<(), String> {
    let image_base = youtube_image_base_url();
    let maxres = format!("{image_base}/vi/{video_id}/maxresdefault.jpg");
    let hq = format!("{image_base}/vi/{video_id}/hqdefault.jpg");

    let tmp_path = output_path.with_extension("tmp_yt_dl.jpg");
    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("falha ao preparar cache de thumbnail: {error}"))?;
    }

    // Try maxresdefault first
    let mut maxres_command =
        tokio::process::Command::new(crate::config::external_command("MYMEDIA_CURL_BIN", "curl"));
    maxres_command
        .kill_on_drop(true)
        .args(["-s", "-L", "-f", "--max-time", "15", &maxres, "-o"])
        .arg(&tmp_path);
    let res = tokio::time::timeout(Duration::from_secs(17), maxres_command.status()).await;

    let mut is_good = match res {
        Ok(Ok(s)) if s.success() => {
            if let Ok(meta) = std::fs::metadata(&tmp_path) {
                // If YouTube lacks maxres, it may return a small 120x90 placeholder (< 2500 bytes)
                meta.len() > 3000
            } else {
                false
            }
        }
        _ => false,
    };

    if !is_good {
        let _ = std::fs::remove_file(&tmp_path);
        let mut hq_command = tokio::process::Command::new(crate::config::external_command(
            "MYMEDIA_CURL_BIN",
            "curl",
        ));
        hq_command
            .kill_on_drop(true)
            .args(["-s", "-L", "-f", "--max-time", "15", &hq, "-o"])
            .arg(&tmp_path);
        let res2 = tokio::time::timeout(Duration::from_secs(17), hq_command.status()).await;

        is_good = match res2 {
            Ok(Ok(s)) if s.success() => {
                if let Ok(meta) = std::fs::metadata(&tmp_path) {
                    meta.len() > 1000
                } else {
                    false
                }
            }
            _ => false,
        };
    }

    if !is_good {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("falha ao obter thumbnail para {video_id}"));
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("falha ao preparar destino de thumbnail: {error}"))?;
    }
    tokio::fs::rename(&tmp_path, output_path)
        .await
        .map_err(|e| format!("erro ao salvar thumbnail: {e}"))?;

    Ok(())
}

fn resolve_destination_thumbnail(
    _media_path: &str,
    _source_path: Option<&str>,
    cache_key: &str,
    cache_dir: &Path,
) -> (PathBuf, PathBuf) {
    let filename = format!("{cache_key}.jpg");
    let local_cache = cache_dir.join(&filename);
    (local_cache.clone(), local_cache)
}

async fn run_sync_cycle(
    pool: &SqlitePool,
    cache_dir: &Path,
    status: &Arc<Mutex<YoutubeWorkerStatus>>,
) {
    match consolidate_duplicate_youtube_channels(pool).await {
        Ok(merged) if merged > 0 => {
            tracing::info!(merged, "duplicate YouTube channel aliases consolidated");
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "failed to consolidate duplicate YouTube channel aliases");
        }
    }

    let query = "
        SELECT m.id, mf.path, mf.size_bytes, mf.modified_at_unix, m.duration_ms, s.path AS source_path,
               c.id AS channel_id, c.name AS channel_name, c.youtube_channel_id,
               m.title, m.is_short, m.youtube_id
        FROM media m
        JOIN media_files mf ON mf.media_id = m.id
        LEFT JOIN source_files sf ON sf.media_file_id = mf.id
        LEFT JOIN media_sources s ON s.id = sf.source_id
        LEFT JOIN media_channels mc ON mc.media_id = m.id
        LEFT JOIN channels c ON c.id = mc.channel_id
        LEFT JOIN thumbnails t ON t.media_id = m.id
        LEFT JOIN youtube_sync_attempts ysa ON ysa.media_id = m.id
        WHERE m.is_available = 1 AND mf.is_available = 1
          AND ((t.is_official IS NULL OR t.is_official = 0 OR t.cache_key != (m.id || '-' || mf.size_bytes || '-' || mf.modified_at_unix)) OR mc.channel_id IS NULL)
          AND ysa.media_id IS NULL
        ORDER BY m.id DESC
    ";

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            i64,
            i64,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            bool,
            Option<String>,
        ),
    >(query)
    .fetch_all(pool)
    .await;

    let Ok(candidates) = rows else {
        tracing::error!("failed to fetch youtube_worker candidates");
        return;
    };

    let yt_settings = load_youtube_settings(pool).await;

    {
        let mut st = status.lock().await;
        st.total_candidates = candidates.len() as u64;
    }

    // Sincroniza metadados e avatares de novos canais ou canais pendentes
    sync_pending_channels(pool).await;

    // Consolida mídias duplicadas pelo YouTube ID preservando a melhor qualidade
    if candidates.is_empty() {
        return;
    }

    tracing::info!(
        candidates = candidates.len(),
        use_cookies = yt_settings.use_browser_cookies,
        browser = %yt_settings.browser,
        "starting official YouTube thumbnail and metadata processing"
    );

    let mut had_updates = false;

    for (
        media_id,
        media_path,
        size_bytes,
        mtime,
        duration_ms,
        source_path,
        existing_channel_id,
        existing_channel_name,
        _existing_yt_channel_id,
        title,
        _is_short,
        existing_youtube_id,
    ) in candidates
    {
        {
            let mut st = status.lock().await;
            st.current_title = Some(title.clone());
        }

        let cache_key = format!("{media_id}-{size_bytes}-{mtime}");
        let filename = Path::new(&media_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("");

        let mut is_search = false;
        let mut resolved_id = extract_youtube_id(filename);
        if resolved_id.is_none()
            && let Some(yid) = &existing_youtube_id
        {
            resolved_id = Some(yid.clone());
        }

        if resolved_id.is_none() {
            // Step 2: Search via yt-dlp with sanitized query and fallback strategies
            let clean_title = sanitize_search_query(filename);

            // Strategy 1: If channel name is available and non-generic, try `{channel} {clean_title}`
            let mut search_res = None;
            if let Some(cname) = &existing_channel_name
                && !is_generic_channel_name(cname)
            {
                let channel_query = build_channel_search_query(cname, &clean_title);
                search_res = search_youtube(
                    &channel_query,
                    &title,
                    duration_ms,
                    yt_settings.use_browser_cookies,
                    &yt_settings.browser,
                )
                .await;
            }

            // Strategy 2: If strategy 1 didn't find anything or was skipped, try `{clean_title}` alone
            if search_res.is_none() {
                search_res = search_youtube(
                    &clean_title,
                    &title,
                    duration_ms,
                    yt_settings.use_browser_cookies,
                    &yt_settings.browser,
                )
                .await;
            }

            if let Some(sr) = search_res {
                resolved_id = Some(sr.youtube_id);
                is_search = true;
            }
        }

        let Some(video_id) = resolved_id else {
            // Candidate not found on YouTube, keep ffmpeg thumbnail and record attempt
            record_sync_failure(pool, media_id, "not_found").await;
            let mut st = status.lock().await;
            st.processed += 1;
            st.failed += 1;
            continue;
        };

        // Fetch rich details from watch page
        let mut details = fetch_youtube_watch_details(
            &video_id,
            yt_settings.use_browser_cookies,
            &yt_settings.browser,
        )
        .await;
        if details.is_none() {
            details = Some(YoutubeMetadata {
                youtube_id: video_id.clone(),
                title: None,
                description: None,
                channel_id: None,
                channel_name: None,
                handle: None,
                avatar_url: None,
                published_at: None,
                tags: Vec::new(),
                categories: Vec::new(),
            });
        }
        let details = details.unwrap();

        // Download thumbnail
        let (dest_path, local_cache) = resolve_destination_thumbnail(
            &media_path,
            source_path.as_deref(),
            &cache_key,
            cache_dir,
        );

        if let Err(err) = download_youtube_thumbnail(&video_id, &dest_path).await {
            tracing::warn!(media_id, %video_id, %err, "failed to download official thumbnail");
            record_sync_failure(pool, media_id, "download_error").await;
            let mut st = status.lock().await;
            st.processed += 1;
            st.failed += 1;
            continue;
        }

        // Copy to local cache dir if different
        if dest_path != local_cache
            && let Err(error) = tokio::fs::copy(&dest_path, &local_cache).await
        {
            tracing::warn!(%error, media_id, "official thumbnail could not be copied to local cache");
        }

        // Execute atomic database update
        let source_type = if is_search {
            "youtube_search"
        } else {
            "youtube_id"
        };

        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::error!(%error, media_id, "failed to begin YouTube synchronization transaction");
                record_sync_failure(pool, media_id, "database_error").await;
                continue;
            }
        };

        macro_rules! required_write {
            ($future:expr, $context:literal) => {
                if let Err(error) = $future.await {
                    tracing::error!(%error, media_id, $context);
                    if let Err(rollback_error) = tx.rollback().await {
                        tracing::error!(%rollback_error, media_id, "failed to roll back YouTube synchronization");
                    }
                    record_sync_failure(pool, media_id, "database_error").await;
                    let mut st = status.lock().await;
                    st.processed += 1;
                    st.failed += 1;
                    continue;
                }
            };
        }

        // 1. Update thumbnails table
        required_write!(sqlx::query(
            "INSERT INTO thumbnails (media_id, cache_key, status, is_official, source, updated_at)
             VALUES (?, ?, 'ready', 1, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(media_id) DO UPDATE SET
                cache_key = excluded.cache_key,
                status = 'ready',
                is_official = 1,
                source = excluded.source,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(media_id)
        .bind(&cache_key)
        .bind(source_type)
        .execute(&mut *tx), "falha ao persistir thumbnail oficial");

        // 2. Verifica se outra mídia já possui esse youtube_id antes de atualizar
        let other_id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM media WHERE youtube_id = ? AND id <> ? LIMIT 1",
        )
        .bind(&video_id)
        .bind(media_id)
        .fetch_optional(&mut *tx)
        .await
        .ok()
        .flatten();

        if let Some(other_media_id) = other_id {
            if let Err(error) = tx.rollback().await {
                tracing::error!(%error, "failed to roll back synchronization before recording duplicate");
            }
            if let Err(error) =
                crate::dedup::propose_duplicate_media(pool, other_media_id, media_id, "youtube_id")
                    .await
            {
                tracing::error!(%error, "failed to record duplicate proposal");
            }
            let mut st = status.lock().await;
            st.processed += 1;
            st.successful += 1;
            continue;
        }

        // Update media table (title if needed, description & youtube_id)
        required_write!(sqlx::query(
            "UPDATE media SET
                title = CASE
                    WHEN ? IS NOT NULL AND ? != '' AND (
                        media.title LIKE '%[' || ? || ']%'
                        OR lower(trim(media.title)) IN ('video', 'video [unknown]', 'sem título', 'unknown')
                        OR media.title = ''
                        OR media.title IS NULL
                    )
                    THEN ?
                    ELSE media.title
                END,
                description = COALESCE(?, description),
                youtube_id = ?,
                published_at = COALESCE(?, published_at),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(&details.title)
        .bind(&details.title)
        .bind(&video_id)
        .bind(&details.title)
        .bind(&details.description)
        .bind(&video_id)
        .bind(&details.published_at)
        .bind(media_id)
        .execute(&mut *tx), "falha ao persistir metadados oficiais");

        // 3. Channels registration & association
        let yt_ch_id = details.channel_id.as_deref();
        let yt_ch_name = details.channel_name.as_deref();
        let yt_avatar = details.avatar_url.as_deref();
        let yt_handle = details.handle.as_deref();

        let mut final_channel_id = String::new();

        if let Some(ch_name) = yt_ch_name {
            // Official YouTube channel takes top priority
            let official_id = yt_ch_id.map(|youtube_id| format!("ch_yt_{youtube_id}"));
            let existing_match = sqlx::query_scalar::<_, String>(
                "SELECT id FROM channels
                 WHERE (youtube_channel_id IS NOT NULL AND youtube_channel_id = ?)
                    OR lower(name) = lower(?)
                 ORDER BY CASE WHEN lower(name) = lower(?) THEN 0 ELSE 1 END,
                          CASE WHEN id = ? THEN 0 ELSE 1 END,
                          id ASC
                 LIMIT 1",
            )
            .bind(yt_ch_id)
            .bind(ch_name)
            .bind(ch_name)
            .bind(official_id.as_deref())
            .fetch_optional(&mut *tx)
            .await
            .ok()
            .flatten();

            let target_channel_id = if let Some(matched_id) = existing_match {
                // Update matched channel metadata
                required_write!(
                    sqlx::query(
                        "UPDATE channels SET
                        name = trim(?),
                        youtube_channel_id = COALESCE(?, youtube_channel_id),
                        handle = COALESCE(?, handle),
                        avatar_url = COALESCE(?, avatar_url),
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?",
                    )
                    .bind(ch_name)
                    .bind(yt_ch_id)
                    .bind(yt_handle)
                    .bind(yt_avatar)
                    .bind(&matched_id)
                    .execute(&mut *tx),
                    "falha ao atualizar canal oficial"
                );
                matched_id
            } else {
                // Create new channel
                let new_id = if let Some(official_id) = official_id {
                    official_id
                } else {
                    let clean_slug = ch_name
                        .to_lowercase()
                        .chars()
                        .filter(|c| c.is_alphanumeric() || *c == ' ')
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join("_");
                    format!("ch_{clean_slug}")
                };

                required_write!(sqlx::query(
                    "INSERT INTO channels (id, name, youtube_channel_id, handle, avatar_url)
                     VALUES (?, ?, ?, ?, ?)
                     ON CONFLICT(id) DO UPDATE SET
                        name = excluded.name,
                        youtube_channel_id = COALESCE(excluded.youtube_channel_id, channels.youtube_channel_id),
                        handle = COALESCE(excluded.handle, channels.handle),
                        avatar_url = COALESCE(excluded.avatar_url, channels.avatar_url),
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                )
                .bind(&new_id)
                .bind(ch_name)
                .bind(yt_ch_id)
                .bind(yt_handle)
                .bind(yt_avatar)
                .execute(&mut *tx), "falha ao criar canal oficial");
                new_id
            };

            if let Some(youtube_channel_id) = yt_ch_id {
                required_write!(
                    sqlx::query(
                        "UPDATE media_channels
                         SET channel_id = ?
                         WHERE channel_id IN (
                            SELECT id FROM channels
                            WHERE youtube_channel_id = ? AND id <> ?
                         )",
                    )
                    .bind(&target_channel_id)
                    .bind(youtube_channel_id)
                    .bind(&target_channel_id)
                    .execute(&mut *tx),
                    "falha ao consolidar associações de aliases do canal"
                );
                required_write!(
                    sqlx::query(
                        "DELETE FROM channels
                         WHERE youtube_channel_id = ? AND id <> ?",
                    )
                    .bind(youtube_channel_id)
                    .bind(&target_channel_id)
                    .execute(&mut *tx),
                    "falha ao remover aliases consolidados do canal"
                );
            }

            // Link media to channel
            required_write!(
                sqlx::query(
                    "INSERT INTO media_channels (media_id, channel_id, origin)
                 VALUES (?, ?, 'youtube')
                 ON CONFLICT(media_id) DO UPDATE SET channel_id = excluded.channel_id",
                )
                .bind(media_id)
                .bind(&target_channel_id)
                .execute(&mut *tx),
                "falha ao associar mídia ao canal oficial"
            );

            final_channel_id = target_channel_id;
        } else if let Some(ch_id) = &existing_channel_id {
            // Fallback to local channel metadata if YouTube provided no channel
            final_channel_id = ch_id.clone();
        }

        // 4. Tags and Categories from YouTube directly to media_tags
        let mut all_tags = details.categories.clone();
        all_tags.extend(details.tags.clone());
        for tag_name in all_tags {
            let sanitized = sanitize_tag_name(&tag_name);
            let trimmed = sanitized.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 80 {
                continue;
            }
            let normalized = trimmed.to_lowercase();
            if let Ok(tag_id) = sqlx::query_scalar::<_, i64>(
                "INSERT INTO tags (normalized_name, display_name) VALUES (?, ?)
                 ON CONFLICT(normalized_name) DO UPDATE SET display_name = CASE
                    WHEN lower(excluded.display_name) < lower(tags.display_name) THEN excluded.display_name
                    ELSE tags.display_name END
                 RETURNING id",
            )
            .bind(&normalized)
            .bind(trimmed)
            .fetch_one(&mut *tx)
            .await
                && let Err(error) = sqlx::query(
                "INSERT INTO media_tags (media_id, tag_id, origin) VALUES (?, ?, 'youtube')
                 ON CONFLICT(media_id, tag_id) DO UPDATE SET origin = 'youtube'",
            )
            .bind(media_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await
            {
                tracing::warn!(%error, media_id, tag_id, "official tag could not be associated");
            }
        }

        match tx.commit().await {
            Ok(()) => {
                if let Err(error) =
                    sqlx::query("DELETE FROM youtube_sync_attempts WHERE media_id = ?")
                        .bind(media_id)
                        .execute(pool)
                        .await
                {
                    tracing::warn!(%error, media_id, "failed to clear old synchronization attempts");
                }
                had_updates = true;
                let mut st = status.lock().await;
                st.successful += 1;
                st.processed += 1;

                let sub_settings = crate::subtitles::load_subtitle_settings(pool).await;
                if sub_settings.auto_download && !sub_settings.languages.is_empty() {
                    let subs_dir = cache_dir.parent().unwrap_or(cache_dir).join("subtitles");
                    if let Err(error) = crate::subtitles::download_youtube_subtitles(
                        pool,
                        media_id,
                        &video_id,
                        &subs_dir,
                        &sub_settings.languages,
                        sub_settings.include_auto_generated,
                        yt_settings.use_browser_cookies,
                        &yt_settings.browser,
                    )
                    .await
                    {
                        tracing::warn!(%error, media_id, "automatic subtitle download failed");
                    }
                }

                // Physical organization is intentionally not triggered by metadata sync.
                // It requires the explicit preview/confirmation flow in the organizer UI.

                if let Some(av) = yt_avatar {
                    download_and_save_channel_avatar(
                        &final_channel_id,
                        av,
                        Some(&media_path),
                        source_path.as_deref(),
                    )
                    .await;
                }
            }
            Err(error) => {
                tracing::error!(%error, media_id, "failed to commit YouTube synchronization transaction");
                record_sync_failure(pool, media_id, "database_error").await;
                let mut st = status.lock().await;
                st.processed += 1;
                st.failed += 1;
            }
        }

        // Polite delay to protect IP
        if is_search {
            tokio::time::sleep(Duration::from_millis(2200)).await;
        } else {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    if had_updates {
        // Re-sync FTS so channel search finds the new channel associations
        if let Ok(mut conn) = pool.acquire().await {
            let _ = crate::catalog::sync_media_fts(&mut conn).await;
        }
    }

    tracing::info!("youtube_worker cycle completed");
}

async fn consolidate_duplicate_youtube_channels(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let youtube_channel_ids = sqlx::query_scalar::<_, String>(
        "SELECT youtube_channel_id
         FROM channels
         WHERE youtube_channel_id IS NOT NULL AND trim(youtube_channel_id) <> ''
         GROUP BY youtube_channel_id
         HAVING COUNT(*) > 1",
    )
    .fetch_all(pool)
    .await?;
    let mut merged = 0_u64;
    let mut reassigned_total = 0_u64;

    for youtube_channel_id in youtube_channel_ids {
        let mut transaction = pool.begin().await?;
        let canonical_id = sqlx::query_scalar::<_, String>(
            "SELECT c.id
             FROM channels c
             LEFT JOIN media_channels mc ON mc.channel_id = c.id
             WHERE c.youtube_channel_id = ?
             GROUP BY c.id
             ORDER BY SUM(CASE WHEN mc.origin = 'youtube' THEN 1 ELSE 0 END) DESC,
                      COUNT(mc.media_id) DESC,
                      length(c.name) ASC,
                      c.id ASC
             LIMIT 1",
        )
        .bind(&youtube_channel_id)
        .fetch_one(&mut *transaction)
        .await?;

        sqlx::query(
            "UPDATE channels
             SET handle = COALESCE(handle, (
                    SELECT handle FROM channels
                    WHERE youtube_channel_id = ? AND handle IS NOT NULL LIMIT 1
                 )),
                 avatar_url = COALESCE(avatar_url, (
                    SELECT avatar_url FROM channels
                    WHERE youtube_channel_id = ? AND avatar_url IS NOT NULL LIMIT 1
                 )),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(&youtube_channel_id)
        .bind(&youtube_channel_id)
        .bind(&canonical_id)
        .execute(&mut *transaction)
        .await?;

        let reassigned = sqlx::query(
            "UPDATE media_channels
             SET channel_id = ?
             WHERE channel_id IN (
                SELECT id FROM channels WHERE youtube_channel_id = ? AND id <> ?
             )",
        )
        .bind(&canonical_id)
        .bind(&youtube_channel_id)
        .bind(&canonical_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();

        let removed = sqlx::query(
            "DELETE FROM channels
             WHERE youtube_channel_id = ? AND id <> ?",
        )
        .bind(&youtube_channel_id)
        .bind(&canonical_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();

        transaction.commit().await?;
        merged += removed;
        reassigned_total += reassigned;
    }

    if reassigned_total > 0 {
        let mut connection = pool.acquire().await?;
        crate::catalog::sync_media_fts(&mut connection).await?;
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_youtube_id() {
        assert_eq!(
            extract_youtube_id("Synthetic Video [abcdefghijk].mp4"),
            Some("abcdefghijk".to_string())
        );
        assert_eq!(extract_youtube_id("Video without id.mp4"), None);
        assert_eq!(extract_youtube_id("Video with [short].mkv"), None);
    }

    #[test]
    fn test_titles_match_flexibly() {
        // Exact match
        assert!(titles_match_flexibly(
            "A COMPILAÇÃO TERMINOU.mp4",
            "A COMPILAÇÃO TERMINOU"
        ));

        // Ignoring symbols and extensions
        assert!(titles_match_flexibly(
            "ALGORITMO #1 | A ESTREIA.mp4",
            "ALGORITMO #1 | A ESTREIA"
        ));

        // Case and accent insensitivity
        assert!(titles_match_flexibly(
            "compilacao terminou.webm",
            "COMPILAÇÃO TERMINOU"
        ));

        // Substring / rich suffix containment
        assert!(titles_match_flexibly(
            "A Ballad for the Wanderer [Sample Narrator].mkv",
            "A Ballad for the Wanderer / Uma balada para o viajante - NARRAÇÃO OFICIAL - SAMPLE NARRATOR"
        ));

        // Channel or prefix match
        assert!(titles_match_flexibly(
            "Project Aurora First Impressions",
            "Project Aurora First Impressions - Example Creator"
        ));

        // Completely different titles
        assert!(!titles_match_flexibly(
            "Curso de Rust Aula 1.mp4",
            "Unrelated Gameplay Episode - Final Stage"
        ));
    }

    #[test]
    fn test_duration_matches_closely() {
        // Exact
        assert!(duration_matches_closely(Some(120_000), 120.0));
        // Within 3 seconds
        assert!(duration_matches_closely(Some(120_000), 122.5));
        assert!(duration_matches_closely(Some(120_000), 117.5));
        // Within 2% for long videos (e.g. 1 hour = 3600s, 30s diff is < 1%)
        assert!(duration_matches_closely(Some(3_600_000), 3630.0));
        // Way off
        assert!(!duration_matches_closely(Some(60_000), 120.0));
        assert!(!duration_matches_closely(Some(120_000), 140.0));
    }

    #[test]
    fn test_youtube_settings_default() {
        let settings = YoutubeSettings::default();
        assert!(!settings.use_browser_cookies);
        assert_eq!(settings.browser, "firefox");

        let serialized = serde_json::to_string(&settings).unwrap();
        let deserialized: YoutubeSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(settings, deserialized);
    }

    #[test]
    fn test_browser_allowlist() {
        assert!(is_valid_browser("firefox"));
        assert!(is_valid_browser("Firefox"));
        assert!(is_valid_browser(" chrome "));
        assert!(is_valid_browser("BRAVE"));
        assert!(is_valid_browser("chromium"));
        assert!(is_valid_browser("edge"));
        assert!(is_valid_browser("opera"));
        assert!(is_valid_browser("safari"));
        assert!(is_valid_browser("vivaldi"));
        assert!(is_valid_browser("whale"));

        assert!(!is_valid_browser(""));
        assert!(!is_valid_browser("   "));
        assert!(!is_valid_browser("internet_explorer"));
        assert!(!is_valid_browser("firefox; rm -rf /"));
        assert!(!is_valid_browser("--version"));
    }

    #[tokio::test]
    async fn test_sync_failure_lifecycle() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();

        // Insert dummy media
        sqlx::query("INSERT INTO media (id, title) VALUES (1, 'Test Video')")
            .execute(&pool)
            .await
            .unwrap();

        // 1. Record failure
        record_sync_failure(&pool, 1, "not_found").await;

        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM youtube_sync_attempts WHERE media_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);

        // 2. Second failure increments attempts
        record_sync_failure(&pool, 1, "not_found").await;
        let attempts: i64 =
            sqlx::query_scalar("SELECT attempts FROM youtube_sync_attempts WHERE media_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempts, 2);

        // 3. Reset failed attempts
        let cleared = reset_failed_attempts(&pool).await.unwrap();
        assert_eq!(cleared, 1);

        let count_after: i64 = sqlx::query_scalar("SELECT count(*) FROM youtube_sync_attempts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count_after, 0);

        // 4. Test reset_unofficial_failed_attempts
        sqlx::query("INSERT INTO media (id, title) VALUES (2, 'Official Video')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO thumbnails (media_id, cache_key, status, is_official) VALUES (2, 'key2', 'ready', 1)")
            .execute(&pool)
            .await
            .unwrap();

        record_sync_failure(&pool, 1, "fail_unofficial").await;
        record_sync_failure(&pool, 2, "fail_official").await;

        let cleared_unofficial = reset_unofficial_failed_attempts(&pool).await.unwrap();
        assert_eq!(cleared_unofficial, 1);

        let has_1: Option<i64> =
            sqlx::query_scalar("SELECT media_id FROM youtube_sync_attempts WHERE media_id = 1")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(has_1.is_none());

        let has_2: Option<i64> =
            sqlx::query_scalar("SELECT media_id FROM youtube_sync_attempts WHERE media_id = 2")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(has_2, Some(2));
    }

    #[tokio::test]
    async fn duplicate_youtube_channel_ids_are_consolidated() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let youtube_channel_id = "UC-warframe-example";
        sqlx::query(
            "INSERT INTO channels (id, name, youtube_channel_id, handle) VALUES
             ('ch_playwarframe', 'PlayWarframe', ?, '@Warframe'),
             ('ch_warframe_music', 'Warframe Music', ?, '@Warframe'),
             ('ch_warframe_devshorts', 'Warframe Devshorts', ?, '@Warframe')",
        )
        .bind(youtube_channel_id)
        .bind(youtube_channel_id)
        .bind(youtube_channel_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO media (id, title, is_available) VALUES
             (1, 'Official one', 1), (2, 'Official two', 1), (3, 'Official three', 1),
             (4, 'Music alias', 1), (5, 'Devshort alias one', 1), (6, 'Devshort alias two', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO media_channels (media_id, channel_id, origin) VALUES
             (1, 'ch_playwarframe', 'migration'),
             (2, 'ch_playwarframe', 'migration'),
             (3, 'ch_playwarframe', 'migration'),
             (4, 'ch_warframe_music', 'migration'),
             (5, 'ch_warframe_devshorts', 'migration'),
             (6, 'ch_warframe_devshorts', 'migration')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let merged = consolidate_duplicate_youtube_channels(&pool).await.unwrap();

        assert_eq!(merged, 2);
        let remaining = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM channels WHERE youtube_channel_id = ?",
        )
        .bind(youtube_channel_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            remaining,
            vec![("ch_playwarframe".to_string(), "PlayWarframe".to_string())]
        );
        let linked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM media_channels WHERE channel_id = 'ch_playwarframe'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(linked, 6);
    }

    #[test]
    fn test_sanitize_search_query() {
        let input = "【星空 OP】夜の旅⧸架空歌手(Covered by Example Artist).mp4";
        let sanitized = sanitize_search_query(input);
        assert_eq!(
            sanitized,
            "星空 OP 夜の旅 架空歌手(Covered by Example Artist)"
        );
        assert!(!sanitized.contains('⧸'));
        assert!(!sanitized.contains('【'));
        assert!(!sanitized.contains('】'));

        let input_special = "OST： Title ＂Special＂ ｜ Demake ⧸⧸ Edit.mkv";
        let sanitized_special = sanitize_search_query(input_special);
        assert_eq!(sanitized_special, "OST Title Special Demake Edit");

        // Emojis, dingbats and symbols stripping
        let input_emoji = "Ambient Session 🐸🫧.mp4";
        assert_eq!(sanitize_search_query(input_emoji), "Ambient Session");

        let input_haircut = "Relaxing Example Session 💇🏻‍♀️ Ambient Roleplay.mp4";
        assert_eq!(
            sanitize_search_query(input_haircut),
            "Relaxing Example Session Ambient Roleplay"
        );

        let input_tm = "Synthetic Treatment & Massage 💆🏻‍♀️ With All the Good™ Sounds.mp4";
        assert_eq!(
            sanitize_search_query(input_tm),
            "Synthetic Treatment & Massage With All the Good Sounds"
        );

        // Trailing artifact "- NA"
        let input_na = "Example Fantasy Roleplay for Relaxation - NA.mp4";
        assert_eq!(
            sanitize_search_query(input_na),
            "Example Fantasy Roleplay for Relaxation"
        );

        // Build channel search query deduplicates adjacent words
        let q = build_channel_search_query("Example Ambient", "Ambient Close Attention");
        assert_eq!(q, "Example Ambient Close Attention");

        assert!(is_generic_channel_name("Music"));
        assert!(!is_generic_channel_name("Example Ambient"));
    }

    #[test]
    fn test_titles_match_multilingual_or_duration() {
        let local_jp = "【星空 OP】夜の旅⧸架空歌手(Covered by Example Artist)";
        let yt_jp = "【星空 OP】夜の旅/架空歌手(Covered by Example Artist)";
        let yt_pt = "【星空 OP】 Jornada Noturna / Cantor Exemplo (Cover por Example Artist)";

        // Direct Japanese match
        assert!(titles_match_multilingual_or_duration(
            local_jp,
            yt_jp,
            Some(258_717),
            259.0
        ));

        // Multilingual match with shared anchors ("op") and close duration.
        assert!(titles_match_multilingual_or_duration(
            local_jp,
            yt_pt,
            Some(258_717),
            259.0
        ));

        // Mismatched duration (different video) should reject
        assert!(!titles_match_multilingual_or_duration(
            local_jp,
            yt_pt,
            Some(258_717),
            120.0
        ));

        // Totally unrelated title should reject even if duration happens to be close
        assert!(!titles_match_multilingual_or_duration(
            "Some Completely Different Video",
            yt_pt,
            Some(258_717),
            259.0
        ));
    }

    #[test]
    fn test_extract_handle_and_search() {
        let html = r#"{"canonicalBaseUrl":"/@ExampleCreator"}"#;
        assert_eq!(extract_handle(html), Some("@ExampleCreator".to_string()));

        let html_search = r#"{"channelRenderer":{"channelId":"UC0000000000000000000000","canonicalBaseUrl":"/@ExampleCreator"}}"#;
        let (cid, handle) = extract_channel_from_search(html_search);
        assert_eq!(cid, Some("UC0000000000000000000000".to_string()));
        assert_eq!(handle, Some("@ExampleCreator".to_string()));
    }
}
