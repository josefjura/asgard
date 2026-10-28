//! Signing in with a passkey, and logging out of this device.

use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{PublicKeyCredential, RequestChallengeResponse};

use super::access::{Member, SessionCookie, removed_session_cookie, session_cookie};
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::auth::CeremonyId;
use crate::views;
use crate::views::nav::Tab;
use crate::views::passkey::CeremonyUrls;

/// Where a request without a valid session is sent.
pub const PATH: &str = "/sign-in";

const START_PATH: &str = "/sign-in/start";
const FINISH_PATH: &str = "/sign-in/finish";

/// Where a Member logs out of this device.
pub const SIGN_OUT_PATH: &str = "/sign-out";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(PATH, get(show))
        .route(START_PATH, post(start))
        .route(FINISH_PATH, post(finish))
        .route(SIGN_OUT_PATH, post(sign_out))
}

/// The sign-in page, or Finance for a Member who is already signed in.
async fn show(member: Option<Member>) -> Response {
    if member.is_some() {
        return Redirect::to(Tab::Finance.href()).into_response();
    }
    fresh(views::sign_in::page(&CeremonyUrls {
        start: START_PATH.to_owned(),
        finish: FINISH_PATH.to_owned(),
    }))
    .into_response()
}

/// What the browser needs to ask its authenticator for a passkey.
#[derive(Serialize)]
struct Challenge {
    ceremony: CeremonyId,
    options: RequestChallengeResponse,
}

async fn start(State(state): State<AppState>) -> Result<Json<Challenge>, AppError> {
    let (ceremony, options) = state.auth.start_signing_in()?;
    Ok(Json(Challenge { ceremony, options }))
}

/// The passkey's answer to a [`Challenge`].
#[derive(Deserialize)]
struct Assertion {
    ceremony: CeremonyId,
    credential: PublicKeyCredential,
}

/// Where the browser goes once it is signed in.
#[derive(Serialize)]
pub struct Landing {
    pub redirect: &'static str,
}

async fn finish(
    State(state): State<AppState>,
    Json(body): Json<Assertion>,
) -> Result<impl IntoResponse, AppError> {
    let session = state
        .auth
        .finish_signing_in(&body.ceremony, &body.credential)
        .await?;
    Ok((
        [(SET_COOKIE, session_cookie(&session))],
        Json(Landing {
            redirect: Tab::Finance.href(),
        }),
    ))
}

async fn sign_out(
    State(state): State<AppState>,
    session: SessionCookie,
) -> Result<Response, AppError> {
    end_session(&state, session, PATH).await
}

