//! Routes, extractors, forms and htmx helpers: parse input, call a service, render a view.

mod access;
mod error;
#[cfg(debug_assertions)]
mod gallery;
mod health;
mod htmx;
pub mod invite;
mod name_form;
mod origin;
mod pages;
mod passkeys;
mod registration;
mod settings;
mod sign_in;
mod static_files;
pub mod welcome;

use axum::Router;
use axum::middleware;
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;
use url::Url;
use webauthn_rs::prelude::WebauthnError;

use crate::assets;
use crate::domain::auth::Auth;
use crate::domain::clock::Clock;
use crate::domain::members::Members;
use crate::views::nav::Tab;

pub use access::Member;
pub use error::AppError;
pub use origin::PublicOrigin;

/// Everything routes need, built once at startup.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub clock: Clock,
    pub public_origin: PublicOrigin,
    pub auth: Auth,
    pub members: Members,
}

impl AppState {
    /// Builds every service for the app served at `public_url`.
    ///
    /// # Errors
    ///
    /// Fails if `public_url` can't be a WebAuthn relying party.
    pub fn new(db: SqlitePool, clock: Clock, public_url: &Url) -> Result<Self, WebauthnError> {
        let members = Members::new(db.clone());
        Ok(Self {
            auth: Auth::new(db.clone(), clock.clone(), members.clone(), public_url)?,
            members,
            public_origin: PublicOrigin::of(public_url),
            db,
            clock,
        })
    }
}

pub fn router(state: AppState) -> Router {
    with_layers(routes(), state)
}

fn routes() -> Router<AppState> {
    let mut router = Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/offline", get(pages::offline))
        .route("/static/{file}", get(static_files::hashed))
        .route(
            assets::SERVICE_WORKER.url,
            get(static_files::service_worker),
        )
        .route(assets::MANIFEST.url, get(static_files::manifest))
        .merge(sign_in::routes())
        .merge(invite::routes())
        .merge(welcome::routes())
        .merge(settings::routes())
        .merge(passkeys::routes());
    for tab in Tab::ALL {
        router = router.route(tab.href(), pages::tab(tab));
    }
    #[cfg(debug_assertions)]
    {
        router = router.route("/dev/gallery", get(gallery::show));
    }
    router
}

