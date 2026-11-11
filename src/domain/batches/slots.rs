//! Brewing slots: dates the Crew has set aside for brewing, before or after it knows what to
//! brew. Any Member plans a slot, moves it or cancels it, and links a Planned Batch to it.
//!
//! A slot is a calendar date in the Crew's time zone, with no time of day. There is at most one
//! slot per date, and none is planned or moved to a date before today. A slot holds at most one
//! Batch and a Batch is on at most one slot; only a Planned Batch is linked or unlinked.
//!
//! A slot's [`SlotState`] follows from its date and its Batch and is never stored. Recording a
//! Brew day on a Batch without a slot links it to a free slot on its brew date.
//!
//! A newly planned slot is announced to the rest of the Crew as [`SlotPlanned`]; moving or
//! cancelling one is not.
//!
//! Each slot brings an ordinary Reminder, due at [`reminder_time`] and saying [`reminder_text`].
//! It follows the slot: moving the slot or changing its Batch rewrites the pending Reminder, and
//! cancelling the slot deletes it. A sent Reminder is never touched, and one a Member deleted
//! isn't brought back until the slot moves.

use jiff::ToSpan;
use jiff::civil::{Date, DateTime};
use sqlx::SqliteConnection;

use super::{BatchName, BatchNumber, Batches};
use crate::domain::clock::unix_seconds;
use crate::domain::members::{MemberId, everyone_but, parse_date};
use crate::domain::reminders::{
    DueAt, NewReminder, ReminderError, ReminderId, ReminderText, Reminders,
};

/// Which Brewing slot: the one a form or link names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(i64);

impl SlotId {
    /// An id a client sent back. Nothing about it is trusted until the slot is found.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Where a Brewing slot is. It follows from the slot's date and its Batch, so no Member sets it
/// and it is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// Today or later, and its Batch, if it has one, isn't brewed yet.
    Upcoming,
    /// The date has passed and no Batch was recorded as brewed on it: the Crew is asked whether
    /// it brewed.
    DidYouBrew,
    /// Its Batch is brewed. A done slot isn't shown.
    Done,
}

impl SlotState {
    /// The state of a slot on `date` when it's `today`, whose Batch is `brewed` or not.
    #[must_use]
    pub fn of(date: Date, today: Date, brewed: bool) -> Self {
        if brewed {
            Self::Done
        } else if date < today {
            Self::DidYouBrew
        } else {
            Self::Upcoming
        }
    }
}

/// The Batch on a slot, as the slot shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotBatch {
    pub number: BatchNumber,
    pub name: BatchName,
}

/// A date the Crew has set aside for brewing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: SlotId,
    pub date: Date,
    /// The Batch brewed that day, or None while nothing is chosen.
    pub batch: Option<SlotBatch>,
    pub state: SlotState,
}

/// A Planned Batch, as a slot offers it to be brewed, with the date of the slot it's on now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedBatch {
    pub number: BatchNumber,
    pub name: BatchName,
    /// The date of the slot it's on, or None while it waits for one.
    pub slot: Option<Date>,
}

/// A Member planned a Brewing slot, for the rest of the Crew to hear about so they keep the day
/// free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPlanned {
    pub slot: SlotId,
    pub date: Date,
    /// The Member who planned it.
    pub planned_by: MemberId,
    /// Every Member except the one who planned it, in id order.
    pub notify: Vec<MemberId>,
}

/// Why a slot couldn't be planned or moved to a date.
#[derive(Debug, thiserror::Error)]
pub enum SlotError {
    /// The date is before today in the Crew's time zone.
    #[error("the date has passed")]
    Past,
    /// Another slot is on that date already.
    #[error("there's a Brewing slot on {0} already")]
    Taken(Date),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// Why a slot couldn't be moved.
#[derive(Debug, thiserror::Error)]
pub enum MoveError {
    /// There's no such slot: it never existed, or a Member cancelled it.
    #[error("no such Brewing slot")]
    NotFound,
    /// The date it would move to was refused, or the database failed.
    #[error(transparent)]
    Date(#[from] SlotError),
}

/// Why a Batch couldn't be linked to a slot or unlinked from one.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// There's no such slot: it never existed, or a Member cancelled it.
    #[error("no such Brewing slot")]
    SlotNotFound,
    /// There's no such Batch: it never existed, or a Member deleted it.
    #[error("no such Batch")]
    BatchNotFound,
    /// The Batch is brewed already, so its slot is done.
    #[error("the Batch is brewed already")]
    NotPlanned,
    /// The slot's own Batch is brewed, so the slot is done and keeps it.
    #[error("the Brewing slot is done")]
    SlotDone,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

impl From<sqlx::Error> for MoveError {
    fn from(err: sqlx::Error) -> Self {
        Self::Date(SlotError::Database(err))
    }
}

/// Whether a slot may be planned or moved to `date` when it's `today`: today or later.
///
/// # Errors
///
/// Returns [`SlotError::Past`] for a date before `today`.
pub fn check_date(date: Date, today: Date) -> Result<(), SlotError> {
    if date < today {
        Err(SlotError::Past)
    } else {
        Ok(())
    }
}

/// When the Reminder of a slot on `date` is due: 09:00 in Prague on the Monday of its week.
///
/// A week runs Monday to Sunday, so the Crew has the days before the slot to get ready. A slot
/// on a Monday is reminded that morning.
///
/// # Panics
///
/// In the first week of jiff's range, the year 9999 BC.
#[must_use]
pub fn reminder_time(date: Date) -> DateTime {
    let monday = date
        .checked_sub(i64::from(date.weekday().to_monday_zero_offset()).days())
        .expect("a slot's Monday is within jiff's range");
    monday.at(9, 0, 0, 0)
}

/// What the Reminder of a slot on `date` says: the day and its Batch, or that none is picked yet.
///
/// # Panics
///
/// Never: a Batch name is far shorter than a Reminder's text may be.
#[must_use]
pub fn reminder_text(date: Date, batch: Option<&SlotBatch>) -> ReminderText {
    let day = date.strftime("%a %-d %b");
    let text = batch.map_or_else(
        || format!("Brew day {day}, and no Batch is picked yet"),
        |batch| format!("Brew day {day}: #{} {}", batch.number.0, batch.name.0),
    );
    ReminderText::parse(&text).expect("a slot's Reminder text is short enough")
}

/// Whether a slot without a pending Reminder gets one when its Reminder follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Missing {
    /// The slot was planned or moved: it gets one if its due time is still ahead, and loses one
    /// whose due time has passed.
    Create,
    /// Only its Batch changed: a Reminder a Member deleted, or one already sent, stays that way.
    Leave,
}

/// `result` of writing a slot's Reminder, with a refusal told as None: its due time passed in the
/// moment since it was worked out, or the ticker sent it meanwhile.
fn unless_refused<T>(result: Result<T, ReminderError>) -> Result<Option<T>, sqlx::Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(ReminderError::NotInFuture | ReminderError::NotFound) => Ok(None),
        Err(ReminderError::Database(err)) => Err(err),
    }
}

