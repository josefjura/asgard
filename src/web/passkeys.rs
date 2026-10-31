//! A Member's passkeys in settings: adding another while signed in, renaming and deleting.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Form, Json, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::{Member, SessionCookie, removed_session_cookie};
use super::error::passkey_name_message;
use super::htmx::{Htmx, fragment_or_page};
use super::registration::{Challenge, DefaultPasskeyName, NewPasskey};
use super::settings;
use super::sign_in::Landing;
use super::{AppError, AppState};
use crate::domain::auth::{PasskeyId, PasskeyName};
use crate::domain::members;
use crate::views;
use crate::views::nav::SETTINGS_HREF;
use crate::views::passkey::CeremonyUrls;
use crate::views::passkeys::Outcome;

/// Where the passkey list's forms post, each under its passkey's id.
const BASE: &str = "/settings/passkeys";
const START_PATH: &str = "/settings/passkeys/start";
const FINISH_PATH: &str = "/settings/passkeys/finish";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(START_PATH, post(start))
        .route(FINISH_PATH, post(finish))
        .route("/settings/passkeys/{id}/name", post(rename))
        .route("/settings/passkeys/{id}/delete", post(delete))
}

/// Where the settings page's script starts and finishes adding a passkey.
pub fn ceremony_urls() -> CeremonyUrls {
    CeremonyUrls {
        start: START_PATH.to_owned(),
        finish: FINISH_PATH.to_owned(),
    }
}

/// The Member's passkey list, showing `outcome`.
///
/// # Errors
///
/// Fails if the database does.
pub async fn list(
    state: &AppState,
    member: &members::Member,
    outcome: Option<Outcome<'_>>,
) -> Result<Markup, AppError> {
    let passkeys = state.auth.passkeys(member.id).await?;
    Ok(views::passkeys::list(BASE, &passkeys, outcome))
}

async fn start(
    State(state): State<AppState>,
    Member(member): Member,
) -> Result<Json<Challenge>, AppError> {
    let (ceremony, options) = state.auth.start_adding_passkey(member.id).await?;
    Ok(Json(Challenge { ceremony, options }))
}

async fn finish(
    State(state): State<AppState>,
    Member(member): Member,
    DefaultPasskeyName(name): DefaultPasskeyName,
    Json(body): Json<NewPasskey>,
) -> Result<Json<Landing>, AppError> {
    state
        .auth
        .finish_adding_passkey(member.id, &body.ceremony, &body.credential, &name)
        .await?;
    tracing::info!(member = member.id.get(), "a Member added a passkey");
    Ok(Json(Landing {
        redirect: SETTINGS_HREF,
    }))
}

#[derive(Deserialize)]
struct RenameForm {
    name: String,
}

async fn rename(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Form(input): Form<RenameForm>,
) -> Result<Response, AppError> {
    let id = PasskeyId::from_client(id);
    match PasskeyName::parse(&input.name) {
        Ok(name) => {
            state.auth.rename_passkey(member.id, id, &name).await?;
            answer(&state, &member, StatusCode::OK, Outcome::Renamed(id), htmx).await
        }
        Err(err) => {
            let message = passkey_name_message(&err);
            let refused = Outcome::RenameRefused {
                id,
                value: &input.name,
                message: &message,
            };
            answer(
                &state,
                &member,
                StatusCode::UNPROCESSABLE_ENTITY,
                refused,
                htmx,
            )
            .await
        }
    }
}

async fn delete(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    SessionCookie(token): SessionCookie,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let name = state
        .auth
        .delete_passkey(member.id, PasskeyId::from_client(id))
        .await?;
    tracing::info!(member = member.id.get(), "a Member deleted a passkey");
    // The passkey this device signed in with takes this device's session with it.
    if let Some(token) = token
        && state.auth.session(&token).await?.is_none()
    {
        let signed_out = AppError::SignInRequired { htmx };
        return Ok(([(SET_COOKIE, removed_session_cookie())], signed_out).into_response());
    }
    answer(
        &state,
        &member,
        StatusCode::OK,
        Outcome::Deleted(&name),
        htmx,
    )
    .await
}

