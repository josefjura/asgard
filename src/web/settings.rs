//! A Member's settings: their display name and logging out of this device.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::Markup;

use super::access::Member;
use super::htmx::Htmx;
use super::name_form::{self, NameForm};
use super::pages::fresh;
use super::sign_in::LOG_OUT_PATH;
use super::{AppError, AppState};
use crate::views;
use crate::views::display_name;
use crate::views::nav::SETTINGS_HREF;

const NAME_PATH: &str = "/settings/name";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(SETTINGS_HREF, get(show))
        .route(NAME_PATH, post(rename))
}

async fn show(Member(member): Member) -> impl IntoResponse {
    let form = display_name::form(NAME_PATH, member.display_name.as_str(), None);
    fresh(page(&form))
}

fn page(name_form: &Markup) -> Markup {
    views::settings::page(name_form, LOG_OUT_PATH)
}

async fn rename(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<NameForm>,
) -> Result<Response, AppError> {
    name_form::submit(&state, &member, &input, NAME_PATH, htmx, page).await
}

#[cfg(test)]
mod tests {
    use axum::http::header::{CACHE_CONTROL, LOCATION};
    use axum::http::{Method, StatusCode};
    use sqlx::SqlitePool;
    use url::form_urlencoded;

    use crate::domain::members::{DISPLAY_NAME_MAX, DisplayName};
    use crate::views::nav::Tab;
    use crate::web::test_support::{
        PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair, request,
    };

    /// Ondra, a Member who joined beside Josef, the Admin, and the session cookie of Ondra's
    /// browser.
    async fn ondra(app: &TestApp) -> String {
        app.sign_in_new_admin("Josef").await;
        let token = app
            .state
            .auth
            .create_test_invite(&DisplayName::parse("Ondra").unwrap())
            .await;
        let response = app
            .join(&crate::web::invite::path(&token), &mut authenticator())
            .await;
        assert_eq!(response.status(), StatusCode::OK, "joining failed");
        let cookie = cookie_pair(&response);
        app.post_with("/welcome/done", &cookie).await;
        cookie
    }

    async fn rename(
        app: &TestApp,
        cookie: &str,
        name: &str,
        htmx: bool,
        origin: &str,
    ) -> (StatusCode, String) {
        let mut post = request(Method::POST, "/settings/name", Some(origin));
        let headers = post.headers_mut();
        headers.insert("cookie", cookie.parse().unwrap());
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        if htmx {
            headers.insert("hx-request", "true".parse().unwrap());
        }
        *post.body_mut() = form_urlencoded::Serializer::new(String::new())
            .append_pair("display_name", name)
            .finish()
            .into();
        let response = app.send(post).await;
        (response.status(), body_text(response).await)
    }

    async fn settings(app: &TestApp, cookie: &str) -> String {
        let response = app.get_with("/settings", cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    #[sqlx::test]
    async fn every_tab_leads_to_settings(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        for tab in Tab::ALL {
            let html = body_text(app.get_with(tab.href(), &cookie).await).await;

            assert!(
                html.contains("class=\"settings-link\" href=\"/settings\""),
                "{}",
                tab.href()
            );
        }
    }

    #[sqlx::test]
    async fn settings_shows_the_members_name_and_a_log_out_button(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let response = app.get_with("/settings", &cookie).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        let html = body_text(response).await;
        assert!(html.contains("<h1>Settings</h1>"));
        assert!(html.contains("hx-post=\"/settings/name\""));
        assert!(html.contains("value=\"Ondra\""));
        assert!(html.contains("<form method=\"post\" action=\"/log-out\">"));
    }

    #[sqlx::test]
    async fn settings_needs_a_signed_in_member(db: SqlitePool) {
        let response = TestApp::new(db).get("/settings").await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
    }

    #[sqlx::test]
    async fn changing_the_name_answers_htmx_with_the_saved_form(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let (status, html) = rename(&app, &cookie, " Ondřej  ", true, PUBLIC_ORIGIN).await;

        assert_eq!(status, StatusCode::OK);
        assert!(html.starts_with("<form class=\"name-form\""));
        assert!(html.contains("hx-post=\"/settings/name\""));
        assert!(html.contains("value=\"Ondřej\""));
        assert!(html.contains("Saved."));
    }

    #[sqlx::test]
    async fn the_new_name_shows_wherever_the_name_appears(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        rename(&app, &cookie, "Ondřej", true, PUBLIC_ORIGIN).await;

        assert!(settings(&app, &cookie).await.contains("value=\"Ondřej\""));
        let token = app
            .state
            .auth
            .create_test_invite(&DisplayName::parse("Karel").unwrap())
            .await;
        let invite = app
            .get_with(&crate::web::invite::path(&token), &cookie)
            .await;
        let html = body_text(invite).await;
        assert!(html.contains("signed in as <strong>Ondřej</strong>"));
    }

    #[sqlx::test]
    async fn a_plain_form_post_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let (status, html) = rename(&app, &cookie, "Ondřej", false, PUBLIC_ORIGIN).await;

        assert_eq!(status, StatusCode::OK);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Settings</h1>"));
        assert!(html.contains("Saved."));
    }

    #[sqlx::test]
    async fn a_name_another_member_has_ignoring_case_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let (status, html) = rename(&app, &cookie, "JOSEF", true, PUBLIC_ORIGIN).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(html.contains("Someone in the Crew is already called JOSEF."));
        assert!(html.contains("aria-invalid=\"true\""));
        assert!(settings(&app, &cookie).await.contains("value=\"Ondra\""));
    }

    #[sqlx::test]
    async fn a_member_can_change_only_the_case_of_their_own_name(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let (status, html) = rename(&app, &cookie, "ONDRA", true, PUBLIC_ORIGIN).await;

        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("Saved."));
        assert!(settings(&app, &cookie).await.contains("value=\"ONDRA\""));
    }

    #[sqlx::test]
    async fn a_blank_name_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let (status, html) = rename(&app, &cookie, "  ", true, PUBLIC_ORIGIN).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(html.contains("Enter a display name."));
        assert!(settings(&app, &cookie).await.contains("value=\"Ondra\""));
    }

    #[sqlx::test]
    async fn a_name_that_is_too_long_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let long = "x".repeat(DISPLAY_NAME_MAX + 1);
        let (status, html) = rename(&app, &cookie, &long, true, PUBLIC_ORIGIN).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(html.contains("aria-invalid=\"true\""));
        assert!(html.contains(&format!("to {DISPLAY_NAME_MAX} characters or fewer")));
        assert!(settings(&app, &cookie).await.contains("value=\"Ondra\""));
    }

    #[sqlx::test]
    async fn changing_the_name_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let (status, _) = rename(&app, &cookie, "Mallory", true, "https://evil.example").await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(settings(&app, &cookie).await.contains("value=\"Ondra\""));
    }

    #[sqlx::test]
    async fn the_log_out_button_ends_this_devices_session(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = ondra(&app).await;

        let response = app.post_with("/log-out", &cookie).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
        let next = app.get_with("/settings", &cookie).await;
        assert_eq!(next.status(), StatusCode::SEE_OTHER);
        assert_eq!(next.headers()[LOCATION], "/sign-in");
    }
}
