//! Reminders: a text the whole Crew is notified about at a given date and time. Each is pending
//! until it is sent.

use std::sync::Arc;
use std::time::Duration;

use jiff::civil::DateTime;
use sqlx::SqlitePool;
use tokio::sync::Notify;

use crate::domain::channel::Emitter;
use crate::domain::clock::{
    Clock, crew_time_of_unix_seconds, unix_seconds, unix_seconds_at_crew_time,
};
use crate::domain::members::{DisplayName, MemberId};

/// The longest text of a Reminder, in characters.
pub const TEXT_MAX: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReminderId(i64);

impl ReminderId {
    /// The Reminder a request names. Whether it exists is up to [`Reminders`] to find out.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

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

/// A Reminder as a Member enters it, or changes it to.
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
    /// There is no Reminder with that id to change: it never existed or was deleted, or, for an
    /// edit, it was already sent.
    #[error("no such Reminder")]
    NotFound,
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

/// A Reminder that has been sent, as the Sent list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentReminder {
    pub id: ReminderId,
    pub text: ReminderText,
    pub sent_at: SentAt,
    pub created_by: DisplayName,
}

/// When a Reminder went out: an instant, shown as a date and time in the Crew's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SentAt(i64);

impl SentAt {
    /// The date and time it shows on the clock in the Crew's time zone.
    #[must_use]
    pub fn crew_time(self) -> DateTime {
        crew_time_of_unix_seconds(self.0)
    }
}

/// A Reminder fell due, for every Member to hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderDue {
    pub reminder: ReminderId,
    pub text: ReminderText,
    /// It went out more than [`LATE_AFTER`] after it was due, as when the server was down.
    pub late: bool,
    /// Every Member, its creator included, in id order.
    pub notify: Vec<MemberId>,
}

/// How long after its due time a Reminder may go out before it counts as late.
pub const LATE_AFTER: Duration = Duration::from_hours(1);

/// How far back the Sent list reaches. Older sent Reminders are kept but not shown.
pub const SENT_SHOWN_FOR: Duration = Duration::from_hours(30 * 24);

/// Creates, edits, deletes and lists Reminders, and sends them when they're due. Any Member may
/// change any Reminder.
#[derive(Debug, Clone)]
pub struct Reminders {
    db: SqlitePool,
    clock: Clock,
    events: Emitter<ReminderDue>,
    /// Told whenever a Reminder is created, edited or deleted, so the ticker can work out again
    /// when the next one is due. Every clone shares it.
    changed: Arc<Notify>,
}

impl Reminders {
    #[must_use]
    pub fn new(db: SqlitePool, clock: Clock, events: Emitter<ReminderDue>) -> Self {
        Self {
            db,
            clock,
            events,
            changed: Arc::new(Notify::new()),
        }
    }

    /// Waits until a Reminder is next created, edited or deleted. A change made while nobody
    /// waited ends the next wait at once, so the one ticker that waits misses none.
    pub async fn changed(&self) {
        self.changed.notified().await;
    }

    /// Sends every Reminder due by now, soonest first: marks it sent and emits a
    /// [`ReminderDue`] for every Member. A Reminder counts as sent once its event is emitted,
    /// whatever becomes of the push. Answers how many were sent.
    ///
    /// # Errors
    ///
    /// Returns a database error. Reminders sent before it stay sent.
    pub async fn send_due(&self) -> Result<usize, sqlx::Error> {
        let now = unix_seconds(self.clock.now());
        let due = sqlx::query!(
            r#"SELECT id AS "id!", text, due_at FROM reminders
               WHERE sent_at IS NULL AND due_at <= ?
               ORDER BY due_at, id"#,
            now
        )
        .fetch_all(&self.db)
        .await?;
        if due.is_empty() {
            return Ok(0);
        }
        let notify: Vec<MemberId> =
            sqlx::query_scalar!(r#"SELECT id AS "id!" FROM members ORDER BY id"#)
                .fetch_all(&self.db)
                .await?
                .into_iter()
                .map(MemberId::from_stored)
                .collect();
        let late_after = i64::try_from(LATE_AFTER.as_secs()).unwrap_or(i64::MAX);
        let mut sent = 0;
        for reminder in due {
            let done = sqlx::query!(
                "UPDATE reminders SET sent_at = ? WHERE id = ? AND sent_at IS NULL",
                now,
                reminder.id,
            )
            .execute(&self.db)
            .await?;
            // Deleted since it was found: nothing to send.
            if done.rows_affected() == 0 {
                continue;
            }
            self.events.emit(ReminderDue {
                reminder: ReminderId(reminder.id),
                text: ReminderText(reminder.text),
                late: now - reminder.due_at > late_after,
                notify: notify.clone(),
            });
            sent += 1;
        }
        Ok(sent)
    }

    /// How long until the soonest pending Reminder is due: nothing if none is pending, and no
    /// time at all if one is overdue.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn until_next_due(&self) -> Result<Option<Duration>, sqlx::Error> {
        let soonest =
            sqlx::query_scalar!("SELECT min(due_at) FROM reminders WHERE sent_at IS NULL")
                .fetch_one(&self.db)
                .await?;
        let now = unix_seconds(self.clock.now());
        Ok(soonest.map(|due_at| {
            Duration::from_secs(u64::try_from(due_at.saturating_sub(now)).unwrap_or(0))
        }))
    }

