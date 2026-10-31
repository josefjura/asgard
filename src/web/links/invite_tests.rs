//! Joining the Crew through an Invite link.

use std::time::Duration;

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE};
use axum::http::{Method, Response, StatusCode};
use cookie::{Cookie, SameSite};
use jiff::civil::date;
use sqlx::SqlitePool;
use webauthn_rs_proto::ResidentKeyRequirement;

use crate::domain::auth::{LINK_LIFETIME, LinkKind, SessionToken};
use crate::domain::members::DisplayName;
use crate::web::test_support::{
    PUBLIC_ORIGIN, Started, TestApp, authenticator, body_text, cookie_pair, request,
};

const ONE_SECOND: Duration = Duration::from_secs(1);

async fn start(app: &TestApp, invite: &str) -> Started {
    app.start_joining(invite)
        .await
        .unwrap_or_else(|response| panic!("starting failed: {}", response.status()))
}

async fn finish(app: &TestApp, invite: &str, started: Started) -> Response<Body> {
    app.finish_joining(invite, started, &mut authenticator(), PUBLIC_ORIGIN)
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
    assert!(html.contains(&format!("data-passkey-start=\"{invite}/start\"")));
    assert!(html.contains(&format!("data-passkey-finish=\"{invite}/finish\"")));
}

