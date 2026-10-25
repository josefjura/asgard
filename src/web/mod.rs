//! Routes, extractors, forms and htmx helpers: parse input, call a service, render a view.

mod error;
#[cfg(debug_assertions)]
mod gallery;
mod health;
mod origin;
mod pages;
mod static_files;

use axum::Router;
use axum::middleware;
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::assets;
use crate::domain::clock::Clock;
use crate::views::nav::Tab;

pub use error::AppError;
pub use origin::PublicOrigin;

/// Everything routes need, built once at startup.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub clock: Clock,
    pub public_origin: PublicOrigin,
}

pub fn router(state: AppState) -> Router {
    with_layers(routes(), state)
}

fn routes() -> Router<AppState> {
    let mut router = Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/offline", get(pages::offline))
        .route("/static/{file}", get(static_files::hashed))
        .route(
            assets::SERVICE_WORKER.url,
            get(static_files::service_worker),
        )
        .route(assets::MANIFEST.url, get(static_files::manifest));
    for tab in Tab::ALL {
        router = router.route(tab.href(), pages::tab(tab));
    }
    #[cfg(debug_assertions)]
    {
        router = router.route("/dev/gallery", get(gallery::show));
    }
    router
}

fn with_layers(routes: Router<AppState>, state: AppState) -> Router {
    routes
        .layer(middleware::from_fn_with_state(
            state.public_origin.clone(),
            origin::require_public_origin,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::time::{Duration, SystemTime};

    use axum::Router;
    use axum::body::Body;
    use axum::http::header::ORIGIN;
    use axum::http::{Method, Request, Response};
    use http_body_util::BodyExt;
    use sqlx::SqlitePool;
    use tower::ServiceExt;
    use url::Url;

    use super::{AppState, PublicOrigin, routes, with_layers};
    use crate::domain::clock::ManualClock;

    /// The origin every test app is served from.
    pub const PUBLIC_ORIGIN: &str = "https://asgard.test";

    /// Where a test app's clock starts: 2026-01-01T00:00:00Z.
    pub fn clock_start() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600)
    }

    /// The app's router with a clock the test controls.
    pub struct TestApp {
        router: Router,
        pub clock: ManualClock,
    }

    impl TestApp {
        pub fn new(db: SqlitePool) -> Self {
            Self::with_routes(db, Router::new())
        }

        /// The app plus `extra` routes, behind the same middleware.
        pub fn with_routes(db: SqlitePool, extra: Router<AppState>) -> Self {
            let clock = ManualClock::starting_at(clock_start());
            let state = AppState {
                db,
                clock: clock.clock(),
                public_origin: PublicOrigin::of(&Url::parse(PUBLIC_ORIGIN).unwrap()),
            };
            Self {
                router: with_layers(routes().merge(extra), state),
                clock,
            }
        }

        pub async fn send(&self, request: Request<Body>) -> Response<Body> {
            self.router.clone().oneshot(request).await.unwrap()
        }

        pub async fn get(&self, uri: &str) -> Response<Body> {
            self.send(request(Method::GET, uri, None)).await
        }
    }

    /// A request with an empty body, sent with `origin` as its `Origin` header, or none.
    pub fn request(method: Method, uri: &str, origin: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }
        builder.body(Body::empty()).unwrap()
    }

    pub async fn get(db: SqlitePool, uri: &str) -> Response<Body> {
        TestApp::new(db).get(uri).await
    }

    pub async fn body_text(response: Response<Body>) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::Router;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing;
    use sqlx::SqlitePool;

    use super::AppState;
    use super::test_support::{TestApp, body_text, clock_start, get};

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

    #[sqlx::test]
    async fn handlers_read_the_time_the_test_app_sets(db: SqlitePool) {
        let probe = Router::new().route(
            "/now",
            routing::get(|State(state): State<AppState>| async move {
                let since_start = state.clock.now().duration_since(clock_start()).unwrap();
                since_start.as_secs().to_string()
            }),
        );
        let app = TestApp::with_routes(db, probe);

        assert_eq!(body_text(app.get("/now").await).await, "0");

        app.clock.advance(Duration::from_secs(90));
        assert_eq!(body_text(app.get("/now").await).await, "90");

        app.clock.set(clock_start() + Duration::from_secs(86_400));
        assert_eq!(body_text(app.get("/now").await).await, "86400");
    }
}
