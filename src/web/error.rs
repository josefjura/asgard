use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// The one error type handlers return.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database unavailable")]
    DatabaseUnavailable(#[source] sqlx::Error),
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("request not sent from our origin")]
    ForeignOrigin,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::DatabaseUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ForeignOrigin => StatusCode::FORBIDDEN,
        };
        if status.is_server_error() {
            tracing::error!(error = ?self, "request failed");
        } else {
            tracing::warn!(error = %self, "request rejected");
        }
        (status, status.canonical_reason().unwrap_or_default()).into_response()
    }
}
