//! The Crew's Quotes: something memorable someone said, with who added it and a folded search
//! form of its text and Author.
//!
//! Quotes emit no events and send no notifications. Any Member may add one; nothing here checks
//! who added it.

use jiff::civil::DateTime;
use sqlx::SqlitePool;
use unicode_normalization::UnicodeNormalization;

use crate::domain::clock::{Clock, crew_time_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, MemberId};

/// The longest Quote, in characters. Line breaks count.
pub const TEXT_MAX: usize = 2000;

/// The longest Author, in characters.
pub const AUTHOR_MAX: usize = 100;

/// How many Quotes a page of the collection lists. The same size as a page of History.
pub const PAGE_SIZE: usize = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QuoteId(i64);

impl QuoteId {
    /// An id a client sent back. Nothing about it is trusted until the Quote is found.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// What was said: trimmed, non-blank and at most [`TEXT_MAX`] characters. Line breaks are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteText(String);

impl QuoteText {
    /// Trims `input` and checks it can be a Quote.
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

    /// A Quote read back from the database, which was checked when it was stored.
    pub(in crate::domain) const fn from_stored(text: String) -> Self {
        Self(text)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("the Quote is blank")]
    Blank,
    #[error("the Quote is longer than {TEXT_MAX} characters")]
    TooLong,
}

/// Who said a Quote: trimmed, non-blank and at most [`AUTHOR_MAX`] characters. Not necessarily
/// a Member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author(String);

impl Author {
    /// Trims `input` and checks it can be an Author.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorError::Blank`] or [`AuthorError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, AuthorError> {
        let author = input.trim();
        if author.is_empty() {
            Err(AuthorError::Blank)
        } else if author.chars().count() > AUTHOR_MAX {
            Err(AuthorError::TooLong)
        } else {
            Ok(Self(author.to_owned()))
        }
    }

    /// An Author read back from the database, which was checked when it was stored.
    pub(in crate::domain) const fn from_stored(author: String) -> Self {
        Self(author)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthorError {
    #[error("the Author is blank")]
    Blank,
    #[error("the Author is longer than {AUTHOR_MAX} characters")]
    TooLong,
}

/// A Quote as a Member adds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewQuote {
    pub text: QuoteText,
    pub author: Author,
}

/// When a Quote was added: an instant, shown as a date and time in the Crew's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddedAt(i64);

impl AddedAt {
    /// The date and time it showed on the clock in the Crew's time zone.
    #[must_use]
    pub fn crew_time(self) -> DateTime {
        crew_time_of_unix_seconds(self.0)
    }
}

/// A Quote as the collection shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub id: QuoteId,
    pub text: QuoteText,
    pub author: Author,
    /// The Member who added it. None for a Quote imported from Heimdall.
    pub added_by: Option<DisplayName>,
    pub added_at: AddedAt,
}

/// A Quote as it is read back from the database, with the display name of who added it.
struct QuoteRow {
    id: i64,
    text: String,
    author: String,
    added_at: i64,
    added_by: Option<String>,
}

impl From<QuoteRow> for Quote {
    fn from(row: QuoteRow) -> Self {
        Self {
            id: QuoteId(row.id),
            text: QuoteText::from_stored(row.text),
            author: Author::from_stored(row.author),
            added_by: row.added_by.map(DisplayName::from_stored),
            added_at: AddedAt(row.added_at),
        }
    }
}

/// One page of the collection, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotePage {
    pub quotes: Vec<Quote>,
    /// Which page this is, from 1.
    pub page: usize,
    /// How many pages there are; at least 1, even with nothing to list.
    pub pages: usize,
}

/// The folded search form of `text`: lowercased, with diacritics removed.
///
/// "Přišel" folds to "prisel", and "PEPA" to "pepa". Combining marks fold the same way as the
/// precomposed letters, so a later search can match either. This does not trim; the Quote
/// service folds text it has already trimmed.
#[must_use]
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect()
}

/// Marks that NFD splits off a letter. The blocks are the combining diacritics Unicode keeps
/// apart from the letter they decorate.
const fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

/// The folded search form of a Quote's text and Author, stored beside them.
struct SearchForm {
    text: String,
    author: String,
}

impl SearchForm {
    fn of(quote: &NewQuote) -> Self {
        Self {
            text: fold(quote.text.as_str()),
            author: fold(quote.author.as_str()),
        }
    }
}

/// Adds Quotes and lists the collection, a page at a time. Any Member may add any Quote.
#[derive(Debug, Clone)]
pub struct Quotes {
    db: SqlitePool,
    clock: Clock,
}