    /// Reminders sent within the last [`SENT_SHOWN_FOR`], newest first.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn sent(&self) -> Result<Vec<SentReminder>, sqlx::Error> {
        let shown_for = i64::try_from(SENT_SHOWN_FOR.as_secs()).unwrap_or(i64::MAX);
        let since = unix_seconds(self.clock.now()).saturating_sub(shown_for);
        let rows = sqlx::query!(
            r#"SELECT reminders.id AS "id!", reminders.text,
                      reminders.sent_at AS "sent_at!", members.display_name AS created_by
               FROM reminders
               JOIN members ON members.id = reminders.created_by
               WHERE reminders.sent_at >= ?
               ORDER BY reminders.sent_at DESC, reminders.due_at DESC, reminders.id DESC"#,
            since
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SentReminder {
                id: ReminderId(row.id),
                text: ReminderText(row.text),
                sent_at: SentAt(row.sent_at),
                created_by: DisplayName::from_stored(row.created_by),
            })
            .collect())
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
        self.changed.notify_one();
        Ok(ReminderId(done.last_insert_rowid()))
    }

    /// The pending Reminder `id`, as its edit form starts out.
    ///
    /// # Errors
    ///
    /// Returns [`ReminderError::NotFound`] unless `id` is pending, or a database error.
    pub async fn to_edit(&self, id: ReminderId) -> Result<PendingReminder, ReminderError> {
        let row = sqlx::query_as!(
            PendingRow,
            r#"SELECT reminders.id AS "id!", reminders.text, reminders.due_at,
                      members.display_name AS created_by
               FROM reminders
               JOIN members ON members.id = reminders.created_by
               WHERE reminders.id = ? AND reminders.sent_at IS NULL"#,
            id.0
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(ReminderError::NotFound)?;
        Ok(row.into())
    }

    /// Changes the pending Reminder `id` to say and be due as `reminder` does. Its creator stays
    /// who they were.
    ///
    /// # Errors
    ///
    /// Returns [`ReminderError::NotInFuture`] unless it would be due after now,
    /// [`ReminderError::NotFound`] unless `id` is pending, or a database error.
    pub async fn edit(&self, id: ReminderId, reminder: &NewReminder) -> Result<(), ReminderError> {
        if reminder.due_at.0 <= unix_seconds(self.clock.now()) {
            return Err(ReminderError::NotInFuture);
        }
        let done = sqlx::query!(
            "UPDATE reminders SET text = ?, due_at = ? WHERE id = ? AND sent_at IS NULL",
            reminder.text.0,
            reminder.due_at.0,
            id.0,
        )
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(ReminderError::NotFound);
        }
        self.changed.notify_one();
        Ok(())
    }

    /// Deletes the Reminder `id`, pending or sent.
    ///
    /// # Errors
    ///
    /// Returns [`ReminderError::NotFound`] if there is no Reminder `id`, or a database error.
    pub async fn delete(&self, id: ReminderId) -> Result<(), ReminderError> {
        let done = sqlx::query!("DELETE FROM reminders WHERE id = ?", id.0)
            .execute(&self.db)
            .await?;
        if done.rows_affected() == 0 {
            return Err(ReminderError::NotFound);
        }
        self.changed.notify_one();
        Ok(())
    }

    /// Every pending Reminder, soonest first; those due at the same time in the order they were
    /// created.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn pending(&self) -> Result<Vec<PendingReminder>, sqlx::Error> {
        let rows = sqlx::query_as!(
            PendingRow,
            r#"SELECT reminders.id AS "id!", reminders.text, reminders.due_at,
                      members.display_name AS created_by
               FROM reminders
               JOIN members ON members.id = reminders.created_by
               WHERE reminders.sent_at IS NULL
               ORDER BY reminders.due_at, reminders.id"#
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(PendingReminder::from).collect())
    }
}

