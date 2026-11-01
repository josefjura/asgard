//! Turning notifications on and off for this device: what the push script posts.

use std::convert::Infallible;
use std::future::ready;

use axum::extract::{FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::header::USER_AGENT;
use axum::http::request::Parts;
use axum::routing::post;
use axum::{Json, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::MemberSession;
use super::{AppError, AppState};
use crate::domain::auth::SessionId;
use crate::domain::push::{Subscription, device_label};
use crate::views::push::{self, PushState, PushUrls};

const SUBSCRIBE_PATH: &str = "/push/subscribe";
const UNSUBSCRIBE_PATH: &str = "/push/unsubscribe";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(SUBSCRIBE_PATH, post(subscribe))
        .route(UNSUBSCRIBE_PATH, post(unsubscribe))
}

/// The notifications control for the device signed in with `session`, as the server knows it.
///
/// # Errors
///
/// Fails if the database does.
pub async fn control(state: &AppState, session: SessionId) -> Result<Markup, AppError> {
    let subscribed = state.push.is_subscribed(session).await?;
    let shown = if subscribed {
        PushState::On
    } else {
        PushState::Off
    };
    Ok(push::control(
        shown,
        &PushUrls {
            public_key: &state.vapid_public_key,
            subscribe: SUBSCRIBE_PATH,
            unsubscribe: UNSUBSCRIBE_PATH,
        },
    ))
}

/// A browser's `PushSubscription.toJSON()`.
#[derive(Deserialize)]
struct SubscriptionJson {
    endpoint: String,
    keys: Keys,
}

#[derive(Deserialize)]
struct Keys {
    p256dh: String,
    auth: String,
}

/// What the requesting device is called, told from its user agent.
struct DeviceLabel(String);

impl<S: Sync> FromRequestParts<S> for DeviceLabel {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Infallible>> + Send {
        let user_agent = parts
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        ready(Ok(Self(device_label(user_agent))))
    }
}

async fn subscribe(
    State(state): State<AppState>,
    MemberSession { member, session }: MemberSession,
    DeviceLabel(label): DeviceLabel,
    Json(body): Json<SubscriptionJson>,
) -> Result<StatusCode, AppError> {
    let subscription = Subscription::parse(&body.endpoint, &body.keys.p256dh, &body.keys.auth)?;
    state.push.subscribe(session, &subscription, &label).await?;
    tracing::info!(member = member.id.get(), "a Member turned on notifications");
    Ok(StatusCode::NO_CONTENT)
}

async fn unsubscribe(
    State(state): State<AppState>,
    MemberSession { member, session }: MemberSession,
) -> Result<StatusCode, AppError> {
    state.push.unsubscribe(session).await?;
    tracing::info!(
        member = member.id.get(),
        "a Member turned off notifications"
    );
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::{CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, USER_AGENT};
    use axum::http::{Method, Request, Response, StatusCode};
    use serde_json::{Value, json};
    use sqlx::SqlitePool;

    use crate::domain::auth::LinkKind;
    use crate::domain::members::DisplayName;
    use crate::domain::push::Device;
    use crate::web::links;
    use crate::web::test_support::{
        PUBLIC_ORIGIN, TestApp, VAPID_PUBLIC_KEY, authenticator, body_text, cookie_pair,
    };

    const ANDROID_CHROME: &str = "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Mobile Safari/537.36";
    const P256DH: &str =
        "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU";
    const AUTH: &str = "CQkJCQkJCQkJCQkJCQkJCQ";

    /// What a browser's `PushSubscription.toJSON()` gives for an endpoint named `name`.
    fn subscription(name: &str) -> Value {
        json!({
            "endpoint": format!("https://push.example/{name}"),
            "expirationTime": null,
            "keys": { "p256dh": P256DH, "auth": AUTH },
        })
    }

    /// A JSON POST, as the push script sends it from `origin`, by a browser holding `cookie`.
    async fn post(
        app: &TestApp,
        uri: &str,
        cookie: Option<&str>,
        origin: &str,
        body: &Value,
    ) -> Response<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(ORIGIN, origin)
            .header(CONTENT_TYPE, "application/json")
            .header(USER_AGENT, ANDROID_CHROME);
        if let Some(cookie) = cookie {
            builder = builder.header(COOKIE, cookie);
        }
        app.send(builder.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn subscribe(app: &TestApp, cookie: &str, body: &Value) -> Response<Body> {
        post(app, "/push/subscribe", Some(cookie), PUBLIC_ORIGIN, body).await
    }

    async fn unsubscribe(app: &TestApp, cookie: &str) -> Response<Body> {
        post(
            app,
            "/push/unsubscribe",
            Some(cookie),
            PUBLIC_ORIGIN,
            &json!({}),
        )
        .await
    }

    async fn devices(app: &TestApp) -> Vec<Device> {
        app.state.push.devices().await.unwrap()
    }

    fn endpoints(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|d| d.subscription.endpoint()).collect()
    }

    #[sqlx::test]
    async fn subscribing_stores_this_devices_subscription(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = subscribe(&app, &cookie, &subscription("phone")).await;

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let devices = devices(&app).await;
        assert_eq!(endpoints(&devices), ["https://push.example/phone"]);
        assert_eq!(devices[0].label, "Android Chrome");
        assert_eq!(devices[0].subscription.p256dh(), P256DH);
        assert_eq!(devices[0].subscription.auth(), AUTH);
    }

    #[sqlx::test]
    async fn subscribing_again_from_the_same_device_does_not_duplicate_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        subscribe(&app, &cookie, &subscription("phone")).await;
        subscribe(&app, &cookie, &subscription("phone")).await;
        subscribe(&app, &cookie, &subscription("renewed")).await;

        assert_eq!(
            endpoints(&devices(&app).await),
            ["https://push.example/renewed"]
        );
    }

    #[sqlx::test]
    async fn each_device_keeps_its_own_subscription(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;

        subscribe(&app, &josef, &subscription("josef")).await;
        subscribe(&app, &ondra, &subscription("ondra")).await;

        assert_eq!(
            endpoints(&devices(&app).await),
            ["https://push.example/josef", "https://push.example/ondra"]
        );
    }

    #[sqlx::test]
    async fn unsubscribing_removes_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        subscribe(&app, &josef, &subscription("josef")).await;
        subscribe(&app, &ondra, &subscription("ondra")).await;

        let response = unsubscribe(&app, &josef).await;

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            endpoints(&devices(&app).await),
            ["https://push.example/ondra"]
        );
    }

    #[sqlx::test]
    async fn logging_out_removes_this_devices_subscription(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        subscribe(&app, &cookie, &subscription("phone")).await;

        app.post_with("/log-out", &cookie).await;

        assert_eq!(devices(&app).await, []);
    }

    #[sqlx::test]
    async fn a_broken_subscription_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let mut insecure = subscription("phone");
        insecure["endpoint"] = json!("http://push.example/phone");

        let response = subscribe(&app, &cookie, &insecure).await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body_text(response).await,
            "This browser's notification settings couldn't be used. Please try again."
        );
        assert_eq!(devices(&app).await, []);
    }

    #[sqlx::test]
    async fn a_signed_out_browser_is_sent_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for (uri, body) in [
            ("/push/subscribe", subscription("phone")),
            ("/push/unsubscribe", json!({})),
        ] {
            let response = post(&app, uri, None, PUBLIC_ORIGIN, &body).await;

            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
            assert_eq!(response.headers()[LOCATION], "/sign-in", "{uri}");
        }
        assert_eq!(devices(&app).await, []);
    }

    #[sqlx::test]
    async fn a_cross_origin_subscribe_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = post(
            &app,
            "/push/subscribe",
            Some(&cookie),
            "https://evil.example",
            &subscription("evil"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(devices(&app).await, []);
    }

    #[sqlx::test]
    async fn a_cross_origin_unsubscribe_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        subscribe(&app, &cookie, &subscription("phone")).await;

        let response = post(
            &app,
            "/push/unsubscribe",
            Some(&cookie),
            "https://evil.example",
            &json!({}),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(devices(&app).await.len(), 1);
    }

    #[sqlx::test]
    async fn settings_shows_whether_this_device_is_subscribed(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let shown =
            |html: &str, state: &str| html.contains(&format!("data-push-state=\"{state}\">"));

        let before = body_text(app.get_with("/settings", &cookie).await).await;
        subscribe(&app, &cookie, &subscription("phone")).await;
        let after = body_text(app.get_with("/settings", &cookie).await).await;

        assert!(before.contains(&format!("data-push-key=\"{VAPID_PUBLIC_KEY}\"")));
        assert!(shown(&before, "off") && !shown(&before, "on"));
        assert!(shown(&after, "on") && !shown(&after, "off"));
    }

    #[sqlx::test]
    async fn another_browser_of_the_same_member_is_not_shown_as_subscribed(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut authenticator = authenticator();
        let phone = app.join_as_admin("Josef", &mut authenticator).await;
        let laptop = cookie_pair(&app.sign_in(&mut authenticator).await);
        subscribe(&app, &phone, &subscription("phone")).await;

        let html = body_text(app.get_with("/settings", &laptop).await).await;

        assert!(html.contains("data-push-state=\"off\">"));
    }

    #[sqlx::test]
    async fn the_welcome_page_holds_the_control(db: SqlitePool) {
        let app = TestApp::new(db);
        app.sign_in_new_admin("Josef").await;
        let token = app
            .state
            .auth
            .create_test_invite(&DisplayName::parse("Ondra").unwrap())
            .await;
        let joined = app
            .join(&links::path(LinkKind::Invite, &token), &mut authenticator())
            .await;
        let cookie = cookie_pair(&joined);
        subscribe(&app, &cookie, &subscription("ondra")).await;

        let html = body_text(app.get_with("/welcome", &cookie).await).await;

        assert!(html.contains(&format!("data-push-key=\"{VAPID_PUBLIC_KEY}\"")));
        assert!(html.contains("data-push-state=\"on\">"));
    }
}