/// The passkey list showing `outcome`: alone for htmx, inside the settings page for a plain
/// form post.
async fn answer(
    state: &AppState,
    member: &members::Member,
    status: StatusCode,
    outcome: Outcome<'_>,
    htmx: bool,
) -> Result<Response, AppError> {
    let list = list(state, member, Some(outcome)).await?;
    Ok(fragment_or_page(htmx, status, list, |list| {
        settings::page(member, &settings::name_form(member), list)
    }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::{COOKIE, LOCATION, SET_COOKIE, USER_AGENT};
    use axum::http::{Method, Response, StatusCode};
    use sqlx::SqlitePool;
    use url::form_urlencoded;

    use crate::domain::auth::{LinkKind, PasskeyId, PasskeySummary, SessionToken};
    use crate::domain::members::{DisplayName, MemberId};
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::links;
    use crate::web::test_support::{
        Authenticator, PUBLIC_ORIGIN, Started, TestApp, authenticator, body_text, cookie_pair,
        json_request, request,
    };

    const IPHONE_SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1";

    /// A Member signed in on the device that holds their first passkey.
    struct Device {
        cookie: String,
        authenticator: Authenticator,
    }

    async fn josef(app: &TestApp) -> Device {
        let mut authenticator = authenticator();
        let cookie = app.join_as_admin("Josef", &mut authenticator).await;
        Device {
            cookie,
            authenticator,
        }
    }

    /// Ondra, a Member who joined beside the Admin, signed in on the device they joined from.
    async fn ondra(app: &TestApp) -> Device {
        let token = app
            .state
            .auth
            .create_test_invite(&DisplayName::parse("Ondra").unwrap())
            .await;
        let mut authenticator = authenticator();
        let response = app
            .join(&links::path(LinkKind::Invite, &token), &mut authenticator)
            .await;
        assert_eq!(response.status(), StatusCode::OK, "joining failed");
        Device {
            cookie: cookie_pair(&response),
            authenticator,
        }
    }

    async fn member_id(app: &TestApp, cookie: &str) -> MemberId {
        let token = SessionToken::from_client(cookie.split_once('=').unwrap().1.to_owned());
        app.state
            .auth
            .session(&token)
            .await
            .unwrap()
            .expect("the session is live")
            .member
            .id
    }

    async fn passkeys(app: &TestApp, cookie: &str) -> Vec<PasskeySummary> {
        app.state
            .auth
            .passkeys(member_id(app, cookie).await)
            .await
            .unwrap()
    }

    async fn start_adding(app: &TestApp, cookie: &str) -> Started {
        let response = app
            .post_json_with("/settings/passkeys/start", cookie, &serde_json::json!({}))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "starting failed");
        let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        Started {
            options: serde_json::from_value(body["options"].clone()).unwrap(),
            ceremony: body["ceremony"].clone(),
        }
    }

    /// Creates the passkey on `authenticator` and sends it from a browser that holds `cookie`
    /// and calls itself `user_agent`.
    async fn finish_adding(
        app: &TestApp,
        cookie: &str,
        started: Started,
        authenticator: &mut Authenticator,
        user_agent: &str,
    ) -> Response<Body> {
        let credential = authenticator.register(PUBLIC_ORIGIN, started.options);
        let mut finish = json_request(
            "/settings/passkeys/finish",
            &serde_json::json!({ "ceremony": started.ceremony, "credential": credential }),
        );
        let headers = finish.headers_mut();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(USER_AGENT, user_agent.parse().unwrap());
        app.send(finish).await
    }

    /// Adds a passkey on a new device while signed in with `cookie`, and returns that device.
    async fn add_device(app: &TestApp, cookie: &str, user_agent: &str) -> Authenticator {
        let mut device = authenticator();
        let started = start_adding(app, cookie).await;
        let response = finish_adding(app, cookie, started, &mut device, user_agent).await;
        assert_eq!(response.status(), StatusCode::OK, "adding failed");
        device
    }

    /// A form post from a browser that holds `cookie`, from our origin unless `origin` says.
    async fn post_form(
        app: &TestApp,
        uri: &str,
        cookie: &str,
        fields: &[(&str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        let mut post = request(Method::POST, uri, Some(PUBLIC_ORIGIN));
        let headers = post.headers_mut();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        if htmx {
            headers.insert(HX_REQUEST, "true".parse().unwrap());
        }
        *post.body_mut() = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish()
            .into();
        app.send(post).await
    }

    async fn rename(app: &TestApp, cookie: &str, id: PasskeyId, name: &str) -> Response<Body> {
        let uri = format!("/settings/passkeys/{}/name", id.get());
        post_form(app, &uri, cookie, &[("name", name)], true).await
    }

    async fn delete(app: &TestApp, cookie: &str, id: PasskeyId) -> Response<Body> {
        let uri = format!("/settings/passkeys/{}/delete", id.get());
        post_form(app, &uri, cookie, &[], true).await
    }

    async fn settings(app: &TestApp, cookie: &str) -> String {
        let response = app.get_with("/settings", cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    #[sqlx::test]
    async fn settings_lists_the_members_passkeys_with_name_and_date(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;

        let html = settings(&app, &josef.cookie).await;

        assert!(html.contains("Passkey added 1 January 2026"));
        assert!(html.contains("value=\"Passkey\""));
        assert!(html.contains("Your only passkey can't be deleted."));
        assert!(html.contains("data-passkey-start=\"/settings/passkeys/start\""));
    }

    #[sqlx::test]
    async fn adding_a_passkey_names_it_after_the_browser(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let mut device = authenticator();
        let started = start_adding(&app, &josef.cookie).await;

        let response =
            finish_adding(&app, &josef.cookie, started, &mut device, IPHONE_SAFARI).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_text(response).await, r#"{"redirect":"/settings"}"#);
        let html = settings(&app, &josef.cookie).await;
        assert!(html.contains("value=\"iPhone Safari\""));
        assert_eq!(passkeys(&app, &josef.cookie).await.len(), 2);
    }

    #[sqlx::test]
    async fn the_added_passkey_signs_its_member_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let mut device = add_device(&app, &josef.cookie, IPHONE_SAFARI).await;

        let response = app.sign_in(&mut device).await;

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_pair(&response);
        assert_eq!(
            member_id(&app, &cookie).await,
            member_id(&app, &josef.cookie).await
        );
    }

    #[sqlx::test]
    async fn adding_asks_the_browser_not_to_repeat_a_passkey(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;

        let started = start_adding(&app, &josef.cookie).await;

        let public_key = started.options.public_key;
        assert_eq!(public_key.exclude_credentials.unwrap().len(), 1);
        assert!(
            public_key
                .authenticator_selection
                .unwrap()
                .require_resident_key
        );
    }

    #[sqlx::test]
    async fn adding_a_passkey_needs_a_signed_in_member(db: SqlitePool) {
        let response = TestApp::new(db)
            .post_json("/settings/passkeys/start", &serde_json::json!({}))
            .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
    }

    #[sqlx::test]
    async fn a_member_cannot_finish_adding_a_passkey_someone_else_started(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let ondra = ondra(&app).await;
        let started = start_adding(&app, &josef.cookie).await;

        let response = finish_adding(
            &app,
            &ondra.cookie,
            started,
            &mut authenticator(),
            IPHONE_SAFARI,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(passkeys(&app, &josef.cookie).await.len(), 1);
        assert_eq!(passkeys(&app, &ondra.cookie).await.len(), 1);
    }

    #[sqlx::test]
    async fn renaming_a_passkey_answers_htmx_with_the_list_showing_the_new_name(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let id = passkeys(&app, &josef.cookie).await[0].id;

        let response = rename(&app, &josef.cookie, id, " Old iPhone ").await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.starts_with("<div class=\"passkeys\" id=\"passkeys\">"));
        assert!(html.contains("value=\"Old iPhone\""));
        assert!(html.contains("Saved."));
        assert!(
            settings(&app, &josef.cookie)
                .await
                .contains("value=\"Old iPhone\"")
        );
    }

    #[sqlx::test]
    async fn a_plain_rename_post_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let id = passkeys(&app, &josef.cookie).await[0].id;
        let uri = format!("/settings/passkeys/{}/name", id.get());

        let response = post_form(&app, &uri, &josef.cookie, &[("name", "Pixel")], false).await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Settings</h1>"));
        assert!(html.contains("value=\"Josef\""));
        assert!(html.contains("value=\"Pixel\""));
    }

    #[sqlx::test]
    async fn a_blank_passkey_name_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let id = passkeys(&app, &josef.cookie).await[0].id;

        let response = rename(&app, &josef.cookie, id, "  ").await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.contains("Enter a name for the passkey."));
        assert!(html.contains("aria-invalid=\"true\""));
        assert_eq!(
            passkeys(&app, &josef.cookie).await[0].name.as_str(),
            "Passkey"
        );
    }

    #[sqlx::test]
    async fn deleting_a_passkey_stops_it_signing_in_and_ends_its_sessions(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let mut lost = add_device(&app, &josef.cookie, IPHONE_SAFARI).await;
        let lost_cookie = cookie_pair(&app.sign_in(&mut lost).await);
        let lost_id = passkeys(&app, &josef.cookie).await[1].id;

        let response = delete(&app, &josef.cookie, lost_id).await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.contains("Deleted <strong>iPhone Safari</strong>."));
        let next = app.get_with("/settings", &lost_cookie).await;
        assert_eq!(next.status(), StatusCode::SEE_OTHER);
        assert_eq!(next.headers()[LOCATION], "/sign-in");
        assert_eq!(
            app.sign_in(&mut lost).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(passkeys(&app, &josef.cookie).await.len(), 1);
    }

    #[sqlx::test]
    async fn deleting_one_passkey_leaves_the_others_signing_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut josef = josef(&app).await;
        add_device(&app, &josef.cookie, IPHONE_SAFARI).await;
        let added = passkeys(&app, &josef.cookie).await[1].id;

        delete(&app, &josef.cookie, added).await;

        assert_eq!(
            app.get_with("/settings", &josef.cookie).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            app.sign_in(&mut josef.authenticator).await.status(),
            StatusCode::OK
        );
    }

    #[sqlx::test]
    async fn deleting_the_passkey_this_device_signed_in_with_signs_it_out(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        add_device(&app, &josef.cookie, IPHONE_SAFARI).await;
        let own = passkeys(&app, &josef.cookie).await[0].id;

        let response = delete(&app, &josef.cookie, own).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[HX_REDIRECT], "/sign-in");
        assert!(
            response.headers()[SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }

    #[sqlx::test]
    async fn the_last_passkey_cannot_be_deleted_even_by_a_hand_made_request(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let only = passkeys(&app, &josef.cookie).await[0].id;

        let response = delete(&app, &josef.cookie, only).await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(
            body_text(response)
                .await
                .contains("Your only passkey can't be deleted.")
        );
        assert_eq!(passkeys(&app, &josef.cookie).await.len(), 1);
        assert_eq!(
            app.get_with("/settings", &josef.cookie).await.status(),
            StatusCode::OK
        );
    }

    #[sqlx::test]
    async fn a_member_cannot_rename_or_delete_another_members_passkey(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        let ondra = ondra(&app).await;
        add_device(&app, &ondra.cookie, IPHONE_SAFARI).await;
        let ondras = passkeys(&app, &ondra.cookie).await[0].id;

        let renamed = rename(&app, &josef.cookie, ondras, "Mine now").await;
        let deleted = delete(&app, &josef.cookie, ondras).await;

        assert_eq!(renamed.status(), StatusCode::NOT_FOUND);
        assert_eq!(deleted.status(), StatusCode::NOT_FOUND);
        let left = passkeys(&app, &ondra.cookie).await;
        assert_eq!(left.len(), 2);
        assert_eq!(left[0].name.as_str(), "Passkey");
    }

    #[sqlx::test]
    async fn deleting_a_passkey_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = josef(&app).await;
        add_device(&app, &josef.cookie, IPHONE_SAFARI).await;
        let added = passkeys(&app, &josef.cookie).await[1].id;

        let mut post = request(
            Method::POST,
            &format!("/settings/passkeys/{}/delete", added.get()),
            Some("https://evil.example"),
        );
        post.headers_mut()
            .insert(COOKIE, josef.cookie.parse().unwrap());
        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(passkeys(&app, &josef.cookie).await.len(), 2);
    }

    #[sqlx::test]
    async fn joining_names_the_first_passkey_after_the_browser(db: SqlitePool) {
        let app = TestApp::new(db);
        let invite = app.admin_invite("Josef").await;
        let started = app
            .start_joining(&invite)
            .await
            .unwrap_or_else(|_| panic!());
        let credential = authenticator().register(PUBLIC_ORIGIN, started.options);
        let mut finish = json_request(
            &format!("{invite}/finish"),
            &serde_json::json!({ "ceremony": started.ceremony, "credential": credential }),
        );
        finish
            .headers_mut()
            .insert(USER_AGENT, IPHONE_SAFARI.parse().unwrap());

        let response = app.send(finish).await;

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_pair(&response);
        assert_eq!(
            passkeys(&app, &cookie).await[0].name.as_str(),
            "iPhone Safari"
        );
    }
}
