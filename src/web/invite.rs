//! Joining the Crew through an Invite link: the page, then the passkey ceremony.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{CreationChallengeResponse, RegisterPublicKeyCredential};

use super::access::session_cookie;
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::auth::{AuthError, CeremonyId, LinkToken};
use crate::domain::members::CREW_NAME;
use crate::views;
use crate::views::nav::Tab;

/// Where an Invite link points.
#[must_use]
pub fn path(token: &LinkToken) -> String {
    format!("/invite/{}", token.as_str())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/invite/{token}", get(show))
        .route("/invite/{token}/start", post(start))
        .route("/invite/{token}/finish", post(finish))
}

async fn show(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, AppError> {
    let token = LinkToken::from_client(token);
    match state.auth.invite(&token).await {
        Ok(invite) => Ok(fresh(views::invite::page(
            CREW_NAME,
            &invite.display_name,
            &path(&token),
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

#[derive(Serialize)]
struct Started {
    ceremony: String,
    options: CreationChallengeResponse,
}

async fn start(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Started>, AppError> {
    let (ceremony, options) = state
        .auth
        .start_joining(&LinkToken::from_client(token))
        .await?;
    Ok(Json(Started {
        ceremony: ceremony.as_str().to_owned(),
        options,
    }))
}

#[derive(Deserialize)]
struct Finish {
    ceremony: String,
    credential: RegisterPublicKeyCredential,
}

#[derive(Serialize)]
struct Finished {
    redirect: &'static str,
}

async fn finish(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<Finish>,
) -> Result<impl IntoResponse, AppError> {
    let joined = state
        .auth
        .finish_joining(
            &LinkToken::from_client(token),
            &CeremonyId::from_client(body.ceremony),
            &body.credential,
        )
        .await?;
    tracing::info!(member = joined.member.get(), "a new Member joined the Crew");
    Ok((
        [(SET_COOKIE, session_cookie(&joined.session))],
        Json(Finished {
            redirect: Tab::Finance.href(),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, SET_COOKIE};
    use axum::http::{Method, Response, StatusCode};
    use cookie::{Cookie, SameSite};
    use jiff::civil::date;
    use serde_json::json;
    use sqlx::SqlitePool;
    use url::Url;
    use webauthn_rs::prelude::CreationChallengeResponse;

    use crate::domain::auth::{LINK_LIFETIME, SessionToken};
    use crate::domain::members::DisplayName;
    use crate::web::test_support::{
        PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair, request,
    };

    const ONE_SECOND: Duration = Duration::from_secs(1);

    /// The ceremony id and options from a start response.
    async fn started(response: Response<Body>) -> (serde_json::Value, CreationChallengeResponse) {
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        let options = serde_json::from_value(body["options"].clone()).unwrap();
        (body["ceremony"].clone(), options)
    }

    async fn start(app: &TestApp, invite: &str) -> (serde_json::Value, CreationChallengeResponse) {
        started(app.post_json(&format!("{invite}/start"), &json!({})).await).await
    }

    async fn finish(
        app: &TestApp,
        invite: &str,
        ceremony: &serde_json::Value,
        options: CreationChallengeResponse,
        origin: &str,
    ) -> Response<Body> {
        let credential = authenticator()
            .do_registration(Url::parse(origin).unwrap(), options)
            .unwrap();
        app.post_json(
            &format!("{invite}/finish"),
            &json!({ "ceremony": ceremony, "credential": credential }),
        )
        .await
    }

    #[sqlx::test]
    async fn the_invite_page_shows_the_crew_and_the_display_name(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;

        let response = app.get(&invite).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        let html = body_text(response).await;
        assert!(html.contains("<h1>Join Asgard</h1>"));
        assert!(html.contains("<strong>Josef</strong>"));
        assert!(html.contains(&format!("data-passkey-join=\"{invite}\"")));
    }

    #[sqlx::test]
    async fn joining_signs_the_new_admin_in_and_lands_on_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;

        let response = app.join(&invite, &mut authenticator()).await;

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_pair(&response);
        assert_eq!(body_text(response).await, r#"{"redirect":"/"}"#);
        let finance = app.get_with("/", &cookie).await;
        assert_eq!(finance.status(), StatusCode::OK);
        assert!(body_text(finance).await.contains("<h1>Finance</h1>"));

        let token = SessionToken::from_client(cookie.split_once('=').unwrap().1.to_owned());
        let member = app
            .state
            .auth
            .session_member(&token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(member.display_name, DisplayName::parse("Josef").unwrap());
        assert!(member.is_admin);
        assert_eq!(member.joined_on, date(2026, 1, 1));
    }

    #[sqlx::test]
    async fn the_session_cookie_is_http_only_secure_and_same_site_lax(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;

        let response = app.join(&invite, &mut authenticator()).await;

        let set_cookie = response.headers()[SET_COOKIE].to_str().unwrap();
        let cookie = Cookie::parse(set_cookie).unwrap();
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    }

    #[sqlx::test]
    async fn a_used_link_shows_the_link_error_page_naming_the_admin(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        app.join(&invite, &mut authenticator()).await;

        let response = app.get(&invite).await;

        assert_eq!(response.status(), StatusCode::GONE);
        let html = body_text(response).await;
        assert!(html.contains("This link no longer works"));
        assert!(html.contains("<strong>Josef</strong>, the Admin,"));
    }

    #[sqlx::test]
    async fn a_used_link_cannot_join_again(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        app.join(&invite, &mut authenticator()).await;

        let response = app.join(&invite, &mut authenticator()).await;

        assert_eq!(response.status(), StatusCode::GONE);
    }

    #[sqlx::test]
    async fn only_one_of_two_racing_ceremonies_can_use_a_link(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        let (first, first_options) = start(&app, &invite).await;
        let (second, second_options) = start(&app, &invite).await;

        let won = finish(&app, &invite, &first, first_options, PUBLIC_ORIGIN).await;
        let lost = finish(&app, &invite, &second, second_options, PUBLIC_ORIGIN).await;

        assert_eq!(won.status(), StatusCode::OK);
        assert_eq!(lost.status(), StatusCode::GONE);
    }

    #[sqlx::test]
    async fn a_link_works_until_just_before_24_hours(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;

        app.clock
            .advance(LINK_LIFETIME.checked_sub(ONE_SECOND).unwrap());

        assert_eq!(app.get(&invite).await.status(), StatusCode::OK);
        let response = app.join(&invite, &mut authenticator()).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test]
    async fn a_link_just_after_24_hours_shows_the_link_error_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;

        app.clock.advance(LINK_LIFETIME + ONE_SECOND);

        let response = app.get(&invite).await;
        assert_eq!(response.status(), StatusCode::GONE);
        assert!(
            body_text(response)
                .await
                .contains("This link no longer works")
        );
        let response = app.join(&invite, &mut authenticator()).await;
        assert_eq!(response.status(), StatusCode::GONE);
    }

    #[sqlx::test]
    async fn a_link_that_expires_mid_ceremony_cannot_be_used(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        app.clock
            .advance(LINK_LIFETIME.checked_sub(ONE_SECOND).unwrap());
        let (ceremony, options) = start(&app, &invite).await;

        app.clock.advance(ONE_SECOND);
        let response = finish(&app, &invite, &ceremony, options, PUBLIC_ORIGIN).await;

        assert_eq!(response.status(), StatusCode::GONE);
    }

    #[sqlx::test]
    async fn an_unknown_link_shows_the_link_error_page_before_there_is_an_admin(db: SqlitePool) {
        let response = TestApp::new(db).get("/invite/made-up").await;

        assert_eq!(response.status(), StatusCode::GONE);
        assert!(
            body_text(response)
                .await
                .contains("Ask the Admin to send you a new one.")
        );
    }

    #[sqlx::test]
    async fn a_passkey_made_for_another_site_is_rejected(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        let (ceremony, mut options) = start(&app, &invite).await;
        options.public_key.rp.id = "evil.example".to_owned();

        let response = finish(&app, &invite, &ceremony, options, "https://evil.example").await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(body_text(response).await.contains("couldn't be verified"));
        assert_eq!(app.get(&invite).await.status(), StatusCode::OK);
    }

    #[sqlx::test]
    async fn a_ceremony_started_on_another_link_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        let other = app.admin_invite("Ondra").await;
        let (ceremony, options) = start(&app, &other).await;

        let response = finish(&app, &invite, &ceremony, options, PUBLIC_ORIGIN).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(body_text(response).await.contains("took too long"));
    }

    #[sqlx::test]
    async fn the_ceremony_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;

        let response = app
            .send(request(
                Method::POST,
                &format!("{invite}/start"),
                Some("https://evil.example"),
            ))
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
