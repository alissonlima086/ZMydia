use std::path::{Path as StdPath, PathBuf};

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::web::AppState;

pub(crate) async fn stream_media(
    State(state): State<AppState>,
    Path(media_id): Path<i64>,
    request: Request<Body>,
) -> Response {
    let row = sqlx::query_as::<_, (String, bool)>(
        "SELECT f.path, f.is_available
         FROM media m
         JOIN media_files f ON f.media_id = m.id
         WHERE m.id = ? AND f.is_available = 1
         ORDER BY f.id LIMIT 1",
    )
    .bind(media_id)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((raw_path, is_available))) = row else {
        return (StatusCode::NOT_FOUND, "Mídia não encontrada").into_response();
    };

    if !is_available {
        return (StatusCode::GONE, "Mídia indisponível no momento").into_response();
    }

    let Ok(canonical_file) = std::fs::canonicalize(&raw_path) else {
        return (StatusCode::NOT_FOUND, "Arquivo físico inacessível").into_response();
    };

    if !canonical_file.is_file() {
        return (StatusCode::NOT_FOUND, "Caminho não é um arquivo regular").into_response();
    }

    let mut is_authorized = {
        let roots = state.authorized_roots.read().await;
        is_path_authorized(&canonical_file, &roots)
    };

    if !is_authorized {
        // Fallback: cache might be cold or recently updated
        state.refresh_authorized_roots().await;
        let roots = state.authorized_roots.read().await;
        is_authorized = is_path_authorized(&canonical_file, &roots);
    }

    if !is_authorized {
        tracing::warn!(
            path = %canonical_file.display(),
            "attempted to stream a file outside active sources"
        );
        return (
            StatusCode::FORBIDDEN,
            "Arquivo fora do escopo de fontes autorizadas",
        )
            .into_response();
    }

    let service = ServeFile::new(&canonical_file);
    match service.oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Erro ao processar streaming",
        )
            .into_response(),
    }
}

pub(crate) fn is_path_authorized(file_path: &StdPath, authorized_roots: &[PathBuf]) -> bool {
    authorized_roots
        .iter()
        .any(|root| file_path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_authorized_subpaths() {
        let roots = vec![PathBuf::from("/media/example-library")];
        assert!(is_path_authorized(
            StdPath::new("/media/example-library/Channel/song.mp4"),
            &roots
        ));
        assert!(!is_path_authorized(StdPath::new("/etc/passwd"), &roots));
        assert!(!is_path_authorized(
            StdPath::new("/media/other-library/video.mp4"),
            &roots
        ));
    }
}
