//! CSRF protection: a request that can change state must come from our own pages.
//!
//! Browsers send `Origin` on every cross-origin request and on every same-origin request except
//! GET and HEAD, so a missing header on any other method is treated as foreign. This sits on top
//! of `SameSite` cookies.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::ORIGIN;
use axum::http::{HeaderValue, Method};
use axum::middleware::Next;
use axum::response::Response;
use url::Url;

use super::AppError;

/// The origin of `PUBLIC_URL`, serialized the way browsers send it in `Origin`.
#[derive(Debug, Clone)]
pub struct PublicOrigin(Arc<str>);

impl PublicOrigin {
    #[must_use]
    pub fn of(public_url: &Url) -> Self {
        Self(public_url.origin().ascii_serialization().into())
    }

    fn matches(&self, origin: &HeaderValue) -> bool {
        origin.as_bytes() == self.0.as_bytes()
    }
}

/// Lets GET and HEAD through and rejects any other request not sent from [`PublicOrigin`].
///
/// # Errors
///
/// Returns [`AppError::ForeignOrigin`] when `Origin` is missing or names another origin.
pub async fn require_public_origin(
    State(public): State<PublicOrigin>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let safe = matches!(*request.method(), Method::GET | Method::HEAD);
    let ours = request
        .headers()
        .get(ORIGIN)
        .is_some_and(|origin| public.matches(origin));
    if safe || ours {
        Ok(next.run(request).await)
    } else {
        Err(AppError::ForeignOrigin)
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::{Method, StatusCode};
    use axum::routing::any;
    use sqlx::SqlitePool;

    use super::*;
    use crate::web::AppState;
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, request};

    fn probe() -> Router<AppState> {
        Router::new().route("/probe", any(|| async { "reached" }))
    }

    async fn send(db: SqlitePool, method: Method, origin: Option<&str>) -> (StatusCode, String) {
        let app = TestApp::with_routes(db, probe());
        let response = app.send(request(method, "/probe", origin)).await;
        (response.status(), body_text(response).await)
    }

    #[test]
    fn serializes_the_public_url_as_a_browser_origin() {
        for (url, origin) in [
            (
                "https://asgard.joseforge.cz/",
                "https://asgard.joseforge.cz",
            ),
            (
                "https://asgard.joseforge.cz:443/",
                "https://asgard.joseforge.cz",
            ),
            ("http://localhost:8080/", "http://localhost:8080"),
            ("https://ÅSGARD.example/", "https://xn--sgard-lra.example"),
        ] {
            let public = PublicOrigin::of(&Url::parse(url).unwrap());
            assert!(public.matches(&HeaderValue::from_static(origin)), "{url}");
        }
    }

    #[sqlx::test]
    async fn rejects_a_post_without_an_origin(db: SqlitePool) {
        assert_eq!(send(db, Method::POST, None).await.0, StatusCode::FORBIDDEN);
    }

    #[sqlx::test]
    async fn rejects_a_post_from_a_foreign_origin(db: SqlitePool) {
        for origin in [
            "https://evil.example",
            "http://asgard.test",
            "https://asgard.test:8443",
            "https://asgard.test.evil.example",
            "null",
        ] {
            assert_eq!(
                send(db.clone(), Method::POST, Some(origin)).await.0,
                StatusCode::FORBIDDEN,
                "{origin}"
            );
        }
    }

    #[sqlx::test]
    async fn every_unsafe_method_needs_our_origin(db: SqlitePool) {
        for method in [Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS] {
            assert_eq!(
                send(db.clone(), method.clone(), Some("https://evil.example"))
                    .await
                    .0,
                StatusCode::FORBIDDEN,
                "{method}"
            );
            assert_eq!(
                send(db.clone(), method.clone(), Some(PUBLIC_ORIGIN)).await,
                (StatusCode::OK, "reached".to_owned()),
                "{method}"
            );
        }
    }

    #[sqlx::test]
    async fn a_post_from_our_origin_reaches_its_handler(db: SqlitePool) {
        assert_eq!(
            send(db, Method::POST, Some(PUBLIC_ORIGIN)).await,
            (StatusCode::OK, "reached".to_owned())
        );
    }

    #[sqlx::test]
    async fn get_and_head_pass_whatever_their_origin(db: SqlitePool) {
        for method in [Method::GET, Method::HEAD] {
            for origin in [None, Some("https://evil.example"), Some(PUBLIC_ORIGIN)] {
                assert_eq!(
                    send(db.clone(), method.clone(), origin).await.0,
                    StatusCode::OK,
                    "{method} {origin:?}"
                );
            }
        }
    }

    #[sqlx::test]
    async fn covers_the_app_routes_too(db: SqlitePool) {
        let app = TestApp::new(db);

        let response = app.send(request(Method::POST, "/", None)).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
