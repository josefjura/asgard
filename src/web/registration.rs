//! What registering a passkey exchanges with the passkey script, whether a person is joining
//! through an Invite link or a Member is adding another passkey.

use std::convert::Infallible;
use std::future::ready;

use axum::extract::FromRequestParts;
use axum::http::header::USER_AGENT;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{CreationChallengeResponse, RegisterPublicKeyCredential};

use crate::domain::auth::{CeremonyId, PasskeyName};

/// What the browser needs to create a passkey.
#[derive(Serialize)]
pub struct Challenge {
    pub ceremony: CeremonyId,
    pub options: CreationChallengeResponse,
}

/// The passkey the browser created for a [`Challenge`].
#[derive(Deserialize)]
pub struct NewPasskey {
    pub ceremony: CeremonyId,
    pub credential: RegisterPublicKeyCredential,
}

/// What a passkey made by the requesting browser is called until its Member renames it.
pub struct DefaultPasskeyName(pub PasskeyName);

impl<S: Sync> FromRequestParts<S> for DefaultPasskeyName {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Infallible>> + Send {
        let user_agent = parts
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        ready(Ok(Self(PasskeyName::from_user_agent(user_agent))))
    }
}
