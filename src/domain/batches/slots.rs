//! Brewing slots: dates the Crew has set aside for brewing, before or after it knows what to
//! brew. Any Member plans a slot, moves it or cancels it.
//!
//! A slot is a calendar date in the Crew's time zone, with no time of day. There is at most one
//! slot per date, and none is planned or moved to a date before today.
//!
//! A newly planned slot is announced to the rest of the Crew as [`SlotPlanned`]; moving or
//! cancelling one is not.

use jiff::civil::Date;

use super::Batches;
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

/// A date the Crew has set aside for brewing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: SlotId,
    pub date: Date,
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

/// A slot as it is stored. Its date was checked when it was stored.
fn slot_of(id: i64, date: &str) -> Result<Slot, sqlx::Error> {
    Ok(Slot {
        id: SlotId(id),
        date: parse_date(date)?,
    })
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
        sqlx::query!(
            r#"SELECT id AS "id!", date FROM brewing_slots WHERE id = ?"#,
            id.0
        )
        .fetch_optional(&self.db)
        .await?
        .map(|row| slot_of(row.id, &row.date))
        .transpose()
    }

    /// The upcoming slots, soonest first: those dated today or later whose Batch, if they have
    /// one, isn't brewed yet.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn upcoming_slots(&self) -> Result<Vec<Slot>, sqlx::Error> {
        let today = self.clock.today().to_string();
        sqlx::query!(
            r#"SELECT brewing_slots.id AS "id!", brewing_slots.date
               FROM brewing_slots
               LEFT JOIN batches ON batches.number = brewing_slots.batch_number
               WHERE brewing_slots.date >= ? AND batches.brew_date IS NULL
               ORDER BY brewing_slots.date"#,
            today,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(|row| slot_of(row.id, &row.date))
        .collect()
    }
}

/// Slots for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{Slot, SlotId};

    /// The slot `id` on `date` (`2026-11-21`).
    ///
    /// # Panics
    ///
    /// If the fixture's date is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn slot(id: i64, date: &str) -> Slot {
        Slot {
            id: SlotId(id),
            date: date.parse().expect("fixture dates are valid"),
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

    use super::{MoveError, SlotError, SlotId, SlotPlanned, check_date};
    use crate::domain::batches::Batches;
    use crate::domain::channel::Channel;
    use crate::domain::clock::ManualClock;
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
            .upcoming_slots()
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
    async fn a_slot_whose_date_has_passed_is_no_longer_upcoming(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, clock) = service(db);
        batches.plan_slot(josef, date(2026, 11, 11)).await.unwrap();
        batches.plan_slot(josef, date(2026, 11, 12)).await.unwrap();

        // Half an hour later it's Thursday in Prague.
        clock.advance(Duration::from_mins(30));

        assert_eq!(dates(&batches).await, [date(2026, 11, 12)]);
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
