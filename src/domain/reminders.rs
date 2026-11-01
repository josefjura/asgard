//! Reminders: a text the whole Crew is notified about at a given date and time. Each is pending
//! until it is sent.

use jiff::civil::DateTime;
use sqlx::SqlitePool;

use crate::domain::clock::{
    Clock, crew_time_of_unix_seconds, unix_seconds, unix_seconds_at_crew_time,
};
use crate::domain::members::{DisplayName, MemberId};

/// The longest text of a Reminder, in characters.
pub const TEXT_MAX: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReminderId(i64);

impl ReminderId {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// What a Reminder says: trimmed, non-blank and at most [`TEXT_MAX`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderText(String);

impl ReminderText {
    /// Trims `input` and checks it can be a Reminder's text.
    ///
    /// # Errors
    ///
    /// Returns [`TextError::Blank`] or [`TextError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, TextError> {
        let text = input.trim();
        if text.is_empty() {
            Err(TextError::Blank)
        } else if text.chars().count() > TEXT_MAX {
            Err(TextError::TooLong)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("the text is blank")]
    Blank,
    #[error("the text is longer than {TEXT_MAX} characters")]
    TooLong,
}

/// When a Reminder is due: an instant, entered and shown as a date and time in the Crew's time
/// zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DueAt(i64);

impl DueAt {
    /// The instant the clock in the Crew's time zone shows `input`, a date and time such as a
    /// `datetime-local` field sends: `2026-11-07T10:00`. Times the clocks skip or repeat are read
    /// as [`unix_seconds_at_crew_time`] reads them.
    ///
    /// # Errors
    ///
    /// Returns [`DueAtError`] for anything that isn't a date and time, including a date alone,
    /// which jiff would read as midnight.
    pub fn parse(input: &str) -> Result<Self, DueAtError> {
        let input = input.trim();
        if !input.contains(':') {
            return Err(DueAtError);
        }
        let time: DateTime = input.parse().map_err(|_| DueAtError)?;
        unix_seconds_at_crew_time(time).map(Self).ok_or(DueAtError)
    }

    /// The date and time it shows on the clock in the Crew's time zone.
    #[must_use]
    pub fn crew_time(self) -> DateTime {
        crew_time_of_unix_seconds(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a date and time")]
pub struct DueAtError;

/// A Reminder as a Member enters it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReminder {
    pub text: ReminderText,
    pub due_at: DueAt,
}

#[derive(Debug, thiserror::Error)]
pub enum ReminderError {
    /// It would be due now or earlier, so it could never be sent.
    #[error("the due time isn't in the future")]
    NotInFuture,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// A Reminder that hasn't been sent yet, as Pending lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReminder {
    pub id: ReminderId,
    pub text: ReminderText,
    pub due_at: DueAt,
    pub created_by: DisplayName,
}

/// Creates Reminders and lists them.
#[derive(Debug, Clone)]
pub struct Reminders {
    db: SqlitePool,
    clock: Clock,
}

impl Reminders {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock) -> Self {
        Self { db, clock }
    }

    /// The date and time it is now in the Crew's time zone: the earliest a Reminder could be
    /// due.
    #[must_use]
    pub fn now(&self) -> DateTime {
        crew_time_of_unix_seconds(unix_seconds(self.clock.now()))
    }

    /// Creates `reminder` for the whole Crew, on behalf of `creator`.
    ///
    /// # Errors
    ///
    /// Returns [`ReminderError::NotInFuture`] unless it is due after now, or a database error.
    pub async fn create(
        &self,
        creator: MemberId,
        reminder: &NewReminder,
    ) -> Result<ReminderId, ReminderError> {
        let now = unix_seconds(self.clock.now());
        if reminder.due_at.0 <= now {
            return Err(ReminderError::NotInFuture);
        }
        let created_by = creator.get();
        let done = sqlx::query!(
            "INSERT INTO reminders (text, due_at, created_by, created_at) VALUES (?, ?, ?, ?)",
            reminder.text.0,
            reminder.due_at.0,
            created_by,
            now,
        )
        .execute(&self.db)
        .await?;
        Ok(ReminderId(done.last_insert_rowid()))
    }

    /// Every pending Reminder, soonest first; those due at the same time in the order they were
    /// created.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn pending(&self) -> Result<Vec<PendingReminder>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT reminders.id AS "id!", reminders.text, reminders.due_at,
                      members.display_name AS created_by
               FROM reminders
               JOIN members ON members.id = reminders.created_by
               WHERE reminders.sent_at IS NULL
               ORDER BY reminders.due_at, reminders.id"#
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| PendingReminder {
                id: ReminderId(row.id),
                text: ReminderText(row.text),
                due_at: DueAt(row.due_at),
                created_by: DisplayName::from_stored(row.created_by),
            })
            .collect())
    }
}