/// `result` of writing a slot onto `date`, with a clash with another slot's date told as
/// [`SlotError::Taken`]. A clash on any other unique column, such as the slot's Batch, stays a
/// database error.
fn taken_on<T>(result: Result<T, sqlx::Error>, date: Date) -> Result<T, SlotError> {
    match result {
        Err(sqlx::Error::Database(err))
            if err.is_unique_violation() && err.message().contains("brewing_slots.date") =>
        {
            Err(SlotError::Taken(date))
        }
        other => Ok(other?),
    }
}

/// A slot as it is read back from the database, with its Batch's name and brew date.
struct SlotRow {
    id: i64,
    date: String,
    batch_number: Option<i64>,
    batch_name: Option<String>,
    brew_date: Option<String>,
}

impl SlotRow {
    /// The slot as it is `today`. Its date and its Batch's name were checked when they were
    /// stored.
    fn slot(self, today: Date) -> Result<Slot, sqlx::Error> {
        let date = parse_date(&self.date)?;
        Ok(Slot {
            id: SlotId(self.id),
            date,
            batch: self
                .batch_number
                .zip(self.batch_name)
                .map(|(number, name)| SlotBatch {
                    number: BatchNumber(number),
                    name: BatchName(name),
                }),
            state: SlotState::of(date, today, self.brew_date.is_some()),
        })
    }
}

impl Batches {
    /// Plans a slot on `date` on behalf of `planned_by`, now, and announces it to the rest of the
    /// Crew. Its Reminder is created on their behalf, unless its due time has passed.
    ///
    /// # Errors
    ///
    /// Returns [`SlotError::Past`] for a date before today, [`SlotError::Taken`] if a slot is on
    /// `date` already, or a database error.
    pub async fn plan_slot(&self, planned_by: MemberId, date: Date) -> Result<SlotId, SlotError> {
        check_date(date, self.clock.today())?;
        let created_at = unix_seconds(self.clock.now());
        let member = planned_by.get();
        let day = date.to_string();
        let mut tx = self.db.begin().await?;
        let id = taken_on(
            sqlx::query_scalar!(
                r#"INSERT INTO brewing_slots (date, created_by, created_at) VALUES (?, ?, ?)
                   RETURNING id AS "id!""#,
                day,
                member,
                created_at,
            )
            .fetch_one(&mut *tx)
            .await,
            date,
        )?;
        let slot = SlotId(id);
        self.follow(&mut tx, slot, Missing::Create).await?;
        tx.commit().await?;
        self.reminders.announce_change();
        let notify = everyone_but(&self.db, planned_by).await?;
        self.events.emit(SlotPlanned {
            slot,
            date,
            planned_by,
            notify,
        });
        Ok(slot)
    }

    /// Moves the slot `id` to `date`, and its pending Reminder with it. A slot without a pending
    /// Reminder gets one, and one whose new due time has passed loses it. Moving it to the date
    /// it's on changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`MoveError::NotFound`] if there's no slot `id`, or [`MoveError::Date`] with
    /// [`SlotError::Past`] for a date before today, [`SlotError::Taken`] if another slot is on
    /// `date`, or a database error.
    pub async fn move_slot(&self, id: SlotId, date: Date) -> Result<(), MoveError> {
        check_date(date, self.clock.today())?;
        let day = date.to_string();
        let mut tx = self.db.begin().await?;
        let was = sqlx::query_scalar!("SELECT date FROM brewing_slots WHERE id = ?", id.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(MoveError::NotFound)?;
        if was == day {
            return Ok(());
        }
        taken_on(
            sqlx::query!("UPDATE brewing_slots SET date = ? WHERE id = ?", day, id.0)
                .execute(&mut *tx)
                .await,
            date,
        )?;
        self.follow(&mut tx, id, Missing::Create).await?;
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }

    /// Cancels the slot `id` for good, and deletes its Reminder if it's pending. A slot already
    /// cancelled stays cancelled.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn cancel_slot(&self, id: SlotId) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;
        let reminder = sqlx::query_scalar!(
            r#"SELECT reminder_id AS "reminder_id?" FROM brewing_slots WHERE id = ?"#,
            id.0
        )
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        if let Some(reminder) = reminder {
            Reminders::delete_pending_on(&mut tx, ReminderId::from_client(reminder)).await?;
        }
        sqlx::query!("DELETE FROM brewing_slots WHERE id = ?", id.0)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }

