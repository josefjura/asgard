use axum::extract::State;
use sqlx::Connection;

use super::{AppError, AppState};

/// Answers whenever the process is up.
pub async fn live() -> &'static str {
    "ok"
}

/// Answers only when the database does.
pub async fn ready(State(state): State<AppState>) -> Result<&'static str, AppError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(AppError::DatabaseUnavailable)?;
    conn.ping().await.map_err(AppError::DatabaseUnavailable)?;
    Ok("ok")
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use sqlx::SqlitePool;

    use crate::web::test_support::{body_text, get};

    #[sqlx::test]
    async fn live_answers_ok(db: SqlitePool) {
        let response = get(db, "/health/live").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_text(response).await, "ok");
    }

    #[sqlx::test]
    async fn ready_answers_ok_when_the_database_does(db: SqlitePool) {
        let response = get(db, "/health/ready").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_text(response).await, "ok");
    }

    #[sqlx::test]
    async fn ready_is_unavailable_when_the_database_is_not(db: SqlitePool) {
        db.close().await;

        let response = get(db, "/health/ready").await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[sqlx::test]
    async fn live_answers_even_when_the_database_does_not(db: SqlitePool) {
        db.close().await;

        let response = get(db, "/health/live").await;

        assert_eq!(response.status(), StatusCode::OK);
    }
}
