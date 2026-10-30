use axum::http::StatusCode;
use axum::http::header::LOCATION;
use axum::response::{IntoResponse, Response};

use super::htmx::HX_REDIRECT;
use super::sign_in;
use crate::domain::auth::{AuthError, PASSKEY_NAME_MAX, PasskeyNameError};
use crate::domain::finance::money::{AMOUNT_MAX, AMOUNT_MIN, AmountError, Crowns};
use crate::domain::finance::{DESCRIPTION_MAX, DescriptionError};
use crate::domain::members::{DISPLAY_NAME_MAX, NameError};
use crate::views::finance::crowns;

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
    /// A link was used by a browser that is already signed in, which could attach a passkey to
    /// the wrong Member.
    #[error("already signed in")]
    AlreadySignedIn,
    /// A signed-in Member who isn't the Admin asked for an admin route.
    #[error("Admin only")]
    AdminOnly,
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
            Self::AlreadySignedIn => (
                StatusCode::CONFLICT,
                Some(
                    "You're already signed in on this device. Log out first to use this link."
                        .to_owned(),
                ),
            ),
            Self::AdminOnly => (
                StatusCode::FORBIDDEN,
                Some("Only the Admin can do that.".to_owned()),
            ),
            Self::Auth(err) => auth_outcome(err),
        }
    }
}

fn auth_outcome(err: &AuthError) -> (StatusCode, Option<String>) {
    let say = |status, message: &str| (status, Some(message.to_owned()));
    match err {
        AuthError::LinkUnusable => say(
            StatusCode::GONE,
            "This link has expired, was already used or was revoked. Ask the Admin for a new one.",
        ),
        AuthError::AdminExists => say(StatusCode::CONFLICT, "The Crew already has an Admin."),
        AuthError::Name(err) => (StatusCode::UNPROCESSABLE_ENTITY, Some(name_message(err))),
        AuthError::NoSuchMember => say(
            StatusCode::NOT_FOUND,
            "That Member isn't in the Crew. Reload the page to see who is.",
        ),
        AuthError::CeremonyExpired => say(
            StatusCode::BAD_REQUEST,
            "That took too long. Please try again.",
        ),
        AuthError::UnknownPasskey => say(
            StatusCode::UNAUTHORIZED,
            "Asgard doesn't know this passkey. Try another one, or ask the Admin for a Device link.",
        ),
        AuthError::NoSuchPasskey => say(
            StatusCode::NOT_FOUND,
            "That passkey no longer exists. Reload the page to see your passkeys.",
        ),
        AuthError::LastPasskey => say(
            StatusCode::CONFLICT,
            "Your only passkey can't be deleted. Add another one first.",
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

/// What a person is told when the passkey name they typed can't be used.
pub fn passkey_name_message(err: &PasskeyNameError) -> String {
    match err {
        PasskeyNameError::Blank => "Enter a name for the passkey.".to_owned(),
        PasskeyNameError::TooLong => {
            format!("Keep the passkey name to {PASSKEY_NAME_MAX} characters or fewer.")
        }
    }
}

/// What a Member is told when the amount they typed for an Expense can't be used.
pub fn amount_message(err: AmountError) -> String {
    match err {
        AmountError::NotWholeCrowns => "Enter the amount in whole crowns.".to_owned(),
        AmountError::OutOfRange => format!(
            "Enter an amount from {} to {}.",
            crowns(Crowns::new(AMOUNT_MIN)),
            crowns(Crowns::new(AMOUNT_MAX))
        ),
    }
}

/// What a Member is told when the description they typed for an Expense can't be used.
pub fn description_message(err: DescriptionError) -> String {
    match err {
        DescriptionError::Blank => "Say what the money was for.".to_owned(),
        DescriptionError::TooLong => {
            format!("Keep the description to {DESCRIPTION_MAX} characters or fewer.")
        }
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
    fn explains_each_unusable_passkey_name() {
        assert_eq!(
            passkey_name_message(&PasskeyNameError::Blank),
            "Enter a name for the passkey."
        );
        assert_eq!(
            passkey_name_message(&PasskeyNameError::TooLong),
            "Keep the passkey name to 40 characters or fewer."
        );
    }

    #[test]
    fn explains_each_unusable_amount() {
        assert_eq!(
            amount_message(AmountError::NotWholeCrowns),
            "Enter the amount in whole crowns."
        );
        assert_eq!(
            amount_message(AmountError::OutOfRange),
            "Enter an amount from 1\u{a0}Kč to 1\u{a0}000\u{a0}000\u{a0}Kč."
        );
    }

    #[test]
    fn explains_each_unusable_description() {
        assert_eq!(
            description_message(DescriptionError::Blank),
            "Say what the money was for."
        );
        assert_eq!(
            description_message(DescriptionError::TooLong),
            "Keep the description to 500 characters or fewer."
        );
    }

    #[test]
    fn server_errors_say_nothing_about_their_cause() {
        let (status, message) = AppError::Database(sqlx::Error::PoolTimedOut).outcome();

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, None);
    }
}