    /// Brings the pending Reminder of the slot `id` in line with the slot's date and Batch, on
    /// `conn`: rewrites it while its due time is ahead, overwriting any edit a Member made. When
    /// `missing` is [`Missing::Create`], it also creates one the slot lacks and deletes one whose
    /// due time has passed. A sent Reminder is never touched. Nothing happens for a slot that
    /// isn't there.
    ///
    /// The caller tells the ticker once its transaction is committed.
    pub(super) async fn follow(
        &self,
        conn: &mut SqliteConnection,
        id: SlotId,
        missing: Missing,
    ) -> Result<(), sqlx::Error> {
        let Some(row) = sqlx::query!(
            r#"SELECT brewing_slots.date, brewing_slots.created_by,
                      reminders.id AS "pending?",
                      batches.number AS "batch_number?", batches.name AS "batch_name?"
               FROM brewing_slots
               LEFT JOIN reminders
                 ON reminders.id = brewing_slots.reminder_id AND reminders.sent_at IS NULL
               LEFT JOIN batches ON batches.number = brewing_slots.batch_number
               WHERE brewing_slots.id = ?"#,
            id.0
        )
        .fetch_optional(&mut *conn)
        .await?
        else {
            return Ok(());
        };
        let date = parse_date(&row.date)?;
        let batch = row
            .batch_number
            .zip(row.batch_name)
            .map(|(number, name)| SlotBatch {
                number: BatchNumber(number),
                name: BatchName(name),
            });
        let now = unix_seconds(self.clock.now());
        let reminder = DueAt::at_crew_time(reminder_time(date))
            .filter(|due_at| due_at.unix_seconds() > now)
            .map(|due_at| NewReminder {
                text: reminder_text(date, batch.as_ref()),
                due_at,
            });
        match (row.pending.map(ReminderId::from_client), reminder) {
            (Some(pending), Some(reminder)) => {
                unless_refused(self.reminders.edit_on(conn, pending, &reminder).await)?;
            }
            (Some(pending), None) if missing == Missing::Create => {
                Reminders::delete_pending_on(conn, pending).await?;
            }
            (None, Some(reminder)) if missing == Missing::Create => {
                let created_by = MemberId::from_stored(row.created_by);
                let created = self.reminders.create_on(conn, created_by, &reminder).await;
                if let Some(created) = unless_refused(created)? {
                    let reminder_id = created.get();
                    sqlx::query!(
                        "UPDATE brewing_slots SET reminder_id = ? WHERE id = ?",
                        reminder_id,
                        id.0
                    )
                    .execute(conn)
                    .await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The slot `id`, or None if there's no such slot: it never existed, or a Member cancelled
    /// it.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn slot(&self, id: SlotId) -> Result<Option<Slot>, sqlx::Error> {
        let today = self.clock.today();
        sqlx::query_as!(
            SlotRow,
            r#"SELECT brewing_slots.id AS "id!", brewing_slots.date,
                      brewing_slots.batch_number AS "batch_number?",
                      batches.name AS "batch_name?", batches.brew_date AS "brew_date?"
               FROM brewing_slots
               LEFT JOIN batches ON batches.number = brewing_slots.batch_number
               WHERE brewing_slots.id = ?"#,
            id.0
        )
        .fetch_optional(&self.db)
        .await?
        .map(|row| row.slot(today))
        .transpose()
    }

    /// The slots that aren't done, by date: first those whose date has passed with nothing
    /// brewed, which ask "Did you brew?", then the upcoming ones, soonest first.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn open_slots(&self) -> Result<Vec<Slot>, sqlx::Error> {
        let today = self.clock.today();
        sqlx::query_as!(
            SlotRow,
            r#"SELECT brewing_slots.id AS "id!", brewing_slots.date,
                      brewing_slots.batch_number AS "batch_number?",
                      batches.name AS "batch_name?", batches.brew_date AS "brew_date?"
               FROM brewing_slots
               LEFT JOIN batches ON batches.number = brewing_slots.batch_number
               WHERE batches.brew_date IS NULL
               ORDER BY brewing_slots.date"#,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(|row| row.slot(today))
        .collect()
    }

    /// The upcoming slots with nothing chosen, soonest first: those a Planned Batch can be
    /// linked to from its page.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn free_slots(&self) -> Result<Vec<Slot>, sqlx::Error> {
        let mut slots = self.open_slots().await?;
        slots.retain(|slot| slot.state == SlotState::Upcoming && slot.batch.is_none());
        Ok(slots)
    }

    /// The next Brewing slot: the soonest upcoming one, with its Batch if one is chosen, or None
    /// while no slot is upcoming. A slot whose date has passed, or whose Batch is brewed, is
    /// never the next one.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn next_slot(&self) -> Result<Option<Slot>, sqlx::Error> {
        let slots = self.open_slots().await?;
        Ok(slots
            .into_iter()
            .find(|slot| slot.state == SlotState::Upcoming))
    }

    /// The Planned Batches, newest first, each with the date of the slot it's on, if any: what a
    /// slot can be given to brew.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn planned_batches(&self) -> Result<Vec<PlannedBatch>, sqlx::Error> {
        sqlx::query!(
            r#"SELECT batches.number AS "number!", batches.name,
                      brewing_slots.date AS "slot_date?"
               FROM batches
               LEFT JOIN brewing_slots ON brewing_slots.batch_number = batches.number
               WHERE batches.brew_date IS NULL AND batches.bottling_date IS NULL
               ORDER BY batches.number DESC"#,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(|row| {
            Ok(PlannedBatch {
                number: BatchNumber(row.number),
                // Checked when it was stored.
                name: BatchName(row.name),
                slot: row.slot_date.as_deref().map(parse_date).transpose()?,
            })
        })
        .collect()
    }

    /// Links the Planned Batch `number` to the slot `id`, to be brewed that day. A Batch on
    /// another slot moves here, leaving that slot with nothing chosen, and a Batch this slot had
    /// before waits for a slot again. The pending Reminders of both slots follow.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::SlotNotFound`] or [`LinkError::BatchNotFound`] if either is gone,
    /// [`LinkError::NotPlanned`] if the Batch is brewed already, [`LinkError::SlotDone`] if the
    /// slot's own Batch is, or a database error.
    pub async fn link(&self, id: SlotId, number: BatchNumber) -> Result<(), LinkError> {
        let mut tx = self.db.begin().await?;
        let batch = sqlx::query!("SELECT brew_date FROM batches WHERE number = ?", number.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(LinkError::BatchNotFound)?;
        if batch.brew_date.is_some() {
            return Err(LinkError::NotPlanned);
        }
        let slot = sqlx::query!(
            r#"SELECT batches.brew_date AS "brew_date?"
               FROM brewing_slots
               LEFT JOIN batches ON batches.number = brewing_slots.batch_number
               WHERE brewing_slots.id = ?"#,
            id.0
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LinkError::SlotNotFound)?;
        if slot.brew_date.is_some() {
            return Err(LinkError::SlotDone);
        }
        // Off its old slot first: a Batch is on one slot at most.
        let old = sqlx::query_scalar!(
            r#"UPDATE brewing_slots SET batch_number = NULL WHERE batch_number = ?
               RETURNING id AS "id!""#,
            number.0
        )
        .fetch_optional(&mut *tx)
        .await?;
        let done = sqlx::query!(
            "UPDATE brewing_slots SET batch_number = ? WHERE id = ?",
            number.0,
            id.0
        )
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Err(LinkError::SlotNotFound);
        }
        if let Some(old) = old.map(SlotId).filter(|&old| old != id) {
            self.follow(&mut tx, old, Missing::Leave).await?;
        }
        self.follow(&mut tx, id, Missing::Leave).await?;
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }

    /// Takes the Batch off the slot `id`, which then has nothing chosen, and its pending Reminder
    /// says so. A slot with nothing chosen stays that way.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::SlotNotFound`] if there's no slot `id`, [`LinkError::NotPlanned`] if
    /// its Batch is brewed already, or a database error.
    pub async fn unlink_slot(&self, id: SlotId) -> Result<(), LinkError> {
        let mut tx = self.db.begin().await?;
        let slot = sqlx::query!(
            r#"SELECT batches.brew_date AS "brew_date?"
               FROM brewing_slots
               LEFT JOIN batches ON batches.number = brewing_slots.batch_number
               WHERE brewing_slots.id = ?"#,
            id.0
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(LinkError::SlotNotFound)?;
        if slot.brew_date.is_some() {
            return Err(LinkError::NotPlanned);
        }
        sqlx::query!(
            "UPDATE brewing_slots SET batch_number = NULL WHERE id = ?",
            id.0
        )
        .execute(&mut *tx)
        .await?;
        self.follow(&mut tx, id, Missing::Leave).await?;
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }

    /// Takes the Planned Batch `number` off its slot, to wait for another; the slot's pending
    /// Reminder says nothing is picked. A Batch without a slot stays without one.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::BatchNotFound`] if there's no Batch `number`,
    /// [`LinkError::NotPlanned`] if it's brewed already, or a database error.
    pub async fn unlink_batch(&self, number: BatchNumber) -> Result<(), LinkError> {
        let mut tx = self.db.begin().await?;
        let batch = sqlx::query!("SELECT brew_date FROM batches WHERE number = ?", number.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(LinkError::BatchNotFound)?;
        if batch.brew_date.is_some() {
            return Err(LinkError::NotPlanned);
        }
        let slot = sqlx::query_scalar!(
            r#"UPDATE brewing_slots SET batch_number = NULL WHERE batch_number = ?
               RETURNING id AS "id!""#,
            number.0
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(slot) = slot {
            self.follow(&mut tx, SlotId(slot), Missing::Leave).await?;
        }
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }
}

/// Slots for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{BatchName, BatchNumber, Slot, SlotBatch, SlotId, SlotState};

    /// The upcoming slot `id` on `date` (`2026-11-21`), with nothing chosen.
    ///
    /// # Panics
    ///
    /// If the fixture's date is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn slot(id: i64, date: &str) -> Slot {
        Slot {
            id: SlotId(id),
            date: date.parse().expect("fixture dates are valid"),
            batch: None,
            state: SlotState::Upcoming,
        }
    }

    /// `slot`, with the Batch `number` called `name` chosen for it.
    ///
    /// # Panics
    ///
    /// If the fixture's name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn with_batch(mut slot: Slot, number: i64, name: &str) -> Slot {
        slot.batch = Some(SlotBatch {
            number: BatchNumber(number),
            name: BatchName::parse(name).expect("fixture names are valid"),
        });
        slot
    }

    /// `slot`, with its date passed and nothing brewed on it.
    #[must_use]
    pub fn passed(slot: Slot) -> Slot {
        Slot {
            state: SlotState::DidYouBrew,
            ..slot
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::identity;
    use std::time::{Duration, SystemTime};

    use jiff::civil::{Date, DateTime, date};
    use sqlx::SqlitePool;
    use tokio::sync::broadcast;

    use super::{
        LinkError, MoveError, PlannedBatch, SlotBatch, SlotError, SlotId, SlotPlanned, SlotState,
        check_date, reminder_text, reminder_time,
    };
    use crate::domain::batches::{BatchName, BatchNumber, Batches, BrewDay, NewBatch, Notes};
    use crate::domain::channel::Channel;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::MemberId;
    use crate::domain::members::fixtures::add_member;
    use crate::domain::reminders::{DueAt, NewReminder, ReminderText, Reminders};

    /// 2026-11-11T22:30:00Z: 23:30 in Prague, on Wednesday 11 November.
    fn wednesday_night() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_mins(29_907_270)
    }

    fn service(db: SqlitePool) -> (Batches, ManualClock) {
        let (batches, clock, _) = announcing(db);
        (batches, clock)
    }

    /// The service, its clock, and the slots it announces as planned.
    fn announcing(db: SqlitePool) -> (Batches, ManualClock, broadcast::Receiver<SlotPlanned>) {
        let clock = ManualClock::starting_at(wednesday_night());
        let (batches, _, planned) = built(db, &clock);
        (batches, clock, planned)
    }

    /// The service, the Reminders it keeps the slots' Reminders in, and its clock.
    fn reminding(db: SqlitePool) -> (Batches, Reminders, ManualClock) {
        let clock = ManualClock::starting_at(wednesday_night());
        let (batches, reminders, _) = built(db, &clock);
        (batches, reminders, clock)
    }

    fn built(
        db: SqlitePool,
        clock: &ManualClock,
    ) -> (Batches, Reminders, broadcast::Receiver<SlotPlanned>) {
        let channel = Channel::new();
        let planned = channel.subscribe();
        let reminders = Reminders::new(db.clone(), clock.clock(), Channel::new().emitter(identity));
        let batches = Batches::new(
            db,
            clock.clock(),
            channel.emitter(identity),
            reminders.clone(),
        );
        (batches, reminders, planned)
    }

    fn emitted(planned: &mut broadcast::Receiver<SlotPlanned>) -> Vec<SlotPlanned> {
        std::iter::from_fn(|| planned.try_recv().ok()).collect()
    }

    async fn dates(batches: &Batches) -> Vec<Date> {
        batches
            .open_slots()
            .await
            .unwrap()
            .into_iter()
            .map(|slot| slot.date)
            .collect()
    }

    #[test]
    fn a_slot_may_be_today_or_later_but_not_before() {
        let today = date(2026, 11, 11);

        assert!(check_date(today, today).is_ok());
        assert!(check_date(date(2026, 11, 22), today).is_ok());
        assert!(matches!(
            check_date(date(2026, 11, 10), today),
            Err(SlotError::Past)
        ));
    }

    #[test]
    fn the_test_clock_is_late_on_wednesday_in_prague() {
        let clock = ManualClock::starting_at(wednesday_night());
        assert_eq!(clock.clock().today(), date(2026, 11, 11));
    }

    #[sqlx::test]
    async fn planned_slots_are_upcoming_soonest_first(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);

        for day in [date(2026, 11, 29), date(2026, 11, 11), date(2026, 11, 22)] {
            batches.plan_slot(josef, day).await.unwrap();
        }

        assert_eq!(
            dates(&batches).await,
            [date(2026, 11, 11), date(2026, 11, 22), date(2026, 11, 29)]
        );
    }

    #[sqlx::test]
    async fn a_slot_cant_be_planned_on_a_taken_date_or_before_today(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let (batches, _) = service(db);
        batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();

        assert!(matches!(
            batches.plan_slot(ondra, date(2026, 11, 22)).await,
            Err(SlotError::Taken(day)) if day == date(2026, 11, 22)
        ));
        assert!(matches!(
            batches.plan_slot(ondra, date(2026, 11, 10)).await,
            Err(SlotError::Past)
        ));
        assert_eq!(dates(&batches).await, [date(2026, 11, 22)]);
    }

    #[sqlx::test]
    async fn a_slot_moves_to_a_free_date_or_stays_where_it_is(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let moved = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();

        batches.move_slot(moved, date(2026, 12, 6)).await.unwrap();
        assert_eq!(
            batches.slot(moved).await.unwrap().unwrap().date,
            date(2026, 12, 6)
        );

        batches.move_slot(moved, date(2026, 12, 6)).await.unwrap();
        assert_eq!(
            dates(&batches).await,
            [date(2026, 11, 29), date(2026, 12, 6)]
        );
    }

    #[sqlx::test]
    async fn a_slot_cant_move_onto_another_or_into_the_past(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let moved = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();

        assert!(matches!(
            batches.move_slot(moved, date(2026, 11, 29)).await,
            Err(MoveError::Date(SlotError::Taken(day))) if day == date(2026, 11, 29)
        ));
        assert!(matches!(
            batches.move_slot(moved, date(2026, 11, 10)).await,
            Err(MoveError::Date(SlotError::Past))
        ));
        assert_eq!(
            batches.slot(moved).await.unwrap().unwrap().date,
            date(2026, 11, 22)
        );
    }

    #[sqlx::test]
    async fn a_cancelled_slot_is_gone_and_frees_its_date(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let cancelled = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();

        batches.cancel_slot(cancelled).await.unwrap();

        assert_eq!(batches.slot(cancelled).await.unwrap(), None);
        assert_eq!(dates(&batches).await, []);
        batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
    }

    #[sqlx::test]
    async fn a_slot_that_isnt_there_cant_be_read_or_moved_and_stays_cancelled(db: SqlitePool) {
        let (batches, _) = service(db);
        let missing = SlotId::from_client(999);

        assert_eq!(batches.slot(missing).await.unwrap(), None);
        assert!(matches!(
            batches.move_slot(missing, date(2026, 11, 22)).await,
            Err(MoveError::NotFound)
        ));
        batches.cancel_slot(missing).await.unwrap();
    }

    #[sqlx::test]
    async fn a_slot_whose_date_has_passed_with_nothing_brewed_asks_did_you_brew(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, clock) = service(db);
        batches.plan_slot(josef, date(2026, 11, 12)).await.unwrap();
        let passed = batches.plan_slot(josef, date(2026, 11, 11)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        batches.link(passed, pils).await.unwrap();

        // Half an hour later it's Thursday in Prague.
        clock.advance(Duration::from_mins(30));

        let open = batches.open_slots().await.unwrap();
        let states: Vec<_> = open.iter().map(|slot| (slot.date, slot.state)).collect();
        assert_eq!(
            states,
            [
                (date(2026, 11, 11), SlotState::DidYouBrew),
                (date(2026, 11, 12), SlotState::Upcoming),
            ],
            "the passed slot first"
        );
        assert_eq!(open[0].batch.as_ref().unwrap().number, pils);
    }

    #[test]
    fn a_slots_state_follows_from_its_date_and_whether_its_batch_is_brewed() {
        let today = date(2026, 11, 11);
        let yesterday = date(2026, 11, 10);
        let later = date(2026, 11, 22);

        assert_eq!(SlotState::of(today, today, false), SlotState::Upcoming);
        assert_eq!(SlotState::of(later, today, false), SlotState::Upcoming);
        assert_eq!(
            SlotState::of(yesterday, today, false),
            SlotState::DidYouBrew
        );
        assert_eq!(SlotState::of(yesterday, today, true), SlotState::Done);
        assert_eq!(SlotState::of(today, today, true), SlotState::Done);
        assert_eq!(SlotState::of(later, today, true), SlotState::Done);
    }

    async fn add_batch(batches: &Batches, by: MemberId, name: &str) -> BatchNumber {
        let batch = NewBatch {
            name: BatchName::parse(name).unwrap(),
            notes: Notes::default(),
        };
        batches.add(by, &batch, None).await.unwrap()
    }

    async fn brew(batches: &Batches, by: MemberId, number: BatchNumber, on: Option<Date>) {
        let brew_day = BrewDay {
            date: on,
            ..BrewDay::default()
        };
        batches
            .record_brew_day(number, by, &brew_day)
            .await
            .unwrap();
    }

    /// The number of the Batch on the slot `id`, if any.
    async fn batch_on(batches: &Batches, id: SlotId) -> Option<BatchNumber> {
        let slot = batches.slot(id).await.unwrap().unwrap();
        slot.batch.map(|batch| batch.number)
    }

    #[sqlx::test]
    async fn a_linked_batch_and_slot_show_each_other(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        assert_eq!(batches.get(pils).await.unwrap().slot, None);
        assert_eq!(batch_on(&batches, slot).await, None);

        batches.link(slot, pils).await.unwrap();

        let shown = batches.slot(slot).await.unwrap().unwrap();
        let on_it = shown.batch.unwrap();
        assert_eq!((on_it.number, on_it.name.as_str()), (pils, "Pils"));
        assert_eq!(shown.state, SlotState::Upcoming);
        assert_eq!(
            batches.get(pils).await.unwrap().slot,
            Some(date(2026, 11, 22))
        );
        assert_eq!(
            batches.page(1).await.unwrap().batches[0].slot,
            Some(date(2026, 11, 22))
        );
    }

    #[sqlx::test]
    async fn linking_a_batch_on_another_slot_moves_it_and_replaces_the_slots_batch(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let first = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let second = batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        let stout = add_batch(&batches, josef, "Stout").await;
        batches.link(first, pils).await.unwrap();
        batches.link(second, stout).await.unwrap();

        batches.link(second, pils).await.unwrap();

        assert_eq!(batch_on(&batches, first).await, None);
        assert_eq!(batch_on(&batches, second).await, Some(pils));
        assert_eq!(batches.get(stout).await.unwrap().slot, None);
        assert_eq!(
            batches.get(pils).await.unwrap().slot,
            Some(date(2026, 11, 29))
        );

        batches.link(second, pils).await.unwrap();
        assert_eq!(batch_on(&batches, second).await, Some(pils), "linked again");
    }

    #[sqlx::test]
    async fn either_side_unlinks(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;

        batches.link(slot, pils).await.unwrap();
        batches.unlink_slot(slot).await.unwrap();
        assert_eq!(batch_on(&batches, slot).await, None);
        assert_eq!(batches.get(pils).await.unwrap().slot, None);
        batches.unlink_slot(slot).await.unwrap();

        batches.link(slot, pils).await.unwrap();
        batches.unlink_batch(pils).await.unwrap();
        assert_eq!(batch_on(&batches, slot).await, None);
        batches.unlink_batch(pils).await.unwrap();
    }

    #[sqlx::test]
    async fn only_a_planned_batch_is_linked_or_unlinked(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let other = batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        batches.link(slot, pils).await.unwrap();
        brew(&batches, josef, pils, Some(date(2026, 11, 22))).await;

        assert!(matches!(
            batches.link(other, pils).await,
            Err(LinkError::NotPlanned)
        ));
        assert!(matches!(
            batches.unlink_batch(pils).await,
            Err(LinkError::NotPlanned)
        ));
        assert!(matches!(
            batches.unlink_slot(slot).await,
            Err(LinkError::NotPlanned)
        ));
        assert_eq!(batch_on(&batches, slot).await, Some(pils));
        assert_eq!(batch_on(&batches, other).await, None);
    }

    #[sqlx::test]
    async fn a_done_slot_keeps_its_brewed_batch(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        let stout = add_batch(&batches, josef, "Stout").await;
        brew(&batches, josef, pils, Some(date(2026, 11, 22))).await;

        assert!(matches!(
            batches.link(slot, stout).await,
            Err(LinkError::SlotDone)
        ));
        assert_eq!(batch_on(&batches, slot).await, Some(pils));
        assert_eq!(batches.free_slots().await.unwrap(), []);
    }

    #[sqlx::test]
    async fn free_slots_are_upcoming_with_nothing_chosen(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, clock) = service(db);
        batches.plan_slot(josef, date(2026, 11, 11)).await.unwrap();
        let taken = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        batches.link(taken, pils).await.unwrap();
        clock.advance(Duration::from_mins(30));

        let free: Vec<Date> = batches
            .free_slots()
            .await
            .unwrap()
            .into_iter()
            .map(|slot| slot.date)
            .collect();
        assert_eq!(free, [date(2026, 11, 29)]);
    }

    #[sqlx::test]
    async fn the_next_slot_is_the_soonest_upcoming_one(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, clock) = service(db);
        assert_eq!(batches.next_slot().await.unwrap(), None, "nothing planned");

        let passing = batches.plan_slot(josef, date(2026, 11, 11)).await.unwrap();
        let done = batches.plan_slot(josef, date(2026, 11, 12)).await.unwrap();
        let next = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        batches.link(done, pils).await.unwrap();
        brew(&batches, josef, pils, Some(date(2026, 11, 11))).await;
        let stout = add_batch(&batches, josef, "Stout").await;
        batches.link(next, stout).await.unwrap();
        assert_eq!(
            batches.next_slot().await.unwrap().map(|slot| slot.id),
            Some(passing),
            "today is next"
        );

        clock.advance(Duration::from_mins(30));

        let slot = batches.next_slot().await.unwrap().expect("a next slot");
        assert_eq!(slot.id, next, "neither the passed nor the done slot");
        assert_eq!(slot.batch.map(|batch| batch.number), Some(stout));
    }

    #[sqlx::test]
    async fn a_slot_or_batch_that_isnt_there_cant_be_linked_or_unlinked(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        let missing_slot = SlotId::from_client(999);
        let missing_batch = BatchNumber::from_client(999);

        assert!(matches!(
            batches.link(missing_slot, pils).await,
            Err(LinkError::SlotNotFound)
        ));
        assert!(matches!(
            batches.link(slot, missing_batch).await,
            Err(LinkError::BatchNotFound)
        ));
        assert!(matches!(
            batches.unlink_slot(missing_slot).await,
            Err(LinkError::SlotNotFound)
        ));
        assert!(matches!(
            batches.unlink_batch(missing_batch).await,
            Err(LinkError::BatchNotFound)
        ));
    }

    #[sqlx::test]
    async fn a_refused_link_leaves_the_batch_on_its_old_slot(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        batches.link(slot, pils).await.unwrap();

        batches
            .link(SlotId::from_client(999), pils)
            .await
            .unwrap_err();

        assert_eq!(batch_on(&batches, slot).await, Some(pils));
    }

    #[sqlx::test]
    async fn brewing_on_a_free_slots_date_links_the_batch_and_the_slot_is_done(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;

        brew(&batches, josef, pils, Some(date(2026, 11, 22))).await;

        assert_eq!(batch_on(&batches, slot).await, Some(pils));
        assert_eq!(
            batches.slot(slot).await.unwrap().unwrap().state,
            SlotState::Done
        );
        assert_eq!(dates(&batches).await, [date(2026, 11, 29)]);
    }

    #[sqlx::test]
    async fn brewing_links_nothing_if_the_batch_has_a_slot_or_the_slot_a_batch(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let taken = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let free = batches.plan_slot(josef, date(2026, 11, 29)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        let stout = add_batch(&batches, josef, "Stout").await;
        let ale = add_batch(&batches, josef, "Ale").await;
        batches.link(taken, pils).await.unwrap();

        // Stout is brewed on the day Pils was planned for, and Pils a week later on the free
        // slot's day. Ale is brewed on no slot's day.
        brew(&batches, josef, stout, Some(date(2026, 11, 22))).await;
        brew(&batches, josef, pils, Some(date(2026, 11, 29))).await;
        brew(&batches, josef, ale, Some(date(2026, 11, 23))).await;

        assert_eq!(batch_on(&batches, taken).await, Some(pils));
        assert_eq!(batch_on(&batches, free).await, None);
        assert_eq!(batches.get(stout).await.unwrap().slot, None);
        assert_eq!(batches.get(ale).await.unwrap().slot, None);
    }

    #[sqlx::test]
    async fn clearing_the_brew_date_opens_the_slot_again(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        brew(&batches, josef, pils, Some(date(2026, 11, 22))).await;

        brew(&batches, josef, pils, None).await;

        assert_eq!(batch_on(&batches, slot).await, Some(pils));
        assert_eq!(dates(&batches).await, [date(2026, 11, 22)]);
    }

    #[sqlx::test]
    async fn deleting_a_linked_batch_leaves_its_slot_with_nothing_chosen(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        batches.link(slot, pils).await.unwrap();

        batches.delete(pils).await.unwrap();

        assert_eq!(batch_on(&batches, slot).await, None);
        assert_eq!(dates(&batches).await, [date(2026, 11, 22)]);
    }

    #[sqlx::test]
    async fn the_planned_batches_newest_first_with_their_slots(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        let brewed = add_batch(&batches, josef, "Brewed").await;
        let stout = add_batch(&batches, josef, "Stout").await;
        batches.link(slot, pils).await.unwrap();
        brew(&batches, josef, brewed, Some(date(2026, 11, 1))).await;

        assert_eq!(
            batches.planned_batches().await.unwrap(),
            [
                PlannedBatch {
                    number: stout,
                    name: BatchName::parse("Stout").unwrap(),
                    slot: None,
                },
                PlannedBatch {
                    number: pils,
                    name: BatchName::parse("Pils").unwrap(),
                    slot: Some(date(2026, 11, 22)),
                },
            ]
        );
    }

    #[sqlx::test]
    async fn a_planned_slot_is_announced_to_everyone_but_its_planner(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (batches, _, mut planned) = announcing(db);

        let slot = batches.plan_slot(ondra, date(2026, 11, 22)).await.unwrap();

        assert_eq!(
            emitted(&mut planned),
            [SlotPlanned {
                slot,
                date: date(2026, 11, 22),
                planned_by: ondra,
                notify: vec![josef, simon],
            }]
        );
    }

    #[sqlx::test]
    async fn moving_cancelling_or_a_refused_plan_announces_nothing(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        add_member(&db, "Ondra", false).await;
        let (batches, _, mut planned) = announcing(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 22)).await.unwrap();
        emitted(&mut planned);

        batches.move_slot(slot, date(2026, 11, 29)).await.unwrap();
        batches
            .plan_slot(josef, date(2026, 11, 29))
            .await
            .unwrap_err();
        batches
            .plan_slot(josef, date(2026, 11, 10))
            .await
            .unwrap_err();
        batches.cancel_slot(slot).await.unwrap();

        assert_eq!(emitted(&mut planned), []);
    }

    /// Each pending Reminder's text and due time on the clock in Prague, soonest first.
    async fn pending(reminders: &Reminders) -> Vec<(String, DateTime)> {
        reminders
            .pending()
            .await
            .unwrap()
            .into_iter()
            .map(|reminder| {
                let due = reminder.due_at.crew_time();
                (reminder.text.as_str().to_owned(), due)
            })
            .collect()
    }

    /// `text` due at `due`, 09:00 on a Monday, as [`pending`] lists it.
    fn due(text: &str, monday: Date) -> (String, DateTime) {
        (text.to_owned(), monday.at(9, 0, 0, 0))
    }

    fn pils(number: i64) -> SlotBatch {
        SlotBatch {
            number: BatchNumber(number),
            name: BatchName::parse("Vzor 10°").unwrap(),
        }
    }

    #[test]
    fn a_slots_reminder_is_due_at_nine_on_the_monday_of_its_week() {
        // Sunday 22 November belongs to the week from Monday 16 November.
        assert_eq!(
            reminder_time(date(2026, 11, 22)),
            date(2026, 11, 16).at(9, 0, 0, 0)
        );
        assert_eq!(
            reminder_time(date(2026, 11, 21)),
            date(2026, 11, 16).at(9, 0, 0, 0)
        );
        assert_eq!(
            reminder_time(date(2026, 11, 16)),
            date(2026, 11, 16).at(9, 0, 0, 0),
            "a Monday slot that morning"
        );
        assert_eq!(
            reminder_time(date(2027, 1, 1)),
            date(2026, 12, 28).at(9, 0, 0, 0),
            "across the new year"
        );
    }

    #[test]
    fn a_slots_reminder_names_the_day_and_its_batch_or_that_none_is_picked() {
        assert_eq!(
            reminder_text(date(2026, 11, 21), Some(&pils(14))).as_str(),
            "Brew day Sat 21 Nov: #14 Vzor 10°"
        );
        assert_eq!(
            reminder_text(date(2026, 11, 21), None).as_str(),
            "Brew day Sat 21 Nov, and no Batch is picked yet"
        );
    }

    #[sqlx::test]
    async fn planning_a_slot_brings_a_pending_reminder_on_its_monday(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let (batches, reminders, _) = reminding(db);

        batches.plan_slot(ondra, date(2026, 11, 21)).await.unwrap();

        assert_eq!(
            pending(&reminders).await,
            [due(
                "Brew day Sat 21 Nov, and no Batch is picked yet",
                date(2026, 11, 16)
            )]
        );
        let reminder = &reminders.pending().await.unwrap()[0];
        assert_eq!(
            reminder.created_by.as_str(),
            "Ondra",
            "on behalf of its planner"
        );
    }

    #[sqlx::test]
    async fn a_slot_whose_monday_has_passed_brings_no_reminder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);

        // It's Wednesday: this week's Monday has passed.
        batches.plan_slot(josef, date(2026, 11, 11)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 14)).await.unwrap();

        assert_eq!(pending(&reminders).await, []);
    }

    #[sqlx::test]
    async fn the_ticker_hears_of_a_slots_reminder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);

        batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();

        tokio::time::timeout(Duration::from_secs(1), reminders.changed())
            .await
            .expect("told of the new Reminder");
    }

    #[sqlx::test]
    async fn moving_a_slot_moves_its_reminder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        let id = reminders.pending().await.unwrap()[0].id;

        batches.move_slot(slot, date(2026, 12, 3)).await.unwrap();

        assert_eq!(
            pending(&reminders).await,
            [due(
                "Brew day Thu 3 Dec, and no Batch is picked yet",
                date(2026, 11, 30)
            )]
        );
        assert_eq!(reminders.pending().await.unwrap()[0].id, id, "the same one");
    }

    #[sqlx::test]
    async fn moving_a_slot_into_a_week_whose_monday_has_passed_drops_its_reminder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();

        batches.move_slot(slot, date(2026, 11, 13)).await.unwrap();

        assert_eq!(pending(&reminders).await, []);
    }

    #[sqlx::test]
    async fn moving_a_slot_without_a_reminder_brings_one_if_its_monday_is_ahead(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 13)).await.unwrap();
        assert_eq!(pending(&reminders).await, []);

        batches.move_slot(slot, date(2026, 11, 14)).await.unwrap();
        assert_eq!(pending(&reminders).await, [], "still this week");

        batches.move_slot(slot, date(2026, 11, 20)).await.unwrap();
        assert_eq!(
            pending(&reminders).await,
            [due(
                "Brew day Fri 20 Nov, and no Batch is picked yet",
                date(2026, 11, 16)
            )]
        );
    }

    #[sqlx::test]
    async fn a_slots_reminder_follows_its_batch(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let first = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        let second = batches.plan_slot(josef, date(2026, 11, 28)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;
        let unpicked = |day| format!("Brew day {day}, and no Batch is picked yet");

        batches.link(first, pils).await.unwrap();
        assert_eq!(
            pending(&reminders).await,
            [
                due("Brew day Sat 21 Nov: #1 Pils", date(2026, 11, 16)),
                due(&unpicked("Sat 28 Nov"), date(2026, 11, 23)),
            ]
        );

        batches.link(second, pils).await.unwrap();
        assert_eq!(
            pending(&reminders).await,
            [
                due(&unpicked("Sat 21 Nov"), date(2026, 11, 16)),
                due("Brew day Sat 28 Nov: #1 Pils", date(2026, 11, 23)),
            ],
            "moved to the other slot"
        );

        let renamed = NewBatch {
            name: BatchName::parse("Vzor 10°").unwrap(),
            notes: Notes::default(),
        };
        batches.edit(pils, josef, &renamed).await.unwrap();
        assert_eq!(
            pending(&reminders).await[1],
            due("Brew day Sat 28 Nov: #1 Vzor 10°", date(2026, 11, 23)),
            "renamed"
        );

        batches.unlink_slot(second).await.unwrap();
        assert_eq!(
            pending(&reminders).await[1],
            due(&unpicked("Sat 28 Nov"), date(2026, 11, 23)),
            "unlinked from the slot"
        );

        batches.link(second, pils).await.unwrap();
        batches.unlink_batch(pils).await.unwrap();
        assert_eq!(
            pending(&reminders).await[1],
            due(&unpicked("Sat 28 Nov"), date(2026, 11, 23)),
            "unlinked from the Batch"
        );

        batches.link(second, pils).await.unwrap();
        batches.delete(pils).await.unwrap();
        assert_eq!(
            pending(&reminders).await[1],
            due(&unpicked("Sat 28 Nov"), date(2026, 11, 23)),
            "its Batch deleted"
        );
    }

    #[sqlx::test]
    async fn brewing_on_a_free_slots_date_names_the_batch_in_its_reminder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;

        brew(&batches, josef, pils, Some(date(2026, 11, 21))).await;

        assert_eq!(
            pending(&reminders).await,
            [due("Brew day Sat 21 Nov: #1 Pils", date(2026, 11, 16))]
        );
    }

    #[sqlx::test]
    async fn a_members_edit_to_a_slots_reminder_is_overwritten_when_the_slot_changes(
        db: SqlitePool,
    ) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        let id = reminders.pending().await.unwrap()[0].id;
        let edit = NewReminder {
            text: ReminderText::parse("Buy malt!").unwrap(),
            due_at: DueAt::parse("2026-11-20T18:00").unwrap(),
        };
        reminders.edit(id, &edit).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;

        batches.link(slot, pils).await.unwrap();

        assert_eq!(
            pending(&reminders).await,
            [due("Brew day Sat 21 Nov: #1 Pils", date(2026, 11, 16))]
        );
    }

    #[sqlx::test]
    async fn cancelling_a_slot_deletes_its_pending_reminder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let cancelled = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 28)).await.unwrap();

        batches.cancel_slot(cancelled).await.unwrap();

        assert_eq!(
            pending(&reminders).await,
            [due(
                "Brew day Sat 28 Nov, and no Batch is picked yet",
                date(2026, 11, 23)
            )]
        );
    }

    #[sqlx::test]
    async fn a_reminder_a_member_deleted_comes_back_only_when_the_slot_moves(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        let id = reminders.pending().await.unwrap()[0].id;
        reminders.delete(id).await.unwrap();
        let pils = add_batch(&batches, josef, "Pils").await;

        batches.link(slot, pils).await.unwrap();
        batches.unlink_slot(slot).await.unwrap();
        batches.link(slot, pils).await.unwrap();
        assert_eq!(pending(&reminders).await, [], "not for a Batch");
        batches.move_slot(slot, date(2026, 11, 21)).await.unwrap();
        assert_eq!(pending(&reminders).await, [], "not to the same date");

        batches.move_slot(slot, date(2026, 11, 22)).await.unwrap();
        assert_eq!(
            pending(&reminders).await,
            [due("Brew day Sun 22 Nov: #1 Pils", date(2026, 11, 16))]
        );
    }

    #[sqlx::test]
    async fn a_sent_reminder_is_never_touched(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, clock) = reminding(db);
        let slot = batches.plan_slot(josef, date(2026, 11, 21)).await.unwrap();
        // Monday 16 November, 09:30 in Prague.
        clock.advance(Duration::from_hours(4 * 24 + 10));
        assert_eq!(reminders.send_due().await.unwrap(), 1);
        let sent = || async {
            let sent = reminders.sent().await.unwrap();
            sent.into_iter()
                .map(|reminder| reminder.text.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        let pils = add_batch(&batches, josef, "Pils").await;

        batches.link(slot, pils).await.unwrap();
        batches.unlink_slot(slot).await.unwrap();
        assert_eq!(pending(&reminders).await, []);

        batches.move_slot(slot, date(2026, 11, 28)).await.unwrap();
        assert_eq!(
            pending(&reminders).await,
            [due(
                "Brew day Sat 28 Nov, and no Batch is picked yet",
                date(2026, 11, 23)
            )],
            "a new one for the new week"
        );

        batches.cancel_slot(slot).await.unwrap();
        assert_eq!(pending(&reminders).await, []);
        assert_eq!(
            sent().await,
            ["Brew day Sat 21 Nov, and no Batch is picked yet"]
        );
    }

    #[sqlx::test]
    async fn a_slots_reminder_is_at_nine_in_prague_across_a_clock_change(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, reminders, _) = reminding(db);
        // The clocks go forward on Sunday 28 March 2027.
        let slot = batches.plan_slot(josef, date(2027, 3, 28)).await.unwrap();
        let winter = reminders.pending().await.unwrap()[0].due_at;

        batches.move_slot(slot, date(2027, 4, 3)).await.unwrap();
        let summer = reminders.pending().await.unwrap()[0].due_at;

        assert_eq!(winter.crew_time(), date(2027, 3, 22).at(9, 0, 0, 0));
        assert_eq!(summer.crew_time(), date(2027, 3, 29).at(9, 0, 0, 0));
        assert_eq!(
            summer.unix_seconds() - winter.unix_seconds(),
            7 * 24 * 3600 - 3600,
            "a week less the hour the clocks skip"
        );
    }
}
