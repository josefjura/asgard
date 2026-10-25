use axum::extract::Path;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};

use crate::assets::hash::fingerprint;
use crate::assets::{self, Asset};
use crate::views;

/// A hashed URL never changes content, so clients may keep it forever.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// A fixed URL must be revalidated, so a new service worker or manifest is picked up.
const REVALIDATE: &str = "no-cache";

/// Serves a hashed asset; an outdated hash is a 404, never stale content.
pub async fn hashed(Path(file): Path<String>) -> Response {
    assets::hashed(&format!("/static/{file}")).map_or_else(
        || StatusCode::NOT_FOUND.into_response(),
        |asset| serve(asset, IMMUTABLE),
    )
}

/// Serves the web manifest at its fixed URL.
pub async fn manifest() -> Response {
    serve(assets::MANIFEST, REVALIDATE)
}

/// Serves the service worker, stamped with the offline page's fingerprint.
///
/// A browser installs a new worker only when the worker's bytes change, and the worker caches
/// the offline page at install, so the stamp keeps that cached copy current.
pub async fn service_worker() -> Response {
    let offline = views::offline::page().into_string();
    let body = format!(
        "{}
// Offline page {:016x}
",
        String::from_utf8_lossy(assets::SERVICE_WORKER.body),
        fingerprint(offline.as_bytes())
    );
    (
        [
            (CONTENT_TYPE, assets::SERVICE_WORKER.content_type),
            (CACHE_CONTROL, REVALIDATE),
        ],
        body,
    )
        .into_response()
}

fn serve(asset: Asset, cache_control: &'static str) -> Response {
    (
        [
            (CONTENT_TYPE, asset.content_type),
            (CACHE_CONTROL, cache_control),
        ],
        asset.body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    use sqlx::SqlitePool;

    use crate::assets;
    use crate::web::test_support::{body_text, get};

    #[sqlx::test]
    async fn serves_the_current_stylesheet_forever(db: SqlitePool) {
        let response = get(db, assets::APP_CSS.url).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/css; charset=utf-8");
        assert_eq!(
            response.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
    }

    #[sqlx::test]
    async fn an_outdated_stylesheet_url_is_not_found(db: SqlitePool) {
        let response = get(db, "/static/app.0000000000000000.css").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn serves_htmx_from_our_own_origin(db: SqlitePool) {
        let response = get(db, assets::HTMX.url).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
    }

    #[sqlx::test]
    async fn stamps_the_service_worker_with_the_offline_page(db: SqlitePool) {
        let offline = crate::views::offline::page().into_string();
        let stamp = format!(
            "// Offline page {:016x}",
            crate::assets::hash::fingerprint(offline.as_bytes())
        );

        let body = body_text(get(db, "/sw.js").await).await;

        assert!(body.contains(&stamp));
    }

    #[sqlx::test]
    async fn serves_the_service_worker_at_the_root_and_revalidates_it(db: SqlitePool) {
        let response = get(db, "/sw.js").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
    }

    #[sqlx::test]
    async fn serves_the_manifest(db: SqlitePool) {
        let response = get(db, "/manifest.webmanifest").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "application/manifest+json"
        );
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
    }
}
