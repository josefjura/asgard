//! The Crew's Batches: the brewery log. Each Batch is one brew, with an automatic Batch number, a
//! name, free-text Notes, its Brew day and Bottling, an optional Recipe, and who added it.
//!
//! Brewing slots, the dates set aside for brewing, live here too, since they only exist for
//! Batches.
//!
//! The only event is a newly planned Brewing slot; Batches themselves send no notifications. Any
//! Member may add, edit or delete any Batch; nothing here checks who added it.

use jiff::civil::{Date, DateTime};
use sqlx::{SqliteConnection, SqlitePool};

use crate::domain::channel::Emitter;
use crate::domain::clock::{Clock, crew_time_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, MemberId, parse_date};
use crate::domain::reminders::Reminders;
use slots::Missing;

mod numbers;
mod recipe;
mod slots;

pub use numbers::{
    Bottling, BrewDay, FG_MAX, FG_MIN, Gravity, GravityError, OG_MAX, OG_MIN, Percent, RuleError,
    VOLUME_MAX, Volume, VolumeError, abv, attenuation, check,
};
pub use recipe::{
    AttachedRecipe, Celsius, Ebc, FermentableLine, HopLine, Ibu, Litres, MashStepLine, Mass,
    Minutes, RECIPE_MAX_BYTES, Range, Recipe, RecipeError, RecipeSummary, StyleSummary, Targets,
    YeastLine,
};
pub use slots::{
    LinkError, MoveError, PlannedBatch, Slot, SlotBatch, SlotError, SlotId, SlotPlanned, SlotState,
    reminder_text, reminder_time,
};

/// Brewing slots for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub use slots::fixtures as slot_fixtures;

/// Recipes for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub use recipe::fixtures as recipe_fixtures;

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
    /// The name a Batch takes from `recipe` when a Member leaves it blank: the Recipe's name, cut
    /// to [`NAME_MAX`] characters. None if the Recipe's name is blank too.
    #[must_use]
    pub fn of_recipe(recipe: &Recipe) -> Option<Self> {
        let name: String = recipe.summary().name.chars().take(NAME_MAX).collect();
        Self::parse(&name).ok()
    }

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
/// breaks are kept, as `\n`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes(String);

impl Notes {
    /// Trims `input` and checks it can be a Batch's Notes.
    ///
    /// # Errors
    ///
    /// Returns [`NotesError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, NotesError> {
        // Browsers send each line break as CRLF but count it as one character against the
        // textarea's maxlength, so it is one character here too.
        let notes = input.trim().replace("\r\n", "\n");
        if notes.chars().count() > NOTES_MAX {
            Err(NotesError::TooLong)
        } else {
            Ok(Self(notes))
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

/// Why a Batch's Brew day or Bottling couldn't be recorded.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// There's no such Batch: it never existed, or a Member deleted it.
    #[error("no such Batch")]
    NotFound,
    /// The new values don't agree with what the Batch already has.
    #[error(transparent)]
    Rule(#[from] RuleError),
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
pub struct Moment(i64);

impl Moment {
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
    pub at: Moment,
}

/// A Batch as the log and its own page show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub number: BatchNumber,
    pub name: BatchName,
    pub notes: Notes,
    pub added_by: DisplayName,
    pub added_at: Moment,
    /// None until a Member edits it.
    pub edited: Option<Edited>,
    pub brew_day: BrewDay,
    pub bottling: Bottling,
    /// The date of the Brewing slot it's brewed on, or None while it waits for one.
    pub slot: Option<Date>,
}

impl Batch {
    /// Where the Batch is, from its brew and bottling dates.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        Stage::of(self.brew_day.date, self.bottling.date)
    }

    /// The ABV worked out from the OG and FG, once both are in.
    #[must_use]
    pub fn abv(&self) -> Option<Percent> {
        abv(self.brew_day.og?, self.bottling.fg?)
    }

    /// The apparent attenuation worked out from the OG and FG, once both are in.
    #[must_use]
    pub fn attenuation(&self) -> Option<Percent> {
        attenuation(self.brew_day.og?, self.bottling.fg?)
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
    brew_date: Option<String>,
    og: Option<i64>,
    volume: Option<i64>,
    bottling_date: Option<String>,
    fg: Option<i64>,
    slot_date: Option<String>,
}

impl TryFrom<BatchRow> for Batch {
    type Error = sqlx::Error;

    fn try_from(row: BatchRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            number: BatchNumber(row.number),
            // Checked when it was stored.
            name: BatchName(row.name),
            notes: Notes(row.notes),
            added_by: DisplayName::from_stored(row.added_by),
            added_at: Moment(row.added_at),
            edited: row.edited_by.zip(row.edited_at).map(|(by, at)| Edited {
                by: DisplayName::from_stored(by),
                at: Moment(at),
            }),
            brew_day: brew_day(row.brew_date.as_deref(), row.og, row.volume)?,
            bottling: bottling(row.bottling_date.as_deref(), row.fg)?,
            slot: row.slot_date.as_deref().map(parse_date).transpose()?,
        })
    }
}

