//! The Shopping list: the Items the Crew needs to buy, each with an Urgency.

use jiff::civil::DateTime;
use sqlx::SqlitePool;

use crate::domain::channel::Emitter;
use crate::domain::clock::{Clock, crew_time_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, MemberId};

/// The longest name of an Item, in characters.
pub const NAME_MAX: usize = 200;

/// The longest note on an Item, in characters.
pub const NOTE_MAX: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemId(i64);

impl ItemId {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// What an Item is: trimmed, non-blank and at most [`NAME_MAX`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemName(String);

impl ItemName {
    /// Trims `input` and checks it can be an Item's name.
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
    #[error("the name is blank")]
    Blank,
    #[error("the name is longer than {NAME_MAX} characters")]
    TooLong,
}

/// More about an Item, such as a quantity or a link to a shop: trimmed, non-blank and at most
/// [`NOTE_MAX`] characters. Its line breaks are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemNote(String);

impl ItemNote {
    /// Trims `input` and checks it can be an Item's note. A blank note is no note.
    ///
    /// # Errors
    ///
    /// Returns [`NoteTooLong`].
    pub fn parse(input: &str) -> Result<Option<Self>, NoteTooLong> {
        let note = input.trim();
        if note.is_empty() {
            Ok(None)
        } else if note.chars().count() > NOTE_MAX {
            Err(NoteTooLong)
        } else {
            Ok(Some(Self(note.to_owned())))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the note is longer than {NOTE_MAX} characters")]
pub struct NoteTooLong;

/// How soon the Crew needs an Item. Not to be confused with how urgently a push is delivered,
/// [`crate::domain::notifications::Urgency`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Urgency {
    Asap,
    /// What an Item is needed by unless someone says otherwise.
    #[default]
    NextBrewday,
    NiceToHave,
}

impl Urgency {
    /// Every Urgency, most urgent first, the order the Shopping list groups Items in.
    pub const ALL: [Self; 3] = [Self::Asap, Self::NextBrewday, Self::NiceToHave];

    /// How a request and the database name it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Asap => "asap",
            Self::NextBrewday => "next-brewday",
            Self::NiceToHave => "nice-to-have",
        }
    }

    /// The Urgency a request names by its [`key`](Self::key), if any.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|urgency| urgency.key() == key)
    }

    fn from_stored(key: &str) -> Result<Self, sqlx::Error> {
        Self::from_key(key)
            .ok_or_else(|| sqlx::Error::Decode(format!("no Urgency is stored as {key:?}").into()))
    }
}

/// An Item as a Member adds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub name: ItemName,
    pub note: Option<ItemNote>,
    pub urgency: Urgency,
}

/// An Item nobody has bought yet, as the Shopping list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenItem {
    pub id: ItemId,
    pub name: ItemName,
    pub note: Option<ItemNote>,
    pub urgency: Urgency,
    pub added_by: DisplayName,
    pub added_at: AddedAt,
}

/// When an Item was added: an instant, shown as a date and time in the Crew's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddedAt(i64);

impl AddedAt {
    /// The date and time it showed on the clock in the Crew's time zone.
    #[must_use]
    pub fn crew_time(self) -> DateTime {
        crew_time_of_unix_seconds(self.0)
    }
}

/// A Member added an *ASAP* Item, for the rest of the Crew to hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsapItemAdded {
    pub item: ItemId,
    pub name: ItemName,
    /// The Member who added it.
    pub added_by: MemberId,
    /// Every Member except the one who added it, in id order.
    pub notify: Vec<MemberId>,
}

/// Adds Items to the Shopping list and lists the open ones. Any Member may change any Item.
#[derive(Debug, Clone)]
pub struct Shopping {
    db: SqlitePool,
    clock: Clock,
    events: Emitter<AsapItemAdded>,
}

