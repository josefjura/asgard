//! Routes, extractors, forms and htmx helpers: parse input, call a service, render a view.

mod error;
mod health;
mod home;

use axum::Router;
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

pub use error::AppError;

/// Everything routes need, built once at startup.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(home::show))
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
