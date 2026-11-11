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

use jiff::civil::Date;

use super::{BatchName, BatchNumber, Batches};
use crate::domain::clock::unix_seconds;
use crate::domain::members::{MemberId, everyone_but, parse_date};

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
    /// Crew.
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
        let id = taken_on(
            sqlx::query_scalar!(
                r#"INSERT INTO brewing_slots (date, created_by, created_at) VALUES (?, ?, ?)
                   RETURNING id AS "id!""#,
                day,
                member,
                created_at,
            )
            .fetch_one(&self.db)
            .await,
            date,
        )?;
        let slot = SlotId(id);
        let notify = everyone_but(&self.db, planned_by).await?;
        self.events.emit(SlotPlanned {
            slot,
            date,
            planned_by,
            notify,
        });
        Ok(slot)
    }

    /// Moves the slot `id` to `date`. Moving it to the date it's on changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`MoveError::NotFound`] if there's no slot `id`, or [`MoveError::Date`] with
    /// [`SlotError::Past`] for a date before today, [`SlotError::Taken`] if another slot is on
    /// `date`, or a database error.
    pub async fn move_slot(&self, id: SlotId, date: Date) -> Result<(), MoveError> {
        check_date(date, self.clock.today())?;
        let day = date.to_string();
        let done = taken_on(
            sqlx::query!("UPDATE brewing_slots SET date = ? WHERE id = ?", day, id.0)
                .execute(&self.db)
                .await,
            date,
        )?;
        if done.rows_affected() == 0 {
            return Err(MoveError::NotFound);
        }
        Ok(())
    }

    /// Cancels the slot `id` for good. A slot already cancelled stays cancelled.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn cancel_slot(&self, id: SlotId) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM brewing_slots WHERE id = ?", id.0)
            .execute(&self.db)
            .await?;
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
    /// before waits for a slot again.
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
        sqlx::query!(
            "UPDATE brewing_slots SET batch_number = NULL WHERE batch_number = ?",
            number.0
        )
        .execute(&mut *tx)
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
        tx.commit().await?;
        Ok(())
    }

    /// Takes the Batch off the slot `id`, which then has nothing chosen. A slot with nothing
    /// chosen stays that way.
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
        tx.commit().await?;
        Ok(())
    }

    /// Takes the Planned Batch `number` off its slot, to wait for another. A Batch without a slot
    /// stays without one.
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
        sqlx::query!(
            "UPDATE brewing_slots SET batch_number = NULL WHERE batch_number = ?",
            number.0
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
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

    use jiff::civil::{Date, date};
    use sqlx::SqlitePool;
    use tokio::sync::broadcast;

    use super::{
        LinkError, MoveError, PlannedBatch, SlotError, SlotId, SlotPlanned, SlotState, check_date,
    };
    use crate::domain::batches::{BatchName, BatchNumber, Batches, BrewDay, NewBatch, Notes};
    use crate::domain::channel::Channel;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::MemberId;
    use crate::domain::members::fixtures::add_member;

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
        let channel = Channel::new();
        let planned = channel.subscribe();
        let batches = Batches::new(db, clock.clock(), channel.emitter(identity));
        (batches, clock, planned)
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
}
