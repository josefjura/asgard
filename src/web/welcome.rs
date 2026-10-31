//! The one-time welcome page a new Member lands on after joining.

use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};

use super::access::Member;
use super::htmx::Htmx;
use super::name_form::{self, NameForm};
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::members::CREW_NAME;
use crate::views;
use crate::views::display_name;
use crate::views::nav::Tab;

/// Where joining lands a new Member.
pub const PATH: &str = "/welcome";

const NAME_PATH: &str = "/welcome/name";
const DONE_PATH: &str = "/welcome/done";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(PATH, get(show))
        .route(NAME_PATH, post(rename))
        .route(DONE_PATH, post(done))
}

/// The welcome page, until the Member has pressed "Got it"; Finance after that.
async fn show(Member(member): Member) -> Response {
    if member.welcomed {
        return Redirect::to(Tab::Finance.href()).into_response();
    }
    let form = display_name::form(NAME_PATH, member.display_name.as_str(), None);
    fresh(page(&form)).into_response()
}

fn page(name_form: &maud::Markup) -> maud::Markup {
    views::welcome::page(CREW_NAME, name_form, DONE_PATH)
}

async fn rename(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<NameForm>,
) -> Result<Response, AppError> {
    name_form::submit(&state, &member, &input, NAME_PATH, htmx, page).await
}

/// "Got it": the welcome page never shows again, and the Member starts on Finance.
async fn done(State(state): State<AppState>, Member(member): Member) -> Result<Redirect, AppError> {
    state.members.mark_welcomed(member.id).await?;
    Ok(Redirect::to(Tab::Finance.href()))
}

#[cfg(test)]
mod tests {
    use axum::http::header::LOCATION;
    use axum::http::{Method, StatusCode};
    use sqlx::SqlitePool;
    use url::form_urlencoded;

    use crate::domain::auth::LinkKind;
    use crate::domain::members::DisplayName;
    use crate::web::links;
    use crate::web::test_support::{
        PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair, request,
    };

    /// A Member called Ondra who just joined beside Josef, the Admin, and the session cookie
    /// of the browser Ondra joined from.
    async fn joined(app: &TestApp) -> String {
        app.sign_in_new_admin("Josef").await;
        let token = app
            .state
            .auth
            .create_test_invite(&DisplayName::parse("Ondra").unwrap())
            .await;
        let response = app
            .join(&links::path(LinkKind::Invite, &token), &mut authenticator())
            .await;
        assert_eq!(response.status(), StatusCode::OK, "joining failed");
        cookie_pair(&response)
    }

    async fn rename(app: &TestApp, cookie: &str, name: &str, htmx: bool) -> (StatusCode, String) {
        let mut post = request(Method::POST, "/welcome/name", Some(PUBLIC_ORIGIN));
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

    #[sqlx::test]
    async fn the_welcome_page_shows_the_new_members_name_and_got_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let welcome = app.get_with("/welcome", &cookie).await;
        assert_eq!(welcome.status(), StatusCode::OK);
        let html = body_text(welcome).await;
        assert!(html.contains("<h1>Welcome to Asgard</h1>"));
        assert!(html.contains("value=\"Ondra\""));
        assert!(html.contains("<form method=\"post\" action=\"/welcome/done\">"));
    }

    #[sqlx::test]
    async fn the_welcome_page_needs_a_signed_in_member(db: SqlitePool) {
        let response = TestApp::new(db).get("/welcome").await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/sign-in");
    }

    #[sqlx::test]
    async fn changing_the_display_name_answers_htmx_with_the_saved_form(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let (status, html) = rename(&app, &cookie, "  Ondřej ", true).await;

        assert_eq!(status, StatusCode::OK);
        assert!(html.starts_with("<form class=\"name-form\""));
        assert!(html.contains("value=\"Ondřej\""));
        assert!(html.contains("Saved."));
        let welcome = body_text(app.get_with("/welcome", &cookie).await).await;
        assert!(welcome.contains("value=\"Ondřej\""));
    }

    #[sqlx::test]
    async fn a_plain_form_post_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let (status, html) = rename(&app, &cookie, "Ondřej", false).await;

        assert_eq!(status, StatusCode::OK);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("Saved."));
    }

    #[sqlx::test]
    async fn a_name_another_member_has_is_refused_with_a_message(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let (status, html) = rename(&app, &cookie, "josef", true).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(html.contains("Someone in the Crew is already called josef."));
        assert!(html.contains("value=\"josef\""));
        assert!(html.contains("aria-invalid=\"true\""));
        let welcome = body_text(app.get_with("/welcome", &cookie).await).await;
        assert!(welcome.contains("value=\"Ondra\""));
    }

    #[sqlx::test]
    async fn a_blank_name_is_refused_with_a_message(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let (status, html) = rename(&app, &cookie, "   ", false).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("Enter a display name."));
    }

    #[sqlx::test]
    async fn got_it_goes_to_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let response = app.post_with("/welcome/done", &cookie).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/");
    }

    #[sqlx::test]
    async fn finance_sends_a_new_member_to_the_welcome_page_until_got_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;

        let before = app.get_with("/", &cookie).await;
        app.post_with("/welcome/done", &cookie).await;
        let after = app.get_with("/", &cookie).await;

        assert_eq!(before.status(), StatusCode::SEE_OTHER);
        assert_eq!(before.headers()[LOCATION], "/welcome");
        assert_eq!(after.status(), StatusCode::OK);
        assert!(body_text(after).await.contains("<h1>Finance</h1>"));
    }

    #[sqlx::test]
    async fn after_got_it_the_welcome_page_no_longer_appears(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;
        app.post_with("/welcome/done", &cookie).await;

        let response = app.get_with("/welcome", &cookie).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/");
    }

    #[sqlx::test]
    async fn got_it_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = joined(&app).await;
        let mut post = request(Method::POST, "/welcome/done", Some("https://evil.example"));
        post.headers_mut().insert("cookie", cookie.parse().unwrap());

        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let welcome = app.get_with("/welcome", &cookie).await;
        assert_eq!(welcome.status(), StatusCode::OK);
    }
}