/// A Brew day as it is stored. Each range was checked when it was stored.
fn brew_day(
    date: Option<&str>,
    og: Option<i64>,
    volume: Option<i64>,
) -> Result<BrewDay, sqlx::Error> {
    Ok(BrewDay {
        date: date.map(parse_date).transpose()?,
        og: og.map(Gravity::from_stored),
        volume: volume.map(Volume::from_stored),
    })
}

/// A Bottling as it is stored. The FG's range was checked when it was stored.
fn bottling(date: Option<&str>, fg: Option<i64>) -> Result<Bottling, sqlx::Error> {
    Ok(Bottling {
        date: date.map(parse_date).transpose()?,
        fg: fg.map(Gravity::from_stored),
    })
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
    events: Emitter<SlotPlanned>,
    /// Where each Brewing slot's Reminder is kept.
    reminders: Reminders,
}

impl Batches {
    #[must_use]
    pub const fn new(
        db: SqlitePool,
        clock: Clock,
        events: Emitter<SlotPlanned>,
        reminders: Reminders,
    ) -> Self {
        Self {
            db,
            clock,
            events,
            reminders,
        }
    }

    /// Adds `batch` with its `recipe`, if it has one, on behalf of `added_by`, now, with the
    /// next Batch number. Nobody is notified.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn add(
        &self,
        added_by: MemberId,
        batch: &NewBatch,
        recipe: Option<&Recipe>,
    ) -> Result<BatchNumber, sqlx::Error> {
        let added_at = unix_seconds(self.clock.now());
        let member = added_by.get();
        let recipe = recipe.map(Recipe::bytes);
        let number = sqlx::query_scalar!(
            r#"INSERT INTO batches (name, notes, recipe, added_by, added_at)
               VALUES (?, ?, ?, ?, ?) RETURNING number AS "number!""#,
            batch.name.0,
            batch.notes.0,
            recipe,
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
                      edited.display_name AS "edited_by?", batches.edited_at,
                      batches.brew_date, batches.og, batches.volume, batches.bottling_date,
                      batches.fg, brewing_slots.date AS "slot_date?"
               FROM batches
               JOIN members AS added ON added.id = batches.added_by
               LEFT JOIN members AS edited ON edited.id = batches.edited_by
               LEFT JOIN brewing_slots ON brewing_slots.batch_number = batches.number
               ORDER BY batches.number DESC
               LIMIT ? OFFSET ?"#,
            limit,
            offset,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(Batch::try_from)
        .collect::<Result<_, _>>()?;
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
                      edited.display_name AS "edited_by?", batches.edited_at,
                      batches.brew_date, batches.og, batches.volume, batches.bottling_date,
                      batches.fg, brewing_slots.date AS "slot_date?"
               FROM batches
               JOIN members AS added ON added.id = batches.added_by
               LEFT JOIN members AS edited ON edited.id = batches.edited_by
               LEFT JOIN brewing_slots ON brewing_slots.batch_number = batches.number
               WHERE batches.number = ?"#,
            number.0,
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(BatchError::NotFound)?;
        Ok(Batch::try_from(row)?)
    }

    /// Today in the Crew's time zone, which the Brew day and Bottling forms start at.
    #[must_use]
    pub fn today(&self) -> Date {
        self.clock.today()
    }

    /// Records `new` as the Batch `number`'s Brew day in place of what it had, edited by
    /// `edited_by` now. A missing value clears the one recorded.
    ///
    /// A Batch without a slot that is brewed on the date of a slot with nothing chosen is linked
    /// to that slot, so the plan follows what was brewed.
    ///
    /// # Errors
    ///
    /// Returns [`RecordError::NotFound`] if there's no Batch `number`, [`RecordError::Rule`] if
    /// `new` disagrees with the Batch's Bottling, or a database error.
    pub async fn record_brew_day(
        &self,
        number: BatchNumber,
        edited_by: MemberId,
        new: &BrewDay,
    ) -> Result<(), RecordError> {
        let mut tx = self.db.begin().await?;
        let row = sqlx::query!(
            "SELECT bottling_date, fg FROM batches WHERE number = ?",
            number.0,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RecordError::NotFound)?;
        check(new, &bottling(row.bottling_date.as_deref(), row.fg)?)?;
        let edited_at = unix_seconds(self.clock.now());
        let member = edited_by.get();
        let brew_date = new.date.map(|date| date.to_string());
        let og = new.og.map(Gravity::thousandths);
        let volume = new.volume.map(Volume::decilitres);
        sqlx::query!(
            "UPDATE batches SET brew_date = ?, og = ?, volume = ?, edited_by = ?, edited_at = ?
             WHERE number = ?",
            brew_date,
            og,
            volume,
            member,
            edited_at,
            number.0,
        )
        .execute(&mut *tx)
        .await?;
        if let Some(brew_date) = brew_date {
            let linked = sqlx::query_scalar!(
                r#"UPDATE brewing_slots SET batch_number = ?
                   WHERE date = ? AND batch_number IS NULL
                     AND NOT EXISTS (SELECT 1 FROM brewing_slots WHERE batch_number = ?)
                   RETURNING id AS "id!""#,
                number.0,
                brew_date,
                number.0,
            )
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(linked) = linked {
                self.follow(&mut tx, SlotId::from_client(linked), Missing::Leave)
                    .await?;
            }
        }
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }

    /// Records `new` as the Batch `number`'s Bottling in place of what it had, edited by
    /// `edited_by` now. A missing value clears the one recorded.
    ///
    /// # Errors
    ///
    /// Returns [`RecordError::NotFound`] if there's no Batch `number`, [`RecordError::Rule`] if
    /// `new` disagrees with the Batch's Brew day, or a database error.
    pub async fn record_bottling(
        &self,
        number: BatchNumber,
        edited_by: MemberId,
        new: &Bottling,
    ) -> Result<(), RecordError> {
        let mut tx = self.db.begin().await?;
        let row = sqlx::query!(
            "SELECT brew_date, og, volume FROM batches WHERE number = ?",
            number.0,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RecordError::NotFound)?;
        check(
            &brew_day(row.brew_date.as_deref(), row.og, row.volume)?,
            new,
        )?;
        let edited_at = unix_seconds(self.clock.now());
        let member = edited_by.get();
        let bottling_date = new.date.map(|date| date.to_string());
        let fg = new.fg.map(Gravity::thousandths);
        sqlx::query!(
            "UPDATE batches SET bottling_date = ?, fg = ?, edited_by = ?, edited_at = ?
             WHERE number = ?",
            bottling_date,
            fg,
            member,
            edited_at,
            number.0,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
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
        let mut tx = self.db.begin().await?;
        let done = sqlx::query!(
            "UPDATE batches SET name = ?, notes = ?, edited_by = ?, edited_at = ?
             WHERE number = ?",
            batch.name.0,
            batch.notes.0,
            member,
            edited_at,
            number.0,
        )
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Err(BatchError::NotFound);
        }
        // Its slot's Reminder names it.
        if let Some(slot) = slot_of(&mut tx, number).await? {
            self.follow(&mut tx, slot, Missing::Leave).await?;
        }
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }

    /// The Recipe attached to the Batch `number`, read from the file as it was uploaded, or None
    /// if it has none.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn recipe(&self, number: BatchNumber) -> Result<Option<AttachedRecipe>, BatchError> {
        let row = sqlx::query!("SELECT recipe FROM batches WHERE number = ?", number.0)
            .fetch_optional(&self.db)
            .await?
            .ok_or(BatchError::NotFound)?;
        Ok(row.recipe.as_deref().map(AttachedRecipe::read))
    }

    /// Attaches `recipe` to the Batch `number`, in place of any it had, edited by `edited_by`
    /// now. Its name stays as it was.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn attach_recipe(
        &self,
        number: BatchNumber,
        edited_by: MemberId,
        recipe: &Recipe,
    ) -> Result<(), BatchError> {
        self.set_recipe(number, edited_by, Some(recipe.bytes()))
            .await
    }

    /// Removes the Batch `number`'s Recipe, edited by `edited_by` now. Removing it from a Batch
    /// without one changes only who edited it when.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn remove_recipe(
        &self,
        number: BatchNumber,
        edited_by: MemberId,
    ) -> Result<(), BatchError> {
        self.set_recipe(number, edited_by, None).await
    }

    async fn set_recipe(
        &self,
        number: BatchNumber,
        edited_by: MemberId,
        recipe: Option<&[u8]>,
    ) -> Result<(), BatchError> {
        let edited_at = unix_seconds(self.clock.now());
        let member = edited_by.get();
        let done = sqlx::query!(
            "UPDATE batches SET recipe = ?, edited_by = ?, edited_at = ? WHERE number = ?",
            recipe,
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

    /// Deletes the Batch `number` for good. Its number is never used again. Its slot, if it had
    /// one, has nothing chosen, and the slot's pending Reminder says so.
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::NotFound`] if there's no Batch `number`, or a database error.
    pub async fn delete(&self, number: BatchNumber) -> Result<(), BatchError> {
        let mut tx = self.db.begin().await?;
        let slot = slot_of(&mut tx, number).await?;
        let done = sqlx::query!("DELETE FROM batches WHERE number = ?", number.0)
            .execute(&mut *tx)
            .await?;
        if done.rows_affected() == 0 {
            return Err(BatchError::NotFound);
        }
        if let Some(slot) = slot {
            self.follow(&mut tx, slot, Missing::Leave).await?;
        }
        tx.commit().await?;
        self.reminders.announce_change();
        Ok(())
    }
}

/// The Brewing slot the Batch `number` is on, if any.
async fn slot_of(
    conn: &mut SqliteConnection,
    number: BatchNumber,
) -> Result<Option<SlotId>, sqlx::Error> {
    let slot = sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM brewing_slots WHERE batch_number = ?"#,
        number.0
    )
    .fetch_optional(conn)
    .await?;
    Ok(slot.map(SlotId::from_client))
}

/// Batches for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::DateTime;

    use super::{
        Batch, BatchName, BatchNumber, Bottling, BrewDay, Edited, Gravity, Moment, Notes, Volume,
    };
    use crate::domain::clock::unix_seconds_at_crew_time;
    use crate::domain::members::DisplayName;

    fn instant(at: &str) -> Moment {
        let at: DateTime = at.parse().expect("fixture times are valid");
        Moment(unix_seconds_at_crew_time(at).expect("fixture times exist"))
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
            brew_day: BrewDay::default(),
            bottling: Bottling::default(),
            slot: None,
        }
    }

    /// `batch`, to be brewed on the slot on `on` (`2026-11-21`).
    ///
    /// # Panics
    ///
    /// If the fixture's date is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn slotted(batch: Batch, on: &str) -> Batch {
        Batch {
            slot: Some(on.parse().expect("fixture dates are valid")),
            ..batch
        }
    }

    /// `batch`, brewed on `on` (`2026-10-03`) with `og` (`1.050`) and `volume` (`23.5`).
    ///
    /// # Panics
    ///
    /// If the fixture's date, OG or Volume is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn brewed(batch: Batch, on: &str, og: Option<&str>, volume: Option<&str>) -> Batch {
        Batch {
            brew_day: BrewDay {
                date: Some(on.parse().expect("fixture dates are valid")),
                og: og.map(|og| Gravity::og(og).expect("fixture OGs are valid")),
                volume: volume
                    .map(|volume| Volume::parse(volume).expect("fixture Volumes are valid")),
            },
            ..batch
        }
    }

    /// `batch`, bottled on `on` (`2026-10-17`) with `fg` (`1.010`).
    ///
    /// # Panics
    ///
    /// If the fixture's date or FG is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn bottled(batch: Batch, on: &str, fg: Option<&str>) -> Batch {
        Batch {
            bottling: Bottling {
                date: Some(on.parse().expect("fixture dates are valid")),
                fg: fg.map(|fg| Gravity::fg(fg).expect("fixture FGs are valid")),
            },
            ..batch
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

    use jiff::civil::{Date, date};
    use sqlx::SqlitePool;

    use super::{
        AttachedRecipe, BatchError, BatchName, BatchNumber, Bottling, BrewDay, Gravity, NAME_MAX,
        NOTES_MAX, NameError, NewBatch, Notes, NotesError, PAGE_SIZE, Recipe, RecordError,
        RuleError, Stage, Volume,
    };
    use crate::domain::batches::recipe_fixtures::{LATIN1, SPARSE, UNETICE};
    use crate::domain::clock::ManualClock;
    use crate::domain::members::DisplayName;
    use crate::domain::members::fixtures::add_member;

    /// 2026-10-24T08:00:00Z: 10:00 in Prague.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    fn service(db: SqlitePool) -> (super::Batches, ManualClock) {
        let clock = ManualClock::starting_at(saturday_morning());
        let reminders = crate::domain::reminders::Reminders::new(
            db.clone(),
            clock.clock(),
            crate::domain::channel::Channel::new().emitter(std::convert::identity),
        );
        let batches = super::Batches::new(
            db,
            clock.clock(),
            crate::domain::channel::Channel::new().emitter(std::convert::identity),
            reminders,
        );
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
    fn a_line_break_a_browser_sends_as_crlf_is_one_character() {
        // As many characters as allowed once each CRLF counts as one, but more if it counted two.
        let typed = format!("{}a", "a\n".repeat(NOTES_MAX / 2 - 1));
        let sent = typed.replace('\n', "\r\n");

        let notes = Notes::parse(&sent).unwrap();

        assert_eq!(notes.as_str(), typed);
        assert_eq!(notes.as_str().chars().count(), NOTES_MAX - 1);
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
                None,
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

        let first = batches
            .add(josef, &new_batch("One", ""), None)
            .await
            .unwrap();
        let second = batches
            .add(josef, &new_batch("Two", ""), None)
            .await
            .unwrap();
        batches.delete(second).await.unwrap();
        let third = batches
            .add(josef, &new_batch("Three", ""), None)
            .await
            .unwrap();

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
                .add(josef, &new_batch(&format!("Batch {n:02}"), ""), None)
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
            .add(josef, &new_batch("Pilsnr", "Typo."), None)
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
        let kept = batches
            .add(josef, &new_batch("Kept", ""), None)
            .await
            .unwrap();
        let gone = batches
            .add(josef, &new_batch("Gone", ""), None)
            .await
            .unwrap();

        batches.delete(gone).await.unwrap();

        assert!(matches!(batches.get(gone).await, Err(BatchError::NotFound)));
        let page = batches.page(1).await.unwrap();
        let numbers: Vec<BatchNumber> = page.batches.iter().map(|batch| batch.number).collect();
        assert_eq!(numbers, [kept]);
    }

    fn brew_day(on: Option<Date>, og: Option<&str>, volume: Option<&str>) -> BrewDay {
        BrewDay {
            date: on,
            og: og.map(|og| Gravity::og(og).unwrap()),
            volume: volume.map(|volume| Volume::parse(volume).unwrap()),
        }
    }

    fn bottling(on: Option<Date>, fg: Option<&str>) -> Bottling {
        Bottling {
            date: on,
            fg: fg.map(|fg| Gravity::fg(fg).unwrap()),
        }
    }

    #[sqlx::test]
    async fn brew_day_and_bottling_move_a_batch_through_its_stages_and_back(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let (batches, clock) = service(db);
        let number = batches
            .add(josef, &new_batch("Pilsner", ""), None)
            .await
            .unwrap();
        clock.advance(Duration::from_hours(1));

        let brewed = brew_day(Some(date(2026, 10, 3)), Some("1.050"), Some("23.5"));
        batches
            .record_brew_day(number, ondra, &brewed)
            .await
            .unwrap();
        let batch = batches.get(number).await.unwrap();
        assert_eq!(batch.stage(), Stage::Brewed);
        assert_eq!(batch.brew_day, brewed);
        assert_eq!(batch.edited.as_ref().unwrap().by.as_str(), "Ondra");
        assert_eq!(batch.abv(), None);

        let bottled = bottling(Some(date(2026, 10, 17)), Some("1.010"));
        batches
            .record_bottling(number, josef, &bottled)
            .await
            .unwrap();
        let batch = batches.get(number).await.unwrap();
        assert_eq!(batch.stage(), Stage::Bottled);
        assert_eq!(batch.bottling, bottled);
        assert_eq!(batch.abv().unwrap().to_string(), "5.3 %");
        assert_eq!(batch.attenuation().unwrap().to_string(), "80.0 %");

        let fg_only = bottling(None, Some("1.010"));
        batches
            .record_bottling(number, josef, &fg_only)
            .await
            .unwrap();
        assert_eq!(batches.get(number).await.unwrap().stage(), Stage::Brewed);

        batches
            .record_brew_day(number, josef, &brew_day(None, Some("1.050"), None))
            .await
            .unwrap();
        let batch = batches.get(number).await.unwrap();
        assert_eq!(batch.stage(), Stage::Planned);
        assert_eq!(batch.brew_day.volume, None);
        assert_eq!(batch.abv().unwrap().to_string(), "5.3 %");
    }

    #[sqlx::test]
    async fn a_value_that_breaks_a_rule_against_the_other_form_changes_nothing(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let number = batches
            .add(josef, &new_batch("Pilsner", ""), None)
            .await
            .unwrap();
        let brewed = brew_day(Some(date(2026, 10, 3)), Some("1.050"), None);
        let bottled = bottling(Some(date(2026, 10, 17)), Some("1.010"));
        batches
            .record_brew_day(number, josef, &brewed)
            .await
            .unwrap();
        batches
            .record_bottling(number, josef, &bottled)
            .await
            .unwrap();

        let refusals = [
            (
                batches
                    .record_brew_day(
                        number,
                        josef,
                        &brew_day(Some(date(2026, 10, 3)), Some("1.010"), None),
                    )
                    .await,
                RuleError::FgNotBelowOg,
            ),
            (
                batches
                    .record_brew_day(
                        number,
                        josef,
                        &brew_day(Some(date(2026, 10, 18)), None, None),
                    )
                    .await,
                RuleError::BottlingBeforeBrewing,
            ),
            (
                batches
                    .record_brew_day(number, josef, &brew_day(None, Some("1.050"), None))
                    .await,
                RuleError::BottlingWithoutBrewing,
            ),
            (
                batches
                    .record_bottling(number, josef, &bottling(Some(date(2026, 10, 2)), None))
                    .await,
                RuleError::BottlingBeforeBrewing,
            ),
            (
                batches
                    .record_bottling(number, josef, &bottling(None, Some("1.051")))
                    .await,
                RuleError::FgNotBelowOg,
            ),
        ];
        for (refused, rule) in refusals {
            assert!(
                matches!(refused, Err(RecordError::Rule(broken)) if broken == rule),
                "{refused:?}"
            );
        }

        let batch = batches.get(number).await.unwrap();
        assert_eq!((batch.brew_day, batch.bottling), (brewed, bottled));
    }

    #[sqlx::test]
    async fn a_batch_that_isnt_there_cant_be_read_edited_or_deleted(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let deleted = batches
            .add(josef, &new_batch("Soon gone", ""), None)
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

    #[test]
    fn a_batch_takes_its_name_from_the_recipe_cut_to_fit() {
        let unetice = Recipe::parse(UNETICE.to_vec()).unwrap();
        assert_eq!(
            BatchName::of_recipe(&unetice).unwrap().as_str(),
            "Únětický Vzor 10.7° (G70 30L)"
        );

        let long = format!(
            "<RECIPES><RECIPE><NAME>{}</NAME></RECIPE></RECIPES>",
            "š".repeat(NAME_MAX + 5)
        );
        let long = Recipe::parse(long.into_bytes()).unwrap();
        assert_eq!(
            BatchName::of_recipe(&long).unwrap().as_str(),
            "š".repeat(NAME_MAX)
        );

        let blank =
            Recipe::parse(b"<RECIPES><RECIPE><NAME> </NAME></RECIPE></RECIPES>".to_vec()).unwrap();
        assert_eq!(BatchName::of_recipe(&blank), None);
    }

    /// The Recipe bytes stored for the Batch `number`.
    async fn stored(db: &SqlitePool, number: BatchNumber) -> Option<Vec<u8>> {
        let number = number.get();
        sqlx::query_scalar!("SELECT recipe FROM batches WHERE number = ?", number)
            .fetch_one(db)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn a_recipe_is_stored_byte_for_byte_and_read_back(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db.clone());
        let latin1 = Recipe::parse(LATIN1.to_vec()).unwrap();

        let number = batches
            .add(josef, &new_batch("Vzor", ""), Some(&latin1))
            .await
            .unwrap();

        assert_eq!(stored(&db, number).await.as_deref(), Some(LATIN1));
        let Some(AttachedRecipe::Readable(summary)) = batches.recipe(number).await.unwrap() else {
            panic!("the Recipe reads");
        };
        assert_eq!(summary.name, "Únětický Vzor 10.7°");
        assert_eq!(summary.fermentables[0].name, "Plzeňský slad");
    }

    #[sqlx::test]
    async fn a_recipe_is_attached_replaced_and_removed(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (batches, clock) = service(db.clone());
        let number = batches
            .add(josef, &new_batch("Pilsner", ""), None)
            .await
            .unwrap();
        assert_eq!(batches.recipe(number).await.unwrap(), None);
        clock.advance(Duration::from_hours(1));

        let unetice = Recipe::parse(UNETICE.to_vec()).unwrap();
        batches
            .attach_recipe(number, simon, &unetice)
            .await
            .unwrap();
        assert_eq!(stored(&db, number).await.as_deref(), Some(UNETICE));
        let batch = batches.get(number).await.unwrap();
        assert_eq!(batch.name.as_str(), "Pilsner", "the name stays");
        assert_eq!(batch.edited.unwrap().by.as_str(), "Šimon");

        let sparse = Recipe::parse(SPARSE.to_vec()).unwrap();
        batches.attach_recipe(number, josef, &sparse).await.unwrap();
        assert_eq!(stored(&db, number).await.as_deref(), Some(SPARSE));

        batches.remove_recipe(number, josef).await.unwrap();
        assert_eq!(stored(&db, number).await, None);
        assert_eq!(batches.recipe(number).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn a_stored_recipe_that_no_longer_reads_still_shows_it_is_attached(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db.clone());
        let number = batches
            .add(josef, &new_batch("Pilsner", ""), None)
            .await
            .unwrap();
        let raw = number.get();
        sqlx::query!("UPDATE batches SET recipe = X'00' WHERE number = ?", raw)
            .execute(&db)
            .await
            .unwrap();

        assert_eq!(
            batches.recipe(number).await.unwrap(),
            Some(AttachedRecipe::Unreadable)
        );
    }

    #[sqlx::test]
    async fn a_recipe_cant_be_attached_to_or_removed_from_a_batch_that_isnt_there(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (batches, _) = service(db);
        let missing = BatchNumber::from_client(999);
        let unetice = Recipe::parse(UNETICE.to_vec()).unwrap();

        assert!(matches!(
            batches.recipe(missing).await,
            Err(BatchError::NotFound)
        ));
        assert!(matches!(
            batches.attach_recipe(missing, josef, &unetice).await,
            Err(BatchError::NotFound)
        ));
        assert!(matches!(
            batches.remove_recipe(missing, josef).await,
            Err(BatchError::NotFound)
        ));
    }
}
