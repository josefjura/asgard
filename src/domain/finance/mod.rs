//! The Crew's shared money: logging Expenses and Payments, and working out everyone's Balance
//! and the Settlements that would make them Even.

pub mod ledger;
pub mod money;
pub mod settlement;

use std::collections::BTreeSet;

use jiff::civil::Date;
use sqlx::{SqliteConnection, SqlitePool};

use self::ledger::{Ledger, LedgerExpense, LedgerPayment, Participant};
use self::money::{Amount, Crowns};
use self::settlement::{Settlement, settlement_plan};
use crate::domain::channel::Emitter;
use crate::domain::clock::{Clock, crew_date_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, Member, MemberId, MemberName, parse_date};

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

/// What an Expense or Payment was for: trimmed, non-blank and at most [`DESCRIPTION_MAX`]
/// characters.
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
    #[error(transparent)]
    Entry(#[from] EntryError),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// Why an Expense or Payment can't be edited or deleted.
#[derive(Debug, thiserror::Error)]
pub enum EntryError {
    /// It never existed, or it was deleted.
    #[error("no such entry")]
    NotFound,
    /// The Member asking is neither the entry's Payer or Sender nor the Admin.
    #[error("the entry isn't the Member's to change")]
    NotAllowed,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// Whether `member` may edit or delete an entry whose money is `payer_or_sender`'s: the Payer of an
/// Expense or the Sender of a Payment. The Admin may change anyone's.
#[must_use]
pub fn may_change(member: &Member, payer_or_sender: MemberId) -> bool {
    member.is_admin || member.id == payer_or_sender
}

/// A Payment as a Member enters it. It is dated when it's logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPayment {
    pub sender: MemberId,
    pub recipient: MemberId,
    pub amount: Amount,
    pub description: Option<Description>,
}

impl NewPayment {
    /// The people the Payment involves: its Sender and its Recipient.
    #[must_use]
    pub const fn involved(&self) -> [MemberId; 2] {
        [self.sender, self.recipient]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("the Sender and the Recipient are the same Member")]
    SameMember,
    #[error("the Sender isn't in the Crew")]
    SenderNotInCrew,
    #[error("the Recipient isn't in the Crew")]
    RecipientNotInCrew,
    #[error(transparent)]
    Entry(#[from] EntryError),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

/// Who last edited an entry, and on which day in the Crew's time zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub by: DisplayName,
    pub on: Date,
}

/// A logged Expense, as lists show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpenseEntry {
    pub id: ExpenseId,
    pub payer: MemberName,
    pub amount: Amount,
    pub description: Description,
    pub expense_date: Date,
    /// `None` until someone edits it.
    pub edited: Option<Edit>,
}

/// A logged Payment, as lists show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEntry {
    pub id: PaymentId,
    pub sender: MemberName,
    pub recipient: DisplayName,
    pub amount: Amount,
    pub description: Option<Description>,
    /// The day in the Crew's time zone it was logged on.
    pub logged_on: Date,
    /// `None` until someone edits it.
    pub edited: Option<Edit>,
}

/// An Expense or a Payment, as lists show them together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Expense(ExpenseEntry),
    Payment(PaymentEntry),
}

impl Entry {
    /// Whether `member` may edit or delete the entry, by [`may_change`].
    #[must_use]
    pub fn may_be_changed_by(&self, member: &Member) -> bool {
        let payer_or_sender = match self {
            Self::Expense(expense) => expense.payer.id,
            Self::Payment(payment) => payment.sender.id,
        };
        may_change(member, payer_or_sender)
    }
}

/// One Member's Balance, with the name to show it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberBalance {
    pub member: MemberName,
    pub balance: Crowns,
}

/// What the Finance home screen shows: everyone's Balance, the Settlement plan and the latest
/// entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    /// Every Member's Balance, in the order they joined.
    pub balances: Vec<MemberBalance>,
    /// The Settlement plan for those Balances.
    pub settlements: Vec<Settlement>,
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

    /// The Settlements `member` sends or receives.
    pub fn settlements_of(&self, member: MemberId) -> impl Iterator<Item = &Settlement> {
        self.settlements.iter().filter(move |settlement| {
            settlement.sender.id == member || settlement.recipient.id == member
        })
    }
}

/// An Expense or a Payment, as an event names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryId {
    Expense(ExpenseId),
    Payment(PaymentId),
}

/// What a Member did to an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryAction {
    Added,
    Edited,
    Deleted,
}

/// A Member added, edited or deleted an Expense or Payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinanceEvent {
    pub entry: EntryId,
    pub action: EntryAction,
    /// The Member who did it.
    pub actor: MemberId,
    /// Everyone the entry involves, before and after an edit, except the actor, in id order:
    /// an Expense's split, or a Payment's Sender and Recipient.
    pub notify: Vec<MemberId>,
}

