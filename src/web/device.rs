//! Setting up a new device through a Device link: the page, then the passkey ceremony, which
//! adds a passkey to an existing Member instead of creating one.
//!
//! A browser that is already signed in is only offered to log out, so a passkey is never
//! attached to the wrong Member.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::access::{Member, SessionCookie, SignedOut, session_cookie};
use super::pages::fresh;
use super::registration::{Challenge, DefaultPasskeyName, NewPasskey};
use super::sign_in::{self, Landing};
use super::{AppError, AppState};
use crate::domain::auth::{AuthError, LinkKind, LinkToken};
use crate::views;
use crate::views::nav::Tab;
use crate::views::passkey::CeremonyUrls;

/// Where a Device link points.
#[must_use]
pub fn path(token: &LinkToken) -> String {
    format!("/device/{}", token.as_str())
}

/// Where a browser that is already signed in logs out before using the link.
fn log_out_path(token: &LinkToken) -> String {
    format!("{}/log-out", path(token))
}

/// Where the device page's script starts and finishes registering a passkey.
fn ceremony_urls(token: &LinkToken) -> CeremonyUrls {
    let device = path(token);
    CeremonyUrls {
        start: format!("{device}/start"),
        finish: format!("{device}/finish"),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/device/{token}", get(show))
        .route("/device/{token}/start", post(start))
        .route("/device/{token}/finish", post(finish))
        .route("/device/{token}/log-out", post(log_out))
}

async fn show(
    State(state): State<AppState>,
    Path(token): Path<String>,
    member: Option<Member>,
) -> Result<Response, AppError> {
    let token = LinkToken::from_client(token);
    match state.auth.device_link(&token).await {
        Ok(_) if let Some(Member(member)) = member => Ok(fresh(views::invite::signed_in(
            &member.display_name,
            LinkKind::Device,
            &log_out_path(&token),
        ))
        .into_response()),
        Ok(link) => Ok(fresh(views::invite::device_page(
            &link.display_name,
            &ceremony_urls(&token),
        ))
        .into_response()),
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
    Path(token): Path<String>,
    _: SignedOut,
) -> Result<Json<Challenge>, AppError> {
    let (ceremony, options) = state
        .auth
        .start_recovering(&LinkToken::from_client(token))
        .await?;
    Ok(Json(Challenge { ceremony, options }))
}

async fn finish(
    State(state): State<AppState>,
    Path(token): Path<String>,
    // Signing in elsewhere mid-ceremony mustn't let the link attach a passkey either.
    _: SignedOut,
    DefaultPasskeyName(name): DefaultPasskeyName,
    Json(body): Json<NewPasskey>,
) -> Result<impl IntoResponse, AppError> {
    let recovered = state
        .auth
        .finish_recovering(
            &LinkToken::from_client(token),
            &body.ceremony,
            &body.credential,
            &name,
        )
        .await?;
    tracing::info!(
        member = recovered.member.get(),
        "a Member set up a new device"
    );
    Ok((
        [(SET_COOKIE, session_cookie(&recovered.session))],
        Json(Landing {
            redirect: Tab::Finance.href(),
        }),
    ))
}

/// Logs this device out and opens the Device link again, now ready to use.
async fn log_out(
    State(state): State<AppState>,
    Path(token): Path<String>,
    session: SessionCookie,
) -> Result<Response, AppError> {
    let device = path(&LinkToken::from_client(token));
    sign_in::end_session(&state, session, &device).await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::header::{CACHE_CONTROL, COOKIE, LOCATION, SET_COOKIE};
    use axum::http::{Method, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::auth::{LINK_LIFETIME, SessionToken};
    use crate::domain::members::{DisplayName, Member};
    use crate::web::test_support::{
        Authenticator, PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair, request,
    };

    const ONE_SECOND: Duration = Duration::from_secs(1);

    /// The Member signed in with `cookie`.
    async fn member(app: &TestApp, cookie: &str) -> Member {
        let token = SessionToken::from_client(cookie.split_once('=').unwrap().1.to_owned());
        app.state
            .auth
            .session(&token)
            .await
            .unwrap()
            .expect("the cookie signs someone in")
            .member
    }

    /// Josef the Admin, and Ondra, who joined with the passkey on `ondras_phone`.
    struct Crew {
        admin: String,
        ondra: Member,
    }

    async fn crew(app: &TestApp, ondras_phone: &mut Authenticator) -> Crew {
        let admin = app.sign_in_new_admin("Josef").await;
        let token = app
            .state
            .auth
            .create_test_invite(&DisplayName::parse("Ondra").unwrap())
            .await;
        let response = app
            .join(&crate::web::invite::path(&token), ondras_phone)
            .await;
        assert_eq!(response.status(), StatusCode::OK, "joining failed");
        let ondra = member(app, &cookie_pair(&response)).await;
        Crew { admin, ondra }
    }

    /// The path of a fresh Device link for Ondra, made by the Admin.
    async fn device_link(app: &TestApp, crew: &Crew) -> String {
        let admin = member(app, &crew.admin).await;
        let token = app
            .state
            .auth
            .create_device_link(crew.ondra.id, admin.id)
            .await
            .unwrap();
        super::path(&token)
    }

    async fn member_count(app: &TestApp) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM members")
            .fetch_one(&app.state.db)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn the_device_page_names_the_member_it_signs_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;

        let response = app.get(&link).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        let html = body_text(response).await;
        assert!(html.contains("<h1>Set up this device</h1>"));
        assert!(html.contains("You'll sign in as <strong>Ondra</strong>."));
        assert!(html.contains(&format!("data-passkey-start=\"{link}/start\"")));
        assert!(html.contains(&format!("data-passkey-finish=\"{link}/finish\"")));
    }

    #[sqlx::test]
    async fn a_device_link_adds_a_passkey_to_its_member_and_signs_them_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut old_phone = authenticator();
        let crew = crew(&app, &mut old_phone).await;
        let link = device_link(&app, &crew).await;
        let mut new_phone = authenticator();

        let response = app.join(&link, &mut new_phone).await;

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_pair(&response);
        assert_eq!(body_text(response).await, r#"{"redirect":"/"}"#);
        assert_eq!(member(&app, &cookie).await, crew.ondra);
        assert_eq!(member_count(&app).await, 2);
        let passkeys = app.state.auth.passkeys(crew.ondra.id).await.unwrap();
        assert_eq!(passkeys.len(), 2);
        assert_eq!(
            app.get_with("/settings", &cookie).await.status(),
            StatusCode::OK
        );
    }

    #[sqlx::test]
    async fn the_members_other_passkeys_keep_working(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut old_phone = authenticator();
        let crew = crew(&app, &mut old_phone).await;
        let link = device_link(&app, &crew).await;
        let mut new_phone = authenticator();
        app.join(&link, &mut new_phone).await;

        for phone in [&mut old_phone, &mut new_phone] {
            let response = app.sign_in(phone).await;

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(member(&app, &cookie_pair(&response)).await, crew.ondra);
        }
    }

    #[sqlx::test]
    async fn a_used_device_link_shows_the_link_error_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;
        app.join(&link, &mut authenticator()).await;

        let response = app.get(&link).await;

        assert_eq!(response.status(), StatusCode::GONE);
        let html = body_text(response).await;
        assert!(html.contains("This link no longer works"));
        assert!(html.contains("<strong>Josef</strong>, the Admin,"));
        assert_eq!(
            app.join(&link, &mut authenticator()).await.status(),
            StatusCode::GONE
        );
        assert_eq!(
            app.state.auth.passkeys(crew.ondra.id).await.unwrap().len(),
            2
        );
    }

    #[sqlx::test]
    async fn only_one_of_two_racing_ceremonies_can_use_a_device_link(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;
        let first = app.start_joining(&link).await.unwrap();
        let second = app.start_joining(&link).await.unwrap();

        let won = app
            .finish_joining(&link, first, &mut authenticator(), PUBLIC_ORIGIN)
            .await;
        let lost = app
            .finish_joining(&link, second, &mut authenticator(), PUBLIC_ORIGIN)
            .await;

        assert_eq!(won.status(), StatusCode::OK);
        assert_eq!(lost.status(), StatusCode::GONE);
        assert_eq!(
            app.state.auth.passkeys(crew.ondra.id).await.unwrap().len(),
            2
        );
    }

    #[sqlx::test]
    async fn an_expired_device_link_shows_the_link_error_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;
        app.clock
            .advance(LINK_LIFETIME.checked_sub(ONE_SECOND).unwrap());
        assert_eq!(app.get(&link).await.status(), StatusCode::OK);

        app.clock.advance(ONE_SECOND);

        let response = app.get(&link).await;
        assert_eq!(response.status(), StatusCode::GONE);
        assert!(
            body_text(response)
                .await
                .contains("This link no longer works")
        );
        assert_eq!(
            app.join(&link, &mut authenticator()).await.status(),
            StatusCode::GONE
        );
    }

    #[sqlx::test]
    async fn a_revoked_device_link_shows_the_link_error_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;
        let id = app.state.auth.open_links().await.unwrap()[0].id;

        app.state.auth.revoke_link(id).await.unwrap();

        let response = app.get(&link).await;
        assert_eq!(response.status(), StatusCode::GONE);
        assert!(
            body_text(response)
                .await
                .contains("This link no longer works")
        );
        assert_eq!(
            app.join(&link, &mut authenticator()).await.status(),
            StatusCode::GONE
        );
    }

    #[sqlx::test]
    async fn an_invite_link_does_not_open_as_a_device_link(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        let as_device = invite.replacen("/invite/", "/device/", 1);

        assert_eq!(app.get(&as_device).await.status(), StatusCode::GONE);
        assert_eq!(
            app.join(&as_device, &mut authenticator()).await.status(),
            StatusCode::GONE
        );
        assert_eq!(member_count(&app).await, 0);
    }

    #[sqlx::test]
    async fn a_device_link_opened_while_signed_in_offers_to_log_out(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;

        let response = app.get_with(&link, &crew.admin).await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.contains("signed in as <strong>Josef</strong>"));
        assert!(html.contains("This link sets up a new device for a Member of the Crew."));
        assert!(html.contains(&format!("<form method=\"post\" action=\"{link}/log-out\">")));
        assert!(!html.contains("data-passkey"));
        assert!(!html.contains("Ondra"));
    }

    #[sqlx::test]
    async fn a_device_link_attaches_nothing_while_signed_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;

        let mut start = request(Method::POST, &format!("{link}/start"), Some(PUBLIC_ORIGIN));
        start
            .headers_mut()
            .insert(COOKIE, crew.admin.parse().unwrap());
        let response = app.send(start).await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(body_text(response).await.contains("Log out first"));
        assert_eq!(app.get(&link).await.status(), StatusCode::OK);
        assert_eq!(
            app.state.auth.passkeys(crew.ondra.id).await.unwrap().len(),
            1
        );
    }

    #[sqlx::test]
    async fn logging_out_from_a_device_link_reopens_it_ready_to_use(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;

        let response = app.post_with(&format!("{link}/log-out"), &crew.admin).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], link.as_str());
        assert!(response.headers().contains_key(SET_COOKIE));
        let html = body_text(app.get(&link).await).await;
        assert!(html.contains("<strong>Ondra</strong>"));
        assert!(html.contains("data-passkey=\"register\""));
    }

    #[sqlx::test]
    async fn the_ceremony_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let crew = crew(&app, &mut authenticator()).await;
        let link = device_link(&app, &crew).await;

        let response = app
            .send(request(
                Method::POST,
                &format!("{link}/start"),
                Some("https://evil.example"),
            ))
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
