use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::RwLock;

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::Instrument;

use crate::{
    auth::recover_password_endpoint,
    catalog,
    downloader::{self, DownloadService},
    feed, organize, playback,
    scanner::ScanQueue,
    search, sources, streaming,
    thumbnail::{self, ThumbnailService},
    watcher::WatcherService,
    youtube_worker::{YoutubeWorkerService, YoutubeWorkerStatus},
};

const CSP: &str = "default-src 'self'; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data: https://yt3.ggpht.com https://*.ggpht.com https://i.ytimg.com https://*.youtube.com https://yt3.googleusercontent.com https://*.googleusercontent.com; media-src 'self' blob:; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) pool: SqlitePool,
    pub(crate) queue: ScanQueue,
    pub(crate) watchers: WatcherService,
    pub(crate) thumbnails: ThumbnailService,
    pub(crate) youtube: YoutubeWorkerService,
    pub(crate) downloader: DownloadService,
    pub(crate) updates: crate::updates::UpdateService,
    pub(crate) authorized_roots: Arc<RwLock<Vec<PathBuf>>>,
    pub(crate) crypto: crate::crypto::AppCrypto,
    pub(crate) rate_limiter: crate::auth::RateLimiter,
    pub(crate) security: crate::config::SecurityConfig,
    pub(crate) extension_dir: PathBuf,
    pub(crate) diagnostics: crate::operations::DiagnosticReport,
    pub(crate) requests: Arc<RequestMetrics>,
    pub(crate) cache_dir: PathBuf,
    #[cfg(test)]
    pub(crate) csrf_token: String,
}

