//! The Crew's Batches: the brewery log. Each Batch is one brew, with an automatic Batch number, a
//! name, free-text Notes and who added it.
//!
//! Batches emit no events and send no notifications. Any Member may add, edit or delete any
//! Batch; nothing here checks who added it.

use jiff::civil::{Date, DateTime};
use sqlx::SqlitePool;

use crate::domain::clock::{Clock, crew_time_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, MemberId};

/// The longest Batch name, in characters.
pub const NAME_MAX: usize = 100;

/// The longest Notes, in characters. Line breaks count.
pub const NOTES_MAX: usize = 5000;

/// How many Batches a page of the log lists. The same size as a page of Quotes.
pub const PAGE_SIZE: usize = 25;

/// A Batch's number: #1, #2, … One more than the highest ever used, and never used again once
/// its Batch is deleted, so a number always means the same brew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BatchNumber(i64);

impl BatchNumber {
    /// A number a client sent back. Nothing about it is trusted until the Batch is found.
    #[must_use]
    pub const fn from_client(number: i64) -> Self {
        Self(number)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// What a Batch is called: trimmed, non-blank and at most [`NAME_MAX`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchName(String);

impl BatchName {
    /// Trims `input` and checks it can name a Batch.
    ///
    /// # Errors
    ///
    /// Returns [`NameError::Blank`] or [`NameError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, NameError> {
        let name = input.trim();
        if name.is_empty() {
            Err(NameError::Blank)
        } else if name.chars().count() > NAME_MAX {
            Err(NameError::TooLong)
        } else {
            Ok(Self(name.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the Batch name is blank")]
    Blank,
    #[error("the Batch name is longer than {NAME_MAX} characters")]
    TooLong,
}

/// Free text about a Batch: trimmed, possibly empty, and at most [`NOTES_MAX`] characters. Line
/// breaks are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes(String);

impl Notes {
    /// Trims `input` and checks it can be a Batch's Notes.
    ///
    /// # Errors
    ///
    /// Returns [`NotesError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, NotesError> {
        let notes = input.trim();
        if notes.chars().count() > NOTES_MAX {
            Err(NotesError::TooLong)
        } else {
            Ok(Self(notes.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotesError {
    #[error("the Notes are longer than {NOTES_MAX} characters")]
    TooLong,
}

/// Where a Batch is: Planned, Brewed or Bottled. It follows from what has been recorded, so no
/// Member sets it by hand, and it is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Planned,
    Brewed,
    Bottled,
}

impl Stage {
    /// The Stage of a Batch brewed on `brew_date` and bottled on `bottling_date`: Bottled once
    /// it has a bottling date, Brewed once it has a brew date, Planned until then.
    #[must_use]
    pub const fn of(brew_date: Option<Date>, bottling_date: Option<Date>) -> Self {
        match (brew_date, bottling_date) {
            (_, Some(_)) => Self::Bottled,
            (Some(_), None) => Self::Brewed,
            (None, None) => Self::Planned,
        }
    }
}

/// Why a Batch couldn't be read, edited or deleted.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    /// There's no such Batch: it never existed, or a Member deleted it.
    #[error("no such Batch")]
    NotFound,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// A Batch's name and Notes, as a Member adds or edits them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBatch {
    pub name: BatchName,
    pub notes: Notes,
}

/// An instant a Batch was added or edited at, shown as a date and time in the Crew's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(i64);

impl Instant {
    /// The date and time it showed on the clock in the Crew's time zone.
    #[must_use]
    pub fn crew_time(self) -> DateTime {
        crew_time_of_unix_seconds(self.0)
    }
}

/// Who last edited a Batch, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edited {
    pub by: DisplayName,
    pub at: Instant,
}

/// A Batch as the log and its own page show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub number: BatchNumber,
    pub name: BatchName,
    pub notes: Notes,
    pub added_by: DisplayName,
    pub added_at: Instant,
    /// None until a Member edits it.
    pub edited: Option<Edited>,
}

impl Batch {
    /// Where the Batch is. Until Brew day and Bottling can be recorded, every Batch is Planned.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        Stage::of(None, None)
    }
}

/// A Batch as it is read back from the database, with the display names of who added and last
/// edited it.
struct BatchRow {
    number: i64,
    name: String,
    notes: String,
    added_by: String,
    added_at: i64,
    edited_by: Option<String>,
    edited_at: Option<i64>,
}

impl From<BatchRow> for Batch {
    fn from(row: BatchRow) -> Self {
        Self {
            number: BatchNumber(row.number),
            // Checked when it was stored.
            name: BatchName(row.name),
            notes: Notes(row.notes),
            added_by: DisplayName::from_stored(row.added_by),
            added_at: Instant(row.added_at),
            edited: row.edited_by.zip(row.edited_at).map(|(by, at)| Edited {
                by: DisplayName::from_stored(by),
                at: Instant(at),
            }),
        }
    }
}

/// One page of the log, newest first by Batch number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchPage {
    pub batches: Vec<Batch>,
    /// Which page this is, from 1.
    pub page: usize,
    /// How many pages there are; at least 1, even with nothing to list.
    pub pages: usize,
}

/// Adds, edits and deletes Batches, and lists the log a page at a time. Any Member may change
/// any Batch.
#[derive(Debug, Clone)]
pub struct Batches {
    db: SqlitePool,
    clock: Clock,
}

impl Batches {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock) -> Self {
        Self { db, clock }
    }

    /// Adds `batch` on behalf of `added_by`, now, with the next Batch number. Nobody is
    /// notified.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn add(
        &self,
        added_by: MemberId,
        batch: &NewBatch,
    ) -> Result<BatchNumber, sqlx::Error> {
        let added_at = unix_seconds(self.clock.now());
        let member = added_by.get();
        let number = sqlx::query_scalar!(
            r#"INSERT INTO batches (name, notes, added_by, added_at)
               VALUES (?, ?, ?, ?) RETURNING number AS "number!""#,
            batch.name.0,
            batch.notes.0,
            member,
            added_at,
        )
        .fetch_one(&self.db)
        .await?;
        Ok(BatchNumber(number))
    }

    /// The `page`th page of the log, newest first by Batch number. A page past the last is the
    /// last, and a page below 1 is the first.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn page(&self, page: usize) -> Result<BatchPage, sqlx::Error> {
        let count = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM batches"#)
            .fetch_one(&self.db)
            .await?;
        let total = usize::try_from(count).unwrap_or(0);
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        let page = page.clamp(1, pages);
        let limit = i64::try_from(PAGE_SIZE).unwrap_or(i64::MAX);
        let offset =
            i64::try_from(page.saturating_sub(1).saturating_mul(PAGE_SIZE)).unwrap_or(i64::MAX);
        let batches = sqlx::query_as!(
            BatchRow,
            r#"SELECT batches.number AS "number!", batches.name, batches.notes,
                      added.display_name AS added_by, batches.added_at,
                      edited.display_name AS "edited_by?", batches.edited_at
               FROM batches
               JOIN members AS added ON added.id = batches.added_by
               LEFT JOIN members AS edited ON edited.id = batches.edited_by
               ORDER BY batches.number DESC
               LIMIT ? OFFSET ?"#,
            limit,
            offset,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(Batch::from)
        .collect();
        Ok(BatchPage {
            batches,
            page,
            pages,
        })
    }

    /// The Batch `number`.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn get(&self, number: BatchNumber) -> Result<Batch, BatchError> {
        let row = sqlx::query_as!(
            BatchRow,
            r#"SELECT batches.number AS "number!", batches.name, batches.notes,
                      added.display_name AS added_by, batches.added_at,
                      edited.display_name AS "edited_by?", batches.edited_at
               FROM batches
               JOIN members AS added ON added.id = batches.added_by
               LEFT JOIN members AS edited ON edited.id = batches.edited_by
               WHERE batches.number = ?"#,
            number.0,
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(BatchError::NotFound)?;
        Ok(Batch::from(row))
    }

    /// Gives the Batch `number` the name and Notes of `batch`, edited by `edited_by` now. Its
    /// number, and who added it when, stay as they were.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn edit(
        &self,
        number: BatchNumber,
        edited_by: MemberId,
        batch: &NewBatch,
    ) -> Result<(), BatchError> {
        let edited_at = unix_seconds(self.clock.now());
        let member = edited_by.get();
        let done = sqlx::query!(
            "UPDATE batches SET name = ?, notes = ?, edited_by = ?, edited_at = ?
             WHERE number = ?",
            batch.name.0,
            batch.notes.0,
            member,
            edited_at,
            number.0,
        )
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(BatchError::NotFound);
        }
        Ok(())
    }

    /// Deletes the Batch `number` for good. Its number is never used again.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn delete(&self, number: BatchNumber) -> Result<(), BatchError> {
        let done = sqlx::query!("DELETE FROM batches WHERE number = ?", number.0)
            .execute(&self.db)
            .await?;
        if done.rows_affected() == 0 {
            return Err(BatchError::NotFound);
        }
        Ok(())
    }
}

