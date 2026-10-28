//! Who is asking: the session cookie and the extractors that guard routes.

use std::convert::Infallible;
use std::future::ready;
use std::sync::{Arc, OnceLock};

use axum::extract::{FromRequestParts, OptionalFromRequestParts, Request};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use cookie::time::Duration;
use cookie::{Cookie, CookieBuilder, SameSite};

use super::htmx::HX_REQUEST;
use super::{AppError, AppState};
use crate::domain::auth::{SESSION_LIFETIME, SessionToken};
use crate::domain::members;

const SESSION_COOKIE: &str = "asgard_session";

/// The session cookie's attributes: script can't read it, it only travels over HTTPS, and
/// other sites' requests don't carry it.
fn session_cookie_builder(value: &str) -> CookieBuilder<'_> {
    Cookie::build((SESSION_COOKIE, value))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
}

/// The `Set-Cookie` value that keeps `token` in the browser for as long as the session lasts.
pub fn session_cookie(token: &SessionToken) -> HeaderValue {
    let max_age = i64::try_from(SESSION_LIFETIME.as_secs()).unwrap_or(i64::MAX);
    let cookie = session_cookie_builder(token.as_str()).max_age(Duration::seconds(max_age));
    // A token is base64url, which is always a valid header value.
    HeaderValue::try_from(cookie.to_string()).expect("a session cookie is a valid header")
}

/// The `Set-Cookie` value that makes the browser forget its session.
pub fn removed_session_cookie() -> HeaderValue {
    let mut cookie = session_cookie_builder("").build();
    cookie.make_removal();
    HeaderValue::try_from(cookie.to_string()).expect("a removal cookie is a valid header")
}

fn session_token(headers: &HeaderMap) -> Option<SessionToken> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(Cookie::split_parse)
        .filter_map(Result::ok)
        .find(|cookie| cookie.name() == SESSION_COOKIE)
        .map(|cookie| SessionToken::from_client(cookie.value().to_owned()))
}

/// Where an extractor leaves a session it renewed, for [`renew_session_cookie`] to send.
#[derive(Clone, Default)]
struct Renewed(Arc<OnceLock<SessionToken>>);

/// Sends a fresh session cookie whenever the request renewed its session, so the browser keeps
/// the cookie as long as the server keeps the session.
pub async fn renew_session_cookie(mut request: Request, next: Next) -> Response {
    let renewed = Renewed::default();
    request.extensions_mut().insert(renewed.clone());
    let mut response = next.run(request).await;
    if let Some(token) = renewed.0.get() {
        response
            .headers_mut()
            .append(SET_COOKIE, session_cookie(token));
    }
    response
}

/// A signed-in Member. Without a valid session, the request is sent to sign in.
#[derive(Debug, Clone)]
pub struct Member(pub members::Member);

impl FromRequestParts<AppState> for Member {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let htmx = parts.headers.contains_key(HX_REQUEST);
        <Self as OptionalFromRequestParts<AppState>>::from_request_parts(parts, state)
            .await?
            .ok_or(AppError::SignInRequired { htmx })
    }
}

/// The signed-in Member if there is one, for pages that also serve signed-out visitors.
impl OptionalFromRequestParts<AppState> for Member {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Option<Self>, AppError> {
        let Some(token) = session_token(&parts.headers) else {
            return Ok(None);
        };
        let Some(session) = state.auth.session(&token).await? else {
            return Ok(None);
        };
        if session.renewed
            && let Some(renewed) = parts.extensions.get::<Renewed>()
        {
            // A second extractor in the same request finds the token already recorded.
            let _ = renewed.0.set(token);
        }
        Ok(Some(Self(session.member)))
    }
}

/// The session this browser's cookie names, valid or not: all that ending it needs.
pub struct SessionCookie(pub Option<SessionToken>);

impl<S: Sync> FromRequestParts<S> for SessionCookie {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Infallible>> + Send {
        ready(Ok(Self(session_token(&parts.headers))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_session_among_other_cookies() {
        let token = SessionToken::generate();
        let mut headers = HeaderMap::new();
        headers.append(COOKIE, HeaderValue::from_static("theme=dark"));
        headers.append(
            COOKIE,
            HeaderValue::try_from(format!("a=1; {SESSION_COOKIE}={}; b=2", token.as_str()))
                .unwrap(),
        );

        assert_eq!(session_token(&headers), Some(token));
    }

    #[test]
    fn no_cookie_means_no_session() {
        assert_eq!(session_token(&HeaderMap::new()), None);
    }

    #[test]
    fn the_removal_cookie_expires_the_session_cookie_at_once() {
        let header = removed_session_cookie();
        let cookie = Cookie::parse(header.to_str().unwrap()).unwrap();

        assert_eq!(cookie.name(), SESSION_COOKIE);
        assert_eq!(cookie.value(), "");
        assert_eq!(cookie.max_age(), Some(Duration::ZERO));
        assert_eq!(cookie.path(), Some("/"));
    }
}
