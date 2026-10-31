//! Using an Invite or Device link: the page, then the passkey ceremony. An Invite link creates a
//! new Member; a Device link adds a passkey to an existing one.
//!
//! A browser that is already signed in is only offered to log out, so a passkey is never
//! attached to the wrong Member.

#[cfg(test)]
mod device_tests;
#[cfg(test)]
mod invite_tests;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};

use super::access::{Member, SessionCookie, SignedOut, session_cookie};
use super::pages::fresh;
use super::registration::{Challenge, DefaultPasskeyName, NewPasskey};
use super::sign_in::{self, Landing};
use super::{AppError, AppState, welcome};
use crate::domain::auth::{AuthError, LinkKind, LinkToken};
use crate::domain::members::CREW_NAME;
use crate::views;
use crate::views::nav::Tab;
use crate::views::passkey::CeremonyUrls;

/// Where every link of `kind` points, each under its token.
const fn base(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Invite => "/invite",
        LinkKind::Device => "/device",
    }
}

/// Where a link of `kind` points.
#[must_use]
pub fn path(kind: LinkKind, token: &LinkToken) -> String {
    format!("{}/{}", base(kind), token.as_str())
}

/// Where a browser that is already signed in logs out before using the link.
fn log_out_path(kind: LinkKind, token: &LinkToken) -> String {
    format!("{}/log-out", path(kind, token))
}

/// Where the link page's script starts and finishes registering a passkey.
fn ceremony_urls(kind: LinkKind, token: &LinkToken) -> CeremonyUrls {
    let link = path(kind, token);
    CeremonyUrls {
        start: format!("{link}/start"),
        finish: format!("{link}/finish"),
    }
}

/// The same routes under each kind's [`base`], which every handler reads its kind from.
pub fn routes() -> Router<AppState> {
    [LinkKind::Invite, LinkKind::Device]
        .into_iter()
        .fold(Router::new(), |router, kind| {
            let link = Router::new()
                .route("/{token}", get(show))
                .route("/{token}/start", post(start))
                .route("/{token}/finish", post(finish))
                .route("/{token}/log-out", post(log_out))
                .layer(Extension(kind));
            router.nest(base(kind), link)
        })
}

async fn show(
    State(state): State<AppState>,
    Extension(kind): Extension<LinkKind>,
    Path(token): Path<String>,
    member: Option<Member>,
) -> Result<Response, AppError> {
    let token = LinkToken::from_client(token);
    let display_name = match kind {
        LinkKind::Invite => state
            .auth
            .invite(&token)
            .await
            .map(|invite| invite.display_name),
        LinkKind::Device => state
            .auth
            .device_link(&token)
            .await
            .map(|link| link.display_name),
    };
    match display_name {
        Ok(_) if let Some(Member(member)) = member => Ok(fresh(views::invite::signed_in(
            &member.display_name,
            kind,
            &log_out_path(kind, &token),
        ))
        .into_response()),
        Ok(display_name) => {
            let ceremony = ceremony_urls(kind, &token);
            let page = match kind {
                LinkKind::Invite => views::invite::page(CREW_NAME, &display_name, &ceremony),
                LinkKind::Device => views::invite::device_page(CREW_NAME, &display_name, &ceremony),
            };
            Ok(fresh(page).into_response())
        }
        Err(AuthError::LinkUnusable) => {
            let admin = state.members.admin_name().await?;
            Ok((
                StatusCode::GONE,
                fresh(views::invite::link_error(admin.as_ref())),
            )
                .into_response())
        }
        Err(err) => Err(err.into()),
    }
}

async fn start(
    State(state): State<AppState>,
    Extension(kind): Extension<LinkKind>,
    Path(token): Path<String>,
    _: SignedOut,
) -> Result<Json<Challenge>, AppError> {
    let token = LinkToken::from_client(token);
    let (ceremony, options) = match kind {
        LinkKind::Invite => state.auth.start_joining(&token).await?,
        LinkKind::Device => state.auth.start_device_setup(&token).await?,
    };
    Ok(Json(Challenge { ceremony, options }))
}

/// Registers the passkey and signs the browser in: a new Member lands on the welcome page, a
/// Member on a new device on Finance.
async fn finish(
    State(state): State<AppState>,
    Extension(kind): Extension<LinkKind>,
    Path(token): Path<String>,
    // Signing in elsewhere mid-ceremony mustn't let the link attach a passkey either.
    _: SignedOut,
    DefaultPasskeyName(name): DefaultPasskeyName,
    Json(body): Json<NewPasskey>,
) -> Result<impl IntoResponse, AppError> {
    let token = LinkToken::from_client(token);
    let (joined, redirect) = match kind {
        LinkKind::Invite => {
            let joined = state
                .auth
                .finish_joining(&token, &body.ceremony, &body.credential, &name)
                .await?;
            tracing::info!(member = joined.member.get(), "a new Member joined the Crew");
            (joined, welcome::PATH)
        }
        LinkKind::Device => {
            let joined = state
                .auth
                .finish_device_setup(&token, &body.ceremony, &body.credential, &name)
                .await?;
            tracing::info!(member = joined.member.get(), "a Member set up a new device");
            (joined, Tab::Finance.href())
        }
    };
    Ok((
        [(SET_COOKIE, session_cookie(&joined.session))],
        Json(Landing { redirect }),
    ))
}

/// Logs this device out and opens the link again, now ready to use.
async fn log_out(
    State(state): State<AppState>,
    Extension(kind): Extension<LinkKind>,
    Path(token): Path<String>,
    session: SessionCookie,
) -> Result<Response, AppError> {
    let link = path(kind, &LinkToken::from_client(token));
    sign_in::end_session(&state, session, &link).await
}
