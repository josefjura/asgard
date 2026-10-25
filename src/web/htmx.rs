//! The htmx request and response headers Asgard uses.

use axum::http::HeaderName;

/// Sent by htmx on every request it makes.
pub const HX_REQUEST: HeaderName = HeaderName::from_static("hx-request");

/// Tells htmx to load another page instead of swapping the response in.
pub const HX_REDIRECT: HeaderName = HeaderName::from_static("hx-redirect");