#[sqlx::test]
async fn joining_signs_the_new_admin_in_and_lands_on_the_welcome_page(db: SqlitePool) {
    let app = TestApp::new(db);
    let invite = app.admin_invite("Josef").await;

    let response = app.join(&invite, &mut authenticator()).await;

    assert_eq!(response.status(), StatusCode::OK);
    let cookie = cookie_pair(&response);
    assert_eq!(body_text(response).await, r#"{"redirect":"/welcome"}"#);
    let welcome = app.get_with("/welcome", &cookie).await;
    assert_eq!(welcome.status(), StatusCode::OK);
    assert!(
        body_text(welcome)
            .await
            .contains("<h1>Welcome to Asgard</h1>")
    );

    let token = SessionToken::from_client(cookie.split_once('=').unwrap().1.to_owned());
    let member = app
        .state
        .auth
        .session(&token)
        .await
        .unwrap()
        .unwrap()
        .member;
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
    let first = start(&app, &invite).await;
    let second = start(&app, &invite).await;

    let won = finish(&app, &invite, first).await;
    let lost = finish(&app, &invite, second).await;

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
    let started = start(&app, &invite).await;

    app.clock.advance(ONE_SECOND);
    let response = finish(&app, &invite, started).await;

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
    let mut started = start(&app, &invite).await;
    started.options.public_key.rp.id = "evil.example".to_owned();

    let response = app
        .finish_joining(
            &invite,
            started,
            &mut authenticator(),
            "https://evil.example",
        )
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(response).await.contains("couldn't be verified"));
    assert_eq!(app.get(&invite).await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn a_ceremony_started_on_another_link_is_refused(db: SqlitePool) {
    let app = TestApp::new(db);
    let invite = app.admin_invite("Josef").await;
    let other = app.admin_invite("Ondra").await;
    let started = start(&app, &other).await;

    let response = finish(&app, &invite, started).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(response).await.contains("took too long"));
}

#[sqlx::test]
async fn only_one_admin_link_can_make_an_admin(db: SqlitePool) {
    let app = TestApp::new(db);
    let first = app.admin_invite("Josef").await;
    let second = app.admin_invite("Ondra").await;
    let started = start(&app, &second).await;
    app.join(&first, &mut authenticator()).await;

    assert_eq!(
        finish(&app, &second, started).await.status(),
        StatusCode::GONE
    );
    assert_eq!(app.get(&second).await.status(), StatusCode::GONE);
}

#[sqlx::test]
async fn joining_asks_for_a_discoverable_passkey(db: SqlitePool) {
    let app = TestApp::new(db);
    let invite = app.admin_invite("Josef").await;

    let started = start(&app, &invite).await;

    let selection = started.options.public_key.authenticator_selection.unwrap();
    assert_eq!(
        selection.resident_key,
        Some(ResidentKeyRequirement::Required)
    );
    assert!(selection.require_resident_key);
}

/// An Invite link for Ondra, opened on a browser where the Admin is signed in.
async fn signed_in_with_an_invite(app: &TestApp) -> (String, String) {
    let cookie = app.sign_in_new_admin("Josef").await;
    let token = app
        .state
        .auth
        .create_test_invite(&DisplayName::parse("Ondra").unwrap())
        .await;
    (cookie, super::path(LinkKind::Invite, &token))
}

#[sqlx::test]
async fn an_invite_link_opened_while_signed_in_offers_to_log_out(db: SqlitePool) {
    let app = TestApp::new(db);
    let (cookie, invite) = signed_in_with_an_invite(&app).await;

    let response = app.get_with(&invite, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("signed in as <strong>Josef</strong>"));
    assert!(html.contains(&format!(
        "<form method=\"post\" action=\"{invite}/log-out\">"
    )));
    assert!(!html.contains("data-passkey"));
    assert!(!html.contains("Ondra"));
}

#[sqlx::test]
async fn an_invite_link_registers_nothing_while_signed_in(db: SqlitePool) {
    let app = TestApp::new(db);
    let (cookie, invite) = signed_in_with_an_invite(&app).await;

    let mut start = request(
        Method::POST,
        &format!("{invite}/start"),
        Some(PUBLIC_ORIGIN),
    );
    start.headers_mut().insert(COOKIE, cookie.parse().unwrap());
    let response = app.send(start).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(body_text(response).await.contains("Log out first"));
    assert_eq!(app.get(&invite).await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn signing_in_mid_ceremony_stops_the_link_attaching_a_passkey(db: SqlitePool) {
    let app = TestApp::new(db);
    let token = app
        .state
        .auth
        .create_test_invite(&DisplayName::parse("Ondra").unwrap())
        .await;
    let invite = super::path(LinkKind::Invite, &token);
    let started = start(&app, &invite).await;
    let cookie = app.sign_in_new_admin("Josef").await;

    let credential = authenticator().register(PUBLIC_ORIGIN, started.options);
    let mut finish = request(
        Method::POST,
        &format!("{invite}/finish"),
        Some(PUBLIC_ORIGIN),
    );
    finish.headers_mut().insert(COOKIE, cookie.parse().unwrap());
    finish
        .headers_mut()
        .insert(CONTENT_TYPE, "application/json".parse().unwrap());
    *finish.body_mut() = Body::from(
        serde_json::json!({ "ceremony": started.ceremony, "credential": credential }).to_string(),
    );
    let response = app.send(finish).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(app.get(&invite).await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn logging_out_from_an_invite_link_reopens_it_ready_to_join(db: SqlitePool) {
    let app = TestApp::new(db);
    let (cookie, invite) = signed_in_with_an_invite(&app).await;

    let response = app.post_with(&format!("{invite}/log-out"), &cookie).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[LOCATION], invite.as_str());
    assert!(response.headers().contains_key(SET_COOKIE));
    assert_eq!(
        app.get_with("/", &cookie).await.status(),
        StatusCode::SEE_OTHER
    );
    let html = body_text(app.get(&invite).await).await;
    assert!(html.contains("<strong>Ondra</strong>"));
    assert!(html.contains("data-passkey=\"register\""));
}

#[sqlx::test]
async fn a_dead_link_opened_while_signed_in_shows_the_link_error_page(db: SqlitePool) {
    let app = TestApp::new(db);
    let cookie = app.sign_in_new_admin("Josef").await;

    let response = app.get_with("/invite/made-up", &cookie).await;

    assert_eq!(response.status(), StatusCode::GONE);
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