/// Logs Expenses and Payments and works out Balances.
#[derive(Debug, Clone)]
pub struct Finance {
    db: SqlitePool,
    clock: Clock,
    events: Emitter<FinanceEvent>,
}

impl Finance {
    #[must_use]
    pub const fn new(db: SqlitePool, clock: Clock, events: Emitter<FinanceEvent>) -> Self {
        Self { db, clock, events }
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
        self.check_expense(expense).await?;
        let split = self.split_on(expense.expense_date).await?;
        let payer = expense.payer.get();
        let amount = expense.amount.get();
        let expense_date = expense.expense_date.to_string();
        let logged_by = logger.get();
        let logged_at = unix_seconds(self.clock.now());
        let done = sqlx::query!(
            "INSERT INTO expenses (payer_id, amount, description, expense_date, logged_by, logged_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            payer,
            amount,
            expense.description.0,
            expense_date,
            logged_by,
            logged_at,
        )
        .execute(&self.db)
        .await?;
        let id = ExpenseId(done.last_insert_rowid());
        self.emit(EntryId::Expense(id), EntryAction::Added, logger, split);
        Ok(id)
    }

    /// Everyone in the Crew on `date`: the Members an Expense of that date is split between.
    async fn split_on(&self, date: Date) -> Result<Vec<MemberId>, sqlx::Error> {
        let date = date.to_string();
        let split = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM members WHERE joined_on <= ?"#,
            date
        )
        .fetch_all(&self.db)
        .await?;
        Ok(split.into_iter().map(MemberId::from_stored).collect())
    }

    /// Emits that `actor` did `action` to `entry`, for everyone `involved` but the actor to hear.
    fn emit(
        &self,
        entry: EntryId,
        action: EntryAction,
        actor: MemberId,
        involved: impl IntoIterator<Item = MemberId>,
    ) {
        let notify = involved
            .into_iter()
            .filter(|&member| member != actor)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.events.emit(FinanceEvent {
            entry,
            action,
            actor,
            notify,
        });
    }

    /// Refuses an Expense dated after today, or paid by someone who hadn't joined by its
    /// Expense date.
    async fn check_expense(&self, expense: &NewExpense) -> Result<(), ExpenseError> {
        if expense.expense_date > self.today() {
            return Err(ExpenseError::FutureDate);
        }
        let payer = expense.payer.get();
        let joined_on = sqlx::query_scalar!("SELECT joined_on FROM members WHERE id = ?", payer)
            .fetch_optional(&self.db)
            .await?;
        match joined_on.as_deref().map(parse_date).transpose()? {
            Some(joined_on) if joined_on <= expense.expense_date => Ok(()),
            _ => Err(ExpenseError::PayerNotInCrew),
        }
    }

    /// The Expense `id` as it stands, for `member` to edit.
    ///
    /// # Errors
    ///
    /// Returns [`EntryError::NotFound`] for an Expense that doesn't exist or was deleted,
    /// [`EntryError::NotAllowed`] unless [`may_change`] lets `member` change it, or a database
    /// error.
    pub async fn expense_to_edit(
        &self,
        member: &Member,
        id: ExpenseId,
    ) -> Result<NewExpense, EntryError> {
        let row = sqlx::query!(
            "SELECT payer_id, amount, description, expense_date FROM expenses
             WHERE id = ? AND deleted_at IS NULL",
            id.0
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(EntryError::NotFound)?;
        let payer = MemberId::from_stored(row.payer_id);
        if !may_change(member, payer) {
            return Err(EntryError::NotAllowed);
        }
        Ok(NewExpense {
            payer,
            amount: Amount::from_stored(row.amount),
            description: Description(row.description),
            expense_date: parse_date(&row.expense_date)?,
        })
    }

    /// Replaces the Expense `id` with `expense`, recording that `editor` edited it now.
    ///
    /// # Errors
    ///
    /// Returns an [`EntryError`] as [`Finance::expense_to_edit`] does, what
    /// [`Finance::log_expense`] returns for an Expense it would refuse, or a database error.
    pub async fn edit_expense(
        &self,
        editor: &Member,
        id: ExpenseId,
        expense: &NewExpense,
    ) -> Result<(), ExpenseError> {
        let before = self.expense_to_edit(editor, id).await?;
        self.check_expense(expense).await?;
        let mut involved = self.split_on(before.expense_date).await?;
        involved.extend(self.split_on(expense.expense_date).await?);
        let payer = before.payer.get();
        let new_payer = expense.payer.get();
        let amount = expense.amount.get();
        let expense_date = expense.expense_date.to_string();
        let edited_by = editor.id.get();
        let edited_at = unix_seconds(self.clock.now());
        // Matching the Payer again keeps an edit off an Expense whose Payer changed since the
        // rights were checked.
        let done = sqlx::query!(
            "UPDATE expenses
             SET payer_id = ?, amount = ?, description = ?, expense_date = ?, edited_by = ?,
                 edited_at = ?
             WHERE id = ? AND payer_id = ? AND deleted_at IS NULL",
            new_payer,
            amount,
            expense.description.0,
            expense_date,
            edited_by,
            edited_at,
            id.0,
            payer,
        )
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(EntryError::NotFound.into());
        }
        self.emit(
            EntryId::Expense(id),
            EntryAction::Edited,
            editor.id,
            involved,
        );
        Ok(())
    }

    /// Deletes the Expense `id`, keeping who deleted it and when.
    ///
    /// # Errors
    ///
    /// Returns an [`EntryError`] as [`Finance::expense_to_edit`] does.
    pub async fn delete_expense(&self, member: &Member, id: ExpenseId) -> Result<(), EntryError> {
        let expense = self.expense_to_edit(member, id).await?;
        let split = self.split_on(expense.expense_date).await?;
        let payer = expense.payer.get();
        let deleted_by = member.id.get();
        let deleted_at = unix_seconds(self.clock.now());
        let done = sqlx::query!(
            "UPDATE expenses SET deleted_by = ?, deleted_at = ?
             WHERE id = ? AND payer_id = ? AND deleted_at IS NULL",
            deleted_by,
            deleted_at,
            id.0,
            payer,
        )
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(EntryError::NotFound);
        }
        self.emit(EntryId::Expense(id), EntryAction::Deleted, member.id, split);
        Ok(())
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
        self.check_payment(payment).await?;
        let sender = payment.sender.get();
        let recipient = payment.recipient.get();
        let amount = payment.amount.get();
        let description = payment.description.as_ref().map(Description::as_str);
        let logged_by = logger.get();
        let logged_at = unix_seconds(self.clock.now());
        let done = sqlx::query!(
            "INSERT INTO payments (sender_id, recipient_id, amount, description, logged_by, logged_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            sender,
            recipient,
            amount,
            description,
            logged_by,
            logged_at,
        )
        .execute(&self.db)
        .await?;
        let id = PaymentId(done.last_insert_rowid());
        self.emit(
            EntryId::Payment(id),
            EntryAction::Added,
            logger,
            payment.involved(),
        );
        Ok(id)
    }

    /// Refuses a Payment to its own Sender, or between people who aren't Members.
    async fn check_payment(&self, payment: &NewPayment) -> Result<(), PaymentError> {
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
            Err(PaymentError::SenderNotInCrew)
        } else if !found.contains(&recipient) {
            Err(PaymentError::RecipientNotInCrew)
        } else {
            Ok(())
        }
    }

    /// The Payment `id` as it stands, for `member` to edit.
    ///
    /// # Errors
    ///
    /// Returns [`EntryError::NotFound`] for a Payment that doesn't exist or was deleted,
    /// [`EntryError::NotAllowed`] unless [`may_change`] lets `member` change it, or a database
    /// error.
    pub async fn payment_to_edit(
        &self,
        member: &Member,
        id: PaymentId,
    ) -> Result<NewPayment, EntryError> {
        let row = sqlx::query!(
            "SELECT sender_id, recipient_id, amount, description FROM payments
             WHERE id = ? AND deleted_at IS NULL",
            id.0
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(EntryError::NotFound)?;
        let sender = MemberId::from_stored(row.sender_id);
        if !may_change(member, sender) {
            return Err(EntryError::NotAllowed);
        }
        Ok(NewPayment {
            sender,
            recipient: MemberId::from_stored(row.recipient_id),
            amount: Amount::from_stored(row.amount),
            description: row.description.map(Description),
        })
    }

    /// Replaces the Payment `id` with `payment`, recording that `editor` edited it now. It keeps
    /// the date it was logged on.
    ///
    /// # Errors
    ///
    /// Returns an [`EntryError`] as [`Finance::payment_to_edit`] does, what
    /// [`Finance::log_payment`] returns for a Payment it would refuse, or a database error.
    pub async fn edit_payment(
        &self,
        editor: &Member,
        id: PaymentId,
        payment: &NewPayment,
    ) -> Result<(), PaymentError> {
        let before = self.payment_to_edit(editor, id).await?;
        self.check_payment(payment).await?;
        let involved = before.involved().into_iter().chain(payment.involved());
        let sender = before.sender.get();
        let new_sender = payment.sender.get();
        let recipient = payment.recipient.get();
        let amount = payment.amount.get();
        let description = payment.description.as_ref().map(Description::as_str);
        let edited_by = editor.id.get();
        let edited_at = unix_seconds(self.clock.now());
        // Matched again like an Expense's Payer in [`Finance::edit_expense`].
        let done = sqlx::query!(
            "UPDATE payments
             SET sender_id = ?, recipient_id = ?, amount = ?, description = ?, edited_by = ?,
                 edited_at = ?
             WHERE id = ? AND sender_id = ? AND deleted_at IS NULL",
            new_sender,
            recipient,
            amount,
            description,
            edited_by,
            edited_at,
            id.0,
            sender,
        )
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(EntryError::NotFound.into());
        }
        self.emit(
            EntryId::Payment(id),
            EntryAction::Edited,
            editor.id,
            involved,
        );
        Ok(())
    }

    /// Deletes the Payment `id`, keeping who deleted it and when.
    ///
    /// # Errors
    ///
    /// Returns an [`EntryError`] as [`Finance::payment_to_edit`] does.
    pub async fn delete_payment(&self, member: &Member, id: PaymentId) -> Result<(), EntryError> {
        let payment = self.payment_to_edit(member, id).await?;
        let sender = payment.sender.get();
        let deleted_by = member.id.get();
        let deleted_at = unix_seconds(self.clock.now());
        let done = sqlx::query!(
            "UPDATE payments SET deleted_by = ?, deleted_at = ?
             WHERE id = ? AND sender_id = ? AND deleted_at IS NULL",
            deleted_by,
            deleted_at,
            id.0,
            sender,
        )
        .execute(&self.db)
        .await?;
        if done.rows_affected() == 0 {
            return Err(EntryError::NotFound);
        }
        self.emit(
            EntryId::Payment(id),
            EntryAction::Deleted,
            member.id,
            payment.involved(),
        );
        Ok(())
    }

    /// Everyone's Balance, the Settlement plan and the latest entries, read together so they
    /// agree.
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
            .collect::<Vec<_>>();
        let settlements = settlement_plan(&balances);
        Ok(Overview {
            balances,
            settlements,
            recent,
        })
    }
}

