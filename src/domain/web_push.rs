//! The production [`PushSender`]: Web Push with VAPID, through the browsers' own push services.

use std::sync::Arc;
use std::time::Duration;

use url::Url;
use web_push::{
    ContentEncoding, HyperWebPushClient, PartialVapidSignatureBuilder, SubscriptionInfo,
    VapidSignatureBuilder, WebPushClient, WebPushError, WebPushMessageBuilder,
};

use super::notifications::{Notification, Outcome, PushSender, Urgency};
use super::push::Subscription;

/// How long a push service keeps a notification for a device that's offline, in seconds.
const TIME_TO_LIVE: u32 = 24 * 60 * 60;

/// How long a push service may take to answer before the push counts as failed.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);

/// Signs each push with the VAPID key and sends it over HTTPS.
#[derive(Clone)]
pub struct WebPushSender {
    client: HyperWebPushClient,
    vapid: Arc<PartialVapidSignatureBuilder>,
    /// Who push services contact about our pushes: the app's own URL.
    subject: Arc<str>,
}

#[derive(Debug, thiserror::Error)]
#[error("the VAPID private key is not a P-256 private key")]
pub struct InvalidVapidKey;

impl WebPushSender {
    /// A sender signing with `vapid_private_key` (raw, base64url without padding) on behalf of
    /// the app at `public_url`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidVapidKey`] if the key can't sign.
    pub fn new(vapid_private_key: &str, public_url: &Url) -> Result<Self, InvalidVapidKey> {
        let vapid = VapidSignatureBuilder::from_base64_no_sub(vapid_private_key)
            .map_err(|_| InvalidVapidKey)?;
        Ok(Self {
            client: HyperWebPushClient::new(),
            vapid: Arc::new(vapid),
            subject: public_url.as_str().trim_end_matches('/').into(),
        })
    }

    async fn try_send(
        &self,
        subscription: &Subscription,
        notification: &Notification,
    ) -> Result<(), WebPushError> {
        let info = SubscriptionInfo::new(
            subscription.endpoint(),
            subscription.p256dh(),
            subscription.auth(),
        );
        let mut signature = (*self.vapid).clone().add_sub_info(&info);
        signature.add_claim("sub", &*self.subject);
        let payload = notification.payload();
        let mut message = WebPushMessageBuilder::new(&info);
        message.set_payload(ContentEncoding::Aes128Gcm, &payload);
        message.set_ttl(TIME_TO_LIVE);
        message.set_urgency(match notification.urgency {
            Urgency::Normal => web_push::Urgency::Normal,
            Urgency::High => web_push::Urgency::High,
        });
        message.set_vapid_signature(signature.build()?);
        self.client.send(message.build()?).await
    }
}

impl PushSender for WebPushSender {
    async fn send(&self, subscription: &Subscription, notification: &Notification) -> Outcome {
        match tokio::time::timeout(ANSWER_WITHIN, self.try_send(subscription, notification)).await {
            Ok(Ok(())) => Outcome::Delivered,
            Ok(Err(WebPushError::EndpointNotFound(_) | WebPushError::EndpointNotValid(_))) => {
                Outcome::Gone
            }
            Ok(Err(err)) => Outcome::Failed(err.to_string()),
            Err(_) => Outcome::Failed(format!(
                "no answer within {} seconds",
                ANSWER_WITHIN.as_secs()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_key_that_cannot_sign() {
        let url = Url::parse("https://asgard.test").unwrap();

        assert!(WebPushSender::new("not a key", &url).is_err());
        assert!(WebPushSender::new("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE", &url).is_ok());
    }
}
