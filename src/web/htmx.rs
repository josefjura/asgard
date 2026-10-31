//! The htmx request and response headers Asgard uses, and the answers that differ for htmx.

use std::convert::Infallible;
use std::future::ready;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderName, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::pages::fresh;

/// Sent by htmx on every request it makes.
pub const HX_REQUEST: HeaderName = HeaderName::from_static("hx-request");

/// Tells htmx to load another page instead of swapping the response in.
pub const HX_REDIRECT: HeaderName = HeaderName::from_static("hx-redirect");

/// Whether htmx sent the request, so a handler can answer with a fragment instead of a page.
pub struct Htmx(pub bool);

impl<S: Sync> FromRequestParts<S> for Htmx {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Infallible>> + Send {
        ready(Ok(Self(parts.headers.contains_key(HX_REQUEST))))
    }
}

/// `fragment` with `status`: alone for htmx to swap in, or inside the page `page` puts it in
/// for a plain form post.
pub fn fragment_or_page(
    htmx: bool,
    status: StatusCode,
    fragment: Markup,
    page: impl FnOnce(&Markup) -> Markup,
) -> Response {
    let body = if htmx { fragment } else { page(&fragment) };
    (status, fresh(body)).into_response()
}

/// Sends the browser to the page at `to`. htmx can't follow a redirect to a whole page, so it
/// is told to load the page instead.
pub fn go_to(htmx: bool, to: &str) -> Response {
    if htmx {
        (StatusCode::OK, [(HX_REDIRECT, to)]).into_response()
    } else {
        Redirect::to(to).into_response()
    }
}