impl AppState {
    pub(crate) async fn refresh_authorized_roots(&self) {
        if let Ok(roots) = sqlx::query_scalar::<_, String>(
            "SELECT path FROM media_sources WHERE enabled = 1 AND removed_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await
        {
            let mut canonical_roots = Vec::with_capacity(roots.len());
            for r in roots {
                let p = PathBuf::from(r);
                if let Ok(canon) = std::fs::canonicalize(&p) {
                    canonical_roots.push(canon);
                } else {
                    canonical_roots.push(p);
                }
            }
            let mut lock = self.authorized_roots.write().await;
            *lock = canonical_roots;
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(pool: SqlitePool) -> Self {
        let thumbnails = ThumbnailService::start(pool.clone(), PathBuf::from("/tmp"));
        let youtube = YoutubeWorkerService::start(pool.clone(), PathBuf::from("/tmp"));
        let queue = ScanQueue::start(pool.clone(), thumbnails.clone(), youtube.clone());
        let watchers = WatcherService::start(pool.clone(), queue.clone());
        let downloader = DownloadService::start(pool.clone(), PathBuf::from("/tmp"), queue.clone());
        crate::download_playlists::PlaylistDownloadScheduler::start(
            pool.clone(),
            downloader.clone(),
        );
        let updates = crate::updates::UpdateService::start(&PathBuf::from(format!(
            "/tmp/zmydia-update-test-{}",
            std::process::id()
        )));
        let crypto = crate::crypto::AppCrypto::from_key_bytes(&[123u8; 32]).unwrap();
        Self {
            pool,
            queue,
            watchers,
            thumbnails,
            youtube,
            downloader,
            updates,
            authorized_roots: Arc::new(RwLock::new(Vec::new())),
            crypto,
            rate_limiter: crate::auth::RateLimiter::default(),
            security: crate::config::SecurityConfig::default(),
            extension_dir: PathBuf::from("/tmp"),
            diagnostics: crate::operations::DiagnosticReport {
                status: "ready",
                version: crate::version::app_version(),
                capabilities: Vec::new(),
            },
            requests: Arc::new(RequestMetrics::default()),
            cache_dir: PathBuf::from("/tmp"),
            csrf_token: "test_csrf".to_owned(),
        }
    }
}

#[derive(Serialize)]
struct Liveness {
    status: &'static str,
    version: &'static str,
}

#[derive(Default)]
pub(crate) struct RequestMetrics {
    total: AtomicU64,
    active: AtomicU64,
    failed: AtomicU64,
    duration_ms: AtomicU64,
}

#[derive(Clone)]
pub struct RuntimeServices {
    downloader: DownloadService,
}

impl RuntimeServices {
    pub async fn shutdown(&self) {
        self.downloader.shutdown().await;
    }
}

pub fn router(
    pool: SqlitePool,
    crypto: crate::crypto::AppCrypto,
    frontend_dir: &Path,
    cache_dir: &Path,
    security: crate::config::SecurityConfig,
) -> Router {
    let extension_dir = Path::new("extension");
    let diagnostics = crate::operations::DiagnosticReport {
        status: "ready",
        version: crate::version::app_version(),
        capabilities: Vec::new(),
    };
    router_with_runtime(
        pool,
        crypto,
        frontend_dir,
        extension_dir,
        cache_dir,
        diagnostics,
        security,
    )
    .0
}

pub fn router_with_runtime(
    pool: SqlitePool,
    crypto: crate::crypto::AppCrypto,
    frontend_dir: &Path,
    extension_dir: &Path,
    cache_dir: &Path,
    diagnostics: crate::operations::DiagnosticReport,
    security: crate::config::SecurityConfig,
) -> (Router, RuntimeServices) {
    let static_files =
        ServeDir::new(frontend_dir).fallback(ServeFile::new(frontend_dir.join("index.html")));
    let thumbnails = ThumbnailService::start(pool.clone(), cache_dir.to_path_buf());
    let youtube = YoutubeWorkerService::start(pool.clone(), cache_dir.to_path_buf());
    let queue = ScanQueue::start(pool.clone(), thumbnails.clone(), youtube.clone());
    let watchers = WatcherService::start(pool.clone(), queue.clone());
    let downloader = DownloadService::start(pool.clone(), cache_dir.to_path_buf(), queue.clone());
    crate::download_playlists::PlaylistDownloadScheduler::start(pool.clone(), downloader.clone());
    let updates = crate::updates::UpdateService::start(cache_dir);
    let authorized_roots = Arc::new(RwLock::new(Vec::new()));
    let rate_limiter = crate::auth::RateLimiter::default();

    let runtime = RuntimeServices {
        downloader: downloader.clone(),
    };
    let state = AppState {
        pool,
        queue,
        watchers,
        thumbnails,
        youtube,
        downloader,
        updates,
        authorized_roots,
        crypto,
        rate_limiter,
        security,
        extension_dir: extension_dir.to_path_buf(),
        diagnostics,
        requests: Arc::new(RequestMetrics::default()),
        cache_dir: cache_dir.to_path_buf(),
        #[cfg(test)]
        csrf_token: "test_csrf".to_owned(),
    };

    let state_init = state.clone();
    tokio::spawn(async move {
        state_init.refresh_authorized_roots().await;
    });

    let app = Router::new()
        .route("/api/health", get(liveness))
        .route("/api/health/live", get(liveness))
        .route("/api/health/ready", get(readiness))
        .route("/api/diagnostics/capabilities", get(capabilities))
        .route("/api/metrics", get(metrics))
        .route("/api/auth/status", get(crate::auth::get_auth_status))
        .route("/api/auth/setup", post(crate::auth::setup_endpoint))
        .route("/api/auth/recover", post(recover_password_endpoint))
        .route("/api/auth/login", post(crate::auth::login_endpoint))
        .route("/api/auth/logout", post(crate::auth::logout_endpoint))
        .route("/api/auth/csrf", get(crate::auth::csrf_endpoint))
        .route(
            "/api/user/profile",
            get(crate::auth::get_user_profile).put(crate::auth::update_user_profile),
        )
        .route("/api/user/password", put(crate::auth::update_user_password))
        .route(
            "/api/extension/tokens",
            get(crate::auth::list_api_tokens).post(crate::auth::create_api_token),
        )
        .route(
            "/api/extension/tokens/{id}",
            delete(crate::auth::revoke_api_token),
        )
        .route(
            "/api/extension/pairings",
            post(crate::auth::create_pairing_challenge),
        )
        .route(
            "/api/extension/pairings/{id}/confirm",
            post(crate::auth::confirm_pairing_challenge),
        )
        .route("/api/dashboard", get(crate::dashboard::get_dashboard_stats))
        .route("/api/extension/info", get(get_extension_info))
        .route("/extension/zmydia", get(serve_detected_extension))
        .route("/extension/zmydia.zip", get(serve_chromium_extension))
        .route("/extension/zmydia.xpi", get(serve_extension_xpi))
        .route("/extension/mymedia.xpi", get(serve_extension_xpi))
        .route("/api/organizer/reorganize-all", post(reorganize_all_media))
        .route(
            "/api/organizer/reorganize-all/preview",
            get(preview_reorganize_all_media),
        )
        .route(
            "/api/filesystem/operations",
            get(crate::filesystem::list_operations),
        )
        .route(
            "/api/filesystem/operations/{id}/undo",
            post(crate::filesystem::undo_operation),
        )
        .route(
            "/api/filesystem/operations/{id}/discard",
            delete(crate::filesystem::discard_operation),
        )
        .route("/api/duplicates", get(crate::filesystem::list_duplicates))
        .route(
            "/api/duplicates/{id}/approve",
            post(crate::filesystem::approve_duplicate),
        )
        .route(
            "/api/duplicates/{id}/reject",
            post(crate::filesystem::reject_duplicate),
        )
        .route(
            "/api/downloads/probe",
            post(downloader::probe_video_endpoint),
        )
        .route(
            "/api/downloads",
            get(downloader::list_downloads_endpoint).post(downloader::create_download_endpoint),
        )
        .route(
            "/api/downloads/{id}",
            axum::routing::delete(downloader::delete_download_endpoint),
        )
        .route(
            "/api/downloads/{id}/file",
            get(downloader::get_download_file_endpoint),
        )
        .route(
            "/api/download-playlists",
            get(crate::download_playlists::list_playlists_endpoint)
                .post(crate::download_playlists::import_playlist_endpoint),
        )
        .route(
            "/api/download-playlists/{id}",
            get(crate::download_playlists::get_playlist_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/start",
            post(crate::download_playlists::start_playlist_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/pause",
            post(crate::download_playlists::pause_playlist_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/skip-cooldown",
            post(crate::download_playlists::skip_playlist_cooldown_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/cancel",
            post(crate::download_playlists::cancel_playlist_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/retry-failed",
            post(crate::download_playlists::retry_failed_playlist_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/items/selection",
            axum::routing::put(crate::download_playlists::update_selection_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/items/{position}/retry",
            post(crate::download_playlists::retry_playlist_item_endpoint),
        )
        .route(
            "/api/download-playlists/{id}/items/{position}/cancel",
            post(crate::download_playlists::cancel_playlist_item_endpoint),
        )
        .route(
            "/api/settings/downloads",
            get(downloader::get_download_settings_endpoint)
                .put(downloader::update_download_settings_endpoint),
        )
        .route("/api/updates/status", get(get_update_status))
        .route("/api/updates/check", post(check_for_updates))
        .route("/api/updates/download", post(download_update))
        .route("/api/updates/apply", post(apply_update))
        .route("/api/youtube/sync", post(sync_youtube))
        .route("/api/youtube/status", get(get_youtube_status))
        .route(
            "/api/settings/youtube",
            get(get_youtube_settings).put(update_youtube_settings),
        )
        .route(
            "/api/settings/home",
            get(get_home_settings).put(update_home_settings),
        )
        .route(
            "/api/settings/recommendation",
            get(crate::recommender::get_recommendation_settings_endpoint)
                .put(crate::recommender::update_recommendation_settings_endpoint),
        )
        .route(
            "/api/settings/subtitles",
            get(get_subtitle_settings).put(update_subtitle_settings),
        )
        .route(
            "/api/settings/listenbrainz",
            get(crate::music::get_listenbrainz_settings_endpoint)
                .put(crate::music::update_listenbrainz_settings_endpoint),
        )
        .route(
            "/api/settings/listenbrainz/validate",
            post(crate::music::validate_token_post),
        )
        .route(
            "/api/music/lookup-mbid",
            get(crate::music::lookup_mbid_endpoint),
        )
        .route(
            "/api/music/search",
            get(crate::music::search_music_endpoint),
        )
        .route("/api/subtitles/sync", post(sync_all_subtitles))
        .route("/api/media/{id}/subtitles", get(get_media_subtitles))
        .route(
            "/api/media/{id}/subtitles/{lang}",
            get(serve_media_subtitle),
        )
        .route(
            "/api/media/{id}/subtitles/download",
            post(download_media_subtitles),
        )
        .route("/api/sources", get(sources::list).post(sources::create))
        .route(
            "/api/sources/{id}",
            axum::routing::patch(sources::update).delete(sources::remove),
        )
        .route("/api/sources/{id}/scan", post(sources::scan_source))
        .route("/api/scans", post(sources::scan_all))
        .route("/api/scans/{id}", get(sources::get_job))
        .route("/api/feed", get(feed::get_feed))
        .route("/api/search", get(search::search_media))
        .route("/api/media", get(catalog::list_media))
        .route("/api/media/{id}", get(catalog::get_media))
        .route(
            "/api/media/{id}/notes",
            axum::routing::put(catalog::update_media_notes),
        )
        .route(
            "/api/media/{id}/music",
            get(crate::music::get_media_music_endpoint)
                .put(crate::music::save_media_music_endpoint)
                .delete(crate::music::delete_media_music_endpoint),
        )
        .route(
            "/api/media/{id}/music/recognize",
            post(crate::music::recognize_media_endpoint),
        )
        .route(
            "/api/media/{id}/music/scrobble",
            post(crate::music::scrobble_media_endpoint),
        )
        .route(
            "/api/media/{id}/music/playing-now",
            post(crate::music::playing_now_media_endpoint),
        )
        .route("/api/tags", get(catalog::list_tags))
        .route("/api/channels", get(catalog::list_channels))
        .route("/api/channels/sync", post(catalog::sync_all_channels))
        .route("/api/channels/{id}", get(catalog::get_channel))
        .route(
            "/api/channels/{id}/mentions",
            get(catalog::get_channel_mentions),
        )
        .route(
            "/api/channels/{id}/avatar",
            get(catalog::get_channel_avatar),
        )
        .route("/api/channels/{id}/sync", post(catalog::sync_channel))
        .route("/api/media/{id}/thumbnail", get(thumbnail::get_thumbnail))
        .route("/api/media/{id}/related", get(catalog::get_related_media))
        .route(
            "/api/media/{id}/stream",
            get(streaming::stream_media).head(streaming::stream_media),
        )
        .route(
            "/api/playback/{media_id}",
            get(playback::get_playback_state).put(playback::update_playback_state),
        )
        .route(
            "/api/playback/{media_id}/scrobble",
            post(playback::record_playback_scrobble),
        )
        .route(
            "/api/playback/{media_id}/engagement",
            post(playback::record_playback_engagement),
        )
        .route("/api/stats/playback", get(playback::get_playback_stats))
        .route(
            "/api/history",
            get(playback::list_history).delete(playback::clear_history),
        )
        .route(
            "/api/history/{id}",
            axum::routing::delete(playback::remove_history_item),
        )
        .route("/api/favorites", get(organize::list_favorites))
        .route(
            "/api/favorites/{media_id}",
            post(organize::add_favorite).delete(organize::remove_favorite),
        )
        .route(
            "/api/favorites/{media_id}/toggle",
            post(organize::toggle_favorite),
        )
        .route(
            "/api/playlists",
            get(organize::list_playlists).post(organize::create_playlist),
        )
        .route(
            "/api/playlists/{id}",
            get(organize::get_playlist)
                .patch(organize::update_playlist)
                .delete(organize::delete_playlist),
        )
        .route(
            "/api/playlists/{id}/items",
            post(organize::add_playlist_item),
        )
        .route(
            "/api/playlists/{id}/items/{media_id}",
            axum::routing::delete(organize::remove_playlist_item),
        )
        .route(
            "/api/playlists/{id}/reorder",
            axum::routing::put(organize::reorder_playlist_items),
        )
        .fallback_service(static_files)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_context_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_middleware,
        ))
        .layer(middleware::from_fn(normalize_error_responses))
        .with_state(state);
    (app, runtime)
}

#[derive(serde::Serialize)]
struct ExtensionInfoResponse {
    has_signed: bool,
    has_unsigned: bool,
    active_type: Option<String>,
    download_url: Option<String>,
    detected_browser: String,
    package_available: bool,
    package_type: Option<String>,
    installation_mode: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserFamily {
    Firefox,
    Chromium,
    Unsupported,
}

impl BrowserFamily {
    fn as_str(self) -> &'static str {
        match self {
            Self::Firefox => "firefox",
            Self::Chromium => "chromium",
            Self::Unsupported => "unsupported",
        }
    }
}

fn detect_browser_family(user_agent: Option<&str>) -> BrowserFamily {
    let Some(user_agent) = user_agent else {
        return BrowserFamily::Unsupported;
    };
    if user_agent.contains("Firefox/") && !user_agent.contains("Seamonkey/") {
        BrowserFamily::Firefox
    } else if ["Chrome/", "Chromium/", "Edg/", "OPR/", "Brave/"]
        .iter()
        .any(|marker| user_agent.contains(marker))
    {
        BrowserFamily::Chromium
    } else {
        BrowserFamily::Unsupported
    }
}

fn browser_from_headers(headers: &axum::http::HeaderMap) -> BrowserFamily {
    detect_browser_family(
        headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
    )
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ExtensionVersion([u64; 4]);

fn parse_extension_version(value: &str) -> Option<ExtensionVersion> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut version = [0_u64; 4];
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        version[index] = part.parse().ok()?;
    }
    Some(ExtensionVersion(version))
}

fn inspect_signed_xpi(path: &Path) -> Result<ExtensionVersion, String> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let has_signature = archive
        .file_names()
        .try_fold(false, |has_signature, name| {
            let name = name.map_err(|error| error.to_string())?;
            Ok::<_, String>(
                has_signature
                    || (name.to_ascii_uppercase().starts_with("META-INF/") && !name.ends_with('/')),
            )
        })?;
    if !has_signature {
        return Err("Mozilla signature metadata is missing".to_string());
    }

    let mut manifest_text = String::new();
    archive
        .by_name("manifest.json")
        .map_err(|error| error.to_string())?
        .read_to_string(&mut manifest_text)
        .map_err(|error| error.to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).map_err(|error| error.to_string())?;
    if manifest.get("name").and_then(|value| value.as_str()) != Some("ZMydia Downloader") {
        return Err("unexpected extension name".to_string());
    }
    if manifest
        .pointer("/browser_specific_settings/gecko/id")
        .and_then(|value| value.as_str())
        != Some("zmydia-downloader@local")
    {
        return Err("unexpected extension identifier".to_string());
    }
    let version = manifest
        .get("version")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "extension version is missing".to_string())?;
    parse_extension_version(version).ok_or_else(|| "extension version is invalid".to_string())
}

fn find_signed_xpi(extension_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(extension_dir.join("signed")).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("xpi") {
                return None;
            }
            match inspect_signed_xpi(&path) {
                Ok(version) => Some((version, path)),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "ignoring invalid signed extension package");
                    None
                }
            }
        })
        .max_by(|(left_version, left_path), (right_version, right_path)| {
            left_version
                .cmp(right_version)
                .then_with(|| left_path.cmp(right_path))
        })
        .map(|(_, path)| path)
}

async fn get_extension_info(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Json<ExtensionInfoResponse> {
    let signed_path = find_signed_xpi(&state.extension_dir);
    let unsigned_path = state.extension_dir.join("unsigned/mymedia.xpi");
    let chromium_source = state.extension_dir.join("manifest.json");
    let has_signed = signed_path.is_some();
    let has_unsigned = unsigned_path.is_file();
    let browser = browser_from_headers(&headers);

    let (active_type, download_url, package_available, package_type, installation_mode) =
        if browser == BrowserFamily::Chromium && chromium_source.is_file() {
            (
                Some("chromium".to_string()),
                Some("/extension/zmydia".to_string()),
                true,
                Some("zip".to_string()),
                Some("unpacked".to_string()),
            )
        } else if browser == BrowserFamily::Firefox && has_signed {
            (
                Some("signed".to_string()),
                Some("/extension/zmydia".to_string()),
                true,
                Some("xpi".to_string()),
                Some("direct".to_string()),
            )
        } else if browser == BrowserFamily::Firefox && has_unsigned {
            (
                Some("unsigned".to_string()),
                Some("/extension/zmydia".to_string()),
                true,
                Some("xpi".to_string()),
                Some("direct".to_string()),
            )
        } else {
            (None, None, false, None, None)
        };

    Json(ExtensionInfoResponse {
        has_signed,
        has_unsigned,
        active_type,
        download_url,
        detected_browser: browser.as_str().to_owned(),
        package_available,
        package_type,
        installation_mode,
    })
}

async fn serve_detected_extension(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    match browser_from_headers(&headers) {
        BrowserFamily::Firefox => serve_firefox_extension(&state).await,
        BrowserFamily::Chromium => serve_chromium_package(&state).await,
        BrowserFamily::Unsupported => (
            StatusCode::BAD_REQUEST,
            "Browser não suportado. Use Firefox ou um navegador baseado em Chromium.",
        )
            .into_response(),
    }
}

async fn serve_extension_xpi(State(state): State<AppState>) -> Response {
    serve_firefox_extension(&state).await
}

async fn serve_firefox_extension(state: &AppState) -> Response {
    let signed_path = find_signed_xpi(&state.extension_dir);
    let unsigned_path = state.extension_dir.join("unsigned/mymedia.xpi");

    let file_path = if let Some(path) = signed_path {
        path
    } else if unsigned_path.is_file() {
        unsigned_path.to_path_buf()
    } else {
        return (
            StatusCode::NOT_FOUND,
            "Package zmydia.xpi not found. Install a ZMydia release that includes the signed Firefox extension",
        )
            .into_response();
    };

    match tokio::fs::read(file_path).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/x-xpinstall")
            .header(
                header::CONTENT_DISPOSITION,
                "inline; filename=\"zmydia.xpi\"",
            )
            .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to serve extension",
                )
                    .into_response()
            }),
        Err(e) => {
            tracing::error!(%e, "failed to read zmydia.xpi package");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read extension package",
            )
                .into_response()
        }
    }
}