/// Pending Reminders for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{DueAt, PendingReminder, ReminderId, ReminderText};
    use crate::domain::members::DisplayName;

    /// A pending Reminder with `id`, due at `due` in Prague (`2026-11-07T10:00`).
    ///
    /// # Panics
    ///
    /// If the fixture's text, time or name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn pending(id: i64, text: &str, due: &str, created_by: &str) -> PendingReminder {
        PendingReminder {
            id: ReminderId(id),
            text: ReminderText::parse(text).expect("fixture texts are valid"),
            due_at: DueAt::parse(due).expect("fixture times are valid"),
            created_by: DisplayName::parse(created_by).expect("fixture names are valid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use jiff::civil::date;

    use super::*;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::fixtures::add_member;

    /// 2026-10-24T08:00:00Z: 10:00 in Prague, the day before the clocks go back.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    fn reminders(db: SqlitePool) -> (Reminders, ManualClock) {
        let clock = ManualClock::starting_at(saturday_morning());
        (Reminders::new(db, clock.clock()), clock)
    }

    fn reminder(text: &str, due: &str) -> NewReminder {
        NewReminder {
            text: ReminderText::parse(text).unwrap(),
            due_at: DueAt::parse(due).unwrap(),
        }
    }

    #[test]
    fn trims_a_text() {
        assert_eq!(
            ReminderText::parse("  Bottle the stout \n")
                .unwrap()
                .as_str(),
            "Bottle the stout"
        );
    }

    #[test]
    fn refuses_a_blank_text() {
        assert_eq!(ReminderText::parse(" \t\n"), Err(TextError::Blank));
    }

    #[test]
    fn limits_a_text_by_characters_not_bytes() {
        let longest = "š".repeat(TEXT_MAX);
        assert_eq!(ReminderText::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            ReminderText::parse(&format!("{longest}š")),
            Err(TextError::TooLong)
        );
    }

    #[test]
    fn reads_a_due_time_as_prague_time() {
        // 10:00 in Prague on 7 November is 09:00Z, in winter time.
        let due = DueAt::parse(" 2026-11-07T10:00 ").unwrap();

        assert_eq!(due, DueAt(1_794_042_000));
        assert_eq!(due.crew_time(), date(2026, 11, 7).at(10, 0, 0, 0));
    }

    #[test]
    fn a_due_time_may_have_seconds() {
        assert_eq!(
            DueAt::parse("2026-11-07T10:00:30"),
            Ok(DueAt(1_794_042_030))
        );
    }

    #[test]
    fn keeps_the_prague_time_across_the_clocks_going_back() {
        // 10:00 is 08:00Z on the Saturday, in summer time, and 09:00Z on the Sunday, in winter.
        let saturday = DueAt::parse("2026-10-24T10:00").unwrap();
        let sunday = DueAt::parse("2026-10-25T10:00").unwrap();

        assert_eq!(sunday.0 - saturday.0, 25 * 3600);
        assert_eq!(saturday.crew_time(), date(2026, 10, 24).at(10, 0, 0, 0));
        assert_eq!(sunday.crew_time(), date(2026, 10, 25).at(10, 0, 0, 0));
    }

    #[test]
    fn refuses_what_isnt_a_date_and_time() {
        for input in [
            "",
            "tomorrow",
            "2026-11-07",
            "2026-11-07T25:00",
            "2026-02-30T10:00",
        ] {
            assert_eq!(DueAt::parse(input), Err(DueAtError), "{input:?}");
        }
    }

    #[sqlx::test]
    async fn tells_the_time_in_prague(db: SqlitePool) {
        let (reminders, _) = reminders(db);

        assert_eq!(reminders.now(), date(2026, 10, 24).at(10, 0, 0, 0));
    }

    #[sqlx::test]
    async fn a_created_reminder_is_pending(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db);

        let id = reminders
            .create(josef, &reminder("Bottle the stout", "2026-10-24T18:00"))
            .await
            .unwrap();

        assert_eq!(
            reminders.pending().await.unwrap(),
            [PendingReminder {
                id,
                text: ReminderText::parse("Bottle the stout").unwrap(),
                due_at: DueAt::parse("2026-10-24T18:00").unwrap(),
                created_by: DisplayName::parse("Josef").unwrap(),
            }]
        );
    }

    #[sqlx::test]
    async fn stores_the_due_time_in_utc_and_who_created_it_when(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let (reminders, _) = reminders(db.clone());

        let id = reminders
            .create(ondra, &reminder("Order hops", "2026-11-07T10:00"))
            .await
            .unwrap();

        let row = sqlx::query!(
            "SELECT due_at, created_by, created_at, sent_at FROM reminders WHERE id = ?",
            id.0
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(row.due_at, 1_794_042_000);
        assert_eq!(row.created_by, ondra.get());
        assert_eq!(row.created_at, 1_792_828_800);
        assert_eq!(row.sent_at, None);
    }

    #[sqlx::test]
    async fn refuses_a_due_time_now_or_earlier(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db);

        // The clock stands at 10:00:00 in Prague.
        for due in [
            "2026-10-24T10:00",
            "2026-10-24T09:59:59",
            "2025-10-24T12:00",
        ] {
            let refused = reminders.create(josef, &reminder("Too late", due)).await;
            assert!(
                matches!(refused, Err(ReminderError::NotInFuture)),
                "{due}: {refused:?}"
            );
        }
        assert_eq!(reminders.pending().await.unwrap(), []);
    }

    #[sqlx::test]
    async fn accepts_a_due_time_a_second_from_now(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db);

        reminders
            .create(josef, &reminder("Just in time", "2026-10-24T10:00:01"))
            .await
            .unwrap();

        assert_eq!(reminders.pending().await.unwrap().len(), 1);
    }

    #[sqlx::test]
    async fn the_future_is_told_by_the_clock(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock) = reminders(db);
        let tomorrow = reminder("Clean the fermenter", "2026-10-25T09:00");

        clock.advance(Duration::from_hours(48));

        assert!(matches!(
            reminders.create(josef, &tomorrow).await,
            Err(ReminderError::NotInFuture)
        ));
    }

    #[sqlx::test]
    async fn pending_is_soonest_first(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (reminders, _) = reminders(db);
        for (creator, text, due) in [
            (josef, "Next week", "2026-10-31T10:00"),
            (simon, "Tonight", "2026-10-24T20:00"),
            (josef, "Tomorrow", "2026-10-25T09:00"),
            (simon, "Also tonight", "2026-10-24T20:00"),
        ] {
            reminders
                .create(creator, &reminder(text, due))
                .await
                .unwrap();
        }

        let pending = reminders.pending().await.unwrap();

        let listed: Vec<(&str, &str)> = pending
            .iter()
            .map(|reminder| (reminder.text.as_str(), reminder.created_by.as_str()))
            .collect();
        assert_eq!(
            listed,
            [
                ("Tonight", "Šimon"),
                ("Also tonight", "Šimon"),
                ("Tomorrow", "Josef"),
                ("Next week", "Josef"),
            ]
        );
    }

    #[sqlx::test]
    async fn a_sent_reminder_isnt_pending(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db.clone());
        let id = reminders
            .create(josef, &reminder("Bottle the stout", "2026-10-24T18:00"))
            .await
            .unwrap();

        sqlx::query!("UPDATE reminders SET sent_at = due_at WHERE id = ?", id.0)
            .execute(&db)
            .await
            .unwrap();

        assert_eq!(reminders.pending().await.unwrap(), []);
    }
}
