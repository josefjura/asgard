//! Who is asking: the session cookie and the extractors that guard routes.

use axum::extract::FromRequestParts;
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use cookie::time::Duration;
use cookie::{Cookie, SameSite};

use super::{AppError, AppState};
use crate::domain::auth::{SESSION_LIFETIME, SessionToken};
use crate::domain::members;

const SESSION_COOKIE: &str = "asgard_session";

/// The `Set-Cookie` value that keeps `token` in the browser: script can't read it, it only
/// travels over HTTPS, and other sites' requests don't carry it.
pub fn session_cookie(token: &SessionToken) -> HeaderValue {
    let max_age = i64::try_from(SESSION_LIFETIME.as_secs()).unwrap_or(i64::MAX);
    let cookie = Cookie::build((SESSION_COOKIE, token.as_str()))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(Duration::seconds(max_age))
        .build();
    // A token is base64url, which is always a valid header value.
    HeaderValue::try_from(cookie.to_string()).expect("a session cookie is a valid header")
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

/// A signed-in Member. Without a valid session, the request is sent to sign in.
#[derive(Debug, Clone)]
pub struct Member(pub members::Member);

impl FromRequestParts<AppState> for Member {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let sign_in = || AppError::SignInRequired {
            htmx: parts.headers.contains_key("hx-request"),
        };
        let token = session_token(&parts.headers).ok_or_else(sign_in)?;
        state
            .auth
            .session_member(&token)
            .await?
            .map(Self)
            .ok_or_else(sign_in)
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
}
