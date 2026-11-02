//! Notifications: what a push tells a Member about an [`Event`], and the [`PushSender`] that
//! delivers one to a device.

use std::future::Future;

use serde::Serialize;

use super::events::Event;
use super::finance::{ChangedEntry, EntryAction, FinanceEvent};
use super::members::{MemberId, MemberName};
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

    /// Who did what to an Expense or Payment, for `wording`'s reader, with its amount and what
    /// it was for.
    fn finance(change: &FinanceEvent, wording: &Wording<'_>) -> Self {
        let actor = change.actor;
        let verb = match change.action {
            EntryAction::Added => "logged",
            EntryAction::Edited => "edited",
            EntryAction::Deleted => "deleted",
        };
        let (what, amount, description) = match &change.entry {
            ChangedEntry::Expense(_, expense) => (
                format!("{} Expense", wording.whose(expense.payer, actor, "an")),
                expense.amount,
                Some(&expense.description),
            ),
            ChangedEntry::Payment(_, payment) => (
                format!(
                    "{} Payment to {}",
                    wording.whose(payment.sender, actor, "a"),
                    wording.whom(payment.recipient, actor)
                ),
                payment.amount,
                payment.description.as_ref(),
            ),
        };
        let title = match &change.entry {
            ChangedEntry::Payment(_, payment)
                if change.action == EntryAction::Added
                    && payment.sender == actor
                    && payment.recipient == wording.reader =>
            {
                format!("{} sent you a Payment", wording.name(actor))
            }
            _ => format!("{} {verb} {what}", wording.name(actor)),
        };
        let amount = amount.crowns();
        Self {
            title,
            body: description.map_or_else(
                || amount.to_string(),
                |description| format!("{amount} · {}", description.as_str()),
            ),
            opens: Opens::Finance,
            urgency: Urgency::Normal,
        }
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
    /// The notifications `event` makes, naming Members as `crew` does. Members may each be told
    /// it in their own words, as "you" for themselves.
    #[must_use]
    pub fn for_event(event: &Event, crew: &[MemberName]) -> Vec<Self> {
        match event {
            Event::ReminderDue(due) => vec![Self {
                notification: Notification::reminder(due),
                to: due.notify.clone(),
            }],
            Event::Finance(change) => change
                .notify
                .iter()
                .map(|&reader| Self {
                    notification: Notification::finance(change, &Wording { crew, reader }),
                    to: vec![reader],
                })
                .collect(),
        }
    }
}

/// How one reader of a notification hears the Members it names.
struct Wording<'a> {
    crew: &'a [MemberName],
    reader: MemberId,
}

