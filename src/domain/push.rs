//! Web Push subscriptions: the devices on which each Member turned notifications on.
//!
//! A subscription belongs to the session that made it, so whatever ends the session (logging
//! out, deleting the passkey that started it) removes the subscription too, and an expired
//! session's subscription is never used.

use base64::Engine;
use base64::alphabet::URL_SAFE;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD};
use sqlx::SqlitePool;
use url::Url;

use super::auth::SessionId;
use super::clock::{Clock, unix_seconds};
use super::members::MemberId;
use super::user_agent;

/// The longest endpoint URL a subscription may have, in bytes. Push services' are a few hundred.
pub const ENDPOINT_MAX: usize = 2048;

/// What a device is called when its browser can't be told from its user agent.
const FALLBACK_LABEL: &str = "Device";

/// Browsers send base64url keys, some with padding and some without.
const BASE64URL: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// A browser's push subscription, checked: where to send pushes and the keys to encrypt them
/// with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    endpoint: String,
    p256dh: String,
    auth: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SubscriptionError {
    #[error("the endpoint is not an https URL of at most {ENDPOINT_MAX} bytes")]
    Endpoint,
    #[error("the keys are not an uncompressed P-256 public key and a 16-byte auth secret")]
    Keys,
}

impl Subscription {
    /// Checks the fields of `PushSubscription.toJSON()`: an https endpoint, the browser's
    /// uncompressed P-256 public key and its 16-byte auth secret, both base64url.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Endpoint`] or [`SubscriptionError::Keys`].
    pub fn parse(endpoint: &str, p256dh: &str, auth: &str) -> Result<Self, SubscriptionError> {
        let url = Url::parse(endpoint).map_err(|_| SubscriptionError::Endpoint)?;
        if url.scheme() != "https" || url.host().is_none() || endpoint.len() > ENDPOINT_MAX {
            return Err(SubscriptionError::Endpoint);
        }
        let p256dh = BASE64URL
            .decode(p256dh)
            .map_err(|_| SubscriptionError::Keys)?;
        let auth = BASE64URL
            .decode(auth)
            .map_err(|_| SubscriptionError::Keys)?;
        if p256dh.len() != 65 || p256dh[0] != 0x04 || auth.len() != 16 {
            return Err(SubscriptionError::Keys);
        }
        Ok(Self {
            endpoint: url.into(),
            p256dh: URL_SAFE_NO_PAD.encode(p256dh),
            auth: URL_SAFE_NO_PAD.encode(auth),
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The browser's public key, base64url without padding.
    #[must_use]
    pub fn p256dh(&self) -> &str {
        &self.p256dh
    }

    /// The browser's auth secret, base64url without padding.
    #[must_use]
    pub fn auth(&self) -> &str {
        &self.auth
    }
}

/// A device a push can go to: a subscription whose session is still live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub member: MemberId,
    pub label: String,
    pub subscription: Subscription,
}

/// What a device is called, told from the user agent of the browser that subscribed.
#[must_use]
pub fn device_label(user_agent: &str) -> String {
    user_agent::describe(user_agent).unwrap_or_else(|| FALLBACK_LABEL.to_owned())
}

/// Stores and removes each session's push subscription.
#[derive(Clone)]
pub struct PushSubscriptions {
    db: SqlitePool,
    clock: Clock,
}

impl PushSubscriptions {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock) -> Self {
        Self { db, clock }
    }

    /// Stores `subscription` for `session`'s Member, labelled `label`. It replaces the
    /// session's earlier subscription, and moves the endpoint over if another session had it,
    /// so a device is never subscribed twice.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn subscribe(
        &self,
        session: SessionId,
        subscription: &Subscription,
        label: &str,
    ) -> Result<(), sqlx::Error> {
        let session = session.get();
        let now = unix_seconds(self.clock.now());
        let mut tx = self.db.begin().await?;
        sqlx::query!(
            "DELETE FROM push_subscriptions WHERE session_id = ? OR endpoint = ?",
            session,
            subscription.endpoint,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO push_subscriptions
                 (session_id, member_id, endpoint, p256dh, auth, device_label, created_at)
             SELECT id, member_id, ?, ?, ?, ?, ? FROM sessions WHERE id = ?",
            subscription.endpoint,
            subscription.p256dh,
            subscription.auth,
            label,
            now,
            session,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Removes `session`'s subscription, if it has one.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn unsubscribe(&self, session: SessionId) -> Result<(), sqlx::Error> {
        let session = session.get();
        sqlx::query!(
            "DELETE FROM push_subscriptions WHERE session_id = ?",
            session
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Whether `session` has a subscription.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn is_subscribed(&self, session: SessionId) -> Result<bool, sqlx::Error> {
        let session = session.get();
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM push_subscriptions WHERE session_id = ?) AS "exists: bool""#,
            session,
        )
        .fetch_one(&self.db)
        .await
    }

    /// Every device a push can go to now, oldest subscription first. A subscription whose
    /// session has expired is left out.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn devices(&self) -> Result<Vec<Device>, sqlx::Error> {
        let now = unix_seconds(self.clock.now());
        let rows = sqlx::query!(
            "SELECT push_subscriptions.member_id, device_label, endpoint, p256dh, auth
             FROM push_subscriptions JOIN sessions ON sessions.id = push_subscriptions.session_id
             WHERE sessions.expires_at > ?
             ORDER BY push_subscriptions.created_at, push_subscriptions.id",
            now,
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| Device {
                member: MemberId::from_stored(row.member_id),
                label: row.device_label,
                subscription: Subscription {
                    endpoint: row.endpoint,
                    p256dh: row.p256dh,
                    auth: row.auth,
                },
            })
            .collect())
    }
}

