//! The notifier: turns each [`Event`] into a notification and pushes it to the devices of the
//! Members it's for.
//!
//! Each event is delivered on its own task and each device's push runs concurrently, so a slow
//! or hung push service holds up nothing but its own push. Delivery is best-effort: a failed
//! push is logged and not retried, and a subscription its push service says is gone is
//! forgotten.

use std::sync::Arc;

use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinSet;

use crate::domain::events::Event;
use crate::domain::members::Members;
use crate::domain::notifications::{Delivery, Outcome, PushSender};
use crate::domain::push::PushSubscriptions;

/// Delivers every event that arrives on `events` until the channel closes, naming Members as
/// `members` does.
pub async fn run<S: PushSender>(
    mut events: broadcast::Receiver<Event>,
    members: Members,
    subscriptions: PushSubscriptions,
    sender: S,
) {
    loop {
        match events.recv().await {
            Ok(event) => {
                tokio::spawn(deliver(
                    event,
                    members.clone(),
                    subscriptions.clone(),
                    sender.clone(),
                ));
            }
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "the notifier fell behind and dropped events");
            }
            Err(RecvError::Closed) => return,
        }
    }
}

/// Pushes each notification `event` makes to every device of the Members it's for.
async fn deliver<S: PushSender>(
    event: Event,
    members: Members,
    subscriptions: PushSubscriptions,
    sender: S,
) {
    let (crew, devices) = match tokio::try_join!(members.all(), subscriptions.devices()) {
        Ok(found) => found,
        Err(err) => {
            tracing::error!(error = %err, "the notifier can't reach the database");
            return;
        }
    };
    let mut pushes = JoinSet::new();
    for delivery in Delivery::for_event(&event, &crew) {
        let notification = Arc::new(delivery.notification);
        for device in devices
            .iter()
            .filter(|device| delivery.to.contains(&device.member))
        {
            let sender = sender.clone();
            let device = device.clone();
            let notification = Arc::clone(&notification);
            pushes.spawn(async move {
                let outcome = sender.send(&device.subscription, &notification).await;
                (device, outcome)
            });
        }
    }
    while let Some(pushed) = pushes.join_next().await {
        let (device, outcome) = match pushed {
            Ok(done) => done,
            Err(err) => {
                tracing::error!(error = %err, "a push task failed");
                continue;
            }
        };
        match outcome {
            Outcome::Delivered => {}
            Outcome::Gone => {
                tracing::info!(
                    member = device.member.get(),
                    device = device.label,
                    "a push subscription is gone, forgetting it"
                );
                if let Err(err) = subscriptions.forget(device.subscription.endpoint()).await {
                    tracing::error!(error = %err, "can't forget a gone push subscription");
                }
            }
            Outcome::Failed(reason) => {
                tracing::warn!(
                    member = device.member.get(),
                    device = device.label,
                    reason,
                    "a push failed"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use sqlx::SqlitePool;

    use super::*;
    use crate::domain::auth::SessionId;
    use crate::domain::auth::fixtures::add_session;
    use crate::domain::auth::passkey_fixtures::add_passkey;
    use crate::domain::channel::{Channel, Emitter};
    use jiff::civil::date;

    use crate::domain::clock::ManualClock;
    use crate::domain::finance::money::Amount;
    use crate::domain::finance::{Description, Finance, NewExpense, NewPayment};
    use crate::domain::members::MemberId;
    use crate::domain::members::fixtures::add_member;
    use crate::domain::notifications::fixtures::{FakeSender, Sent, drain};
    use crate::domain::notifications::{Notification, Opens, Urgency};
    use crate::domain::push::fixtures::subscription;
    use crate::domain::reminders::{ReminderDue, ReminderId, ReminderText};

    /// A Crew whose devices the notifier pushes to.
    struct Crew {
        members: Members,
        subscriptions: PushSubscriptions,
        finance: Finance,
        due: Emitter<ReminderDue>,
        channel: Channel<Event>,
    }

    impl Crew {
        fn new(db: SqlitePool) -> Self {
            let clock = ManualClock::starting_at(SystemTime::UNIX_EPOCH);
            let channel = Channel::new();
            // After the fixture Members joined, so Finance takes their Expenses.
            let april = ManualClock::starting_at(
                date(2026, 4, 1)
                    .at(12, 0, 0, 0)
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .unwrap()
                    .timestamp()
                    .into(),
            );
            Self {
                members: Members::new(db.clone()),
                finance: Finance::new(db.clone(), april.clock(), channel.emitter(Event::Finance)),
                subscriptions: PushSubscriptions::new(db, clock.clock()),
                due: channel.emitter(Event::ReminderDue),
                channel,
            }
        }

        /// Starts the notifier with `sender`.
        fn notify_with(&self, sender: FakeSender) {
            tokio::spawn(run(
                self.channel.subscribe(),
                self.members.clone(),
                self.subscriptions.clone(),
                sender,
            ));
        }

        /// Signs `member` in on a new device subscribed at `https://push.example/<device>`.
        async fn subscribe(&self, db: &SqlitePool, member: MemberId, device: &str) {
            let passkey = add_passkey(db, member, device, 0).await;
            let session: SessionId = add_session(db, member, passkey, 1_000).await;
            self.subscriptions
                .subscribe(session, &subscription(device), device)
                .await
                .unwrap();
        }

        fn reminder_due(&self, text: &str, late: bool, notify: &[MemberId]) {
            self.due.emit(ReminderDue {
                reminder: ReminderId::from_client(1),
                text: ReminderText::parse(text).unwrap(),
                late,
                notify: notify.to_vec(),
            });
        }

        async fn endpoints(&self) -> Vec<String> {
            self.subscriptions
                .devices()
                .await
                .unwrap()
                .into_iter()
                .map(|device| device.subscription.endpoint().to_owned())
                .collect()
        }
    }

    fn reminder(body: &str) -> Notification {
        Notification {
            title: "Reminder".to_owned(),
            body: body.to_owned(),
            opens: Opens::Reminders,
            urgency: Urgency::High,
        }
    }

    fn sent(device: &str, notification: &Notification) -> Sent {
        Sent {
            endpoint: format!("https://push.example/{device}"),
            notification: notification.clone(),
        }
    }

    #[sqlx::test]
    async fn a_due_reminder_reaches_every_device_of_every_member(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let karel = add_member(&db, "Karel", false).await;
        crew.subscribe(&db, josef, "josef-phone").await;
        crew.subscribe(&db, josef, "josef-laptop").await;
        crew.subscribe(&db, ondra, "ondra-phone").await;
        let (sender, mut pushes) = FakeSender::delivering();
        crew.notify_with(sender);

        // Karel has no device subscribed, and is skipped.
        crew.reminder_due("Bottle the stout", false, &[josef, ondra, karel]);

        let expected = reminder("Bottle the stout");
        assert_eq!(
            drain(&mut pushes).await,
            [
                sent("josef-laptop", &expected),
                sent("josef-phone", &expected),
                sent("ondra-phone", &expected),
            ]
        );
    }

    #[sqlx::test]
    async fn only_the_members_named_hear_of_it(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        crew.subscribe(&db, josef, "josef").await;
        crew.subscribe(&db, ondra, "ondra").await;
        let (sender, mut pushes) = FakeSender::delivering();
        crew.notify_with(sender);

        crew.reminder_due("Bottle the stout", false, &[ondra]);

        let endpoints: Vec<_> = drain(&mut pushes)
            .await
            .into_iter()
            .map(|push| push.endpoint)
            .collect();
        assert_eq!(endpoints, ["https://push.example/ondra"]);
    }

    #[sqlx::test]
    async fn a_late_reminder_says_it_is_late(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        crew.subscribe(&db, josef, "josef").await;
        let (sender, mut pushes) = FakeSender::delivering();
        crew.notify_with(sender);

        crew.reminder_due("Bottle the stout", true, &[josef]);

        assert_eq!(
            drain(&mut pushes).await,
            [sent("josef", &reminder("(late) Bottle the stout"))]
        );
    }

    #[sqlx::test]
    async fn a_gone_subscription_is_forgotten_and_a_failed_one_kept(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        crew.subscribe(&db, josef, "gone").await;
        crew.subscribe(&db, josef, "failing").await;
        crew.subscribe(&db, josef, "fine").await;
        let (sender, mut pushes) = FakeSender::new(|endpoint| {
            Some(match endpoint {
                "https://push.example/gone" => Outcome::Gone,
                "https://push.example/failing" => Outcome::Failed("500".to_owned()),
                _ => Outcome::Delivered,
            })
        });
        crew.notify_with(sender);

        crew.reminder_due("Bottle the stout", false, &[josef]);

        assert_eq!(drain(&mut pushes).await.len(), 3);
        assert_eq!(
            crew.endpoints().await,
            ["https://push.example/failing", "https://push.example/fine"]
        );
    }

    #[sqlx::test]
    async fn a_push_service_that_never_answers_holds_up_no_other_event(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        crew.subscribe(&db, josef, "josef").await;
        let (sender, mut pushes) = FakeSender::hanging();
        crew.notify_with(sender);

        crew.reminder_due("First", false, &[josef]);
        crew.reminder_due("Second", false, &[josef]);

        let bodies: Vec<_> = tokio::time::timeout(Duration::from_secs(5), async {
            let mut bodies = Vec::new();
            while bodies.len() < 2 {
                bodies.push(pushes.recv().await.unwrap().notification.body);
            }
            bodies.sort();
            bodies
        })
        .await
        .expect("both Reminders are pushed while the other's push hangs");
        assert_eq!(bodies, ["First", "Second"]);
    }

    fn finance(title: &str, body: &str) -> Notification {
        Notification {
            title: title.to_owned(),
            body: body.to_owned(),
            opens: Opens::Finance,
            urgency: Urgency::Normal,
        }
    }

    #[sqlx::test]
    async fn an_expense_reaches_its_split_but_not_the_actor(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        crew.subscribe(&db, josef, "josef").await;
        crew.subscribe(&db, ondra, "ondra").await;
        crew.subscribe(&db, simon, "simon").await;
        let (sender, mut pushes) = FakeSender::delivering();
        crew.notify_with(sender);

        crew.finance
            .log_expense(
                ondra,
                &NewExpense {
                    payer: ondra,
                    amount: Amount::new(1240).unwrap(),
                    description: Description::parse("Hops").unwrap(),
                    expense_date: date(2026, 3, 20),
                },
            )
            .await
            .unwrap();

        let expected = finance("Ondra logged an Expense", "1\u{a0}240\u{a0}Kč · Hops");
        assert_eq!(
            drain(&mut pushes).await,
            [sent("josef", &expected), sent("simon", &expected)]
        );
    }

    #[sqlx::test]
    async fn a_payment_reaches_only_its_other_side(db: SqlitePool) {
        let crew = Crew::new(db.clone());
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        crew.subscribe(&db, josef, "josef-laptop").await;
        crew.subscribe(&db, josef, "josef-phone").await;
        crew.subscribe(&db, ondra, "ondra").await;
        crew.subscribe(&db, simon, "simon").await;
        let (sender, mut pushes) = FakeSender::delivering();
        crew.notify_with(sender);

        crew.finance
            .log_payment(
                ondra,
                &NewPayment {
                    sender: ondra,
                    recipient: josef,
                    amount: Amount::new(500).unwrap(),
                    description: None,
                },
            )
            .await
            .unwrap();

        let expected = finance("Ondra sent you a Payment", "500\u{a0}Kč");
        assert_eq!(
            drain(&mut pushes).await,
            [
                sent("josef-laptop", &expected),
                sent("josef-phone", &expected)
            ]
        );
    }
}