impl Quotes {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock) -> Self {
        Self { db, clock }
    }

    /// Adds `quote` on behalf of `added_by`, now, and stores the folded search form of its text
    /// and Author. Nobody is notified.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn add(&self, added_by: MemberId, quote: &NewQuote) -> Result<QuoteId, sqlx::Error> {
        let added_at = unix_seconds(self.clock.now());
        let member = added_by.get();
        let search = SearchForm::of(quote);
        let id = sqlx::query_scalar!(
            r#"INSERT INTO quotes (text, author, added_by, added_at, text_search, author_search)
               VALUES (?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            quote.text.0,
            quote.author.0,
            member,
            added_at,
            search.text,
            search.author,
        )
        .fetch_one(&self.db)
        .await?;
        Ok(QuoteId(id))
    }

    /// The `page`th page of the collection, newest first. A page past the last is the last, and
    /// a page below 1 is the first.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn page(&self, page: usize) -> Result<QuotePage, sqlx::Error> {
        let count = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM quotes"#)
            .fetch_one(&self.db)
            .await?;
        let total = usize::try_from(count).unwrap_or(0);
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        let page = page.clamp(1, pages);
        let limit = i64::try_from(PAGE_SIZE).unwrap_or(i64::MAX);
        let offset =
            i64::try_from(page.saturating_sub(1).saturating_mul(PAGE_SIZE)).unwrap_or(i64::MAX);
        let quotes = sqlx::query_as!(
            QuoteRow,
            r#"SELECT quotes.id AS "id!", quotes.text, quotes.author,
                      quotes.added_at AS "added_at!",
                      members.display_name AS "added_by?"
               FROM quotes
               LEFT JOIN members ON members.id = quotes.added_by
               ORDER BY quotes.added_at DESC, quotes.id DESC
               LIMIT ? OFFSET ?"#,
            limit,
            offset,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(Quote::from)
        .collect();
        Ok(QuotePage {
            quotes,
            page,
            pages,
        })
    }

    /// One Quote picked at random, each as likely as any other, or None when there are none.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn random(&self) -> Result<Option<Quote>, sqlx::Error> {
        let row = sqlx::query_as!(
            QuoteRow,
            r#"SELECT quotes.id AS "id!", quotes.text, quotes.author,
                      quotes.added_at AS "added_at!",
                      members.display_name AS "added_by?"
               FROM quotes
               LEFT JOIN members ON members.id = quotes.added_by
               ORDER BY RANDOM()
               LIMIT 1"#,
        )
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Quote::from))
    }

    /// Each distinct Author once, in alphabetical order, for the add form to suggest.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn authors(&self) -> Result<Vec<Author>, sqlx::Error> {
        let authors = sqlx::query_scalar!(
            r#"SELECT DISTINCT author AS "author!" FROM quotes ORDER BY author"#
        )
        .fetch_all(&self.db)
        .await?;
        Ok(authors.into_iter().map(Author::from_stored).collect())
    }
}