impl Wording<'_> {
    /// `member`'s display name, as the Crew knows it now.
    fn name(&self, member: MemberId) -> &str {
        self.crew
            .iter()
            .find(|named| named.id == member)
            .map_or("Someone", |named| named.display_name.as_str())
    }

    /// Whose an Expense or Payment `actor` changed is: "an"/"a" for the actor's own, "your" for
    /// the reader's, or the owner's name.
    fn whose(&self, owner: MemberId, actor: MemberId, article: &'static str) -> String {
        if owner == actor {
            article.to_owned()
        } else if owner == self.reader {
            "your".to_owned()
        } else {
            format!("{}'s", self.name(owner))
        }
    }

    /// Who a Payment `actor` changed went to: "you", "them" for the actor, or their name.
    fn whom(&self, recipient: MemberId, actor: MemberId) -> String {
        if recipient == self.reader {
            "you".to_owned()
        } else if recipient == actor {
            "them".to_owned()
        } else {
            self.name(recipient).to_owned()
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
    use jiff::civil::date;

    use super::*;
    use crate::domain::finance::money::Amount;
    use crate::domain::finance::{Description, ExpenseId, NewExpense, NewPayment, PaymentId};
    use crate::domain::members::DisplayName;
    use crate::domain::reminders::{ReminderId, ReminderText};

    const JOSEF: MemberId = MemberId::from_stored(1);
    const ONDRA: MemberId = MemberId::from_stored(2);
    const SIMON: MemberId = MemberId::from_stored(3);

    fn crew() -> Vec<MemberName> {
        [(JOSEF, "Josef"), (ONDRA, "Ondra"), (SIMON, "Šimon")]
            .into_iter()
            .map(|(id, name)| MemberName {
                id,
                display_name: DisplayName::from_stored(name.to_owned()),
            })
            .collect()
    }

    fn due(text: &str, late: bool) -> Event {
        Event::ReminderDue(ReminderDue {
            reminder: ReminderId::from_client(1),
            text: ReminderText::parse(text).unwrap(),
            late,
            notify: vec![JOSEF, ONDRA],
        })
    }

    fn expense(payer: MemberId, crowns: i64, description: &str) -> ChangedEntry {
        ChangedEntry::Expense(
            ExpenseId::from_client(1),
            NewExpense {
                payer,
                amount: Amount::new(crowns).unwrap(),
                description: Description::parse(description).unwrap(),
                expense_date: date(2026, 3, 20),
            },
        )
    }

    fn payment(
        sender: MemberId,
        recipient: MemberId,
        crowns: i64,
        description: &str,
    ) -> ChangedEntry {
        ChangedEntry::Payment(
            PaymentId::from_client(1),
            NewPayment {
                sender,
                recipient,
                amount: Amount::new(crowns).unwrap(),
                description: Description::parse_optional(description).unwrap(),
            },
        )
    }

    fn finance(
        entry: ChangedEntry,
        action: EntryAction,
        actor: MemberId,
        notify: &[MemberId],
    ) -> Event {
        Event::Finance(FinanceEvent {
            entry,
            action,
            actor,
            notify: notify.to_vec(),
        })
    }

    /// Each reader of `event`, with the title and body they get.
    fn told(event: &Event) -> Vec<(MemberId, String, String)> {
        Delivery::for_event(event, &crew())
            .into_iter()
            .map(|delivery| {
                let [reader] = delivery.to[..] else {
                    panic!("a Finance notification is worded for one reader");
                };
                (
                    reader,
                    delivery.notification.title,
                    delivery.notification.body,
                )
            })
            .collect()
    }

    fn line(reader: MemberId, title: &str, body: &str) -> (MemberId, String, String) {
        (reader, title.to_owned(), body.to_owned())
    }

    #[test]
    fn a_due_reminder_tells_everyone_its_text() {
        assert_eq!(
            Delivery::for_event(&due("Bottle the stout", false), &crew()),
            [Delivery {
                notification: Notification {
                    title: "Reminder".to_owned(),
                    body: "Bottle the stout".to_owned(),
                    opens: Opens::Reminders,
                    urgency: Urgency::High,
                },
                to: vec![JOSEF, ONDRA],
            }]
        );
    }

    #[test]
    fn a_late_reminder_says_so() {
        let delivery = &Delivery::for_event(&due("Bottle the stout", true), &crew())[0];

        assert_eq!(delivery.notification.body, "(late) Bottle the stout");
    }

    #[test]
    fn an_expense_says_who_did_what_how_much_and_what_for() {
        let cases = [
            (EntryAction::Added, "Ondra logged an Expense"),
            (EntryAction::Edited, "Ondra edited an Expense"),
            (EntryAction::Deleted, "Ondra deleted an Expense"),
        ];
        for (action, title) in cases {
            let event = finance(expense(ONDRA, 1240, "Hops"), action, ONDRA, &[JOSEF, SIMON]);

            assert_eq!(
                told(&event),
                [
                    line(JOSEF, title, "1\u{a0}240\u{a0}Kč · Hops"),
                    line(SIMON, title, "1\u{a0}240\u{a0}Kč · Hops"),
                ],
                "{action:?}"
            );
        }
    }

    #[test]
    fn an_expense_someone_else_paid_names_its_payer() {
        let event = finance(
            expense(ONDRA, 300, "Malt"),
            EntryAction::Added,
            JOSEF,
            &[ONDRA, SIMON],
        );

        assert_eq!(
            told(&event),
            [
                line(ONDRA, "Josef logged your Expense", "300\u{a0}Kč · Malt"),
                line(SIMON, "Josef logged Ondra's Expense", "300\u{a0}Kč · Malt"),
            ]
        );
    }

    #[test]
    fn a_payment_to_the_reader_says_they_were_sent_it() {
        let event = finance(
            payment(ONDRA, JOSEF, 500, ""),
            EntryAction::Added,
            ONDRA,
            &[JOSEF],
        );

        assert_eq!(
            told(&event),
            [line(JOSEF, "Ondra sent you a Payment", "500\u{a0}Kč")]
        );
    }

    #[test]
    fn a_payment_is_worded_for_each_side() {
        let cases = [
            // Its Sender edits or deletes it.
            (
                payment(ONDRA, JOSEF, 500, "Beer money"),
                EntryAction::Edited,
                ONDRA,
                vec![line(
                    JOSEF,
                    "Ondra edited a Payment to you",
                    "500\u{a0}Kč · Beer money",
                )],
            ),
            (
                payment(ONDRA, JOSEF, 500, ""),
                EntryAction::Deleted,
                ONDRA,
                vec![line(JOSEF, "Ondra deleted a Payment to you", "500\u{a0}Kč")],
            ),
            // Its Recipient logs that it came.
            (
                payment(ONDRA, JOSEF, 500, ""),
                EntryAction::Added,
                JOSEF,
                vec![line(
                    ONDRA,
                    "Josef logged your Payment to them",
                    "500\u{a0}Kč",
                )],
            ),
            // The Admin logs it for two others.
            (
                payment(ONDRA, SIMON, 500, ""),
                EntryAction::Added,
                JOSEF,
                vec![
                    line(ONDRA, "Josef logged your Payment to Šimon", "500\u{a0}Kč"),
                    line(SIMON, "Josef logged Ondra's Payment to you", "500\u{a0}Kč"),
                ],
            ),
            // The Admin made Šimon's Payment to Ondra his own: Šimon is on neither side now.
            (
                payment(JOSEF, ONDRA, 500, ""),
                EntryAction::Edited,
                JOSEF,
                vec![
                    line(ONDRA, "Josef edited a Payment to you", "500\u{a0}Kč"),
                    line(SIMON, "Josef edited a Payment to Ondra", "500\u{a0}Kč"),
                ],
            ),
        ];
        for (entry, action, actor, expected) in cases {
            let notify: Vec<_> = expected.iter().map(|(reader, ..)| *reader).collect();

            assert_eq!(
                told(&finance(entry, action, actor, &notify)),
                expected,
                "{action:?} by {actor:?}"
            );
        }
    }

    #[test]
    fn every_finance_notification_opens_finance() {
        let events = [
            finance(
                expense(ONDRA, 300, "Malt"),
                EntryAction::Added,
                ONDRA,
                &[JOSEF],
            ),
            finance(
                payment(ONDRA, JOSEF, 300, ""),
                EntryAction::Deleted,
                ONDRA,
                &[JOSEF],
            ),
        ];
        for event in &events {
            for delivery in Delivery::for_event(event, &crew()) {
                assert_eq!(delivery.notification.opens, Opens::Finance);
                assert_eq!(delivery.notification.urgency, Urgency::Normal);
            }
        }
    }

    #[test]
    fn a_member_the_crew_does_not_name_is_someone() {
        let stranger = MemberId::from_stored(9);
        let event = finance(
            expense(stranger, 300, "Malt"),
            EntryAction::Added,
            stranger,
            &[JOSEF],
        );

        assert_eq!(told(&event)[0].1, "Someone logged an Expense");
    }

    #[test]
    fn the_payload_names_the_tab_to_open() {
        let notification = Delivery::for_event(&due("Bottle \"the\" stout", false), &crew())
            .remove(0)
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
