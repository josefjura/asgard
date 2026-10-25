use axum::http::StatusCode;
use axum::http::header::LOCATION;
use axum::response::{IntoResponse, Response};

use super::htmx::HX_REDIRECT;
use super::sign_in;
use crate::domain::auth::AuthError;

/// The one error type handlers return.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database unavailable")]
    DatabaseUnavailable(#[source] sqlx::Error),
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("request not sent from our origin")]
    ForeignOrigin,
    /// No valid session. `htmx` is whether htmx sent the request, which can't follow a redirect.
    #[error("sign-in required")]
    SignInRequired { htmx: bool },
    #[error(transparent)]
    Auth(#[from] AuthError),
}

impl AppError {
    const fn status(&self) -> StatusCode {
        match self {
            Self::DatabaseUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ForeignOrigin => StatusCode::FORBIDDEN,
            Self::SignInRequired { htmx: true } => StatusCode::UNAUTHORIZED,
            Self::SignInRequired { htmx: false } => StatusCode::SEE_OTHER,
            Self::Auth(err) => match err {
                AuthError::LinkUnusable => StatusCode::GONE,
                AuthError::AdminExists => StatusCode::CONFLICT,
                AuthError::Name(_) => StatusCode::UNPROCESSABLE_ENTITY,
                AuthError::CeremonyExpired | AuthError::PasskeyRejected(_) => {
                    StatusCode::BAD_REQUEST
                }
                AuthError::Webauthn(_) | AuthError::Credential(_) | AuthError::Database(_) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            },
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if let Self::SignInRequired { htmx } = self {
            let header = if htmx { HX_REDIRECT } else { LOCATION };
            return (status, [(header, sign_in::PATH)]).into_response();
        }
        if status.is_server_error() {
            tracing::error!(error = ?self, "request failed");
        } else {
            tracing::warn!(error = %self, "request rejected");
        }
        // An auth error's message tells the person what to do; the others stay generic.
        let body = match &self {
            Self::Auth(err) if status.is_client_error() => err.to_string(),
            _ => status.canonical_reason().unwrap_or_default().to_owned(),
        };
        (status, body).into_response()
    }
}