fn with_layers(routes: Router<AppState>, state: AppState) -> Router {
    routes
        .layer(middleware::from_fn(access::renew_session_cookie))
        .layer(middleware::from_fn_with_state(
            state.public_origin.clone(),
            origin::require_public_origin,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::time::{Duration, SystemTime};

    use axum::Router;
    use axum::body::Body;
    use axum::http::header::{CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE};
    use axum::http::{Method, Request, Response, StatusCode};
    use http_body_util::BodyExt;
    use sqlx::SqlitePool;
    use tower::ServiceExt;
    use url::Url;
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;
    use webauthn_rs::prelude::{
        Base64UrlSafeData, CreationChallengeResponse, PublicKeyCredential,
        RegisterPublicKeyCredential, RequestChallengeResponse,
    };
    use webauthn_rs_proto::AllowCredentials;

    use super::{AppState, routes, with_layers};
    use crate::domain::clock::ManualClock;
    use crate::domain::members::DisplayName;

    /// The origin every test app is served from.
    pub const PUBLIC_ORIGIN: &str = "https://asgard.test";

    /// Where a test app's clock starts: 2026-01-01T00:00:00Z.
    pub fn clock_start() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(490_896)
    }

    /// A software passkey: real WebAuthn, with its keys held in memory.
    ///
    /// `SoftPasskey` can't store discoverable credentials, so this keeps what a discoverable one
    /// would: each credential it made and the user handle it was made for. Signing in then
    /// picks a credential and hands back its user handle, as a phone does. The handle isn't
    /// signed, so the server's checks run unchanged.
    pub struct Authenticator {
        soft: WebauthnAuthenticator<SoftPasskey>,
        discoverable: Vec<Discoverable>,
    }

    /// A credential as a discoverable passkey keeps it on the device.
    #[derive(Clone)]
    struct Discoverable {
        credential_id: Base64UrlSafeData,
        user_handle: Base64UrlSafeData,
    }

    impl Authenticator {
        /// Creates a passkey for the registration `options`, as a browser at `origin` would.
        pub fn register(
            &mut self,
            origin: &str,
            mut options: CreationChallengeResponse,
        ) -> RegisterPublicKeyCredential {
            let user_handle = options.public_key.user.id.clone();
            if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
                selection.require_resident_key = false;
            }
            let credential = self
                .soft
                .do_registration(Url::parse(origin).unwrap(), options)
                .unwrap();
            self.discoverable.push(Discoverable {
                credential_id: credential.raw_id.clone(),
                user_handle,
            });
            credential
        }

        /// Answers the sign-in `options` with the newest passkey, as a browser at `origin` would.
        pub fn authenticate(
            &mut self,
            origin: &str,
            mut options: RequestChallengeResponse,
        ) -> PublicKeyCredential {
            let Discoverable {
                credential_id,
                user_handle,
            } = self
                .discoverable
                .last()
                .expect("the authenticator has a passkey")
                .clone();
            assert!(
                options.public_key.allow_credentials.is_empty(),
                "signing in lets the device choose its passkey"
            );
            options.public_key.allow_credentials = vec![AllowCredentials {
                type_: "public-key".to_owned(),
                id: credential_id,
                transports: None,
            }];
            let mut credential = self
                .soft
                .do_authentication(Url::parse(origin).unwrap(), options)
                .unwrap();
            credential.response.user_handle = Some(user_handle);
            credential
        }
    }

    pub fn authenticator() -> Authenticator {
        Authenticator {
            soft: WebauthnAuthenticator::new(SoftPasskey::new(true)),
            discoverable: Vec::new(),
        }
    }

    /// The app's router with a clock the test controls.
    pub struct TestApp {
        router: Router,
        pub clock: ManualClock,
        pub state: AppState,
    }

    impl TestApp {
        pub fn new(db: SqlitePool) -> Self {
            Self::with_routes(db, Router::new())
        }

        /// The app plus `extra` routes, behind the same middleware.
        pub fn with_routes(db: SqlitePool, extra: Router<AppState>) -> Self {
            let clock = ManualClock::starting_at(clock_start());
            let public_url = Url::parse(PUBLIC_ORIGIN).unwrap();
            let state = AppState::new(db, clock.clock(), &public_url).unwrap();
            Self {
                router: with_layers(routes().merge(extra), state.clone()),
                clock,
                state,
            }
        }

        pub async fn send(&self, request: Request<Body>) -> Response<Body> {
            self.router.clone().oneshot(request).await.unwrap()
        }

        pub async fn get(&self, uri: &str) -> Response<Body> {
            self.send(request(Method::GET, uri, None)).await
        }

        /// A GET from a browser that holds `cookie`.
        pub async fn get_with(&self, uri: &str, cookie: &str) -> Response<Body> {
            let mut request = request(Method::GET, uri, None);
            request
                .headers_mut()
                .insert(COOKIE, cookie.parse().unwrap());
            self.send(request).await
        }

        /// A POST from one of our own pages by a browser that holds `cookie`, as a form sends it.
        pub async fn post_with(&self, uri: &str, cookie: &str) -> Response<Body> {
            let mut request = request(Method::POST, uri, Some(PUBLIC_ORIGIN));
            request
                .headers_mut()
                .insert(COOKIE, cookie.parse().unwrap());
            self.send(request).await
        }

        /// A JSON POST from one of our own pages, the way the passkey script sends it.
        pub async fn post_json(&self, uri: &str, body: &serde_json::Value) -> Response<Body> {
            self.send(json_request(uri, body)).await
        }

        /// A JSON POST from the passkey script on a browser that holds `cookie`.
        pub async fn post_json_with(
            &self,
            uri: &str,
            cookie: &str,
            body: &serde_json::Value,
        ) -> Response<Body> {
            let mut request = json_request(uri, body);
            request
                .headers_mut()
                .insert(COOKIE, cookie.parse().unwrap());
            self.send(request).await
        }

        /// The path of a fresh bootstrap Admin Invite link for `name`.
        pub async fn admin_invite(&self, name: &str) -> String {
            let token = self
                .state
                .auth
                .create_admin_invite(&DisplayName::parse(name).unwrap())
                .await
                .unwrap();
            super::invite::path(&token)
        }

        /// Asks to register a passkey through the Invite link at `invite_path`, the way the
        /// invite page's script does. Any answer but OK comes back as the error.
        pub async fn start_joining(
            &self,
            invite_path: &str,
        ) -> Result<Started, Box<Response<Body>>> {
            let response = self
                .post_json(&format!("{invite_path}/start"), &serde_json::json!({}))
                .await;
            if response.status() != StatusCode::OK {
                return Err(Box::new(response));
            }
            let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
            Ok(Started {
                options: serde_json::from_value(body["options"].clone()).unwrap(),
                ceremony: body["ceremony"].clone(),
            })
        }

        /// Creates the passkey on `authenticator`, as a browser at `origin` would, and sends it.
        pub async fn finish_joining(
            &self,
            invite_path: &str,
            started: Started,
            authenticator: &mut Authenticator,
            origin: &str,
        ) -> Response<Body> {
            let credential = authenticator.register(origin, started.options);
            self.post_json(
                &format!("{invite_path}/finish"),
                &serde_json::json!({ "ceremony": started.ceremony, "credential": credential }),
            )
            .await
        }

        /// Registers a passkey on `authenticator` through the Invite link at `invite_path` and
        /// returns the last response.
        pub async fn join(
            &self,
            invite_path: &str,
            authenticator: &mut Authenticator,
        ) -> Response<Body> {
            match self.start_joining(invite_path).await {
                Ok(started) => {
                    self.finish_joining(invite_path, started, authenticator, PUBLIC_ORIGIN)
                        .await
                }
                Err(response) => *response,
            }
        }

        /// Bootstraps an Admin called `name` who has been through the welcome page, and returns
        /// their browser's session cookie.
        pub async fn sign_in_new_admin(&self, name: &str) -> String {
            self.join_as_admin(name, &mut authenticator()).await
        }

        /// Bootstraps an Admin called `name` whose passkey is on `authenticator`, presses "Got
        /// it" on the welcome page, and returns the session cookie of the browser they joined
        /// from.
        pub async fn join_as_admin(&self, name: &str, authenticator: &mut Authenticator) -> String {
            let invite = self.admin_invite(name).await;
            let response = self.join(&invite, authenticator).await;
            assert_eq!(response.status(), StatusCode::OK, "joining failed");
            let cookie = cookie_pair(&response);
            let got_it = self.post_with("/welcome/done", &cookie).await;
            assert_eq!(got_it.status(), StatusCode::SEE_OTHER, "\"Got it\" failed");
            cookie
        }

        /// Signs in with `authenticator`'s passkey, the way the sign-in page's script does,
        /// and returns the last response.
        pub async fn sign_in(&self, authenticator: &mut Authenticator) -> Response<Body> {
            let assertion = self.sign_in_assertion(authenticator).await;
            self.post_json("/sign-in/finish", &assertion).await
        }

        /// Starts signing in and answers with `authenticator`'s passkey: the body the script
        /// would send to finish.
        pub async fn sign_in_assertion(
            &self,
            authenticator: &mut Authenticator,
        ) -> serde_json::Value {
            let response = self
                .post_json("/sign-in/start", &serde_json::json!({}))
                .await;
            assert_eq!(response.status(), StatusCode::OK, "starting failed");
            let body: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
            let options = serde_json::from_value(body["options"].clone()).unwrap();
            let credential = authenticator.authenticate(PUBLIC_ORIGIN, options);
            serde_json::json!({ "ceremony": body["ceremony"], "credential": credential })
        }
    }

    /// What the passkey script receives when it starts a registration.
    pub struct Started {
        pub ceremony: serde_json::Value,
        pub options: CreationChallengeResponse,
    }

    /// The `name=value` a browser sends back for the cookie `response` sets.
    pub fn cookie_pair(response: &Response<Body>) -> String {
        let set_cookie = response.headers()[SET_COOKIE].to_str().unwrap();
        set_cookie.split(';').next().unwrap().to_owned()
    }

    /// A JSON POST from our own origin.
    pub fn json_request(uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(ORIGIN, PUBLIC_ORIGIN)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// A request with an empty body, sent with `origin` as its `Origin` header, or none.
    pub fn request(method: Method, uri: &str, origin: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }
        builder.body(Body::empty()).unwrap()
    }

    pub async fn get(db: SqlitePool, uri: &str) -> Response<Body> {
        TestApp::new(db).get(uri).await
    }

    pub async fn body_text(response: Response<Body>) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::Router;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing;
    use sqlx::SqlitePool;

    use super::AppState;
    use super::test_support::{TestApp, body_text, clock_start, get};

    /// CI runs the suite with `--release` too, where this proves the gallery is absent.
    #[sqlx::test]
    async fn gallery_exists_only_in_debug_builds(db: SqlitePool) {
        let response = get(db, "/dev/gallery").await;

        let expected = if cfg!(debug_assertions) {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(response.status(), expected);
    }

    #[sqlx::test]
    async fn handlers_read_the_time_the_test_app_sets(db: SqlitePool) {
        let probe = Router::new().route(
            "/now",
            routing::get(|State(state): State<AppState>| async move {
                let since_start = state.clock.now().duration_since(clock_start()).unwrap();
                since_start.as_secs().to_string()
            }),
        );
        let app = TestApp::with_routes(db, probe);

        assert_eq!(body_text(app.get("/now").await).await, "0");

        app.clock.advance(Duration::from_secs(90));
        assert_eq!(body_text(app.get("/now").await).await, "90");

        app.clock.set(clock_start() + Duration::from_hours(24));
        assert_eq!(body_text(app.get("/now").await).await, "86400");
    }
}
