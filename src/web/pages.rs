use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{MethodRouter, get};
use maud::Markup;

use super::access::Member;
use super::{AppState, welcome};
use crate::views;
use crate::views::nav::Tab;

/// A tab's page, for signed-in Members only. Finance, the home page, sends a Member to the
/// welcome page until they have pressed "Got it" there.
pub fn tab(tab: Tab) -> MethodRouter<AppState> {
    get(move |Member(member): Member| async move {
        if tab == Tab::Finance && !member.welcomed {
            return Redirect::to(welcome::PATH).into_response();
        }
        fresh(views::tabs::page(tab)).into_response()
    })
}

/// The page the service worker caches and shows when there is no network.
pub async fn offline() -> impl IntoResponse {
    fresh(views::offline::page())
}

/// Pages are always revalidated, so they never point at an outdated asset URL.
pub fn fresh(page: Markup) -> impl IntoResponse {
    ([(CACHE_CONTROL, "no-cache")], page)
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode, header};
    use sqlx::SqlitePool;

    use crate::domain::auth::SESSION_LIFETIME;
    use crate::views::nav::Tab;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{TestApp, body_text, get, request};

    #[sqlx::test]
    async fn every_tab_is_reachable_and_marked_active(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        for tab in Tab::ALL {
            let response = app.get_with(tab.href(), &cookie).await;

            assert_eq!(response.status(), StatusCode::OK, "{}", tab.href());
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
            let html = body_text(response).await;
            assert!(html.contains(&format!("href=\"{}\" aria-current=\"page\"", tab.href())));
        }
    }

    #[sqlx::test]
    async fn finance_is_the_home_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = body_text(app.get_with("/", &cookie).await).await;

        assert!(html.contains("<h1>Finance</h1>"));
    }

    #[sqlx::test]
    async fn signed_out_every_tab_redirects_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for tab in Tab::ALL {
            let response = app.get(tab.href()).await;

            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{}", tab.href());
            assert_eq!(response.headers()[header::LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn signed_out_every_tab_tells_htmx_to_load_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for tab in Tab::ALL {
            let mut htmx = request(Method::GET, tab.href(), None);
            htmx.headers_mut()
                .insert(HX_REQUEST, "true".parse().unwrap());
            let response = app.send(htmx).await;

            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{}",
                tab.href()
            );
            assert_eq!(response.headers()[HX_REDIRECT], "/sign-in");
            assert!(!response.headers().contains_key(header::LOCATION));
        }
    }

    #[sqlx::test]
    async fn an_unknown_session_is_signed_out(db: SqlitePool) {
        let response = TestApp::new(db)
            .get_with("/", "asgard_session=made-up")
            .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }

    #[sqlx::test]
    async fn a_session_ends_once_unused_for_its_lifetime(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        app.clock.advance(SESSION_LIFETIME);

        assert_eq!(
            app.get_with("/", &cookie).await.status(),
            StatusCode::SEE_OTHER
        );
    }

    #[sqlx::test]
    async fn serves_the_offline_page(db: SqlitePool) {
        let response = get(db, "/offline").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("You're offline"));
    }
}