/// Quotes for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::DateTime;

    use super::{AddedAt, Author, Quote, QuoteId, QuoteText};
    use crate::domain::clock::unix_seconds_at_crew_time;
    use crate::domain::members::DisplayName;

    /// A Quote with `id`, said by `author` and added at `added` in Prague (`2026-10-24T18:00`)
    /// by `added_by`, or by nobody when `added_by` is [`None`].
    ///
    /// # Panics
    ///
    /// If the fixture's text, Author, time or Member is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn quote(id: i64, text: &str, author: &str, added_by: Option<&str>, added: &str) -> Quote {
        let added: DateTime = added.parse().expect("fixture times are valid");
        Quote {
            id: QuoteId::from_client(id),
            text: QuoteText::parse(text).expect("fixture quotes are valid"),
            author: Author::parse(author).expect("fixture authors are valid"),
            added_by: added_by
                .map(|name| DisplayName::parse(name).expect("fixture names are valid")),
            added_at: AddedAt(unix_seconds_at_crew_time(added).expect("fixture times exist")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use jiff::civil::date;
    use sqlx::SqlitePool;

    use super::{
        AUTHOR_MAX, Author, AuthorError, NewQuote, PAGE_SIZE, QuoteText, TEXT_MAX, TextError, fold,
    };
    use crate::domain::clock::{ManualClock, unix_seconds};
    use crate::domain::members::DisplayName;
    use crate::domain::members::fixtures::add_member;

    /// 2026-10-24T08:00:00Z: 10:00 in Prague.
    fn saturday_morning() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(498_008)
    }

    fn service(db: SqlitePool) -> (super::Quotes, ManualClock) {
        let clock = ManualClock::starting_at(saturday_morning());
        let quotes = super::Quotes::new(db, clock.clock());
        (quotes, clock)
    }

    fn new_quote(text: &str, author: &str) -> NewQuote {
        NewQuote {
            text: QuoteText::parse(text).unwrap(),
            author: Author::parse(author).unwrap(),
        }
    }

    #[test]
    fn folds_case_and_czech_diacritics() {
        assert_eq!(fold("Přišel"), "prisel");
        assert_eq!(fold("PEPA"), "pepa");
        assert_eq!(fold("PŘIŠEL"), "prisel");
        assert_eq!(fold("áčďéěíňóřšťúůýž"), "acdeeinorstuuyz");
        assert_eq!(fold("ÁČĎÉĚÍŇÓŘŠŤÚŮÝŽ"), "acdeeinorstuuyz");
        // A caron typed as its own mark folds like the precomposed letter.
        assert_eq!(fold("e\u{030C}"), "e");
        assert_eq!(fold("Přišel sem"), "prisel sem");
        assert_eq!(fold("prisel"), fold(&fold("Přišel")));
    }

    #[test]
    fn folding_keeps_spaces_and_line_breaks_and_does_not_trim() {
        assert_eq!(fold("  PEPA  "), "  pepa  ");
        assert_eq!(fold("A\nB"), "a\nb");
    }

    #[test]
    fn trims_a_quote_and_keeps_its_line_breaks() {
        assert_eq!(
            QuoteText::parse("\n  line one\nline two  \n")
                .unwrap()
                .as_str(),
            "line one\nline two"
        );
    }

    #[test]
    fn refuses_a_blank_quote() {
        assert_eq!(QuoteText::parse(" \n\t"), Err(TextError::Blank));
    }

    #[test]
    fn limits_a_quote_by_characters_not_bytes() {
        let longest = "š".repeat(TEXT_MAX);
        assert_eq!(QuoteText::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            QuoteText::parse(&format!("{longest}š")),
            Err(TextError::TooLong)
        );
    }

    #[test]
    fn trims_an_author() {
        assert_eq!(Author::parse("  Pepa \n").unwrap().as_str(), "Pepa");
    }

    #[test]
    fn refuses_a_blank_author() {
        assert_eq!(Author::parse(" \t"), Err(AuthorError::Blank));
    }

    #[test]
    fn limits_an_author_by_characters_not_bytes() {
        let longest = "ř".repeat(AUTHOR_MAX);
        assert_eq!(Author::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            Author::parse(&format!("{longest}ř")),
            Err(AuthorError::TooLong)
        );
    }

    #[sqlx::test]
    async fn an_added_quote_is_newest_with_who_added_it_when(db: SqlitePool) {
        let ondra = add_member(&db, "Ondra", false).await;
        let (quotes, _) = service(db);

        let id = quotes
            .add(ondra, &new_quote("  To pivo je spíš polévka. ", " Pepa "))
            .await
            .unwrap();

        let page = quotes.page(1).await.unwrap();
        assert_eq!(page.page, 1);
        assert_eq!(page.pages, 1);
        assert_eq!(page.quotes.len(), 1);
        let quote = &page.quotes[0];
        assert_eq!(quote.id, id);
        assert_eq!(quote.text.as_str(), "To pivo je spíš polévka.");
        assert_eq!(quote.author.as_str(), "Pepa");
        assert_eq!(
            quote.added_by.as_ref().map(DisplayName::as_str),
            Some("Ondra")
        );
        assert_eq!(
            quote.added_at.crew_time(),
            date(2026, 10, 24).at(10, 0, 0, 0)
        );
    }

    #[sqlx::test]
    async fn a_quote_stores_the_folded_search_form_of_its_text_and_author(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, _) = service(db.clone());

        quotes
            .add(josef, &new_quote("  Přišel sem\n ", " PEPA "))
            .await
            .unwrap();

        let row = sqlx::query!("SELECT text, author, text_search, author_search FROM quotes")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(row.text, "Přišel sem");
        assert_eq!(row.author, "PEPA");
        assert_eq!(row.text_search, fold("Přišel sem"));
        assert_eq!(row.author_search, "pepa");
    }

    #[sqlx::test]
    async fn a_quote_as_long_as_allowed_is_stored(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, _) = service(db);
        let text = "š".repeat(TEXT_MAX);
        let author = "ř".repeat(AUTHOR_MAX);

        quotes.add(josef, &new_quote(&text, &author)).await.unwrap();

        let quote = &quotes.page(1).await.unwrap().quotes[0];
        assert_eq!(quote.text.as_str(), text);
        assert_eq!(quote.author.as_str(), author);
    }

    #[sqlx::test]
    async fn the_collection_is_newest_first_and_a_page_past_the_last_is_the_last(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, clock) = service(db);
        for n in 1..=PAGE_SIZE + 1 {
            quotes
                .add(josef, &new_quote(&format!("Quote {n:02}"), "Pepa"))
                .await
                .unwrap();
            clock.advance(Duration::from_secs(1));
        }

        let first = quotes.page(1).await.unwrap();
        assert_eq!(first.quotes.len(), PAGE_SIZE);
        assert_eq!(first.page, 1);
        assert_eq!(first.pages, 2);
        assert_eq!(first.quotes[0].text.as_str(), "Quote 26");
        assert_eq!(first.quotes.last().unwrap().text.as_str(), "Quote 02");

        let last = quotes.page(2).await.unwrap();
        assert_eq!(last.page, 2);
        assert_eq!(last.quotes.len(), 1);
        assert_eq!(last.quotes[0].text.as_str(), "Quote 01");

        let past = quotes.page(99).await.unwrap();
        assert_eq!(past.page, 2);
        assert_eq!(past.quotes[0].text.as_str(), "Quote 01");

        let before = quotes.page(0).await.unwrap();
        assert_eq!(before.page, 1);
        assert_eq!(before.quotes[0].text.as_str(), "Quote 26");
    }

    #[sqlx::test]
    async fn quotes_added_at_the_same_moment_are_newest_id_first(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, _) = service(db);
        for name in ["First", "Second"] {
            quotes.add(josef, &new_quote(name, "Pepa")).await.unwrap();
        }

        let page = quotes.page(1).await.unwrap();
        let texts: Vec<&str> = page
            .quotes
            .iter()
            .map(|quote| quote.text.as_str())
            .collect();
        assert_eq!(texts, ["Second", "First"]);
    }

    #[sqlx::test]
    async fn an_empty_collection_is_one_empty_page(db: SqlitePool) {
        let (quotes, _) = service(db);

        let page = quotes.page(3).await.unwrap();

        assert_eq!(page.quotes.len(), 0);
        assert_eq!(page.page, 1);
        assert_eq!(page.pages, 1);
    }

    #[sqlx::test]
    async fn a_quote_without_added_by_is_listed_without_one(db: SqlitePool) {
        let (quotes, _) = service(db.clone());
        let text = "Imported from the old book";
        let author = "A friend";
        let text_search = fold(text);
        let author_search = fold(author);
        let added_at = unix_seconds(saturday_morning());
        sqlx::query!(
            "INSERT INTO quotes (text, author, added_at, text_search, author_search)
             VALUES (?, ?, ?, ?, ?)",
            text,
            author,
            added_at,
            text_search,
            author_search,
        )
        .execute(&db)
        .await
        .unwrap();

        let quote = &quotes.page(1).await.unwrap().quotes[0];
        assert_eq!(quote.text.as_str(), text);
        assert_eq!(quote.author.as_str(), author);
        assert_eq!(quote.added_by, None);
    }

    #[sqlx::test]
    async fn there_is_no_random_quote_without_quotes(db: SqlitePool) {
        let (quotes, _) = service(db);

        assert_eq!(quotes.random().await.unwrap(), None);
    }

    #[sqlx::test]
    async fn the_only_quote_is_the_random_one(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, _) = service(db);
        let id = quotes
            .add(josef, &new_quote("Only this", "Pepa"))
            .await
            .unwrap();

        for _ in 0..5 {
            let quote = quotes.random().await.unwrap().unwrap();
            assert_eq!(quote.id, id);
            assert_eq!(quote.text.as_str(), "Only this");
            assert_eq!(
                quote.added_by.as_ref().map(DisplayName::as_str),
                Some("Josef")
            );
        }
    }

    #[sqlx::test]
    async fn a_random_quote_is_always_a_stored_one_and_any_can_come_up(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, _) = service(db);
        let mut stored = std::collections::HashSet::new();
        for n in 1..=3 {
            stored.insert(
                quotes
                    .add(josef, &new_quote(&format!("Quote {n}"), "Pepa"))
                    .await
                    .unwrap(),
            );
        }

        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let quote = quotes.random().await.unwrap().unwrap();
            assert!(stored.contains(&quote.id), "{quote:?}");
            seen.insert(quote.id);
        }
        // Each of three comes up in 200 picks unless the choice isn't uniform; missing one by
        // chance is (2/3)^200.
        assert_eq!(seen, stored);
    }

    #[sqlx::test]
    async fn authors_are_each_distinct_value_once_in_alphabetical_order(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let (quotes, _) = service(db);
        for author in ["Pepa", "Ondra", "Pepa", "pepa"] {
            quotes
                .add(josef, &new_quote("Something memorable", author))
                .await
                .unwrap();
        }

        let listed = quotes.authors().await.unwrap();
        let authors: Vec<&str> = listed.iter().map(Author::as_str).collect();
        assert_eq!(authors, ["Ondra", "Pepa", "pepa"]);
    }
}