async fn serve_chromium_extension(State(state): State<AppState>) -> Response {
    serve_chromium_package(&state).await
}

async fn serve_chromium_package(state: &AppState) -> Response {
    let directory = state.extension_dir.clone();
    let bytes =
        match tokio::task::spawn_blocking(move || package_chromium_extension(&directory)).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                tracing::error!(%error, "failed to package Chromium extension");
                return (StatusCode::NOT_FOUND, error).into_response();
            }
            Err(error) => {
                tracing::error!(%error, "Chromium extension packaging task failed");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Falha ao preparar a extensão Chromium".to_owned(),
                )
                    .into_response();
            }
        };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"zmydia-chromium.zip\"",
        )
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Falha ao entregar a extensão Chromium",
            )
                .into_response()
        })
}

fn package_chromium_extension(directory: &Path) -> Result<Vec<u8>, String> {
    use std::io::Cursor;
    let manifest = directory.join("manifest.json");
    if !manifest.is_file() {
        return Err("Pacote Chromium não está disponível nesta instalação".to_owned());
    }
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let manifest_object = manifest
        .as_object_mut()
        .ok_or_else(|| "Manifesto da extensão é inválido".to_owned())?;
    manifest_object.remove("browser_specific_settings");
    manifest_object.insert(
        "background".to_owned(),
        serde_json::json!({ "service_worker": "service-worker.js" }),
    );
    if let Some(scripts) = manifest_object
        .get_mut("content_scripts")
        .and_then(|value| value.as_array_mut())
    {
        for script in scripts {
            if let Some(files) = script.get_mut("js").and_then(|value| value.as_array_mut()) {
                files.insert(0, serde_json::Value::String("compat.js".to_owned()));
            }
        }
    }

    let cursor = Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for child in ["background", "content", "icons", "popup"] {
        let source = directory.join(child);
        for entry in walkdir::WalkDir::new(&source).follow_links(false) {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(directory)
                .map_err(|error| error.to_string())?;
            let name = relative.to_string_lossy().replace('\\', "/");
            let mut contents = std::fs::read(entry.path()).map_err(|error| error.to_string())?;
            if name == "popup/popup.html" {
                let popup = String::from_utf8(contents).map_err(|error| error.to_string())?;
                let transformed = popup.replace(
                    "  <script src=\"popup.js\"></script>",
                    "  <script src=\"../compat.js\"></script>\n  <script src=\"popup.js\"></script>",
                );
                if transformed == popup {
                    return Err("Não foi possível adaptar o popup para Chromium".to_owned());
                }
                contents = transformed.into_bytes();
            }
            add_zip_file(&mut archive, &name, &contents, options)?;
        }
    }
    let manifest = format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?
    );
    add_zip_file(&mut archive, "manifest.json", manifest.as_bytes(), options)?;
    add_zip_file(
        &mut archive,
        "compat.js",
        b"globalThis.browser ??= globalThis.chrome;\n",
        options,
    )?;
    add_zip_file(
        &mut archive,
        "service-worker.js",
        b"globalThis.browser = globalThis.chrome;\nimportScripts('background/background.js');\n",
        options,
    )?;
    add_zip_file(
        &mut archive,
        "COMO-INSTALAR.txt",
        b"Abra chrome://extensions, ative o Modo do desenvolvedor e escolha Carregar sem compactacao. Selecione esta pasta extraida.\n",
        options,
    )?;
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| error.to_string())
}

