//! The admin page, for the Admin only: making Invite links, and listing and revoking the open
//! ones.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use super::access::Admin;
use super::error::name_message;
use super::htmx::Htmx;
use super::pages::fresh;
use super::{AppError, AppState, invite};
use crate::domain::auth::{AuthError, LinkId};
use crate::domain::members::DisplayName;
use crate::views;
use crate::views::admin::Outcome;
use crate::views::nav::ADMIN_HREF;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(ADMIN_HREF, get(show))
        .route("/admin/invites", post(create_invite))
        .route("/admin/links/{id}/revoke", post(revoke))
}

async fn show(State(state): State<AppState>, _: Admin) -> Result<Response, AppError> {
    answer(&state, StatusCode::OK, None, false).await
}

#[derive(Deserialize)]
struct InviteForm {
    display_name: String,
}

async fn create_invite(
    State(state): State<AppState>,
    Admin(admin): Admin,
    Htmx(htmx): Htmx,
    Form(input): Form<InviteForm>,
) -> Result<Response, AppError> {
    let created = match DisplayName::parse(&input.display_name) {
        Ok(name) => state
            .auth
            .create_invite(&name, admin.id)
            .await
            .map(|token| (name, token)),
        Err(err) => Err(err.into()),
    };
    match created {
        Ok((name, token)) => {
            tracing::info!(admin = admin.id.get(), "the Admin made an Invite link");
            let url = state.public_origin.url(&invite::path(&token));
            let outcome = Outcome::Created {
                name: &name,
                url: &url,
            };
            answer(&state, StatusCode::OK, Some(outcome), htmx).await
        }
        Err(AuthError::Name(err)) => {
            let message = name_message(&err);
            let refused = Outcome::Refused {
                value: &input.display_name,
                message: &message,
            };
            answer(
                &state,
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(refused),
                htmx,
            )
            .await
        }
        Err(err) => Err(err.into()),
    }
}

/// Revokes the link if it is still open. One that isn't, say revoked in another tab, just
/// drops out of the list the Admin gets back.
async fn revoke(
    State(state): State<AppState>,
    Admin(admin): Admin,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let revoked = state.auth.revoke_link(LinkId::from_client(id)).await?;
    if revoked.is_some() {
        tracing::info!(admin = admin.id.get(), "the Admin revoked a link");
    }
    let outcome = revoked.as_ref().map(Outcome::Revoked);
    answer(&state, StatusCode::OK, outcome, htmx).await
}

