use maud::Markup;

use crate::views;

pub async fn show() -> Markup {
    views::home::page()
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};
    use sqlx::SqlitePool;

    use crate::web::test_support::get;

    #[sqlx::test]
    async fn serves_the_placeholder_page_as_html(db: SqlitePool) {
        let response = get(db, "/").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
    }
}
