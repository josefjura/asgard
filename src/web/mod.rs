//! Routes, extractors, forms and htmx helpers: parse input, call a service, render a view.

mod error;
#[cfg(debug_assertions)]
mod gallery;
mod health;
mod pages;
mod static_files;

use axum::Router;
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::assets;
use crate::views::nav::Tab;

pub use error::AppError;

/// Everything routes need, built once at startup.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
}

pub fn router(state: AppState) -> Router {
    let mut router = Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/offline", get(pages::offline))
        .route("/static/{file}", get(static_files::hashed));
    for tab in Tab::ALL {
        router = router.route(tab.href(), pages::tab(tab));
    }
    for asset in assets::FIXED {
        router = router.route(asset.url, static_files::fixed(*asset));
    }
    #[cfg(debug_assertions)]
    {
        router = router.route("/dev/gallery", get(gallery::show));
    }
    router.layer(TraceLayer::new_for_http()).with_state(state)
}

#[cfg(test)]
pub(crate) mod test_support {
    use axum::body::Body;
    use axum::http::{Request, Response};
    use http_body_util::BodyExt;
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    use super::{AppState, router};

    pub async fn get(db: SqlitePool, uri: &str) -> Response<Body> {
        router(AppState { db })
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    pub async fn body_text(response: Response<Body>) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use sqlx::SqlitePool;

    use super::test_support::get;

    /// CI runs the suite with `--release` too, where this proves the gallery is absent.
    #[sqlx::test]
    async fn gallery_exists_only_in_debug_builds(db: SqlitePool) {
        let response = get(db, "/dev/gallery").await;

        let expected = if cfg!(debug_assertions) {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(response.status(), expected);
    }
}
