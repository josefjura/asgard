//! Notifications: what a push tells a Member about an [`Event`], and the [`PushSender`] that
//! delivers one to a device.

use std::future::Future;

use serde::Serialize;

use super::events::Event;
use super::members::MemberId;
use super::push::Subscription;
use super::reminders::ReminderDue;

/// The tab a notification opens when tapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    Finance,
    Shopping,
    Reminders,
}

impl Opens {
    /// The tab's path, the same as its link in the navigation.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Finance => "/",
            Self::Shopping => "/shopping",
            Self::Reminders => "/reminders",
        }
    }
}

/// How urgently a push service should deliver a notification to a device that's saving power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Normal,
    High,
}

/// One push message, as the device shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub opens: Opens,
    pub urgency: Urgency,
}

/// What the service worker receives: everything it needs to show the notification and to open
/// the right tab when it's tapped.
#[derive(Serialize)]
struct Payload<'a> {
    title: &'a str,
    body: &'a str,
    url: &'static str,
}

impl Notification {
    /// The JSON the service worker's `push` handler reads.
    ///
    /// # Panics
    ///
    /// Never: a payload of strings always serializes.
    #[must_use]
    pub fn payload(&self) -> Vec<u8> {
        serde_json::to_vec(&Payload {
            title: &self.title,
            body: &self.body,
            url: self.opens.path(),
        })
        .expect("a payload of strings always serializes")
    }

    fn reminder(due: &ReminderDue) -> Self {
        let text = due.text.as_str();
        Self {
            title: "Reminder".to_owned(),
            body: if due.late {
                format!("(late) {text}")
            } else {
                text.to_owned()
            },
            opens: Opens::Reminders,
            urgency: Urgency::High,
        }
    }
}

/// A notification and the Members whose devices get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub notification: Notification,
    /// Never the Member whose action caused it, unless the event says otherwise (a Reminder
    /// reaches its creator too).
    pub to: Vec<MemberId>,
}

impl Delivery {
    /// The notification `event` makes, if it makes one yet.
    #[must_use]
    pub fn for_event(event: &Event) -> Option<Self> {
        match event {
            Event::ReminderDue(due) => Some(Self {
                notification: Notification::reminder(due),
                to: due.notify.clone(),
            }),
            // Finance notifications come with their own wording (#68).
            Event::Finance(_) => None,
        }
    }
}

/// What became of one push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// The push service no longer knows the subscription (404 or 410), so it never will again.
    Gone,
    /// Anything else. It isn't retried.
    Failed(String),
}

/// Sends one notification to one device.
pub trait PushSender: Clone + Send + Sync + 'static {
    fn send(
        &self,
        subscription: &Subscription,
        notification: &Notification,
    ) -> impl Future<Output = Outcome> + Send;
}

/// A [`PushSender`] for tests: it records every push and answers as told, or never.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc;

    use super::{Notification, Outcome, PushSender};
    use crate::domain::push::Subscription;

    /// A push the fake was asked to send.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Sent {
        pub endpoint: String,
        pub notification: Notification,
    }

    /// What the fake answers a push to an endpoint with; `None` never answers.
    type Answer = dyn Fn(&str) -> Option<Outcome> + Send + Sync;

    /// Answers each push as its [`Answer`] says.
    #[derive(Clone)]
    pub struct FakeSender {
        sent: mpsc::UnboundedSender<Sent>,
        answer: Arc<Answer>,
    }

    impl FakeSender {
        /// A fake that answers with `answer`, and the pushes it was asked to send.
        pub fn new(
            answer: impl Fn(&str) -> Option<Outcome> + Send + Sync + 'static,
        ) -> (Self, mpsc::UnboundedReceiver<Sent>) {
            let (sent, pushes) = mpsc::unbounded_channel();
            let fake = Self {
                sent,
                answer: Arc::new(answer),
            };
            (fake, pushes)
        }

        /// A fake that delivers every push.
        pub fn delivering() -> (Self, mpsc::UnboundedReceiver<Sent>) {
            Self::new(|_| Some(Outcome::Delivered))
        }

        /// A fake whose push service never answers.
        pub fn hanging() -> (Self, mpsc::UnboundedReceiver<Sent>) {
            Self::new(|_| None)
        }
    }

    impl PushSender for FakeSender {
        async fn send(&self, subscription: &Subscription, notification: &Notification) -> Outcome {
            let endpoint = subscription.endpoint().to_owned();
            let answer = (self.answer)(&endpoint);
            // The test may have stopped listening; the push still counts as sent.
            let _ = self.sent.send(Sent {
                endpoint,
                notification: notification.clone(),
            });
            match answer {
                Some(outcome) => outcome,
                None => std::future::pending().await,
            }
        }
    }

    /// Every push sent until none has come for a moment, sorted by endpoint.
    pub async fn drain(pushes: &mut mpsc::UnboundedReceiver<Sent>) -> Vec<Sent> {
        let mut sent = Vec::new();
        while let Ok(Some(push)) =
            tokio::time::timeout(Duration::from_millis(300), pushes.recv()).await
        {
            sent.push(push);
        }
        sent.sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::finance::{EntryAction, EntryId, ExpenseId, FinanceEvent};
    use crate::domain::reminders::{ReminderId, ReminderText};

    fn due(text: &str, late: bool) -> Event {
        Event::ReminderDue(ReminderDue {
            reminder: ReminderId::from_client(1),
            text: ReminderText::parse(text).unwrap(),
            late,
            notify: vec![MemberId::from_stored(1), MemberId::from_stored(2)],
        })
    }

    #[test]
    fn a_due_reminder_tells_everyone_its_text() {
        assert_eq!(
            Delivery::for_event(&due("Bottle the stout", false)),
            Some(Delivery {
                notification: Notification {
                    title: "Reminder".to_owned(),
                    body: "Bottle the stout".to_owned(),
                    opens: Opens::Reminders,
                    urgency: Urgency::High,
                },
                to: vec![MemberId::from_stored(1), MemberId::from_stored(2)],
            })
        );
    }

    #[test]
    fn a_late_reminder_says_so() {
        let delivery = Delivery::for_event(&due("Bottle the stout", true)).unwrap();

        assert_eq!(delivery.notification.body, "(late) Bottle the stout");
    }

    #[test]
    fn finance_makes_no_notification_yet() {
        let event = Event::Finance(FinanceEvent {
            entry: EntryId::Expense(ExpenseId::from_client(1)),
            action: EntryAction::Added,
            actor: MemberId::from_stored(1),
            notify: vec![MemberId::from_stored(2)],
        });

        assert_eq!(Delivery::for_event(&event), None);
    }

    #[test]
    fn the_payload_names_the_tab_to_open() {
        let notification = Delivery::for_event(&due("Bottle \"the\" stout", false))
            .unwrap()
            .notification;

        let payload: serde_json::Value = serde_json::from_slice(&notification.payload()).unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "title": "Reminder",
                "body": "Bottle \"the\" stout",
                "url": "/reminders",
            })
        );
    }
}