/// The [`RECENT_ENTRIES`] Expenses and Payments logged last, newest first.
async fn recent_entries(conn: &mut SqliteConnection) -> Result<Vec<Entry>, sqlx::Error> {
    let recent_expenses = sqlx::query!(
        r#"SELECT expenses.id AS "id!", payer_id, payers.display_name AS payer, amount,
                  description, expense_date, logged_at, editors.display_name AS "editor?",
                  edited_at
           FROM expenses
           JOIN members AS payers ON payers.id = expenses.payer_id
           LEFT JOIN members AS editors ON editors.id = expenses.edited_by
           WHERE deleted_at IS NULL
           ORDER BY logged_at DESC, expenses.id DESC
           LIMIT ?"#,
        RECENT_ENTRIES
    )
    .fetch_all(&mut *conn)
    .await?;
    let recent_payments = sqlx::query!(
        r#"SELECT payments.id AS "id!", sender_id, senders.display_name AS sender,
                  recipients.display_name AS recipient, amount, description, logged_at,
                  editors.display_name AS "editor?", edited_at
           FROM payments
           JOIN members AS senders ON senders.id = payments.sender_id
           JOIN members AS recipients ON recipients.id = payments.recipient_id
           LEFT JOIN members AS editors ON editors.id = payments.edited_by
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
                payer: MemberName {
                    id: MemberId::from_stored(row.payer_id),
                    display_name: DisplayName::from_stored(row.payer),
                },
                amount: Amount::from_stored(row.amount),
                description: Description(row.description),
                expense_date: parse_date(&row.expense_date)?,
                edited: edit(row.editor, row.edited_at),
            });
            Ok((row.logged_at, entry))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    recent.extend(recent_payments.into_iter().map(|row| {
        let entry = Entry::Payment(PaymentEntry {
            id: PaymentId(row.id),
            sender: MemberName {
                id: MemberId::from_stored(row.sender_id),
                display_name: DisplayName::from_stored(row.sender),
            },
            recipient: DisplayName::from_stored(row.recipient),
            amount: Amount::from_stored(row.amount),
            description: row.description.map(Description),
            logged_on: crew_date_of_unix_seconds(row.logged_at),
            edited: edit(row.editor, row.edited_at),
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

/// An entry's last edit from its stored editor and instant, if it was ever edited.
fn edit(editor: Option<String>, edited_at: Option<i64>) -> Option<Edit> {
    Some(Edit {
        by: DisplayName::from_stored(editor?),
        on: crew_date_of_unix_seconds(edited_at?),
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use std::convert::identity;

    use jiff::civil::date;
    use jiff::tz::TimeZone;
    use tokio::sync::broadcast;

    use super::*;
    use crate::domain::channel::Channel;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::Members;
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
        let events = Channel::new().emitter(identity);
        (Finance::new(db, clock.clock(), events), clock)
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

    fn member_name(id: MemberId, display_name: &str) -> MemberName {
        MemberName {
            id,
            display_name: name(display_name),
        }
    }

    /// The Member `id` as their session finds them.
    async fn member(db: &SqlitePool, id: MemberId) -> Member {
        Members::new(db.clone()).find(id).await.unwrap().unwrap()
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
    async fn suggests_settlements_from_the_balances(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let simon = add_member(&db, "Šimon", false).await;
        let finance = finance(db);
        finance
            .log_expense(josef, &expense(ondra, 900, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        finance
            .log_payment(josef, &payment(josef, ondra, 300, ""))
            .await
            .unwrap();

        let overview = finance.overview().await.unwrap();

        let plan: Vec<_> = overview
            .settlements
            .iter()
            .map(|s| (s.sender.id, s.recipient.id, s.amount.get()))
            .collect();
        assert_eq!(plan, [(simon, ondra, 300)]);
        assert_eq!(overview.settlements_of(josef).count(), 0);
        assert_eq!(overview.settlements_of(ondra).count(), 1);
        assert_eq!(overview.settlements_of(simon).count(), 1);
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
                    payer: member_name(ondra, "Ondra"),
                    amount: Amount::new(250).unwrap(),
                    description: Description::parse("Hops").unwrap(),
                    expense_date: date(2026, 3, 15),
                    edited: None,
                }),
                Entry::Expense(ExpenseEntry {
                    id: first,
                    payer: member_name(josef, "Josef"),
                    amount: Amount::new(1240).unwrap(),
                    description: Description::parse("Fermenter").unwrap(),
                    expense_date: date(2026, 3, 20),
                    edited: None,
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
                sender: member_name(ondra, "Ondra"),
                recipient: name("Josef"),
                amount: Amount::new(500).unwrap(),
                description: None,
                logged_on: date(2026, 4, 2),
                edited: None,
            })
        );
        assert_eq!(
            recent[2],
            Entry::Payment(PaymentEntry {
                id: earlier,
                sender: member_name(josef, "Josef"),
                recipient: name("Ondra"),
                amount: Amount::new(200).unwrap(),
                description: Some(Description::parse("For the hops").unwrap()),
                logged_on: date(2026, 4, 2),
                edited: None,
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

    /// Josef the Admin, and Ondra and Šimon, who aren't.
    async fn crew_of_three(db: &SqlitePool) -> (Member, Member, Member) {
        let josef = add_member(db, "Josef", true).await;
        let ondra = add_member(db, "Ondra", false).await;
        let simon = add_member(db, "Šimon", false).await;
        (
            member(db, josef).await,
            member(db, ondra).await,
            member(db, simon).await,
        )
    }

    #[test]
    fn the_payer_or_sender_and_the_admin_may_change_an_entry() {
        let payer_or_sender = MemberId::from_client(2);
        let member = |id, is_admin| Member {
            id: MemberId::from_client(id),
            display_name: name("Anyone"),
            is_admin,
            joined_on: date(2026, 3, 14),
            welcomed: true,
        };

        assert!(may_change(&member(2, false), payer_or_sender));
        assert!(may_change(&member(1, true), payer_or_sender));
        assert!(!may_change(&member(3, false), payer_or_sender));
    }

    #[sqlx::test]
    async fn the_payer_edits_their_expense_and_the_split_follows(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let simon_id = simon.id.get();
        // Šimon joins later, so moving the Expense date past that day brings him into the split.
        sqlx::query!(
            "UPDATE members SET joined_on = '2026-03-25' WHERE id = ?",
            simon_id
        )
        .execute(&db)
        .await
        .unwrap();
        let finance = finance(db);
        let id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        assert_eq!(
            balances(&finance.overview().await.unwrap()),
            [("Josef", -150), ("Ondra", 150), ("Šimon", 0)]
        );

        let edited = expense(josef.id, 600, "Malt and hops", date(2026, 3, 30));
        finance.edit_expense(&ondra, id, &edited).await.unwrap();

        let overview = finance.overview().await.unwrap();
        assert_eq!(
            balances(&overview),
            [("Josef", 400), ("Ondra", -200), ("Šimon", -200)]
        );
        assert_eq!(
            overview.recent,
            [Entry::Expense(ExpenseEntry {
                id,
                payer: member_name(josef.id, "Josef"),
                amount: Amount::new(600).unwrap(),
                description: Description::parse("Malt and hops").unwrap(),
                expense_date: date(2026, 3, 30),
                edited: Some(Edit {
                    by: name("Ondra"),
                    on: date(2026, 4, 2),
                }),
            })]
        );
    }

    #[sqlx::test]
    async fn only_the_payer_or_the_admin_may_change_an_expense(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let finance = finance(db);
        // Šimon logs it, but Ondra paid.
        let id = finance
            .log_expense(simon.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        let edited = expense(ondra.id, 900, "Malt", date(2026, 3, 20));

        for refused in [
            finance.expense_to_edit(&simon, id).await.map(drop),
            finance
                .edit_expense(&simon, id, &edited)
                .await
                .map_err(|err| match err {
                    ExpenseError::Entry(err) => err,
                    err => panic!("{err:?}"),
                }),
            finance.delete_expense(&simon, id).await,
        ] {
            assert!(
                matches!(refused, Err(EntryError::NotAllowed)),
                "{refused:?}"
            );
        }
        assert_eq!(amount_of(&finance.overview().await.unwrap().recent[0]), 300);

        assert_eq!(
            finance.expense_to_edit(&ondra, id).await.unwrap(),
            expense(ondra.id, 300, "Malt", date(2026, 3, 20))
        );
        assert_eq!(
            finance.expense_to_edit(&josef, id).await.unwrap().payer,
            ondra.id
        );
        finance.edit_expense(&josef, id, &edited).await.unwrap();
        let recent = finance.overview().await.unwrap().recent;
        assert_eq!(amount_of(&recent[0]), 900);
        assert!(recent[0].may_be_changed_by(&ondra));
        assert!(recent[0].may_be_changed_by(&josef));
        assert!(!recent[0].may_be_changed_by(&simon));
    }

    #[sqlx::test]
    async fn the_payer_who_hands_an_expense_on_can_no_longer_change_it(db: SqlitePool) {
        let (_, ondra, simon) = crew_of_three(&db).await;
        let finance = finance(db);
        let id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        finance
            .edit_expense(
                &ondra,
                id,
                &expense(simon.id, 300, "Malt", date(2026, 3, 20)),
            )
            .await
            .unwrap();

        let refused = finance.delete_expense(&ondra, id).await;
        assert!(
            matches!(refused, Err(EntryError::NotAllowed)),
            "{refused:?}"
        );
        finance.delete_expense(&simon, id).await.unwrap();
    }

    #[sqlx::test]
    async fn an_edit_is_checked_like_a_new_expense(db: SqlitePool) {
        let (_, ondra, _) = crew_of_three(&db).await;
        let finance = finance(db);
        let id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        let future = finance
            .edit_expense(
                &ondra,
                id,
                &expense(ondra.id, 300, "Malt", date(2026, 4, 3)),
            )
            .await;
        let before_joining = finance
            .edit_expense(
                &ondra,
                id,
                &expense(ondra.id, 300, "Malt", date(2026, 3, 13)),
            )
            .await;

        assert!(
            matches!(future, Err(ExpenseError::FutureDate)),
            "{future:?}"
        );
        assert!(
            matches!(before_joining, Err(ExpenseError::PayerNotInCrew)),
            "{before_joining:?}"
        );
        let recent = finance.overview().await.unwrap().recent;
        assert!(
            matches!(&recent[0], Entry::Expense(e) if e.expense_date == date(2026, 3, 20) && e.edited.is_none()),
            "{recent:?}"
        );
    }

    #[sqlx::test]
    async fn a_deleted_expense_leaves_the_lists_and_the_balances(db: SqlitePool) {
        let (josef, ondra, _) = crew_of_three(&db).await;
        let (finance, clock) = finance_and_clock(db.clone());
        let id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        clock.advance(Duration::from_secs(60));

        finance.delete_expense(&josef, id).await.unwrap();

        let overview = finance.overview().await.unwrap();
        assert_eq!(
            balances(&overview),
            [("Josef", 0), ("Ondra", 0), ("Šimon", 0)]
        );
        assert_eq!(overview.recent, []);
        assert_eq!(overview.settlements, []);
        let row = sqlx::query!("SELECT deleted_by, deleted_at FROM expenses")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(
            (row.deleted_by, row.deleted_at),
            (
                Some(josef.id.get()),
                Some(unix_seconds(late_on_the_first_of_april()) + 60)
            )
        );
        for gone in [
            finance.expense_to_edit(&ondra, id).await.map(drop),
            finance.delete_expense(&ondra, id).await,
        ] {
            assert!(matches!(gone, Err(EntryError::NotFound)), "{gone:?}");
        }
        let edit = finance
            .edit_expense(&ondra, id, &expense(ondra.id, 1, "Malt", date(2026, 3, 20)))
            .await;
        assert!(
            matches!(edit, Err(ExpenseError::Entry(EntryError::NotFound))),
            "{edit:?}"
        );
    }

    #[sqlx::test]
    async fn the_sender_edits_their_payment_but_not_its_date(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let (finance, clock) = finance_and_clock(db);
        let id = finance
            .log_payment(ondra.id, &payment(ondra.id, josef.id, 300, ""))
            .await
            .unwrap();
        clock.advance(Duration::from_hours(48));

        assert_eq!(
            finance.payment_to_edit(&ondra, id).await.unwrap(),
            payment(ondra.id, josef.id, 300, "")
        );
        finance
            .edit_payment(
                &ondra,
                id,
                &payment(ondra.id, simon.id, 200, "For the hops"),
            )
            .await
            .unwrap();

        let overview = finance.overview().await.unwrap();
        assert_eq!(
            balances(&overview),
            [("Josef", 0), ("Ondra", 200), ("Šimon", -200)]
        );
        assert_eq!(
            overview.recent,
            [Entry::Payment(PaymentEntry {
                id,
                sender: member_name(ondra.id, "Ondra"),
                recipient: name("Šimon"),
                amount: Amount::new(200).unwrap(),
                description: Some(Description::parse("For the hops").unwrap()),
                logged_on: date(2026, 4, 2),
                edited: Some(Edit {
                    by: name("Ondra"),
                    on: date(2026, 4, 4),
                }),
            })]
        );
    }

    #[sqlx::test]
    async fn only_the_sender_or_the_admin_may_change_a_payment(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let finance = finance(db);
        // Josef logs it, but Ondra sent the money to Šimon.
        let id = finance
            .log_payment(josef.id, &payment(ondra.id, simon.id, 300, ""))
            .await
            .unwrap();

        for refused in [
            finance.payment_to_edit(&simon, id).await.map(drop),
            finance
                .edit_payment(&simon, id, &payment(ondra.id, simon.id, 1, ""))
                .await
                .map_err(|err| match err {
                    PaymentError::Entry(err) => err,
                    err => panic!("{err:?}"),
                }),
            finance.delete_payment(&simon, id).await,
        ] {
            assert!(
                matches!(refused, Err(EntryError::NotAllowed)),
                "{refused:?}"
            );
        }

        finance
            .edit_payment(&josef, id, &payment(ondra.id, simon.id, 100, ""))
            .await
            .unwrap();
        assert_eq!(amount_of(&finance.overview().await.unwrap().recent[0]), 100);
        let same = finance
            .edit_payment(&ondra, id, &payment(ondra.id, ondra.id, 100, ""))
            .await;
        assert!(matches!(same, Err(PaymentError::SameMember)), "{same:?}");

        finance.delete_payment(&ondra, id).await.unwrap();
        let overview = finance.overview().await.unwrap();
        assert_eq!(overview.recent, []);
        assert_eq!(
            balances(&overview),
            [("Josef", 0), ("Ondra", 0), ("Šimon", 0)]
        );
    }

    /// The Finance service and a subscription to the events it emits.
    fn finance_and_events(db: SqlitePool) -> (Finance, broadcast::Receiver<FinanceEvent>) {
        let clock = ManualClock::starting_at(late_on_the_first_of_april());
        let events = Channel::new();
        let subscription = events.subscribe();
        (
            Finance::new(db, clock.clock(), events.emitter(identity)),
            subscription,
        )
    }

    /// Every event emitted since the last call.
    fn emitted(subscription: &mut broadcast::Receiver<FinanceEvent>) -> Vec<FinanceEvent> {
        std::iter::from_fn(|| subscription.try_recv().ok()).collect()
    }

    fn event(
        entry: EntryId,
        action: EntryAction,
        actor: &Member,
        notify: &[&Member],
    ) -> FinanceEvent {
        FinanceEvent {
            entry,
            action,
            actor: actor.id,
            notify: notify.iter().map(|member| member.id).collect(),
        }
    }

    /// Moves Šimon's join date to 25 March 2026, after the other fixture Members' 14 March.
    async fn simon_joins_late(db: &SqlitePool, simon: &Member) {
        let simon = simon.id.get();
        sqlx::query!(
            "UPDATE members SET joined_on = '2026-03-25' WHERE id = ?",
            simon
        )
        .execute(db)
        .await
        .unwrap();
    }

    #[sqlx::test]
    async fn an_expense_notifies_its_split_except_the_actor(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let (finance, mut events) = finance_and_events(db);

        // The Admin logs it for Ondra, who paid.
        let id = finance
            .log_expense(josef.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        finance
            .edit_expense(
                &ondra,
                id,
                &expense(ondra.id, 600, "Malt", date(2026, 3, 20)),
            )
            .await
            .unwrap();
        finance.delete_expense(&josef, id).await.unwrap();

        let id = EntryId::Expense(id);
        assert_eq!(
            emitted(&mut events),
            [
                event(id, EntryAction::Added, &josef, &[&ondra, &simon]),
                event(id, EntryAction::Edited, &ondra, &[&josef, &simon]),
                event(id, EntryAction::Deleted, &josef, &[&ondra, &simon]),
            ]
        );
    }

    #[sqlx::test]
    async fn an_expense_notifies_only_those_in_the_crew_on_its_date(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        simon_joins_late(&db, &simon).await;
        let (finance, mut events) = finance_and_events(db);

        finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        assert_eq!(emitted(&mut events)[0].notify, [josef.id]);
    }

    #[sqlx::test]
    async fn moving_an_expense_across_a_join_date_notifies_both_splits(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        simon_joins_late(&db, &simon).await;
        let (finance, mut events) = finance_and_events(db);
        let id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        // Into Šimon's time in the Crew, and back out of it: he hears about both.
        finance
            .edit_expense(
                &ondra,
                id,
                &expense(ondra.id, 300, "Malt", date(2026, 3, 30)),
            )
            .await
            .unwrap();
        finance
            .edit_expense(
                &ondra,
                id,
                &expense(ondra.id, 300, "Malt", date(2026, 3, 20)),
            )
            .await
            .unwrap();

        let notified: Vec<_> = emitted(&mut events)
            .into_iter()
            .map(|event| event.notify)
            .collect();
        assert_eq!(
            notified,
            [
                vec![josef.id],
                vec![josef.id, simon.id],
                vec![josef.id, simon.id]
            ]
        );
    }

    #[sqlx::test]
    async fn the_admin_handing_an_expense_on_notifies_both_payers(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        simon_joins_late(&db, &simon).await;
        let (finance, mut events) = finance_and_events(db);
        let id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();

        finance
            .edit_expense(
                &josef,
                id,
                &expense(simon.id, 300, "Malt", date(2026, 3, 30)),
            )
            .await
            .unwrap();

        assert_eq!(
            emitted(&mut events)[1],
            event(
                EntryId::Expense(id),
                EntryAction::Edited,
                &josef,
                &[&ondra, &simon]
            )
        );
    }

    #[sqlx::test]
    async fn a_payment_notifies_its_sender_and_recipient_except_the_actor(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let (finance, mut events) = finance_and_events(db);

        let id = finance
            .log_payment(ondra.id, &payment(ondra.id, josef.id, 300, ""))
            .await
            .unwrap();
        finance
            .edit_payment(&ondra, id, &payment(ondra.id, josef.id, 200, ""))
            .await
            .unwrap();
        finance.delete_payment(&josef, id).await.unwrap();
        let logged_for_others = finance
            .log_payment(josef.id, &payment(ondra.id, simon.id, 100, ""))
            .await
            .unwrap();
        // The Admin, neither its Sender nor its Recipient, edits it.
        finance
            .edit_payment(
                &josef,
                logged_for_others,
                &payment(ondra.id, simon.id, 50, ""),
            )
            .await
            .unwrap();

        let id = EntryId::Payment(id);
        let for_others = EntryId::Payment(logged_for_others);
        assert_eq!(
            emitted(&mut events),
            [
                event(id, EntryAction::Added, &ondra, &[&josef]),
                event(id, EntryAction::Edited, &ondra, &[&josef]),
                event(id, EntryAction::Deleted, &josef, &[&ondra]),
                event(for_others, EntryAction::Added, &josef, &[&ondra, &simon]),
                event(for_others, EntryAction::Edited, &josef, &[&ondra, &simon]),
            ]
        );
    }

    #[sqlx::test]
    async fn changing_who_a_payment_is_between_notifies_old_and_new(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let (finance, mut events) = finance_and_events(db);
        let id = finance
            .log_payment(ondra.id, &payment(ondra.id, josef.id, 300, ""))
            .await
            .unwrap();

        // The Admin, its Recipient, says it was Šimon who sent it.
        finance
            .edit_payment(&josef, id, &payment(simon.id, josef.id, 300, ""))
            .await
            .unwrap();
        // Šimon, its Sender now, says he sent it to Ondra.
        finance
            .edit_payment(&simon, id, &payment(simon.id, ondra.id, 300, ""))
            .await
            .unwrap();

        let id = EntryId::Payment(id);
        assert_eq!(
            emitted(&mut events)[1..],
            [
                event(id, EntryAction::Edited, &josef, &[&ondra, &simon]),
                event(id, EntryAction::Edited, &simon, &[&josef, &ondra]),
            ]
        );
    }

    #[sqlx::test]
    async fn a_refused_change_emits_nothing(db: SqlitePool) {
        let (josef, ondra, simon) = crew_of_three(&db).await;
        let (finance, mut events) = finance_and_events(db);
        let expense_id = finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        let payment_id = finance
            .log_payment(ondra.id, &payment(ondra.id, josef.id, 300, ""))
            .await
            .unwrap();
        emitted(&mut events);

        finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 4, 3)))
            .await
            .unwrap_err();
        finance
            .edit_expense(
                &simon,
                expense_id,
                &expense(ondra.id, 1, "Malt", date(2026, 3, 20)),
            )
            .await
            .unwrap_err();
        finance
            .delete_expense(&simon, expense_id)
            .await
            .unwrap_err();
        finance
            .log_payment(ondra.id, &payment(ondra.id, ondra.id, 300, ""))
            .await
            .unwrap_err();
        finance
            .edit_payment(&ondra, payment_id, &payment(ondra.id, ondra.id, 1, ""))
            .await
            .unwrap_err();
        finance
            .delete_payment(&simon, payment_id)
            .await
            .unwrap_err();

        assert_eq!(emitted(&mut events), []);
    }

    #[sqlx::test]
    async fn logging_needs_no_one_listening(db: SqlitePool) {
        let (josef, ondra, _) = crew_of_three(&db).await;
        let (finance, events) = finance_and_events(db);
        drop(events);

        finance
            .log_expense(ondra.id, &expense(ondra.id, 300, "Malt", date(2026, 3, 20)))
            .await
            .unwrap();
        finance
            .log_payment(ondra.id, &payment(ondra.id, josef.id, 300, ""))
            .await
            .unwrap();
    }
}
