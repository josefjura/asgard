use axum::routing::{MethodRouter, get};
use maud::Markup;

use super::AppState;
use crate::views;
use crate::views::nav::Tab;

/// A tab's page.
pub fn tab(tab: Tab) -> MethodRouter<AppState> {
    get(move || async move { views::tabs::page(tab) })
}

/// The page the service worker caches and shows when there is no network.
pub async fn offline() -> Markup {
    views::offline::page()
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};
    use sqlx::SqlitePool;

    use crate::views::nav::Tab;
    use crate::web::test_support::{body_text, get};

    #[sqlx::test]
    async fn every_tab_is_reachable_and_marked_active(db: SqlitePool) {
        for tab in Tab::ALL {
            let response = get(db.clone(), tab.href()).await;

            assert_eq!(response.status(), StatusCode::OK, "{}", tab.href());
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            let html = body_text(response).await;
            assert!(html.contains(&format!("href=\"{}\" aria-current=\"page\"", tab.href())));
        }
    }

    #[sqlx::test]
    async fn finance_is_the_home_page(db: SqlitePool) {
        let html = body_text(get(db, "/").await).await;

        assert!(html.contains("<h1>Finance</h1>"));
    }

    #[sqlx::test]
    async fn serves_the_offline_page(db: SqlitePool) {
        let response = get(db, "/offline").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("You're offline"));
    }
}