/// The links showing `outcome`: alone for htmx, inside the admin page otherwise.
async fn answer(
    state: &AppState,
    status: StatusCode,
    outcome: Option<Outcome<'_>>,
    htmx: bool,
) -> Result<Response, AppError> {
    let open = state.auth.open_links().await?;
    let links = views::admin::links(ADMIN_HREF, &open, outcome);
    let body = if htmx {
        links
    } else {
        views::admin::page(&links)
    };
    Ok((status, fresh(body)).into_response())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, COOKIE, LOCATION};
    use axum::http::{Method, Response, StatusCode};
    use sqlx::SqlitePool;
    use url::form_urlencoded;

    use crate::domain::auth::{LINK_LIFETIME, SessionToken};
    use crate::domain::members::DisplayName;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{
        PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair, request,
    };

    /// A form post from a browser that holds `cookie`, sent by htmx unless `htmx` says not.
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

    async fn make_link(app: &TestApp, cookie: &str, name: &str) -> Response<Body> {
        post_form(
            app,
            "/admin/invites",
            cookie,
            &[("display_name", name)],
            true,
        )
        .await
    }

    /// The path of the new Invite link the admin page shows, made for `name`.
    async fn invite_path(app: &TestApp, cookie: &str, name: &str) -> String {
        let response = make_link(app, cookie, name).await;
        assert_eq!(response.status(), StatusCode::OK, "making the link failed");
        let html = body_text(response).await;
        let start = html.find("id=\"new-link\" type=\"text\" value=\"").unwrap()
            + "id=\"new-link\" type=\"text\" value=\"".len();
        let url = &html[start..start + html[start..].find('"').unwrap()];
        url.strip_prefix(PUBLIC_ORIGIN)
            .expect("the link is on our origin")
            .to_owned()
    }

    async fn admin_page(app: &TestApp, cookie: &str) -> String {
        let response = app.get_with("/admin", cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    /// The id of the open link for `name`, read from its revoke button.
    async fn link_id(app: &TestApp, cookie: &str, name: &str) -> String {
        let html = admin_page(app, cookie).await;
        let at = html
            .find(&format!("Revoke the link for {name}?"))
            .expect("the link is listed");
        let before = &html[..at];
        let start = before.rfind("/admin/links/").unwrap() + "/admin/links/".len();
        before[start..start + before[start..].find('/').unwrap()].to_owned()
    }

    async fn revoke(app: &TestApp, cookie: &str, id: &str) -> Response<Body> {
        post_form(app, &format!("/admin/links/{id}/revoke"), cookie, &[], true).await
    }

    /// Ondra, a Member without the Admin role, signed in beside Josef, the Admin.
    async fn ondra(app: &TestApp, admin: &str) -> String {
        let path = invite_path(app, admin, "Ondra").await;
        let response = app.join(&path, &mut authenticator()).await;
        assert_eq!(response.status(), StatusCode::OK, "joining failed");
        cookie_pair(&response)
    }

    #[sqlx::test]
    async fn a_new_person_joins_through_the_admins_link_as_a_non_admin_member(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        let path = invite_path(&app, &admin, " Ondra ").await;

        assert!(
            body_text(app.get(&path).await)
                .await
                .contains("<strong>Ondra</strong>")
        );
        let response = app.join(&path, &mut authenticator()).await;

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = cookie_pair(&response);
        let token = SessionToken::from_client(cookie.split_once('=').unwrap().1.to_owned());
        let member = app
            .state
            .auth
            .session(&token)
            .await
            .unwrap()
            .unwrap()
            .member;
        assert_eq!(member.display_name, DisplayName::parse("Ondra").unwrap());
        assert!(!member.is_admin);
    }

    #[sqlx::test]
    async fn making_a_link_answers_htmx_with_the_link_ready_to_copy(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;

        let response = make_link(&app, &admin, "Ondra").await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.starts_with("<div class=\"admin-links\" id=\"links\">"));
        assert!(html.contains("Send this link to <strong>Ondra</strong>."));
        assert!(html.contains(&format!("value=\"{PUBLIC_ORIGIN}/invite/")));
        assert!(html.contains("data-copy=\"new-link\""));
        assert!(html.contains("Invite for <strong>Ondra</strong>"));
    }

    #[sqlx::test]
    async fn a_plain_form_post_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;

        let response = post_form(
            &app,
            "/admin/invites",
            &admin,
            &[("display_name", "Ondra")],
            false,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Admin</h1>"));
        assert!(html.contains("data-copy=\"new-link\""));
    }

    #[sqlx::test]
    async fn a_name_taken_in_any_case_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        ondra(&app, &admin).await;

        for taken in ["JOSEF", "ondra"] {
            let response = make_link(&app, &admin, taken).await;

            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{taken}"
            );
            let html = body_text(response).await;
            assert!(html.contains(&format!("Someone in the Crew is already called {taken}.")));
            assert!(html.contains(&format!("value=\"{taken}\"")));
            assert!(!html.contains("data-copy"));
        }
        assert!(admin_page(&app, &admin).await.contains("No open links."));
    }

    #[sqlx::test]
    async fn a_blank_name_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;

        let response = make_link(&app, &admin, "  ").await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body_text(response).await.contains("Enter a display name."));
    }

    #[sqlx::test]
    async fn the_page_lists_open_links_and_leaves_out_used_expired_and_revoked_ones(
        db: SqlitePool,
    ) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        invite_path(&app, &admin, "Expired").await;
        app.clock.advance(LINK_LIFETIME / 2);
        let used = invite_path(&app, &admin, "Used").await;
        app.join(&used, &mut authenticator()).await;
        invite_path(&app, &admin, "Revoked").await;
        invite_path(&app, &admin, "Open").await;
        let revoked = link_id(&app, &admin, "Revoked").await;
        revoke(&app, &admin, &revoked).await;
        app.clock.advance(LINK_LIFETIME / 2);

        let html = admin_page(&app, &admin).await;

        assert_eq!(html.matches("Invite for ").count(), 1);
        assert!(html.contains("Invite for <strong>Open</strong>"));
        assert!(html.contains("Works until 2 January, 13:00"));
    }

    #[sqlx::test]
    async fn a_revoked_link_shows_the_link_error_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        let path = invite_path(&app, &admin, "Ondra").await;
        let id = link_id(&app, &admin, "Ondra").await;

        let response = revoke(&app, &admin, &id).await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.contains("Revoked the link for <strong>Ondra</strong>."));
        assert!(html.contains("No open links."));
        let opened = app.get(&path).await;
        assert_eq!(opened.status(), StatusCode::GONE);
        assert!(
            body_text(opened)
                .await
                .contains("This link no longer works")
        );
        assert_eq!(
            app.join(&path, &mut authenticator()).await.status(),
            StatusCode::GONE
        );
    }

    #[sqlx::test]
    async fn revoking_a_link_twice_just_lists_the_open_links(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        invite_path(&app, &admin, "Ondra").await;
        let id = link_id(&app, &admin, "Ondra").await;
        revoke(&app, &admin, &id).await;

        let response = revoke(&app, &admin, &id).await;

        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(!html.contains("Revoked the link"));
        assert!(html.contains("No open links."));
    }

    #[sqlx::test]
    async fn a_member_who_is_not_the_admin_gets_403_on_every_admin_route(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        invite_path(&app, &admin, "Karel").await;
        let id = link_id(&app, &admin, "Karel").await;
        let ondra = ondra(&app, &admin).await;

        let page = app.get_with("/admin", &ondra).await;
        let made = make_link(&app, &ondra, "Mallory").await;
        let revoked = revoke(&app, &ondra, &id).await;

        for response in [page, made, revoked] {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(body_text(response).await, "Only the Admin can do that.");
        }
        let html = admin_page(&app, &admin).await;
        assert!(html.contains("Invite for <strong>Karel</strong>"));
        assert!(!html.contains("Mallory"));
    }

    #[sqlx::test]
    async fn only_the_admins_settings_link_to_the_admin_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;
        let ondra = ondra(&app, &admin).await;
        app.post_with("/welcome/done", &ondra).await;

        let admins = body_text(app.get_with("/settings", &admin).await).await;
        let members = body_text(app.get_with("/settings", &ondra).await).await;

        assert!(admins.contains("href=\"/admin\""));
        assert!(!members.contains("/admin"));
    }

    #[sqlx::test]
    async fn the_admin_page_is_fresh_and_marks_nothing_in_the_navigation(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;

        let response = app.get_with("/admin", &admin).await;

        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        let html = body_text(response).await;
        assert!(html.contains("<title>Admin | Asgard</title>"));
        assert!(!html.contains("aria-current"));
    }

    #[sqlx::test]
    async fn signed_out_the_admin_page_sends_the_browser_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        let page = app.get("/admin").await;
        let mut htmx = request(Method::POST, "/admin/invites", Some(PUBLIC_ORIGIN));
        htmx.headers_mut()
            .insert(HX_REQUEST, "true".parse().unwrap());
        let made = app.send(htmx).await;

        assert_eq!(page.status(), StatusCode::SEE_OTHER);
        assert_eq!(page.headers()[LOCATION], "/sign-in");
        assert_eq!(made.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(made.headers()[HX_REDIRECT], "/sign-in");
    }

    #[sqlx::test]
    async fn making_a_link_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let admin = app.sign_in_new_admin("Josef").await;

        let mut post = request(Method::POST, "/admin/invites", Some("https://evil.example"));
        post.headers_mut().insert(COOKIE, admin.parse().unwrap());
        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(admin_page(&app, &admin).await.contains("No open links."));
    }
}
