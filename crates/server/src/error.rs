use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;
use thiserror::Error;

pub fn error_json(code: &str, message: impl Into<String>) -> serde_json::Value {
    json!({
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuração inválida: {0}")]
    Config(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Crypto(#[from] crate::crypto::CryptoError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        if let Self::Database(error) = &self {
            crate::db::observe_sqlite_error(error);
        }
        tracing::error!(error = %self, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json("internal_error", "erro interno do servidor")),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_has_stable_code_and_message() {
        let body = error_json("not_found", "mídia não encontrada");
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "mídia não encontrada");
    }
}
