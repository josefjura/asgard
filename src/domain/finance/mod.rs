//! The Crew's shared money: logging Expenses and Payments and working out everyone's Balance
//! from them.

pub mod ledger;
pub mod money;

use jiff::civil::Date;
use sqlx::{SqliteConnection, SqlitePool};

use self::ledger::{Ledger, LedgerExpense, LedgerPayment, Participant};
use self::money::{Amount, Crowns};
use crate::domain::clock::{Clock, crew_date_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, MemberId, MemberName, parse_date};

/// The longest description of an Expense or Payment, in characters.
pub const DESCRIPTION_MAX: usize = 500;

/// How many of the latest entries the Finance home screen lists.
pub const RECENT_ENTRIES: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExpenseId(i64);

impl ExpenseId {
    /// An id a client sent back. Nothing about it is trusted until the Expense is found.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaymentId(i64);

impl PaymentId {
    /// An id a client sent back. Nothing about it is trusted until the Payment is found.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// What an Expense or Payment was for: trimmed, non-blank and at most [`DESCRIPTION_MAX`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description(String);

impl Description {
    /// Trims `input` and checks it can describe an Expense.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptionError::Blank`] or [`DescriptionError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, DescriptionError> {
        let description = input.trim();
        if description.is_empty() {
            Err(DescriptionError::Blank)
        } else if description.chars().count() > DESCRIPTION_MAX {
            Err(DescriptionError::TooLong)
        } else {
            Ok(Self(description.to_owned()))
        }
    }

    /// Like [`Description::parse`], but blank `input` is no description, as a Payment needs none.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptionError::TooLong`].
    pub fn parse_optional(input: &str) -> Result<Option<Self>, DescriptionError> {
        match Self::parse(input) {
            Ok(description) => Ok(Some(description)),
            Err(DescriptionError::Blank) => Ok(None),
            Err(err) => Err(err),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DescriptionError {
    #[error("the description is blank")]
    Blank,
    #[error("the description is longer than {DESCRIPTION_MAX} characters")]
    TooLong,
}

/// An Expense as a Member enters it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExpense {
    pub payer: MemberId,
    pub amount: Amount,
    pub description: Description,
    pub expense_date: Date,
}

#[derive(Debug, thiserror::Error)]
pub enum ExpenseError {
    #[error("the Expense date is in the future")]
    FutureDate,
    #[error("the Payer wasn't in the Crew on the Expense date")]
    PayerNotInCrew,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// A Payment as a Member enters it. It is dated when it's logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPayment {
    pub sender: MemberId,
    pub recipient: MemberId,
    pub amount: Amount,
    pub description: Option<Description>,
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("the Sender and the Recipient are the same Member")]
    SameMember,
    #[error("the Sender isn't in the Crew")]
    SenderNotInCrew,
    #[error("the Recipient isn't in the Crew")]
    RecipientNotInCrew,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// A logged Expense, as lists show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpenseEntry {
    pub id: ExpenseId,
    pub payer: DisplayName,
    pub amount: Amount,
    pub description: Description,
    pub expense_date: Date,
}

/// A logged Payment, as lists show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEntry {
    pub id: PaymentId,
    pub sender: DisplayName,
    pub recipient: DisplayName,
    pub amount: Amount,
    pub description: Option<Description>,
    /// The day in the Crew's time zone it was logged on.
    pub logged_on: Date,
}

/// An Expense or a Payment, as lists show them together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Expense(ExpenseEntry),
    Payment(PaymentEntry),
}

/// One Member's Balance, with the name to show it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberBalance {
    pub member: MemberName,
    pub balance: Crowns,
}

/// What the Finance home screen shows: everyone's Balance and the latest entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    /// Every Member's Balance, in the order they joined.
    pub balances: Vec<MemberBalance>,
    /// The [`RECENT_ENTRIES`] entries logged last, newest first.
    pub recent: Vec<Entry>,
}