fn add_zip_file(
    archive: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
    name: &str,
    contents: &[u8],
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    use std::io::Write;
    archive
        .start_file(name, options)
        .map_err(|error| error.to_string())?;
    archive
        .write_all(contents)
        .map_err(|error| error.to_string())
}

async fn liveness() -> Json<Liveness> {
    Json(Liveness {
        status: "ok",
        version: crate::version::app_version(),
    })
}

async fn readiness(State(state): State<AppState>) -> Response {
    let database_ok = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    let sources_ready = crate::operations::source_capabilities(&state.pool)
        .await
        .iter()
        .all(|item| item.status == "ok");
    let ready = database_ok && sources_ready && state.diagnostics.ready();
    let body = serde_json::json!({
        "status": if ready { "ready" } else { "not_ready" },
        "database": if database_ok { "ok" } else { "error" },
        "version": crate::version::app_version()
    });
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(body),
    )
        .into_response()
}

async fn capabilities(State(state): State<AppState>) -> Json<crate::operations::DiagnosticReport> {
    let mut report = state.diagnostics.clone();
    report
        .capabilities
        .retain(|item| !item.name.starts_with("source:"));
    report
        .capabilities
        .extend(crate::operations::source_capabilities(&state.pool).await);
    report.status = if report.ready() { "ready" } else { "not_ready" };
    Json(report)
}

async fn metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
    let queued = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM download_jobs WHERE status IN ('queued','probing','downloading','organizing','awaiting_index')")
        .fetch_one(&state.pool).await.unwrap_or(-1);
    let failed =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM download_jobs WHERE status='failed'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(-1);
    let duration_ms_average = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG((julianday(COALESCE(finished_at, datetime('now'))) - julianday(started_at)) * 86400000.0) FROM download_jobs WHERE started_at IS NOT NULL",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(None);
    let cache_dir = state.cache_dir.clone();
    let (cache_files, cache_bytes) = tokio::task::spawn_blocking(move || {
        let mut files = 0_u64;
        let mut bytes = 0_u64;
        for entry in walkdir::WalkDir::new(cache_dir).into_iter().flatten() {
            if let Ok(metadata) = entry.metadata()
                && metadata.is_file()
            {
                files += 1;
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        (files, bytes)
    })
    .await
    .unwrap_or_default();
    let cache_free_bytes = fs2::available_space(&state.cache_dir).unwrap_or(0);
    Json(serde_json::json!({
        "http": {
            "requests_total": state.requests.total.load(Ordering::Relaxed),
            "requests_active": state.requests.active.load(Ordering::Relaxed),
            "requests_failed": state.requests.failed.load(Ordering::Relaxed),
            "duration_ms_total": state.requests.duration_ms.load(Ordering::Relaxed)
        },
        "downloads": { "active_or_queued": queued, "failed": failed, "duration_ms_average": duration_ms_average },
        "database": { "pool_size": state.pool.size(), "idle": state.pool.num_idle(), "sqlite_busy_total": crate::db::sqlite_busy_total() },
        "cache": { "files": cache_files, "bytes": cache_bytes, "filesystem_free_bytes": cache_free_bytes }
    }))
}

async fn request_context_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            format!(
                "mm-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            )
        });
    request.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id).expect("request id valid"),
    );
    state.requests.total.fetch_add(1, Ordering::Relaxed);
    state.requests.active.fetch_add(1, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let span = tracing::info_span!("http_request", request_id = %request_id, method = %request.method(), path = %request.uri().path());
    let mut response = next.run(request).instrument(span).await;
    state.requests.active.fetch_sub(1, Ordering::Relaxed);
    state.requests.duration_ms.fetch_add(
        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        Ordering::Relaxed,
    );
    if response.status().is_server_error() {
        state.requests.failed.fetch_add(1, Ordering::Relaxed);
    }
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id).expect("request id valid"),
    );
    response
}