/// A pending Reminder as the database stores it, joined with its creator's name.
struct PendingRow {
    id: i64,
    text: String,
    due_at: i64,
    created_by: String,
}

impl From<PendingRow> for PendingReminder {
    fn from(row: PendingRow) -> Self {
        Self {
            id: ReminderId(row.id),
            text: ReminderText(row.text),
            due_at: DueAt(row.due_at),
            created_by: DisplayName::from_stored(row.created_by),
        }
    }
}

/// Pending and sent Reminders for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{DueAt, PendingReminder, ReminderId, ReminderText, SentAt, SentReminder};
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

    /// A sent Reminder with `id`, sent at `sent` in Prague (`2026-10-20T08:00`).
    ///
    /// # Panics
    ///
    /// If the fixture's text, time or name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn sent(id: i64, text: &str, sent: &str, created_by: &str) -> SentReminder {
        SentReminder {
            id: ReminderId(id),
            text: ReminderText::parse(text).expect("fixture texts are valid"),
            sent_at: SentAt(DueAt::parse(sent).expect("fixture times are valid").0),
            created_by: DisplayName::parse(created_by).expect("fixture names are valid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::identity;
    use std::time::{Duration, SystemTime};

    use jiff::civil::date;
    use tokio::sync::broadcast;

    use super::*;
    use crate::domain::channel::Channel;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::fixtures::add_member;

    /// 2026-10-24T08:00:00Z: 10:00 in Prague, the day before the clocks go back.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    fn reminders(db: SqlitePool) -> (Reminders, ManualClock) {
        let clock = ManualClock::starting_at(saturday_morning());
        let events = Channel::new().emitter(identity);
        (Reminders::new(db, clock.clock(), events), clock)
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

        mark_sent(&db, id).await;

        assert_eq!(reminders.pending().await.unwrap(), []);
    }

    /// Marks `id` sent, as the ticker will.
    async fn mark_sent(db: &SqlitePool, id: ReminderId) {
        sqlx::query!("UPDATE reminders SET sent_at = due_at WHERE id = ?", id.0)
            .execute(db)
            .await
            .unwrap();
    }

    async fn create(reminders: &Reminders, creator: MemberId, text: &str, due: &str) -> ReminderId {
        reminders
            .create(creator, &reminder(text, due))
            .await
            .unwrap()
    }

    fn texts(pending: &[PendingReminder]) -> Vec<&str> {
        pending
            .iter()
            .map(|reminder| reminder.text.as_str())
            .collect()
    }

    #[sqlx::test]
    async fn a_reminder_to_edit_is_as_pending_shows_it(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let (reminders, _) = reminders(db);
        let id = create(&reminders, ondra, "Order hops", "2026-11-07T10:00").await;

        assert_eq!(
            reminders.to_edit(id).await.unwrap(),
            reminders.pending().await.unwrap()[0]
        );
    }

    #[sqlx::test]
    async fn editing_changes_the_text_and_time_and_reorders_pending(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let (reminders, _) = reminders(db);
        let hops = create(&reminders, ondra, "Order hops", "2026-10-24T18:00").await;
        create(&reminders, josef, "Bottle the stout", "2026-10-25T09:00").await;

        reminders
            .edit(hops, &reminder("  Order more hops ", "2026-11-07T10:00"))
            .await
            .unwrap();

        let pending = reminders.pending().await.unwrap();
        assert_eq!(texts(&pending), ["Bottle the stout", "Order more hops"]);
        // Whoever edits it, its creator stays.
        assert_eq!(
            pending[1],
            PendingReminder {
                id: hops,
                text: ReminderText::parse("Order more hops").unwrap(),
                due_at: DueAt::parse("2026-11-07T10:00").unwrap(),
                created_by: DisplayName::parse("Ondra").unwrap(),
            }
        );
    }

    #[sqlx::test]
    async fn editing_to_a_time_now_or_earlier_changes_nothing(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db);
        let id = create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;
        let before = reminders.pending().await.unwrap();

        // The clock stands at 10:00:00 in Prague.
        for due in ["2026-10-24T10:00", "2026-10-23T18:00"] {
            let refused = reminders.edit(id, &reminder("Too late", due)).await;
            assert!(
                matches!(refused, Err(ReminderError::NotInFuture)),
                "{due}: {refused:?}"
            );
        }
        assert_eq!(reminders.pending().await.unwrap(), before);
    }

    #[sqlx::test]
    async fn only_a_pending_reminder_can_be_edited(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db.clone());
        let sent = create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;
        mark_sent(&db, sent).await;
        let later = reminder("Bottle it later", "2026-10-25T18:00");

        for id in [sent, ReminderId::from_client(999)] {
            assert!(matches!(
                reminders.to_edit(id).await,
                Err(ReminderError::NotFound)
            ));
            assert!(matches!(
                reminders.edit(id, &later).await,
                Err(ReminderError::NotFound)
            ));
        }
    }

    #[sqlx::test]
    async fn a_deleted_reminder_leaves_pending(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let (reminders, _) = reminders(db);
        let hops = create(&reminders, ondra, "Order hops", "2026-10-24T18:00").await;
        create(&reminders, josef, "Bottle the stout", "2026-10-25T09:00").await;

        reminders.delete(hops).await.unwrap();

        assert_eq!(
            texts(&reminders.pending().await.unwrap()),
            ["Bottle the stout"]
        );
        assert!(matches!(
            reminders.to_edit(hops).await,
            Err(ReminderError::NotFound)
        ));
    }

    #[sqlx::test]
    async fn a_sent_reminder_can_still_be_deleted(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, _) = reminders(db.clone());
        let id = create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;
        mark_sent(&db, id).await;

        reminders.delete(id).await.unwrap();

        let left = sqlx::query_scalar!("SELECT count(*) FROM reminders")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(left, 0);
    }

    fn reminders_and_events(
        db: SqlitePool,
    ) -> (Reminders, ManualClock, broadcast::Receiver<ReminderDue>) {
        let clock = ManualClock::starting_at(saturday_morning());
        let channel = Channel::new();
        let subscription = channel.subscribe();
        let reminders = Reminders::new(db, clock.clock(), channel.emitter(identity));
        (reminders, clock, subscription)
    }

    fn emitted(subscription: &mut broadcast::Receiver<ReminderDue>) -> Vec<ReminderDue> {
        std::iter::from_fn(|| subscription.try_recv().ok()).collect()
    }

    fn due(reminder: ReminderId, text: &str, late: bool, notify: &[MemberId]) -> ReminderDue {
        ReminderDue {
            reminder,
            text: ReminderText::parse(text).unwrap(),
            late,
            notify: notify.to_vec(),
        }
    }

    fn sent_texts(sent: &[SentReminder]) -> Vec<&str> {
        sent.iter().map(|reminder| reminder.text.as_str()).collect()
    }

    #[sqlx::test]
    async fn a_due_reminder_is_sent_to_every_member_creator_included(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (reminders, clock, mut events) = reminders_and_events(db);
        let id = create(&reminders, ondra, "Bottle the stout", "2026-10-24T18:00").await;

        // 18:00:00, just due.
        clock.advance(Duration::from_hours(8));
        assert_eq!(reminders.send_due().await.unwrap(), 1);

        assert_eq!(
            emitted(&mut events),
            [due(id, "Bottle the stout", false, &[josef, ondra, simon])]
        );
        assert_eq!(reminders.pending().await.unwrap(), []);
        assert_eq!(
            reminders.sent().await.unwrap(),
            [SentReminder {
                id,
                text: ReminderText::parse("Bottle the stout").unwrap(),
                sent_at: SentAt(unix_seconds(clock.clock().now())),
                created_by: DisplayName::parse("Ondra").unwrap(),
            }]
        );
    }

    #[sqlx::test]
    async fn sending_twice_sends_nothing_twice(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock, mut events) = reminders_and_events(db);
        create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;
        clock.advance(Duration::from_hours(9));

        reminders.send_due().await.unwrap();
        assert_eq!(reminders.send_due().await.unwrap(), 0);

        assert_eq!(emitted(&mut events).len(), 1);
        assert_eq!(reminders.sent().await.unwrap().len(), 1);
    }

    #[sqlx::test]
    async fn a_reminder_not_yet_due_stays_pending(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock, mut events) = reminders_and_events(db);
        create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;
        create(
            &reminders,
            josef,
            "Clean the fermenter",
            "2026-10-24T18:00:01",
        )
        .await;

        clock.advance(Duration::from_hours(8));
        reminders.send_due().await.unwrap();

        assert_eq!(
            texts(&reminders.pending().await.unwrap()),
            ["Clean the fermenter"]
        );
        assert_eq!(
            sent_texts(&reminders.sent().await.unwrap()),
            ["Bottle the stout"]
        );
        assert_eq!(emitted(&mut events).len(), 1);
    }

    #[sqlx::test]
    async fn a_reminder_more_than_an_hour_overdue_is_sent_late(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock, mut events) = reminders_and_events(db);
        let on_time = create(&reminders, josef, "An hour ago", "2026-10-24T11:00").await;
        let late = create(&reminders, josef, "Longer ago", "2026-10-24T10:59:59").await;

        // 12:00:00: one is exactly an hour overdue, the other a second more.
        clock.advance(Duration::from_hours(2));
        reminders.send_due().await.unwrap();

        assert_eq!(
            emitted(&mut events),
            [
                due(late, "Longer ago", true, &[josef]),
                due(on_time, "An hour ago", false, &[josef]),
            ]
        );
    }

    #[sqlx::test]
    async fn a_sent_reminder_is_sent_when_the_step_ran(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock, _) = reminders_and_events(db);
        create(&reminders, josef, "Bottle the stout", "2026-10-24T18:00").await;

        // The server was down overnight; the Reminder goes out at 08:15 on Sunday, winter time.
        clock.advance(Duration::from_mins(23 * 60 + 15));
        reminders.send_due().await.unwrap();

        assert_eq!(
            reminders.sent().await.unwrap()[0].sent_at.crew_time(),
            date(2026, 10, 25).at(8, 15, 0, 0)
        );
    }

    #[sqlx::test]
    async fn sent_shows_the_last_30_days_newest_first(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock, _) = reminders_and_events(db);
        for (text, due) in [
            ("Oldest", "2026-10-24T11:00"),
            ("Middle", "2026-10-25T11:00"),
            ("Newest", "2026-10-26T11:00"),
        ] {
            create(&reminders, josef, text, due).await;
        }
        for _ in 0..3 {
            clock.advance(Duration::from_hours(24));
            reminders.send_due().await.unwrap();
        }
        assert_eq!(
            sent_texts(&reminders.sent().await.unwrap()),
            ["Newest", "Middle", "Oldest"]
        );

        // "Oldest" went out a day after the clock started; 30 days after that it's still shown, a
        // second more and it's not.
        clock.set(saturday_morning() + Duration::from_hours(24 + 30 * 24));
        assert_eq!(reminders.sent().await.unwrap().len(), 3);
        clock.advance(Duration::from_secs(1));
        assert_eq!(
            sent_texts(&reminders.sent().await.unwrap()),
            ["Newest", "Middle"]
        );
    }

    #[sqlx::test]
    async fn waits_until_the_soonest_pending_reminder_is_due(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (reminders, clock, _) = reminders_and_events(db);
        assert_eq!(reminders.until_next_due().await.unwrap(), None);

        create(&reminders, josef, "Later", "2026-10-24T18:00").await;
        create(&reminders, josef, "Sooner", "2026-10-24T10:30").await;
        assert_eq!(
            reminders.until_next_due().await.unwrap(),
            Some(Duration::from_mins(30))
        );

        // Overdue is due now.
        clock.advance(Duration::from_hours(1));
        assert_eq!(
            reminders.until_next_due().await.unwrap(),
            Some(Duration::ZERO)
        );
    }

    #[tokio::test]
    async fn a_change_is_kept_until_it_is_waited_for() {
        let db = SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let (reminders, _) = reminders(db);

        reminders.changed.notify_one();

        tokio::time::timeout(Duration::from_secs(1), reminders.changed())
            .await
            .expect("a change made while nobody waited still wakes the next wait");
    }

    #[sqlx::test]
    async fn deleting_a_reminder_that_doesnt_exist_is_not_found(db: SqlitePool) {
        let (reminders, _) = reminders(db);

        assert!(matches!(
            reminders.delete(ReminderId::from_client(999)).await,
            Err(ReminderError::NotFound)
        ));
    }
}
