use axum::http::StatusCode;
use axum::http::header::LOCATION;
use axum::response::{IntoResponse, Response};

use super::htmx::HX_REDIRECT;
use super::sign_in;
use crate::domain::auth::AuthError;
use crate::domain::members::{DISPLAY_NAME_MAX, NameError};

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
    /// The status, and for a mistake the person can act on, a sentence telling them what to do.
    fn outcome(&self) -> (StatusCode, Option<String>) {
        match self {
            Self::DatabaseUnavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, None),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, None),
            Self::ForeignOrigin => (StatusCode::FORBIDDEN, None),
            Self::SignInRequired { htmx: true } => (StatusCode::UNAUTHORIZED, None),
            Self::SignInRequired { htmx: false } => (StatusCode::SEE_OTHER, None),
            Self::Auth(err) => auth_outcome(err),
        }
    }
}

fn auth_outcome(err: &AuthError) -> (StatusCode, Option<String>) {
    let say = |status, message: &str| (status, Some(message.to_owned()));
    match err {
        AuthError::LinkUnusable => say(
            StatusCode::GONE,
            "This link has expired or was already used. Ask the Admin for a new one.",
        ),
        AuthError::AdminExists => say(StatusCode::CONFLICT, "The Crew already has an Admin."),
        AuthError::Name(err) => (StatusCode::UNPROCESSABLE_ENTITY, Some(name_message(err))),
        AuthError::CeremonyExpired => say(
            StatusCode::BAD_REQUEST,
            "That took too long. Please try again.",
        ),
        AuthError::PasskeyRejected(_) => say(
            StatusCode::BAD_REQUEST,
            "Your passkey couldn't be verified. Please try again.",
        ),
        AuthError::Webauthn(_) | AuthError::Credential(_) | AuthError::Database(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, None)
        }
    }
}

/// What a person is told when the display name they chose can't be used.
pub fn name_message(err: &NameError) -> String {
    match err {
        NameError::Blank => "Enter a display name.".to_owned(),
        NameError::TooLong => {
            format!("Keep the display name to {DISPLAY_NAME_MAX} characters or fewer.")
        }
        NameError::Taken(name) => format!("Someone in the Crew is already called {name}."),
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = self.outcome();
        if let Self::SignInRequired { htmx } = self {
            // htmx 2 acts on HX-Redirect before it looks at the status, so a 401 still redirects.
            let header = if htmx { HX_REDIRECT } else { LOCATION };
            return (status, [(header, sign_in::PATH)]).into_response();
        }
        if status.is_server_error() {
            tracing::error!(error = ?self, "request failed");
        } else {
            tracing::warn!(error = %self, "request rejected");
        }
        let body =
            message.unwrap_or_else(|| status.canonical_reason().unwrap_or_default().to_owned());
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::members::fixtures::name;

    #[test]
    fn explains_each_unusable_display_name() {
        assert_eq!(name_message(&NameError::Blank), "Enter a display name.");
        assert_eq!(
            name_message(&NameError::TooLong),
            "Keep the display name to 40 characters or fewer."
        );
        assert_eq!(
            name_message(&NameError::Taken(name("Josef"))),
            "Someone in the Crew is already called Josef."
        );
    }

    #[test]
    fn server_errors_say_nothing_about_their_cause() {
        let (status, message) = AppError::Database(sqlx::Error::PoolTimedOut).outcome();

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, None);
    }
}