/// Ends this device's session, forgets its cookie and sends the browser to `then`.
///
/// # Errors
///
/// Fails if the database does.
pub async fn end_session(
    state: &AppState,
    SessionCookie(token): SessionCookie,
    then: &str,
) -> Result<Response, AppError> {
    if let Some(token) = token {
        state.auth.end_session(&token).await?;
    }
    Ok(([(SET_COOKIE, removed_session_cookie())], Redirect::to(then)).into_response())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::header::{CACHE_CONTROL, COOKIE, LOCATION, SET_COOKIE};
    use axum::http::{Method, StatusCode};
    use cookie::Cookie;
    use sqlx::SqlitePool;

    use crate::domain::auth::{SESSION_LIFETIME, SESSION_RENEWAL_INTERVAL};
    use crate::web::test_support::{
        PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair, request,
    };

    const ONE_SECOND: Duration = Duration::from_secs(1);
    const DAY: Duration = Duration::from_hours(24);

    #[sqlx::test]
    async fn the_sign_in_page_has_one_passkey_button_and_no_username(db: SqlitePool) {
        let response = TestApp::new(db).get("/sign-in").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        let html = body_text(response).await;
        assert!(html.contains("Sign in with passkey"));
        assert!(html.contains("data-passkey=\"sign-in\""));
        assert!(html.contains("data-passkey-start=\"/sign-in/start\""));
        assert!(html.contains("data-passkey-finish=\"/sign-in/finish\""));
        assert!(!html.contains("<input"));
    }

    #[sqlx::test]
    async fn a_signed_in_member_visiting_sign_in_goes_to_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = app.get_with("/sign-in", &cookie).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/");
    }

    #[sqlx::test]
    async fn a_member_signs_in_with_their_passkey_and_reaches_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut phone = authenticator();
        app.join_as_admin("Josef", &mut phone).await;

        let response = app.sign_in(&mut phone).await;

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_pair(&response);
        assert_eq!(body_text(response).await, r#"{"redirect":"/"}"#);
        let finance = app.get_with("/", &cookie).await;
        assert_eq!(finance.status(), StatusCode::OK);
        assert!(body_text(finance).await.contains("<h1>Finance</h1>"));
    }

    #[sqlx::test]
    async fn each_sign_in_is_its_own_session(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut phone = authenticator();
        let joined = app.join_as_admin("Josef", &mut phone).await;

        let first = cookie_pair(&app.sign_in(&mut phone).await);
        let second = cookie_pair(&app.sign_in(&mut phone).await);

        assert_ne!(first, second);
        app.post_with("/sign-out", &first).await;
        assert_eq!(
            app.get_with("/", &first).await.status(),
            StatusCode::SEE_OTHER
        );
        assert_eq!(app.get_with("/", &second).await.status(), StatusCode::OK);
        assert_eq!(app.get_with("/", &joined).await.status(), StatusCode::OK);
    }

    #[sqlx::test]
    async fn a_passkey_asgard_never_stored_cannot_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        let mut phone = authenticator();
        // The passkey is created, but the browser never sends it back to finish joining.
        let started = app.start_joining(&invite).await.unwrap();
        phone.register(PUBLIC_ORIGIN, started.options);

        let response = app.sign_in(&mut phone).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(SET_COOKIE));
        assert!(
            body_text(response)
                .await
                .contains("Asgard doesn't know this passkey")
        );
    }

    #[sqlx::test]
    async fn a_sign_in_ceremony_works_once(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut phone = authenticator();
        app.join_as_admin("Josef", &mut phone).await;
        let start = app
            .post_json("/sign-in/start", &serde_json::json!({}))
            .await;
        let body: serde_json::Value = serde_json::from_str(&body_text(start).await).unwrap();
        let options = serde_json::from_value(body["options"].clone()).unwrap();
        let credential = phone.authenticate(PUBLIC_ORIGIN, options);
        let finish = serde_json::json!({ "ceremony": body["ceremony"], "credential": credential });

        let first = app.post_json("/sign-in/finish", &finish).await;
        let replayed = app.post_json("/sign-in/finish", &finish).await;

        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(replayed.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test]
    async fn logging_out_ends_the_session_and_forgets_the_cookie(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = app.post_with("/sign-out", &cookie).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
        let removal = Cookie::parse(response.headers()[SET_COOKIE].to_str().unwrap()).unwrap();
        assert_eq!(removal.value(), "");
        assert_eq!(removal.max_age(), Some(cookie::time::Duration::ZERO));
        let next = app.get_with("/", &cookie).await;
        assert_eq!(next.status(), StatusCode::SEE_OTHER);
        assert_eq!(next.headers()[LOCATION], "/sign-in");
    }

    #[sqlx::test]
    async fn logging_out_without_a_session_still_lands_on_sign_in(db: SqlitePool) {
        let response = TestApp::new(db)
            .post_with("/sign-out", "asgard_session=made-up")
            .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
    }

    #[sqlx::test]
    async fn logging_out_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let mut foreign = request(Method::POST, "/sign-out", Some("https://evil.example"));
        foreign
            .headers_mut()
            .insert(COOKIE, cookie.parse().unwrap());

        assert_eq!(app.send(foreign).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(app.get_with("/", &cookie).await.status(), StatusCode::OK);
    }

    #[sqlx::test]
    async fn a_session_used_regularly_outlives_90_days_from_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        for _ in 0..6 {
            app.clock.advance(30 * DAY);
            assert_eq!(app.get_with("/", &cookie).await.status(), StatusCode::OK);
        }
    }

    #[sqlx::test]
    async fn a_session_unused_for_just_over_90_days_is_rejected(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        app.clock.advance(30 * DAY);
        assert_eq!(app.get_with("/", &cookie).await.status(), StatusCode::OK);

        app.clock.advance(SESSION_LIFETIME + ONE_SECOND);

        let response = app.get_with("/", &cookie).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
    }

    #[sqlx::test]
    async fn a_session_unused_for_just_under_90_days_still_works(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        app.clock.advance(30 * DAY);
        assert_eq!(app.get_with("/", &cookie).await.status(), StatusCode::OK);

        app.clock
            .advance(SESSION_LIFETIME.checked_sub(ONE_SECOND).unwrap());

        assert_eq!(app.get_with("/", &cookie).await.status(), StatusCode::OK);
    }

    #[sqlx::test]
    async fn renewal_writes_at_most_once_per_interval(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        // Used often within the interval: nothing renews, so it still ends 90 days after sign-in.
        for _ in 0..3 {
            app.clock.advance(Duration::from_hours(6));
            let response = app.get_with("/", &cookie).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key(SET_COOKIE));
        }
        app.clock.advance(
            SESSION_LIFETIME
                .checked_sub(Duration::from_hours(18))
                .unwrap(),
        );

        assert_eq!(
            app.get_with("/", &cookie).await.status(),
            StatusCode::SEE_OTHER
        );
    }

    #[sqlx::test]
    async fn a_renewed_session_sends_the_browser_a_fresh_cookie(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        app.clock.advance(SESSION_RENEWAL_INTERVAL);
        let renewed = app.get_with("/", &cookie).await;
        let again = app.get_with("/", &cookie).await;

        assert_eq!(cookie_pair(&renewed), cookie);
        let fresh = Cookie::parse(renewed.headers()[SET_COOKIE].to_str().unwrap()).unwrap();
        assert_eq!(fresh.http_only(), Some(true));
        assert_eq!(fresh.secure(), Some(true));
        assert_eq!(fresh.max_age(), Some(cookie::time::Duration::days(90)));
        assert!(!again.headers().contains_key(SET_COOKIE));
    }
}