impl Shopping {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock, events: Emitter<AsapItemAdded>) -> Self {
        Self { db, clock, events }
    }

    /// Adds `item` to the Shopping list on behalf of `adder`. An *ASAP* Item is announced to the
    /// rest of the Crew.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn add(&self, adder: MemberId, item: &NewItem) -> Result<ItemId, sqlx::Error> {
        let added_at = unix_seconds(self.clock.now());
        let added_by = adder.get();
        let note = item.note.as_ref().map(ItemNote::as_str);
        let urgency = item.urgency.key();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO items (name, note, urgency, added_by, added_at) VALUES (?, ?, ?, ?, ?)
               RETURNING id AS "id!""#,
            item.name.0,
            note,
            urgency,
            added_by,
            added_at,
        )
        .fetch_one(&self.db)
        .await?;
        let id = ItemId(id);
        if item.urgency == Urgency::Asap {
            let notify = sqlx::query_scalar!(
                r#"SELECT id AS "id!" FROM members WHERE id != ? ORDER BY id"#,
                added_by
            )
            .fetch_all(&self.db)
            .await?
            .into_iter()
            .map(MemberId::from_stored)
            .collect();
            self.events.emit(AsapItemAdded {
                item: id,
                name: item.name.clone(),
                added_by: adder,
                notify,
            });
        }
        Ok(id)
    }

    /// Every open Item, most urgent first; within an Urgency, those added longest ago first.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn open(&self) -> Result<Vec<OpenItem>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT items.id AS "id!", items.name, items.note, items.urgency, items.added_at,
                      members.display_name AS added_by
               FROM items
               JOIN members ON members.id = items.added_by
               ORDER BY items.added_at, items.id"#
        )
        .fetch_all(&self.db)
        .await?;
        let mut open = rows
            .into_iter()
            .map(|row| {
                Ok(OpenItem {
                    id: ItemId(row.id),
                    name: ItemName(row.name),
                    note: row.note.map(ItemNote),
                    urgency: Urgency::from_stored(&row.urgency)?,
                    added_by: DisplayName::from_stored(row.added_by),
                    added_at: AddedAt(row.added_at),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        // Stable, so each Urgency keeps the oldest first.
        open.sort_by_key(|item| item.urgency);
        Ok(open)
    }
}

/// Open Items for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{AddedAt, ItemId, ItemName, ItemNote, OpenItem, Urgency};
    use crate::domain::clock::unix_seconds_at_crew_time;
    use crate::domain::members::DisplayName;

    /// An open Item with `id`, added at `added` in Prague (`2026-10-20T08:00`).
    ///
    /// # Panics
    ///
    /// If the fixture's name, note, time or adder is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn open(
        id: i64,
        name: &str,
        note: Option<&str>,
        urgency: Urgency,
        added: &str,
        added_by: &str,
    ) -> OpenItem {
        let added = added.parse().expect("fixture times are valid");
        OpenItem {
            id: ItemId(id),
            name: ItemName::parse(name).expect("fixture names are valid"),
            note: note.map(|note| {
                ItemNote::parse(note)
                    .expect("fixture notes are valid")
                    .expect("fixture notes aren't blank")
            }),
            urgency,
            added_by: DisplayName::parse(added_by).expect("fixture names are valid"),
            added_at: AddedAt(unix_seconds_at_crew_time(added).expect("fixture times exist")),
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

    /// 2026-10-24T08:00:00Z: 10:00 in Prague.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    fn shopping(db: SqlitePool) -> (Shopping, ManualClock, broadcast::Receiver<AsapItemAdded>) {
        let clock = ManualClock::starting_at(saturday_morning());
        let channel = Channel::new();
        let subscription = channel.subscribe();
        let shopping = Shopping::new(db, clock.clock(), channel.emitter(identity));
        (shopping, clock, subscription)
    }

    fn item(name: &str, note: &str, urgency: Urgency) -> NewItem {
        NewItem {
            name: ItemName::parse(name).unwrap(),
            note: ItemNote::parse(note).unwrap(),
            urgency,
        }
    }

    fn emitted(subscription: &mut broadcast::Receiver<AsapItemAdded>) -> Vec<AsapItemAdded> {
        std::iter::from_fn(|| subscription.try_recv().ok()).collect()
    }

    fn names(open: &[OpenItem]) -> Vec<(&str, Urgency)> {
        open.iter()
            .map(|item| (item.name.as_str(), item.urgency))
            .collect()
    }

    #[test]
    fn trims_a_name() {
        assert_eq!(
            ItemName::parse("  Pilsner malt \n").unwrap().as_str(),
            "Pilsner malt"
        );
    }

    #[test]
    fn refuses_a_blank_name() {
        assert_eq!(ItemName::parse(" \t\n"), Err(NameError::Blank));
    }

    #[test]
    fn limits_a_name_by_characters_not_bytes() {
        let longest = "š".repeat(NAME_MAX);
        assert_eq!(ItemName::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            ItemName::parse(&format!("{longest}š")),
            Err(NameError::TooLong)
        );
    }

    #[test]
    fn a_blank_note_is_no_note() {
        assert_eq!(ItemNote::parse(" \n "), Ok(None));
    }

    #[test]
    fn trims_a_note_and_keeps_its_line_breaks() {
        assert_eq!(
            ItemNote::parse("\n 2 packs\nfrom the usual shop  ")
                .unwrap()
                .unwrap()
                .as_str(),
            "2 packs\nfrom the usual shop"
        );
    }

    #[test]
    fn limits_a_note_by_characters_not_bytes() {
        let longest = "ř".repeat(NOTE_MAX);
        assert_eq!(
            ItemNote::parse(&longest).unwrap().unwrap().as_str(),
            longest
        );
        assert_eq!(ItemNote::parse(&format!("{longest}ř")), Err(NoteTooLong));
    }

    #[test]
    fn urgencies_go_most_urgent_first_and_default_to_next_brewday() {
        assert_eq!(
            Urgency::ALL,
            [Urgency::Asap, Urgency::NextBrewday, Urgency::NiceToHave]
        );
        assert_eq!(Urgency::default(), Urgency::NextBrewday);
    }

    #[test]
    fn an_urgency_is_found_by_its_key() {
        for urgency in Urgency::ALL {
            assert_eq!(Urgency::from_key(urgency.key()), Some(urgency));
        }
        assert_eq!(Urgency::from_key("yesterday"), None);
    }

    #[sqlx::test]
    async fn an_added_item_is_open_with_who_added_it_when(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let (shopping, _, _) = shopping(db);

        let id = shopping
            .add(ondra, &item(" Saaz ", " 2 packs ", Urgency::NiceToHave))
            .await
            .unwrap();

        let open = shopping.open().await.unwrap();
        assert_eq!(
            open,
            [OpenItem {
                id,
                name: ItemName::parse("Saaz").unwrap(),
                note: ItemNote::parse("2 packs").unwrap(),
                urgency: Urgency::NiceToHave,
                added_by: DisplayName::parse("Ondra").unwrap(),
                added_at: AddedAt(unix_seconds(saturday_morning())),
            }]
        );
        assert_eq!(
            open[0].added_at.crew_time(),
            date(2026, 10, 24).at(10, 0, 0, 0)
        );
    }

    #[sqlx::test]
    async fn an_item_may_have_no_note(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (shopping, _, _) = shopping(db);

        shopping
            .add(josef, &item("Saaz", "", Urgency::Asap))
            .await
            .unwrap();

        assert_eq!(shopping.open().await.unwrap()[0].note, None);
    }

    #[sqlx::test]
    async fn open_is_grouped_by_urgency_and_oldest_first_within_one(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (shopping, clock, _) = shopping(db);
        for (adder, name, urgency) in [
            (josef, "Bottle caps", Urgency::NiceToHave),
            (simon, "Pilsner malt", Urgency::NextBrewday),
            (josef, "Star San", Urgency::Asap),
            (simon, "Saaz", Urgency::NextBrewday),
            (josef, "CO₂", Urgency::Asap),
            (simon, "A new hydrometer", Urgency::NiceToHave),
        ] {
            shopping.add(adder, &item(name, "", urgency)).await.unwrap();
            clock.advance(Duration::from_mins(1));
        }

        assert_eq!(
            names(&shopping.open().await.unwrap()),
            [
                ("Star San", Urgency::Asap),
                ("CO₂", Urgency::Asap),
                ("Pilsner malt", Urgency::NextBrewday),
                ("Saaz", Urgency::NextBrewday),
                ("Bottle caps", Urgency::NiceToHave),
                ("A new hydrometer", Urgency::NiceToHave),
            ]
        );
    }

    #[sqlx::test]
    async fn items_added_at_once_are_in_the_order_they_were_added(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (shopping, _, _) = shopping(db);
        for name in ["First", "Second", "Third"] {
            shopping
                .add(josef, &item(name, "", Urgency::NextBrewday))
                .await
                .unwrap();
        }

        let open = shopping.open().await.unwrap();
        let listed: Vec<&str> = open.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(listed, ["First", "Second", "Third"]);
    }

    #[sqlx::test]
    async fn an_asap_item_is_announced_to_everyone_but_its_adder(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        let (shopping, _, mut events) = shopping(db);

        let id = shopping
            .add(ondra, &item("Star San", "the big bottle", Urgency::Asap))
            .await
            .unwrap();

        assert_eq!(
            emitted(&mut events),
            [AsapItemAdded {
                item: id,
                name: ItemName::parse("Star San").unwrap(),
                added_by: ondra,
                notify: vec![josef, simon],
            }]
        );
    }

    #[sqlx::test]
    async fn other_urgencies_are_not_announced(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        add_member(&db, "Ondra", false).await;
        let (shopping, _, mut events) = shopping(db);

        for urgency in [Urgency::NextBrewday, Urgency::NiceToHave] {
            shopping
                .add(josef, &item("Saaz", "", urgency))
                .await
                .unwrap();
        }

        assert_eq!(emitted(&mut events), []);
    }
}