async fn security_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !valid_host(request.headers().get(header::HOST)) {
        return (StatusCode::BAD_REQUEST, "Host não autorizado").into_response();
    }

    if request.method() == Method::OPTIONS {
        let origin = request.headers().get(header::ORIGIN);
        if let Some(origin) = origin
            && is_allowed_origin_for_state(origin, &state).await
        {
            let mut response = StatusCode::NO_CONTENT.into_response();
            let headers = response.headers_mut();
            headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("content-type, x-csrf-token, authorization"),
            );
            headers.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("86400"),
            );
            return response;
        }
        return (StatusCode::FORBIDDEN, "Origem não autorizada").into_response();
    }

    if is_mutating(request.method()) {
        let origin = request.headers().get(header::ORIGIN).cloned();
        let fetch_site = request.headers().get("sec-fetch-site").cloned();
        if !valid_request_origin(origin, fetch_site, &state).await {
            return (StatusCode::FORBIDDEN, "Origem não autorizada").into_response();
        }

        let has_bearer = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("Bearer "));

        let csrf_exempt = matches!(
            request.uri().path(),
            "/api/auth/login" | "/api/auth/setup" | "/api/auth/recover"
        );
        if !has_bearer && !csrf_exempt {
            let csrf_header = request
                .headers()
                .get("x-csrf-token")
                .and_then(|value| value.to_str().ok());

            let session = crate::auth::extract_session_cookie(request.headers());
            let valid_csrf = if let (Some(token), Some(session)) = (csrf_header, session) {
                let session_hash = sha256_hex(&session);
                let csrf_hash = sha256_hex(token);
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM sessions WHERE id = ? AND csrf_hash = ? AND expires_at > datetime('now')",
                )
                .bind(session_hash)
                .bind(csrf_hash)
                .fetch_one(&state.pool)
                .await
                .unwrap_or(0) == 1
            } else {
                false
            };
            #[cfg(test)]
            let valid_csrf = valid_csrf || csrf_header == Some(state.csrf_token.as_str());
            if !valid_csrf {
                return (StatusCode::FORBIDDEN, "Token CSRF inválido ou ausente").into_response();
            }
        }
    }

    let req_origin = request.headers().get(header::ORIGIN).cloned();
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );

    if let Some(origin) = req_origin
        && is_allowed_origin_for_state(&origin, &state).await
    {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }

    response
}

async fn normalize_error_responses(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .unwrap_or_default();
    let message = std::str::from_utf8(&bytes)
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("erro HTTP"));
    let code = match status {
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        StatusCode::BAD_GATEWAY => "upstream_error",
        StatusCode::SERVICE_UNAVAILABLE => "upstream_unavailable",
        _ if status.is_client_error() => "invalid_request",
        _ => "internal_error",
    };
    let payload =
        serde_json::to_vec(&crate::error::error_json(code, message)).unwrap_or_else(|_| {
            br#"{"error":{"code":"internal_error","message":"erro interno do servidor"}}"#.to_vec()
        });
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Response::from_parts(parts, Body::from(payload))
}

fn is_allowed_origin(origin: &HeaderValue, security: &crate::config::SecurityConfig) -> bool {
    let Ok(origin_str) = origin.to_str() else {
        return false;
    };
    if crate::config::is_extension_origin(origin_str) {
        return security
            .extension_origins
            .iter()
            .any(|allowed| allowed == origin_str);
    }
    origin_str
        .strip_prefix("http://")
        .or_else(|| origin_str.strip_prefix("https://"))
        .and_then(|authority| authority.split('/').next())
        .is_some_and(|authority| valid_host(HeaderValue::from_str(authority).ok().as_ref()))
}

async fn is_allowed_origin_for_state(origin: &HeaderValue, state: &AppState) -> bool {
    if is_allowed_origin(origin, &state.security) {
        return true;
    }
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    if !crate::config::is_extension_origin(origin) {
        return false;
    }
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM api_tokens WHERE extension_origin = ? AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > datetime('now'))",
    )
    .bind(origin)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0) > 0
}

fn valid_host(host: Option<&HeaderValue>) -> bool {
    let Some(host) = host.and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let hostname = host
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .unwrap_or_else(|| host.split(':').next().unwrap_or(host));

    matches!(
        hostname.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

fn is_mutating(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

async fn valid_request_origin(
    origin: Option<HeaderValue>,
    fetch_site: Option<HeaderValue>,
    state: &AppState,
) -> bool {
    if let Some(origin) = origin.as_ref() {
        return is_allowed_origin_for_state(origin, state).await;
    }

    if let Some(site) = fetch_site.as_ref()
        && !matches!(
            site.to_str().ok(),
            Some("same-origin") | Some("same-site") | Some("none")
        )
    {
        return false;
    }

    true
}

fn sha256_hex(value: &str) -> String {
    use ring::digest::{Context, SHA256};
    let mut context = Context::new(&SHA256);
    context.update(value.as_bytes());
    context
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
async fn get_csrf(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"token": state.csrf_token}))
}

#[cfg(test)]
fn generate_csrf_token() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0_u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .expect("secure random");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(serde::Deserialize, Default)]
struct YoutubeSyncQuery {
    reset_failed: Option<bool>,
    force_unofficial: Option<bool>,
}

async fn sync_youtube(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<YoutubeSyncQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    if query.force_unofficial.unwrap_or(false) {
        let _ = crate::youtube_worker::reset_unofficial_failed_attempts(&state.pool).await;
    } else if query.reset_failed.unwrap_or(false) {
        let _ = crate::youtube_worker::reset_failed_attempts(&state.pool).await;
    }
    state.youtube.trigger();
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": "queued" })),
    )
}

async fn get_youtube_status(State(state): State<AppState>) -> Json<YoutubeWorkerStatus> {
    Json(state.youtube.get_status().await)
}

async fn get_update_status(State(state): State<AppState>) -> Json<crate::updates::UpdateStatus> {
    Json(state.updates.status().await)
}

async fn check_for_updates(
    State(state): State<AppState>,
) -> Result<Json<crate::updates::UpdateStatus>, (StatusCode, String)> {
    state
        .updates
        .check()
        .await
        .map(Json)
        .map_err(|error| (StatusCode::BAD_GATEWAY, error))
}

async fn download_update(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<crate::updates::UpdateStatus>), (StatusCode, String)> {
    state
        .updates
        .start_download()
        .await
        .map(|status| (StatusCode::ACCEPTED, Json(status)))
        .map_err(|error| (StatusCode::CONFLICT, error))
}

