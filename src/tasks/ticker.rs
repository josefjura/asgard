//! The Reminder ticker: sends each Reminder when it's due.
//!
//! The loop only decides when to look. It sleeps until the soonest pending Reminder is due and
//! wakes early when Reminders change; the sending is [`Reminders::send_due`]. It looks once
//! straight away, so Reminders that fell due while the server was down go out late.

use std::time::Duration;

use crate::domain::reminders::Reminders;

/// The longest the ticker sleeps before looking again, in case the system clock jumps while it
/// sleeps.
const LONGEST_SLEEP: Duration = Duration::from_hours(1);

/// How long the ticker waits before trying again after the database failed.
const RETRY_AFTER: Duration = Duration::from_mins(1);

/// Sends Reminders as they fall due, forever. Run one ticker per database.
pub async fn run(reminders: Reminders) {
    loop {
        let sleep = match tick(&reminders).await {
            Ok(Some(until_due)) => until_due.min(LONGEST_SLEEP),
            Ok(None) => LONGEST_SLEEP,
            Err(err) => {
                tracing::error!(error = %err, "the Reminder ticker can't reach the database");
                RETRY_AFTER
            }
        };
        tokio::select! {
            () = tokio::time::sleep(sleep) => {}
            () = reminders.changed() => {}
        }
    }
}

/// Sends what is due, and answers how long until the next pending Reminder is due, if any is.
async fn tick(reminders: &Reminders) -> Result<Option<Duration>, sqlx::Error> {
    let sent = reminders.send_due().await?;
    if sent > 0 {
        tracing::info!(sent, "sent due Reminders");
    }
    reminders.until_next_due().await
}

#[cfg(test)]
mod tests {
    use std::convert::identity;
    use std::time::{Duration, SystemTime};

    use sqlx::SqlitePool;
    use tokio::sync::broadcast;
    use tokio::time::timeout;

    use super::*;
    use crate::domain::channel::Channel;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::MemberId;
    use crate::domain::members::fixtures::add_member;
    use crate::domain::reminders::{DueAt, NewReminder, ReminderDue, ReminderText};

    /// 2026-10-24T08:00:00Z: 10:00 in Prague.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    async fn create(reminders: &Reminders, creator: MemberId, text: &str, due: &str) {
        let reminder = NewReminder {
            text: ReminderText::parse(text).unwrap(),
            due_at: DueAt::parse(due).unwrap(),
        };
        reminders.create(creator, &reminder).await.unwrap();
    }

    async fn next(events: &mut broadcast::Receiver<ReminderDue>) -> Option<ReminderDue> {
        timeout(Duration::from_secs(5), events.recv())
            .await
            .ok()?
            .ok()
    }

    fn reminders(
        db: SqlitePool,
        clock: &ManualClock,
    ) -> (Reminders, broadcast::Receiver<ReminderDue>) {
        let channel = Channel::new();
        let events = channel.subscribe();
        let reminders = Reminders::new(db, clock.clock(), channel.emitter(identity));
        (reminders, events)
    }

    #[sqlx::test]
    async fn on_starting_it_sends_what_fell_due_while_the_server_was_down(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let clock = ManualClock::starting_at(saturday_morning());
        let (reminders, mut events) = reminders(db, &clock);
        create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;
        clock.advance(Duration::from_hours(24));

        tokio::spawn(run(reminders));

        let due = next(&mut events)
            .await
            .expect("the overdue Reminder is sent");
        assert_eq!(due.text.as_str(), "Bottle the stout");
        assert!(due.late);
    }

    #[sqlx::test]
    async fn creating_an_earlier_reminder_wakes_the_ticker(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let clock = ManualClock::starting_at(saturday_morning());
        let (reminders, mut events) = reminders(db, &clock);
        create(&reminders, josef, "Later", "2026-10-24T11:00").await;
        tokio::spawn(run(reminders.clone()));
        // Give it time to look and go to sleep until "Later" is due, an hour from now.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Due a second from now: unless the ticker wakes and works out its sleep again, it
        // sleeps through it.
        create(&reminders, josef, "Sooner", "2026-10-24T10:00:01").await;
        clock.advance(Duration::from_secs(2));

        let due = next(&mut events)
            .await
            .expect("the sooner Reminder is sent");
        assert_eq!(due.text.as_str(), "Sooner");
        assert!(!due.late);
    }
}
