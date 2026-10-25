use axum::response::IntoResponse;

use super::pages::fresh;
use crate::views;

/// Where a request without a valid session is sent.
pub const PATH: &str = "/sign-in";

pub async fn show() -> impl IntoResponse {
    fresh(views::sign_in::placeholder())
}