async fn apply_update(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<crate::updates::UpdateStatus>), (StatusCode, String)> {
    state
        .updates
        .apply()
        .await
        .map(|status| (StatusCode::ACCEPTED, Json(status)))
        .map_err(|error| (StatusCode::CONFLICT, error))
}

async fn reorganize_all_media(
    State(state): State<AppState>,
) -> Result<Json<crate::organizer::OrganizeSummary>, (StatusCode, String)> {
    if !crate::organizer::media_mutations_enabled() {
        return Err((
            StatusCode::FORBIDDEN,
            "reorganização desabilitada; defina MYMEDIA_ALLOW_MEDIA_MUTATIONS=1 para autorizá-la explicitamente".to_owned(),
        ));
    }
    crate::organizer::organize_all_official_media(&state.pool)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn preview_reorganize_all_media(
    State(state): State<AppState>,
) -> Result<Json<crate::organizer::OrganizePreview>, (StatusCode, String)> {
    crate::organizer::preview_all_official_media(&state.pool)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn get_youtube_settings(
    State(state): State<AppState>,
) -> Json<crate::youtube_worker::YoutubeSettings> {
    let settings = crate::youtube_worker::load_youtube_settings(&state.pool).await;
    Json(settings)
}

async fn update_youtube_settings(
    State(state): State<AppState>,
    Json(mut payload): Json<crate::youtube_worker::YoutubeSettings>,
) -> Result<Json<crate::youtube_worker::YoutubeSettings>, StatusCode> {
    payload.browser = payload.browser.trim().to_ascii_lowercase();
    if !crate::youtube_worker::is_valid_browser(&payload.browser) {
        return Err(StatusCode::BAD_REQUEST);
    }
    crate::youtube_worker::save_youtube_settings(&state.pool, &payload)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to save YouTube settings");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if payload.use_browser_cookies {
        let _ = crate::youtube_worker::reset_failed_attempts(&state.pool).await;
        state.youtube.trigger();
    }
    Ok(Json(payload))
}

async fn get_home_settings(State(state): State<AppState>) -> Json<crate::catalog::HomeSettings> {
    let settings = crate::catalog::load_home_settings(&state.pool).await;
    Json(settings)
}

async fn update_home_settings(
    State(state): State<AppState>,
    Json(payload): Json<crate::catalog::HomeSettings>,
) -> Result<Json<crate::catalog::HomeSettings>, StatusCode> {
    crate::catalog::save_home_settings(&state.pool, &payload)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to save home settings");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let updated = crate::catalog::load_home_settings(&state.pool).await;
    Ok(Json(updated))
}

async fn get_subtitle_settings(
    State(state): State<AppState>,
) -> Json<crate::subtitles::SubtitleSettings> {
    let settings = crate::subtitles::load_subtitle_settings(&state.pool).await;
    Json(settings)
}

async fn update_subtitle_settings(
    State(state): State<AppState>,
    Json(payload): Json<crate::subtitles::SubtitleSettings>,
) -> Result<Json<crate::subtitles::SubtitleSettings>, StatusCode> {
    crate::subtitles::save_subtitle_settings(&state.pool, &payload)
        .await
        .map_err(|e| {
            tracing::error!(%e, "failed to save subtitle settings");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let updated = crate::subtitles::load_subtitle_settings(&state.pool).await;
    Ok(Json(updated))
}

async fn get_media_subtitles(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
) -> Result<Json<Vec<crate::subtitles::SubtitleTrackSummary>>, StatusCode> {
    let list = crate::subtitles::list_subtitles_for_media(&state.pool, media_id)
        .await
        .map_err(|e| {
            tracing::error!(%e, media_id, "failed to list subtitles");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(list))
}

async fn serve_media_subtitle(
    State(state): State<AppState>,
    AxumPath((media_id, lang)): AxumPath<(i64, String)>,
) -> Result<Response, StatusCode> {
    let file_path = crate::subtitles::get_subtitle_file(&state.pool, media_id, &lang)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if !file_path.is_file() {
        return Err(StatusCode::NOT_FOUND);
    }

    let content = tokio::fs::read(&file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(content))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize, Default)]
struct DownloadSubtitlesPayload {
    languages: Option<Vec<String>>,
}

async fn download_media_subtitles(
    State(state): State<AppState>,
    AxumPath(media_id): AxumPath<i64>,
    payload: Option<Json<DownloadSubtitlesPayload>>,
) -> Result<Json<Vec<crate::subtitles::SubtitleTrackSummary>>, (StatusCode, String)> {
    let languages = payload.and_then(|p| p.0.languages);
    crate::subtitles::sync_subtitles_for_media(
        &state.pool,
        state.thumbnails.cache_dir(),
        media_id,
        languages,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let list = crate::subtitles::list_subtitles_for_media(&state.pool, media_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "falha ao listar".to_string(),
            )
        })?;

    Ok(Json(list))
}

async fn sync_all_subtitles(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let count =
        crate::subtitles::sync_all_pending_subtitles(&state.pool, state.thumbnails.cache_dir())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "downloaded": count })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[test]
    fn accepts_only_loopback_hosts() {
        assert!(valid_host(Some(&HeaderValue::from_static(
            "127.0.0.1:4173"
        ))));
        assert!(valid_host(Some(&HeaderValue::from_static(
            "localhost:4173"
        ))));
        assert!(valid_host(Some(&HeaderValue::from_static("[::1]:4173"))));
        assert!(!valid_host(Some(&HeaderValue::from_static(
            "evil.example:4173"
        ))));
        assert!(!valid_host(None));
    }

    #[test]
    fn selects_a_valid_signed_extension_by_semantic_version() {
        assert!(parse_extension_version("1.10.0") > parse_extension_version("1.9.9"));
        assert!(parse_extension_version("1.2-beta").is_none());

        let extension_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extension");
        let selected = find_signed_xpi(&extension_dir).expect("signed extension package");
        let version = inspect_signed_xpi(&selected).expect("valid signed extension package");
        assert!(version >= ExtensionVersion([0, 1, 0, 0]));
    }

    #[test]
    fn detects_firefox_and_chromium_browser_families() {
        assert_eq!(
            detect_browser_family(Some("Mozilla/5.0 Firefox/145.0")),
            BrowserFamily::Firefox
        );
        assert_eq!(
            detect_browser_family(Some("Mozilla/5.0 Chrome/142.0 Safari/537.36")),
            BrowserFamily::Chromium
        );
        assert_eq!(
            detect_browser_family(Some("Mozilla/5.0 Chrome/142.0 Edg/142.0")),
            BrowserFamily::Chromium
        );
        assert_eq!(
            detect_browser_family(Some("Mozilla/5.0 Version/18.0 Safari/605.1.15")),
            BrowserFamily::Unsupported
        );
    }

    #[test]
    fn packages_the_chromium_extension_as_a_zip() {
        use std::io::Read;
        let extension_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extension");
        let bytes = package_chromium_extension(&extension_dir).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut manifest = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(manifest["name"], "ZMydia Downloader");
        assert_eq!(
            manifest["background"]["service_worker"],
            "service-worker.js"
        );
        assert!(manifest.get("browser_specific_settings").is_none());
    }

    #[tokio::test]
    async fn serves_the_package_selected_from_the_browser_user_agent() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.extension_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extension");
        let app = Router::new()
            .route("/api/extension/info", get(get_extension_info))
            .route("/extension/zmydia", get(serve_detected_extension))
            .with_state(state);

        let chrome_info = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/extension/info")
                    .header(header::USER_AGENT, "Mozilla/5.0 Chrome/142.0 Safari/537.36")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let info: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(chrome_info.into_body(), 16 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(info["detected_browser"], "chromium");
        assert_eq!(info["package_type"], "zip");

        let chrome_package = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/extension/zmydia")
                    .header(header::USER_AGENT, "Mozilla/5.0 Chrome/142.0 Safari/537.36")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(chrome_package.status(), StatusCode::OK);
        assert_eq!(
            chrome_package.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/zip"
        );

        let firefox_package = app
            .oneshot(
                Request::builder()
                    .uri("/extension/zmydia")
                    .header(header::USER_AGENT, "Mozilla/5.0 Firefox/145.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(firefox_package.status(), StatusCode::OK);
        assert_eq!(
            firefox_package.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/x-xpinstall"
        );
    }

    #[tokio::test]
    async fn plain_text_handler_errors_are_normalized_to_the_api_envelope() {
        let app = Router::new()
            .route(
                "/test-error",
                get(|| async { (StatusCode::NOT_FOUND, "registro ausente") }),
            )
            .layer(middleware::from_fn(normalize_error_responses));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/test-error")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"]["code"], "not_found");
        assert_eq!(payload["error"]["message"], "registro ausente");
    }

    #[tokio::test]
    async fn csrf_protection_on_mutating_requests() {
        use axum::routing::post;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.csrf_token = "secret_csrf_123".to_owned();

        let app = Router::new()
            .route("/api/auth/csrf", get(get_csrf))
            .route("/test-mutate", post(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. GET /api/auth/csrf retorna o token configurado
        let csrf_req = Request::builder()
            .uri("/api/auth/csrf")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(csrf_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let csrf_resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(csrf_resp["token"].as_str(), Some("secret_csrf_123"));

        // 2. POST sem token -> 403 Forbidden
        let bad_post = Request::builder()
            .method("POST")
            .uri("/test-mutate")
            .header(header::HOST, "127.0.0.1:4173")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(bad_post).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // 3. POST com token inválido -> 403 Forbidden
        let wrong_token_post = Request::builder()
            .method("POST")
            .uri("/test-mutate")
            .header(header::HOST, "127.0.0.1:4173")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "wrong_token")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(wrong_token_post).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // 4. POST com token correto -> 200 OK
        let good_post = Request::builder()
            .method("POST")
            .uri("/test-mutate")
            .header(header::HOST, "127.0.0.1:4173")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "secret_csrf_123")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(good_post).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_generate_csrf_token_safe() {
        let token = generate_csrf_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_home_settings_endpoint_lifecycle() {
        use axum::routing::get;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.csrf_token = "csrf_home_123".to_owned();

        let app = Router::new()
            .route(
                "/api/settings/home",
                get(get_home_settings).put(update_home_settings),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. GET initial home settings -> empty
        let get_req = Request::builder()
            .uri("/api/settings/home")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let body: crate::catalog::HomeSettings = serde_json::from_slice(&bytes).unwrap();
        assert!(body.hidden_tags.is_empty());

        // 2. PUT with hidden tags
        let put_body = serde_json::json!({
            "hidden_tags": ["#Spoiler", "vlog", "  VLOG "]
        });
        let put_req = Request::builder()
            .method("PUT")
            .uri("/api/settings/home")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::CONTENT_TYPE, "application/json")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "csrf_home_123")
            .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(put_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let updated: crate::catalog::HomeSettings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(updated.hidden_tags, vec!["spoiler", "vlog"]);

        // 3. GET after PUT confirms persistence
        let get_req2 = Request::builder()
            .uri("/api/settings/home")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res2 = app.clone().oneshot(get_req2).await.unwrap();
        assert_eq!(res2.status(), StatusCode::OK);
        let bytes2 = axum::body::to_bytes(res2.into_body(), 1024).await.unwrap();
        let body2: crate::catalog::HomeSettings = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(body2.hidden_tags, vec!["spoiler", "vlog"]);
    }

    #[tokio::test]
    async fn test_subtitle_endpoints_lifecycle() {
        use axum::routing::get;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.csrf_token = "csrf_sub_123".to_owned();

        let app = Router::new()
            .route(
                "/api/settings/subtitles",
                get(get_subtitle_settings).put(update_subtitle_settings),
            )
            .route("/api/media/{id}/subtitles", get(get_media_subtitles))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. GET initial subtitle settings -> default (auto_download: true, pt, en)
        let get_req = Request::builder()
            .uri("/api/settings/subtitles")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let body: crate::subtitles::SubtitleSettings = serde_json::from_slice(&bytes).unwrap();
        assert!(body.auto_download);
        assert_eq!(body.languages, vec!["pt", "en"]);

        // 2. PUT with custom languages
        let put_body = serde_json::json!({
            "auto_download": true,
            "languages": ["pt", "en", "es"],
            "include_auto_generated": true
        });
        let put_req = Request::builder()
            .method("PUT")
            .uri("/api/settings/subtitles")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::CONTENT_TYPE, "application/json")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "csrf_sub_123")
            .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(put_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let updated: crate::subtitles::SubtitleSettings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(updated.languages, vec!["en", "es", "pt"]);
        assert!(updated.include_auto_generated);

        // 3. GET /api/media/1/subtitles -> empty list
        let subs_req = Request::builder()
            .uri("/api/media/1/subtitles")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(subs_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let list: Vec<crate::subtitles::SubtitleTrackSummary> =
            serde_json::from_slice(&bytes).unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_listenbrainz_endpoints_lifecycle() {
        use axum::routing::get;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.csrf_token = "csrf_lb_123".to_owned();

        let app = Router::new()
            .route(
                "/api/settings/listenbrainz",
                get(crate::music::get_listenbrainz_settings_endpoint)
                    .put(crate::music::update_listenbrainz_settings_endpoint),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. GET initial settings -> default disabled
        let get_req = Request::builder()
            .uri("/api/settings/listenbrainz")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let body: crate::music::ListenBrainzSettings = serde_json::from_slice(&bytes).unwrap();
        assert!(!body.enabled);
        assert_eq!(body.user_token, "");

        // 2. PUT with active token
        let put_body = serde_json::json!({
            "enabled": true,
            "user_token": "token_abc_xyz",
            "username": "example_user",
            "auto_scrobble": true
        });
        let put_req = Request::builder()
            .method("PUT")
            .uri("/api/settings/listenbrainz")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::CONTENT_TYPE, "application/json")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "csrf_lb_123")
            .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(put_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let updated: crate::music::ListenBrainzSettings = serde_json::from_slice(&bytes).unwrap();
        assert!(updated.enabled);
        assert!(updated.has_token);
        assert_eq!(updated.token_hint.as_deref(), Some("••••••••_xyz"));
        assert_eq!(
            updated.user_token, "",
            "Token em texto plano nunca deve ser retornado na resposta da API"
        );
        assert_eq!(updated.username.as_deref(), Some("example_user"));

        // O token interno no banco de dados deve ser decifrado corretamente pelo serviço
        let in_db = crate::music::load_listenbrainz_settings(&state.pool, &state.crypto).await;
        assert_eq!(in_db.user_token, "token_abc_xyz");
    }

    #[tokio::test]
    async fn test_download_endpoints_lifecycle() {
        use axum::routing::get;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "INSERT INTO media_sources (id, path, mode, status) VALUES (99, '/tmp/download-target', 'managed', 'ready')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut state = AppState::for_test(pool.clone());
        state.csrf_token = "csrf_dl_123".to_owned();

        let app = Router::new()
            .route(
                "/api/settings/downloads",
                get(crate::downloader::get_download_settings_endpoint)
                    .put(crate::downloader::update_download_settings_endpoint),
            )
            .route(
                "/api/downloads",
                get(crate::downloader::list_downloads_endpoint)
                    .post(crate::downloader::create_download_endpoint),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. GET initial download settings
        let get_req = Request::builder()
            .uri("/api/settings/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let s: crate::downloader::DownloadSettings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(s.target_source_id, None);
        assert_eq!(s.preferred_container, "mkv");

        // 2. PUT download settings with source id
        let put_body = serde_json::json!({
            "target_source_id": 99,
            "preferred_container": "mp4"
        });
        let put_req = Request::builder()
            .method("PUT")
            .uri("/api/settings/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::CONTENT_TYPE, "application/json")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "csrf_dl_123")
            .body(Body::from(serde_json::to_vec(&put_body).unwrap()))
            .unwrap();
        let res = app.clone().oneshot(put_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let updated: crate::downloader::DownloadSettings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(updated.target_source_id, Some(99));
        assert_eq!(updated.preferred_container, "mp4");

        // 3. GET /api/downloads -> empty list initially
        let list_req = Request::builder()
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(list_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 10)
            .await
            .unwrap();
        let list: Vec<crate::downloader::DownloadJob> = serde_json::from_slice(&bytes).unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_extension_endpoints() {
        use axum::routing::post;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.csrf_token = "csrf_ext_test".to_owned();
        state.security.extension_origins = vec![
            "moz-extension://test-uuid-1234".to_owned(),
            "chrome-extension://abcdefghijklmnop".to_owned(),
        ];

        let app = Router::new()
            .route("/api/extension/info", get(get_extension_info))
            .route("/api/downloads", post(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. GET /api/extension/info
        let req = Request::builder()
            .uri("/api/extension/info")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        let info: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(info.get("has_signed").is_some());
        assert!(info.get("has_unsigned").is_some());

        // 2. Preflight OPTIONS from moz-extension://
        let options_req = Request::builder()
            .method("OPTIONS")
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::ORIGIN, "moz-extension://test-uuid-1234")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(options_req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "moz-extension://test-uuid-1234"
        );

        let chrome_options_req = Request::builder()
            .method("OPTIONS")
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::ORIGIN, "chrome-extension://abcdefghijklmnop")
            .body(Body::empty())
            .unwrap();
        let chrome_res = app.clone().oneshot(chrome_options_req).await.unwrap();
        assert_eq!(chrome_res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            chrome_res
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "chrome-extension://abcdefghijklmnop"
        );

        // 3. Preflight OPTIONS from evil.com -> 403 Forbidden
        let evil_options_req = Request::builder()
            .method("OPTIONS")
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::ORIGIN, "https://evil.com")
            .body(Body::empty())
            .unwrap();
        let evil_res = app.clone().oneshot(evil_options_req).await.unwrap();
        assert_eq!(evil_res.status(), StatusCode::FORBIDDEN);

        // 4. Preflight OPTIONS without origin -> 403 Forbidden
        let no_origin_options_req = Request::builder()
            .method("OPTIONS")
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .body(Body::empty())
            .unwrap();
        let no_origin_res = app.clone().oneshot(no_origin_options_req).await.unwrap();
        assert_eq!(no_origin_res.status(), StatusCode::FORBIDDEN);

        // 5. Preflight OPTIONS from local loopback (e.g. dev frontend) -> 204 No Content
        let local_options_req = Request::builder()
            .method("OPTIONS")
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::ORIGIN, "http://127.0.0.1:5173")
            .body(Body::empty())
            .unwrap();
        let local_res = app.clone().oneshot(local_options_req).await.unwrap();
        assert_eq!(local_res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            local_res
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "http://127.0.0.1:5173"
        );

        // 6. An origin discovered during pairing must be accepted on the actual POST,
        // not only during the CORS preflight.
        sqlx::query(
            "INSERT INTO users (id, username, password_hash) VALUES (7, 'extension-test', 'unused')",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO api_tokens (user_id, token_hash, token_hint, name, scopes, extension_origin)
             VALUES (7, 'unused', '••••test', 'paired', 'downloads:write', 'moz-extension://paired-from-db')",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        let paired_post = Request::builder()
            .method("POST")
            .uri("/api/downloads")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::ORIGIN, "moz-extension://paired-from-db")
            .header(header::AUTHORIZATION, "Bearer test")
            .body(Body::empty())
            .unwrap();
        let paired_post_res = app.clone().oneshot(paired_post).await.unwrap();
        assert_eq!(paired_post_res.status(), StatusCode::OK);
        assert_eq!(
            paired_post_res
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "moz-extension://paired-from-db"
        );
    }

    #[tokio::test]
    async fn test_youtube_settings_browser_allowlist() {
        use axum::routing::get;
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let mut state = AppState::for_test(pool);
        state.csrf_token = "csrf_yt_sec".to_owned();

        let app = Router::new()
            .route(
                "/api/settings/youtube",
                get(get_youtube_settings).put(update_youtube_settings),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                security_middleware,
            ))
            .with_state(state.clone());

        // 1. PUT with unsupported/malicious browser -> 400 Bad Request
        let bad_put = serde_json::json!({
            "use_browser_cookies": true,
            "browser": "malicious_browser; evil_command"
        });
        let bad_req = Request::builder()
            .method("PUT")
            .uri("/api/settings/youtube")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::CONTENT_TYPE, "application/json")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "csrf_yt_sec")
            .body(Body::from(serde_json::to_vec(&bad_put).unwrap()))
            .unwrap();
        let bad_res = app.clone().oneshot(bad_req).await.unwrap();
        assert_eq!(bad_res.status(), StatusCode::BAD_REQUEST);

        // 2. PUT with valid browser -> 200 OK
        let good_put = serde_json::json!({
            "use_browser_cookies": true,
            "browser": "Chrome"
        });
        let good_req = Request::builder()
            .method("PUT")
            .uri("/api/settings/youtube")
            .header(header::HOST, "127.0.0.1:4173")
            .header(header::CONTENT_TYPE, "application/json")
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", "csrf_yt_sec")
            .body(Body::from(serde_json::to_vec(&good_put).unwrap()))
            .unwrap();
        let good_res = app.clone().oneshot(good_req).await.unwrap();
        assert_eq!(good_res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(good_res.into_body(), 1024)
            .await
            .unwrap();
        let updated: crate::youtube_worker::YoutubeSettings =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(updated.browser, "chrome");
        assert!(updated.use_browser_cookies);
    }
}