/// Test data for the tests that need a subscription without a browser.
#[cfg(test)]
pub(crate) mod fixtures {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    use super::Subscription;

    /// A subscription at `https://push.example/<name>` with well-formed keys.
    pub fn subscription(name: &str) -> Subscription {
        let mut p256dh = [7_u8; 65];
        p256dh[0] = 0x04;
        Subscription::parse(
            &format!("https://push.example/{name}"),
            &URL_SAFE_NO_PAD.encode(p256dh),
            &URL_SAFE_NO_PAD.encode([9_u8; 16]),
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::fixtures::subscription;
    use super::*;
    use crate::domain::auth::Auth;
    use crate::domain::auth::fixtures::add_session;
    use crate::domain::auth::passkey_fixtures::add_passkey;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::fixtures::add_member;

    /// When the fixture sessions expire, in Unix seconds.
    const EXPIRES_AT: i64 = 1_000;

    fn subscriptions(db: SqlitePool) -> (PushSubscriptions, ManualClock) {
        let clock = ManualClock::starting_at(SystemTime::UNIX_EPOCH);
        (PushSubscriptions::new(db, clock.clock()), clock)
    }

    /// A Member called `name` with one passkey and a session it started.
    async fn signed_in(db: &SqlitePool, name: &str) -> (MemberId, SessionId) {
        let member = add_member(db, name, false).await;
        let passkey = add_passkey(db, member, "Phone", 0).await;
        (member, add_session(db, member, passkey, EXPIRES_AT).await)
    }

    fn endpoints(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|d| d.subscription.endpoint()).collect()
    }

    const KEY: &str =
        "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU";
    const AUTH: &str = "CQkJCQkJCQkJCQkJCQkJCQ";

    #[test]
    fn parses_a_browsers_subscription() {
        let parsed = Subscription::parse("https://push.example/abc", KEY, AUTH).unwrap();

        assert_eq!(parsed.endpoint(), "https://push.example/abc");
        assert_eq!(parsed.p256dh(), KEY);
        assert_eq!(parsed.auth(), AUTH);
    }

    #[test]
    fn accepts_padded_keys_and_stores_them_unpadded() {
        let parsed = Subscription::parse(
            "https://push.example/abc",
            &format!("{KEY}="),
            &format!("{AUTH}=="),
        )
        .unwrap();

        assert_eq!(parsed.p256dh(), KEY);
        assert_eq!(parsed.auth(), AUTH);
    }

    #[test]
    fn refuses_an_endpoint_that_is_not_https() {
        let long = format!("https://push.example/{}", "a".repeat(ENDPOINT_MAX));
        for endpoint in ["http://push.example/abc", "push.example/abc", "", &long] {
            assert_eq!(
                Subscription::parse(endpoint, KEY, AUTH),
                Err(SubscriptionError::Endpoint),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn refuses_keys_of_the_wrong_shape() {
        let compressed = URL_SAFE_NO_PAD.encode([2_u8; 33]);
        let short_auth = URL_SAFE_NO_PAD.encode([9_u8; 8]);
        for (p256dh, auth) in [
            ("not base64!", AUTH),
            (compressed.as_str(), AUTH),
            (KEY, short_auth.as_str()),
            (KEY, ""),
        ] {
            assert_eq!(
                Subscription::parse("https://push.example/abc", p256dh, auth),
                Err(SubscriptionError::Keys),
                "{p256dh} {auth}"
            );
        }
    }

    #[test]
    fn labels_a_device_after_its_browser() {
        assert_eq!(
            device_label(
                "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Mobile Safari/537.36"
            ),
            "Android Chrome"
        );
        assert_eq!(device_label("curl/8.9.1"), "Device");
    }

    #[sqlx::test]
    async fn subscribing_stores_the_device_for_the_sessions_member(db: SqlitePool) {
        let (ondra, session) = signed_in(&db, "Ondra").await;
        let (push, _) = subscriptions(db);

        push.subscribe(session, &subscription("ondra"), "Android Chrome")
            .await
            .unwrap();

        assert!(push.is_subscribed(session).await.unwrap());
        assert_eq!(
            push.devices().await.unwrap(),
            [Device {
                member: ondra,
                label: "Android Chrome".to_owned(),
                subscription: subscription("ondra"),
            }]
        );
    }

    #[sqlx::test]
    async fn subscribing_again_from_the_same_session_replaces_the_subscription(db: SqlitePool) {
        let (_, session) = signed_in(&db, "Ondra").await;
        let (push, _) = subscriptions(db);

        push.subscribe(session, &subscription("old"), "Phone")
            .await
            .unwrap();
        push.subscribe(session, &subscription("old"), "Phone")
            .await
            .unwrap();
        push.subscribe(session, &subscription("new"), "Phone")
            .await
            .unwrap();

        assert_eq!(
            endpoints(&push.devices().await.unwrap()),
            ["https://push.example/new"]
        );
    }

    #[sqlx::test]
    async fn an_endpoint_moves_to_the_session_that_subscribes_it_last(db: SqlitePool) {
        let (_, before) = signed_in(&db, "Ondra").await;
        let (karel, after) = signed_in(&db, "Karel").await;
        let (push, _) = subscriptions(db);

        push.subscribe(before, &subscription("shared"), "Phone")
            .await
            .unwrap();
        push.subscribe(after, &subscription("shared"), "Phone")
            .await
            .unwrap();

        let devices = push.devices().await.unwrap();
        assert_eq!(endpoints(&devices), ["https://push.example/shared"]);
        assert_eq!(devices[0].member, karel);
        assert!(!push.is_subscribed(before).await.unwrap());
    }

    #[sqlx::test]
    async fn unsubscribing_removes_only_that_sessions_subscription(db: SqlitePool) {
        let (_, ondra) = signed_in(&db, "Ondra").await;
        let (_, karel) = signed_in(&db, "Karel").await;
        let (push, _) = subscriptions(db);
        push.subscribe(ondra, &subscription("ondra"), "Phone")
            .await
            .unwrap();
        push.subscribe(karel, &subscription("karel"), "Phone")
            .await
            .unwrap();

        push.unsubscribe(ondra).await.unwrap();
        push.unsubscribe(ondra).await.unwrap();

        assert!(!push.is_subscribed(ondra).await.unwrap());
        assert_eq!(
            endpoints(&push.devices().await.unwrap()),
            ["https://push.example/karel"]
        );
    }

    #[sqlx::test]
    async fn deleting_a_passkey_removes_the_subscriptions_of_its_sessions(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let lost = add_passkey(&db, ondra, "Lost phone", 0).await;
        let kept = add_passkey(&db, ondra, "Laptop", 0).await;
        let lost_session = add_session(&db, ondra, lost, EXPIRES_AT).await;
        let kept_session = add_session(&db, ondra, kept, EXPIRES_AT).await;
        let (push, _) = subscriptions(db.clone());
        push.subscribe(lost_session, &subscription("lost"), "Phone")
            .await
            .unwrap();
        push.subscribe(kept_session, &subscription("kept"), "Laptop")
            .await
            .unwrap();

        let (_, clock) = subscriptions(db.clone());
        let members = crate::domain::members::Members::new(db.clone());
        let auth = Auth::new(
            db,
            clock.clock(),
            members,
            &Url::parse("https://asgard.test").unwrap(),
        )
        .unwrap();
        auth.delete_passkey(ondra, lost).await.unwrap();

        assert_eq!(
            endpoints(&push.devices().await.unwrap()),
            ["https://push.example/kept"]
        );
    }

    #[sqlx::test]
    async fn an_expired_sessions_subscription_is_not_used(db: SqlitePool) {
        let (_, session) = signed_in(&db, "Ondra").await;
        let (push, clock) = subscriptions(db);
        push.subscribe(session, &subscription("ondra"), "Phone")
            .await
            .unwrap();

        clock.set(SystemTime::UNIX_EPOCH + Duration::from_secs(EXPIRES_AT.unsigned_abs()));

        assert_eq!(push.devices().await.unwrap(), []);
    }
}
