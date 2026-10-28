//! The htmx request and response headers Asgard uses.

use std::convert::Infallible;
use std::future::ready;

use axum::extract::FromRequestParts;
use axum::http::HeaderName;
use axum::http::request::Parts;

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