/// Batches for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::DateTime;

    use super::{Batch, BatchName, BatchNumber, Edited, Instant, Notes};
    use crate::domain::clock::unix_seconds_at_crew_time;
    use crate::domain::members::DisplayName;

    fn instant(at: &str) -> Instant {
        let at: DateTime = at.parse().expect("fixture times are valid");
        Instant(unix_seconds_at_crew_time(at).expect("fixture times exist"))
    }

    fn name(name: &str) -> DisplayName {
        DisplayName::parse(name).expect("fixture names are valid")
    }

    /// Batch `number` called `batch_name` with `notes`, added by `added_by` at `added` in Prague
    /// (`2026-10-24T18:00`).
    ///
    /// # Panics
    ///
    /// If the fixture's name, Notes, time or Member is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn batch(number: i64, batch_name: &str, notes: &str, added_by: &str, added: &str) -> Batch {
        Batch {
            number: BatchNumber(number),
            name: BatchName::parse(batch_name).expect("fixture names are valid"),
            notes: Notes::parse(notes).expect("fixture Notes are valid"),
            added_by: name(added_by),
            added_at: instant(added),
            edited: None,
        }
    }

    /// `batch`, last edited by `by` at `at` in Prague.
    ///
    /// # Panics
    ///
    /// If the fixture's Member or time is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn edited(batch: Batch, by: &str, at: &str) -> Batch {
        Batch {
            edited: Some(Edited {
                by: name(by),
                at: instant(at),
            }),
            ..batch
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use jiff::civil::date;
    use sqlx::SqlitePool;

    use super::{
        BatchError, BatchName, BatchNumber, NAME_MAX, NOTES_MAX, NameError, NewBatch, Notes,
        NotesError, PAGE_SIZE, Stage,
    };
    use crate::domain::clock::ManualClock;
    use crate::domain::members::DisplayName;
    use crate::domain::members::fixtures::add_member;

    /// 2026-10-24T08:00:00Z: 10:00 in Prague.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    fn service(db: SqlitePool) -> (super::Batches, ManualClock) {
        let clock = ManualClock::starting_at(saturday_morning());
        let batches = super::Batches::new(db, clock.clock());
        (batches, clock)
    }

    fn new_batch(name: &str, notes: &str) -> NewBatch {
        NewBatch {
            name: BatchName::parse(name).unwrap(),
            notes: Notes::parse(notes).unwrap(),
        }
    }

    #[test]
    fn trims_a_name() {
        assert_eq!(
            BatchName::parse("  Únětický Vzor 10.7° \n")
                .unwrap()
                .as_str(),
            "Únětický Vzor 10.7°"
        );
    }

    #[test]
    fn refuses_a_blank_name() {
        assert_eq!(BatchName::parse(" \n\t"), Err(NameError::Blank));
    }

    #[test]
    fn limits_a_name_by_characters_not_bytes() {
        let longest = "š".repeat(NAME_MAX);
        assert_eq!(BatchName::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            BatchName::parse(&format!("{longest}š")),
            Err(NameError::TooLong)
        );
    }

    #[test]
    fn notes_may_be_empty_and_keep_their_line_breaks() {
        assert!(Notes::parse(" \n ").unwrap().is_empty());
        assert_eq!(
            Notes::parse("\n Dry hopped on day 4.\nTasted grassy.  \n")
                .unwrap()
                .as_str(),
            "Dry hopped on day 4.\nTasted grassy."
        );
    }

    #[test]
    fn limits_notes_by_characters_not_bytes() {
        let longest = "ř".repeat(NOTES_MAX);
        assert_eq!(Notes::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            Notes::parse(&format!("{longest}ř")),
            Err(NotesError::TooLong)
        );
    }

    #[test]
    fn the_stage_follows_from_the_dates_bottled_over_brewed_over_planned() {
        let brewed = Some(date(2026, 10, 3));
        let bottled = Some(date(2026, 10, 17));

        assert_eq!(Stage::of(None, None), Stage::Planned);
        assert_eq!(Stage::of(brewed, None), Stage::Brewed);
        assert_eq!(Stage::of(brewed, bottled), Stage::Bottled);
        assert_eq!(Stage::of(None, bottled), Stage::Bottled);
    }

    #[sqlx::test]
    async fn an_added_batch_is_planned_with_who_added_it_when(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let (batches, _) = service(db);

        let number = batches
            .add(
                ondra,
                &new_batch(" Pilsner ", "Saaz only.\nOpen fermenter."),
            )
            .await
            .unwrap();

        let batch = batches.get(number).await.unwrap();
        assert_eq!(batch.number, number);
        assert_eq!(batch.name.as_str(), "Pilsner");
        assert_eq!(batch.notes.as_str(), "Saaz only.\nOpen fermenter.");
        assert_eq!(batch.added_by.as_str(), "Ondra");
        assert_eq!(
            batch.added_at.crew_time(),
            date(2026, 10, 24).at(10, 0, 0, 0)
        );
        assert_eq!(batch.edited, None);
        assert_eq!(batch.stage(), Stage::Planned);
    }

    #[sqlx::test]
    async fn batch_numbers_count_up_from_one_and_are_never_reused(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);

        let first = batches.add(josef, &new_batch("One", "")).await.unwrap();
        let second = batches.add(josef, &new_batch("Two", "")).await.unwrap();
        batches.delete(second).await.unwrap();
        let third = batches.add(josef, &new_batch("Three", "")).await.unwrap();

        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert_eq!(third.get(), 3);
    }

    #[sqlx::test]
    async fn the_log_is_newest_first_and_a_page_past_the_last_is_the_last(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        for n in 1..=PAGE_SIZE + 1 {
            batches
                .add(josef, &new_batch(&format!("Batch {n:02}"), ""))
                .await
                .unwrap();
        }
        let names = |page: &super::BatchPage| -> Vec<String> {
            page.batches
                .iter()
                .map(|batch| batch.name.as_str().to_owned())
                .collect()
        };

        let first = batches.page(1).await.unwrap();
        assert_eq!((first.page, first.pages), (1, 2));
        assert_eq!(first.batches.len(), PAGE_SIZE);
        assert_eq!(names(&first)[0], "Batch 26");
        assert_eq!(names(&first)[PAGE_SIZE - 1], "Batch 02");

        let last = batches.page(2).await.unwrap();
        assert_eq!(names(&last), ["Batch 01"]);
        assert_eq!(batches.page(99).await.unwrap(), last);
        assert_eq!(batches.page(0).await.unwrap(), first);
    }

    #[sqlx::test]
    async fn an_empty_log_is_one_empty_page(db: SqlitePool) {
        let (batches, _) = service(db);

        let page = batches.page(3).await.unwrap();

        assert_eq!(page.batches, []);
        assert_eq!((page.page, page.pages), (1, 1));
    }

    #[sqlx::test]
    async fn any_member_edits_a_batch_keeping_its_number_and_who_added_it(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (batches, clock) = service(db);
        let number = batches
            .add(josef, &new_batch("Pilsnr", "Typo."))
            .await
            .unwrap();
        clock.advance(Duration::from_hours(2));

        batches
            .edit(number, simon, &new_batch(" Pilsner ", ""))
            .await
            .unwrap();

        let batch = batches.get(number).await.unwrap();
        assert_eq!(batch.number, number);
        assert_eq!(batch.name.as_str(), "Pilsner");
        assert!(batch.notes.is_empty());
        assert_eq!(batch.added_by.as_str(), "Josef");
        assert_eq!(
            batch.added_at.crew_time(),
            date(2026, 10, 24).at(10, 0, 0, 0)
        );
        let edited = batch.edited.unwrap();
        assert_eq!(edited.by, DisplayName::parse("Šimon").unwrap());
        assert_eq!(edited.at.crew_time(), date(2026, 10, 24).at(12, 0, 0, 0));
    }

    #[sqlx::test]
    async fn a_deleted_batch_is_gone_from_everywhere(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let kept = batches.add(josef, &new_batch("Kept", "")).await.unwrap();
        let gone = batches.add(josef, &new_batch("Gone", "")).await.unwrap();

        batches.delete(gone).await.unwrap();

        assert!(matches!(batches.get(gone).await, Err(BatchError::NotFound)));
        let page = batches.page(1).await.unwrap();
        let numbers: Vec<BatchNumber> = page.batches.iter().map(|batch| batch.number).collect();
        assert_eq!(numbers, [kept]);
    }

    #[sqlx::test]
    async fn a_batch_that_isnt_there_cant_be_read_edited_or_deleted(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let deleted = batches
            .add(josef, &new_batch("Soon gone", ""))
            .await
            .unwrap();
        batches.delete(deleted).await.unwrap();

        for number in [deleted, BatchNumber::from_client(999)] {
            assert!(matches!(
                batches.get(number).await,
                Err(BatchError::NotFound)
            ));
            assert!(matches!(
                batches.edit(number, josef, &new_batch("New", "")).await,
                Err(BatchError::NotFound)
            ));
            assert!(matches!(
                batches.delete(number).await,
                Err(BatchError::NotFound)
            ));
        }
    }
}