impl Overview {
    /// `member`'s Balance; Even for someone not in the Crew.
    #[must_use]
    pub fn balance_of(&self, member: MemberId) -> Crowns {
        self.balances
            .iter()
            .find(|balance| balance.member.id == member)
            .map_or(Crowns::ZERO, |balance| balance.balance)
    }
}

/// Logs Expenses and Payments and works out Balances.
#[derive(Debug, Clone)]
pub struct Finance {
    db: SqlitePool,
    clock: Clock,
}

impl Finance {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock) -> Self {
        Self { db, clock }
    }

    /// Today in the Crew's time zone: the Expense date a new Expense starts with, and the
    /// latest one it may have.
    #[must_use]
    pub fn today(&self) -> Date {
        self.clock.today()
    }

    /// Logs `expense`, entered by `logger`.
    ///
    /// # Errors
    ///
    /// Returns [`ExpenseError::FutureDate`] for an Expense date after today,
    /// [`ExpenseError::PayerNotInCrew`] if the Payer hadn't joined by the Expense date, or a
    /// database error.
    pub async fn log_expense(
        &self,
        logger: MemberId,
        expense: &NewExpense,
    ) -> Result<ExpenseId, ExpenseError> {
        if expense.expense_date > self.today() {
            return Err(ExpenseError::FutureDate);
        }
        let payer = expense.payer.get();
        let joined_on = sqlx::query_scalar!("SELECT joined_on FROM members WHERE id = ?", payer)
            .fetch_optional(&self.db)
            .await?;
        match joined_on.as_deref().map(parse_date).transpose()? {
            Some(joined_on) if joined_on <= expense.expense_date => {}
            _ => return Err(ExpenseError::PayerNotInCrew),
        }
        let amount = expense.amount.get();
        let expense_date = expense.expense_date.to_string();
        let logger = logger.get();
        let logged_at = unix_seconds(self.clock.now());
        let done = sqlx::query!(
            "INSERT INTO expenses (payer_id, amount, description, expense_date, logged_by, logged_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            payer,
            amount,
            expense.description.0,
            expense_date,
            logger,
            logged_at,
        )
        .execute(&self.db)
        .await?;
        Ok(ExpenseId(done.last_insert_rowid()))
    }

    /// Logs `payment`, entered by `logger` and dated now.
    ///
    /// # Errors
    ///
    /// Returns [`PaymentError::SameMember`] if the Sender is the Recipient,
    /// [`PaymentError::SenderNotInCrew`] or [`PaymentError::RecipientNotInCrew`] for someone who
    /// isn't a Member, or a database error.
    pub async fn log_payment(
        &self,
        logger: MemberId,
        payment: &NewPayment,
    ) -> Result<PaymentId, PaymentError> {
        if payment.sender == payment.recipient {
            return Err(PaymentError::SameMember);
        }
        let sender = payment.sender.get();
        let recipient = payment.recipient.get();
        let found = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM members WHERE id IN (?, ?)"#,
            sender,
            recipient
        )
        .fetch_all(&self.db)
        .await?;
        if !found.contains(&sender) {
            return Err(PaymentError::SenderNotInCrew);
        }
        if !found.contains(&recipient) {
            return Err(PaymentError::RecipientNotInCrew);
        }
        let amount = payment.amount.get();
        let description = payment.description.as_ref().map(Description::as_str);
        let logger = logger.get();
        let logged_at = unix_seconds(self.clock.now());
        let done = sqlx::query!(
            "INSERT INTO payments (sender_id, recipient_id, amount, description, logged_by, logged_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            sender,
            recipient,
            amount,
            description,
            logger,
            logged_at,
        )
        .execute(&self.db)
        .await?;
        Ok(PaymentId(done.last_insert_rowid()))
    }

    /// Everyone's Balance and the latest entries, read together so they agree.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn overview(&self) -> Result<Overview, sqlx::Error> {
        let mut tx = self.db.begin().await?;
        let members =
            sqlx::query!(r#"SELECT id AS "id!", display_name, joined_on FROM members ORDER BY id"#)
                .fetch_all(&mut *tx)
                .await?;
        let expenses = sqlx::query!(
            "SELECT payer_id, amount, expense_date FROM expenses WHERE deleted_at IS NULL"
        )
        .fetch_all(&mut *tx)
        .await?;
        let payments = sqlx::query!(
            "SELECT sender_id, recipient_id, amount FROM payments WHERE deleted_at IS NULL"
        )
        .fetch_all(&mut *tx)
        .await?;
        let recent = recent_entries(&mut tx).await?;
        tx.commit().await?;

        let participants = members
            .iter()
            .map(|row| {
                Ok(Participant {
                    id: MemberId::from_stored(row.id),
                    joined_on: parse_date(&row.joined_on)?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?;
        let mut ledger = Ledger::new(participants);
        for row in &expenses {
            ledger.add_expense(&LedgerExpense {
                payer: MemberId::from_stored(row.payer_id),
                amount: Amount::from_stored(row.amount),
                expense_date: parse_date(&row.expense_date)?,
            });
        }
        for row in &payments {
            ledger.add_payment(&LedgerPayment {
                sender: MemberId::from_stored(row.sender_id),
                recipient: MemberId::from_stored(row.recipient_id),
                amount: Amount::from_stored(row.amount),
            });
        }
        let balances = members
            .into_iter()
            .zip(ledger.balances())
            .map(|(row, balance)| MemberBalance {
                member: MemberName {
                    id: MemberId::from_stored(row.id),
                    display_name: DisplayName::from_stored(row.display_name),
                },
                balance: balance.crowns,
            })
            .collect();
        Ok(Overview { balances, recent })
    }
}

/// The [`RECENT_ENTRIES`] Expenses and Payments logged last, newest first.
async fn recent_entries(conn: &mut SqliteConnection) -> Result<Vec<Entry>, sqlx::Error> {
    let recent_expenses = sqlx::query!(
        r#"SELECT expenses.id AS "id!", members.display_name AS payer, amount, description,
                  expense_date, logged_at
           FROM expenses JOIN members ON members.id = expenses.payer_id
           WHERE deleted_at IS NULL
           ORDER BY logged_at DESC, expenses.id DESC
           LIMIT ?"#,
        RECENT_ENTRIES
    )
    .fetch_all(&mut *conn)
    .await?;
    let recent_payments = sqlx::query!(
        r#"SELECT payments.id AS "id!", senders.display_name AS sender,
                  recipients.display_name AS recipient, amount, description, logged_at
           FROM payments
           JOIN members AS senders ON senders.id = payments.sender_id
           JOIN members AS recipients ON recipients.id = payments.recipient_id
           WHERE deleted_at IS NULL
           ORDER BY logged_at DESC, payments.id DESC
           LIMIT ?"#,
        RECENT_ENTRIES
    )
    .fetch_all(&mut *conn)
    .await?;

    // Each list is newest first already; a stable sort keeps that order between entries
    // logged in the same second.
    let mut recent = recent_expenses
        .into_iter()
        .map(|row| {
            let entry = Entry::Expense(ExpenseEntry {
                id: ExpenseId(row.id),
                payer: DisplayName::from_stored(row.payer),
                amount: Amount::from_stored(row.amount),
                description: Description(row.description),
                expense_date: parse_date(&row.expense_date)?,
            });
            Ok((row.logged_at, entry))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    recent.extend(recent_payments.into_iter().map(|row| {
        let entry = Entry::Payment(PaymentEntry {
            id: PaymentId(row.id),
            sender: DisplayName::from_stored(row.sender),
            recipient: DisplayName::from_stored(row.recipient),
            amount: Amount::from_stored(row.amount),
            description: row.description.map(Description),
            logged_on: crew_date_of_unix_seconds(row.logged_at),
        });
        (row.logged_at, entry)
    }));
    recent.sort_by_key(|&(logged_at, _)| std::cmp::Reverse(logged_at));
    Ok(recent
        .into_iter()
        .map(|(_, entry)| entry)
        .take(usize::try_from(RECENT_ENTRIES).unwrap_or(usize::MAX))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use jiff::civil::date;
    use jiff::tz::TimeZone;

    use super::*;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::fixtures::{add_member, name};

    /// 2026-04-01T22:30:00Z, which is already 2 April in Prague (UTC+2 in summer).
    fn late_on_the_first_of_april() -> SystemTime {
        date(2026, 4, 1)
            .at(22, 30, 0, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap()
            .timestamp()
            .into()
    }

    fn finance(db: SqlitePool) -> Finance {
        finance_and_clock(db).0
    }

    fn finance_and_clock(db: SqlitePool) -> (Finance, ManualClock) {
        let clock = ManualClock::starting_at(late_on_the_first_of_april());
        (Finance::new(db, clock.clock()), clock)
    }

    fn expense(payer: MemberId, crowns: i64, description: &str, expense_date: Date) -> NewExpense {
        NewExpense {
            payer,
            amount: Amount::new(crowns).unwrap(),
            description: Description::parse(description).unwrap(),
            expense_date,
        }
    }

    fn payment(
        sender: MemberId,
        recipient: MemberId,
        crowns: i64,
        description: &str,
    ) -> NewPayment {
        NewPayment {
            sender,
            recipient,
            amount: Amount::new(crowns).unwrap(),
            description: Description::parse_optional(description).unwrap(),
        }
    }

    fn balances(overview: &Overview) -> Vec<(&str, i64)> {
        overview
            .balances
            .iter()
            .map(|balance| (balance.member.display_name.as_str(), balance.balance.get()))
            .collect()
    }

    #[test]
    fn trims_a_description() {
        assert_eq!(
            Description::parse("  Malt and hops \n").unwrap().as_str(),
            "Malt and hops"
        );
    }

    #[test]
    fn refuses_a_blank_description() {
        for input in ["", "   ", "\t\n"] {
            assert_eq!(
                Description::parse(input),
                Err(DescriptionError::Blank),
                "{input:?}"
            );
        }
    }

    #[test]
    fn limits_a_description_by_characters_not_bytes() {
        let longest = "ř".repeat(DESCRIPTION_MAX);

        assert_eq!(Description::parse(&longest).unwrap().as_str(), longest);
        assert_eq!(
            Description::parse(&format!("{longest}x")),
            Err(DescriptionError::TooLong)
        );
    }

    #[sqlx::test]
    async fn today_is_the_date_in_prague(db: SqlitePool) {
        assert_eq!(finance(db).today(), date(2026, 4, 2));
    }

    #[sqlx::test]
    async fn everyone_is_even_before_any_expense(db: SqlitePool) {
        add_member(&db, "Josef", true).await;
        add_member(&db, "Ondra", false).await;

        let overview = finance(db).overview().await.unwrap();

        assert_eq!(balances(&overview), [("Josef", 0), ("Ondra", 0)]);
        assert_eq!(overview.recent, []);
    }

    #[sqlx::test]
    async fn a_logged_expense_moves_the_balances(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        add_member(&db, "Šimon", false).await;
        let finance = finance(db);

        finance
            .log_expense(josef, &expense(ondra, 100, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        let overview = finance.overview().await.unwrap();
        assert_eq!(
            balances(&overview),
            [("Josef", -33), ("Ondra", 66), ("Šimon", -33)]
        );
        assert_eq!(overview.balance_of(ondra), Crowns::new(66));
    }

    #[sqlx::test]
    async fn records_who_logged_an_expense_for_someone_else(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;

        finance(db.clone())
            .log_expense(josef, &expense(ondra, 100, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        let row = sqlx::query!("SELECT payer_id, logged_by FROM expenses")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(
            (row.payer_id, row.logged_by),
            (ondra.get(), Some(josef.get()))
        );
    }

    #[sqlx::test]
    async fn lists_the_latest_entries_newest_first(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let finance = finance(db);
        let first = finance
            .log_expense(josef, &expense(josef, 1240, "Fermenter", date(2026, 3, 20)))
            .await
            .unwrap();
        let second = finance
            .log_expense(josef, &expense(ondra, 250, "Hops", date(2026, 3, 15)))
            .await
            .unwrap();

        let overview = finance.overview().await.unwrap();

        assert_eq!(
            overview.recent,
            [
                Entry::Expense(ExpenseEntry {
                    id: second,
                    payer: name("Ondra"),
                    amount: Amount::new(250).unwrap(),
                    description: Description::parse("Hops").unwrap(),
                    expense_date: date(2026, 3, 15),
                }),
                Entry::Expense(ExpenseEntry {
                    id: first,
                    payer: name("Josef"),
                    amount: Amount::new(1240).unwrap(),
                    description: Description::parse("Fermenter").unwrap(),
                    expense_date: date(2026, 3, 20),
                }),
            ]
        );
    }

    #[sqlx::test]
    async fn lists_only_the_ten_latest_entries(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let finance = finance(db);
        for crowns in 1..=11 {
            finance
                .log_expense(josef, &expense(josef, crowns, "Malt", date(2026, 3, 20)))
                .await
                .unwrap();
        }

        let recent = finance.overview().await.unwrap().recent;

        let amounts: Vec<i64> = recent.iter().map(amount_of).collect();
        assert_eq!(amounts, [11, 10, 9, 8, 7, 6, 5, 4, 3, 2]);
    }

    #[sqlx::test]
    async fn an_expense_may_be_dated_today_in_prague(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;

        finance(db)
            .log_expense(josef, &expense(josef, 100, "Malt", date(2026, 4, 2)))
            .await
            .unwrap();
    }

    #[sqlx::test]
    async fn refuses_an_expense_dated_in_the_future(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let finance = finance(db);

        let result = finance
            .log_expense(josef, &expense(josef, 100, "Malt", date(2026, 4, 3)))
            .await;

        assert!(
            matches!(result, Err(ExpenseError::FutureDate)),
            "{result:?}"
        );
        assert_eq!(finance.overview().await.unwrap().recent, []);
    }

    #[sqlx::test]
    async fn refuses_a_payer_who_had_not_joined_by_the_expense_date(db: SqlitePool) {
        // Fixture Members join on 2026-03-14.
        let josef = add_member(&db, "Josef", true).await;
        let finance = finance(db);

        let result = finance
            .log_expense(josef, &expense(josef, 100, "Malt", date(2026, 3, 13)))
            .await;

        assert!(
            matches!(result, Err(ExpenseError::PayerNotInCrew)),
            "{result:?}"
        );
        finance
            .log_expense(josef, &expense(josef, 100, "Malt", date(2026, 3, 14)))
            .await
            .unwrap();
    }

    #[sqlx::test]
    async fn refuses_a_payer_who_is_not_a_member(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;

        let result = finance(db)
            .log_expense(
                josef,
                &expense(MemberId::from_client(99), 100, "Malt", date(2026, 3, 20)),
            )
            .await;

        assert!(
            matches!(result, Err(ExpenseError::PayerNotInCrew)),
            "{result:?}"
        );
    }

    fn amount_of(entry: &Entry) -> i64 {
        match entry {
            Entry::Expense(expense) => expense.amount.get(),
            Entry::Payment(payment) => payment.amount.get(),
        }
    }

    #[test]
    fn a_payment_may_have_no_description() {
        for input in ["", "   ", "\t\n"] {
            assert_eq!(Description::parse_optional(input), Ok(None), "{input:?}");
        }
        assert_eq!(
            Description::parse_optional(" For the hops ").unwrap(),
            Some(Description::parse("For the hops").unwrap())
        );
        assert_eq!(
            Description::parse_optional(&"x".repeat(DESCRIPTION_MAX + 1)),
            Err(DescriptionError::TooLong)
        );
    }

    #[sqlx::test]
    async fn a_logged_payment_moves_both_balances(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        let finance = finance(db);
        finance
            .log_expense(josef, &expense(josef, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        finance
            .log_payment(ondra, &payment(ondra, josef, 100, ""))
            .await
            .unwrap();

        let overview = finance.overview().await.unwrap();
        assert_eq!(
            balances(&overview),
            [("Josef", 100), ("Ondra", 0), ("Šimon", -100)]
        );
        assert_eq!(overview.balance_of(simon), Crowns::new(-100));
    }

    #[sqlx::test]
    async fn records_who_logged_a_payment_and_when(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;

        finance(db.clone())
            .log_payment(josef, &payment(ondra, josef, 500, "For the hops"))
            .await
            .unwrap();

        let row = sqlx::query!(
            "SELECT sender_id, recipient_id, description, logged_by, logged_at FROM payments"
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(
            (
                row.sender_id,
                row.recipient_id,
                row.description.as_deref(),
                row.logged_by
            ),
            (
                ondra.get(),
                josef.get(),
                Some("For the hops"),
                Some(josef.get())
            )
        );
        assert_eq!(row.logged_at, unix_seconds(late_on_the_first_of_april()));
    }

    #[sqlx::test]
    async fn refuses_a_payment_to_oneself(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let finance = finance(db);

        let result = finance
            .log_payment(josef, &payment(josef, josef, 100, ""))
            .await;

        assert!(
            matches!(result, Err(PaymentError::SameMember)),
            "{result:?}"
        );
        assert_eq!(finance.overview().await.unwrap().recent, []);
    }

    #[sqlx::test]
    async fn refuses_a_sender_or_recipient_who_is_not_a_member(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let stranger = MemberId::from_client(99);
        let finance = finance(db);

        let from_stranger = finance
            .log_payment(josef, &payment(stranger, josef, 100, ""))
            .await;
        let to_stranger = finance
            .log_payment(josef, &payment(josef, stranger, 100, ""))
            .await;

        assert!(
            matches!(from_stranger, Err(PaymentError::SenderNotInCrew)),
            "{from_stranger:?}"
        );
        assert!(
            matches!(to_stranger, Err(PaymentError::RecipientNotInCrew)),
            "{to_stranger:?}"
        );
    }

    #[sqlx::test]
    async fn lists_payments_among_the_latest_entries(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let (finance, clock) = finance_and_clock(db);
        let earlier = finance
            .log_payment(josef, &payment(josef, ondra, 200, "For the hops"))
            .await
            .unwrap();
        clock.advance(Duration::from_secs(60));
        finance
            .log_expense(ondra, &expense(ondra, 250, "Hops", date(2026, 3, 15)))
            .await
            .unwrap();
        clock.advance(Duration::from_secs(60));
        let later = finance
            .log_payment(ondra, &payment(ondra, josef, 500, ""))
            .await
            .unwrap();

        let recent = finance.overview().await.unwrap().recent;

        assert_eq!(
            recent.iter().map(amount_of).collect::<Vec<_>>(),
            [500, 250, 200]
        );
        assert_eq!(
            recent[0],
            Entry::Payment(PaymentEntry {
                id: later,
                sender: name("Ondra"),
                recipient: name("Josef"),
                amount: Amount::new(500).unwrap(),
                description: None,
                logged_on: date(2026, 4, 2),
            })
        );
        assert_eq!(
            recent[2],
            Entry::Payment(PaymentEntry {
                id: earlier,
                sender: name("Josef"),
                recipient: name("Ondra"),
                amount: Amount::new(200).unwrap(),
                description: Some(Description::parse("For the hops").unwrap()),
                logged_on: date(2026, 4, 2),
            })
        );
    }

    #[sqlx::test]
    async fn the_latest_entries_mix_payments_and_expenses_up_to_ten(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let (finance, clock) = finance_and_clock(db);
        for crowns in 1..=12 {
            if crowns % 2 == 0 {
                finance
                    .log_payment(josef, &payment(josef, ondra, crowns, ""))
                    .await
                    .unwrap();
            } else {
                finance
                    .log_expense(josef, &expense(josef, crowns, "Malt", date(2026, 3, 20)))
                    .await
                    .unwrap();
            }
            clock.advance(Duration::from_secs(1));
        }

        let recent = finance.overview().await.unwrap().recent;

        let amounts: Vec<i64> = recent.iter().map(amount_of).collect();
        assert_eq!(amounts, [12, 11, 10, 9, 8, 7, 6, 5, 4, 3]);
    }
}
