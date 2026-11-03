//! What the import will write, worked out from Heimdall's rows before anything is written: the
//! Members, and every entry converted to Asgard's model. Every check that needs only Heimdall's
//! data and the options happens here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

use super::source::Heimdall;
use crate::domain::clock::crew_date_of_unix_seconds;
use crate::domain::finance::Description;
use crate::domain::finance::money::Amount;
use crate::domain::members::{DisplayName, NameError};
use crate::domain::quotes::{Author, NewQuote, QuoteText};
use crate::domain::reminders::ReminderText;

/// A Heimdall user given another display name in Asgard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    /// Their display name in Heimdall, exactly.
    pub from: String,
    /// The display name they get in Asgard.
    pub to: String,
}

/// How the Admin asked for the import to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    /// The display name, after renames, of the Member who becomes the Admin.
    pub admin: String,
    pub renames: Vec<Rename>,
    /// Run everything, report, and roll back.
    pub dry_run: bool,
}

/// The kinds of entry the import brings over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    Expense,
    Payment,
    Reminder,
    Quote,
}

impl EntryKind {
    pub const ALL: [Self; 4] = [Self::Expense, Self::Payment, Self::Reminder, Self::Quote];

    #[must_use]
    pub const fn plural(self) -> &'static str {
        match self {
            Self::Expense => "Expenses",
            Self::Payment => "Payments",
            Self::Reminder => "Reminders",
            Self::Quote => "Quotes",
        }
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Expense => "Expense",
            Self::Payment => "Payment",
            Self::Reminder => "Reminder",
            Self::Quote => "Quote",
        })
    }
}

/// Why the import stopped before writing anything, or couldn't finish.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("cannot read Heimdall's database: {0}")]
    Heimdall(#[source] sqlx::Error),
    #[error(
        "Asgard's database already has Members or entries; the import only runs against a fresh one"
    )]
    NotEmpty,
    #[error(
        "Heimdall's user {id} ({name:?}) is still the unassigned slot {discord_id:?}; Heimdall \
         splits every Expense across it too, so assign or remove it in Heimdall first"
    )]
    UnassignedSlot {
        id: i64,
        name: String,
        discord_id: String,
    },
    #[error("{kind} {id} in Heimdall refers to user {user}, who isn't in Heimdall's users")]
    UnknownUser { kind: EntryKind, id: i64, user: i64 },
    #[error("--rename {0:?}: Heimdall has no user with that display name")]
    UnknownRename(String),
    #[error("Heimdall's user {heimdall_name:?} can't be a Member: {err}")]
    InvalidName {
        heimdall_name: String,
        #[source]
        err: NameError,
    },
    #[error("two Members would be called {0} (names ignore case); give one another with --rename")]
    DuplicateName(DisplayName),
    #[error("--admin {0:?} matches no imported Member")]
    UnknownAdmin(String),
    #[error("{kind} {id} in Heimdall can't be imported: {reason}")]
    InvalidEntry {
        kind: EntryKind,
        id: i64,
        reason: String,
    },
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// A Member to create, for the Heimdall user `heimdall_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMember {
    pub heimdall_id: i64,
    pub display_name: DisplayName,
    pub is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedExpense {
    pub payer: i64,
    pub amount: Amount,
    pub description: Description,
    pub expense_date: Date,
    pub logged_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPayment {
    pub sender: i64,
    pub recipient: i64,
    pub amount: Amount,
    pub description: Option<Description>,
    pub logged_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedReminder {
    pub text: ReminderText,
    pub created_by: i64,
    pub due_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedQuote {
    pub quote: NewQuote,
    pub added_at: i64,
}

/// An amount that wasn't whole crowns in Heimdall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rounding {
    pub kind: EntryKind,
    pub id: i64,
    pub original: f64,
    pub rounded: i64,
}

/// Everything the import will write. People are still Heimdall user ids; the import maps them
/// to Members once it has created them.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub members: Vec<PlannedMember>,
    /// When every Member joins the Crew: the Prague date of Heimdall's earliest entry, so every
    /// imported Expense is split across all of them as Heimdall split it.
    pub joined_on: Date,
    pub expenses: Vec<PlannedExpense>,
    pub payments: Vec<PlannedPayment>,
    pub reminders: Vec<PlannedReminder>,
    pub quotes: Vec<PlannedQuote>,
    pub rounded: Vec<Rounding>,
}

impl Plan {
    /// Converts `heimdall` to what Asgard will store. `today` is the Members' joined date if
    /// Heimdall has no entries at all.
    ///
    /// # Errors
    ///
    /// Returns the first [`ImportError`] found: an unassigned slot, an unknown user, a rename
    /// or Admin that matches nobody, a name that is invalid or taken twice, or an entry Asgard
    /// can't hold.
    pub fn new(
        heimdall: &Heimdall,
        options: &ImportOptions,
        today: Date,
    ) -> Result<Self, ImportError> {
        let members = members(heimdall, options)?;
        let known: BTreeSet<i64> = members.iter().map(|member| member.heimdall_id).collect();
        let check = |kind, id, user| {
            if known.contains(&user) {
                Ok(user)
            } else {
                Err(ImportError::UnknownUser { kind, id, user })
            }
        };
        let mut rounded = Vec::new();

        let mut expenses = Vec::new();
        for row in &heimdall.expenses {
            let kind = EntryKind::Expense;
            let logged_at = instant(kind, row.id, "created_at", row.created_at.as_deref())?;
            expenses.push(PlannedExpense {
                payer: check(kind, row.id, row.owner_id)?,
                amount: amount(kind, row.id, row.amount, &mut rounded)?,
                description: Description::parse(&row.description)
                    .map_err(|err| invalid(kind, row.id, &err))?,
                expense_date: crew_date_of_unix_seconds(logged_at),
                logged_at,
            });
        }

        let mut payments = Vec::new();
        for row in &heimdall.payments {
            let kind = EntryKind::Payment;
            if row.owner_id == row.recipient_id {
                return Err(invalid(kind, row.id, &"the Sender is also the Recipient"));
            }
            payments.push(PlannedPayment {
                sender: check(kind, row.id, row.owner_id)?,
                recipient: check(kind, row.id, row.recipient_id)?,
                amount: amount(kind, row.id, row.amount, &mut rounded)?,
                description: Description::parse_optional(&row.description)
                    .map_err(|err| invalid(kind, row.id, &err))?,
                logged_at: instant(kind, row.id, "created_at", row.created_at.as_deref())?,
            });
        }

        let mut reminders = Vec::new();
        for row in &heimdall.reminders {
            let kind = EntryKind::Reminder;
            reminders.push(PlannedReminder {
                text: ReminderText::parse(&row.text).map_err(|err| invalid(kind, row.id, &err))?,
                created_by: check(kind, row.id, row.created_by)?,
                due_at: instant(kind, row.id, "date", Some(&row.date))?,
                created_at: instant(kind, row.id, "created_at", row.created_at.as_deref())?,
            });
        }

        let mut quotes = Vec::new();
        for row in &heimdall.quotes {
            let kind = EntryKind::Quote;
            quotes.push(PlannedQuote {
                quote: NewQuote {
                    text: QuoteText::parse(&row.text).map_err(|err| invalid(kind, row.id, &err))?,
                    author: Author::parse(&row.author)
                        .map_err(|err| invalid(kind, row.id, &err))?,
                },
                added_at: instant(kind, row.id, "created_at", row.created_at.as_deref())?,
            });
        }

        let joined_on = expenses
            .iter()
            .map(|expense| expense.expense_date)
            .chain(
                payments
                    .iter()
                    .map(|payment| crew_date_of_unix_seconds(payment.logged_at)),
            )
            .min()
            .unwrap_or(today)
            .min(today);
        Ok(Self {
            members,
            joined_on,
            expenses,
            payments,
            reminders,
            quotes,
            rounded,
        })
    }
}

/// One Member per Heimdall user, renamed as asked, with the Admin marked.
fn members(
    heimdall: &Heimdall,
    options: &ImportOptions,
) -> Result<Vec<PlannedMember>, ImportError> {
    if let Some(user) = heimdall.users.iter().find(|user| user.is_unassigned()) {
        return Err(ImportError::UnassignedSlot {
            id: user.id,
            name: user.display_name.clone(),
            discord_id: user.discord_id.clone(),
        });
    }
    let renames: BTreeMap<&str, &str> = options
        .renames
        .iter()
        .map(|rename| (rename.from.as_str(), rename.to.as_str()))
        .collect();
    if let Some(unknown) = renames
        .keys()
        .find(|&&from| !heimdall.users.iter().any(|user| user.display_name == from))
    {
        return Err(ImportError::UnknownRename((*unknown).to_owned()));
    }

    let admin_key = DisplayName::parse(&options.admin)
        .map(|name| name.key())
        .map_err(|_| ImportError::UnknownAdmin(options.admin.clone()))?;
    let mut keys = BTreeSet::new();
    let mut members = Vec::new();
    for user in &heimdall.users {
        let name = renames
            .get(user.display_name.as_str())
            .copied()
            .unwrap_or(&user.display_name);
        let display_name = DisplayName::parse(name).map_err(|err| ImportError::InvalidName {
            heimdall_name: user.display_name.clone(),
            err,
        })?;
        let key = display_name.key();
        if !keys.insert(key.clone()) {
            return Err(ImportError::DuplicateName(display_name));
        }
        members.push(PlannedMember {
            heimdall_id: user.id,
            display_name,
            is_admin: key == admin_key,
        });
    }
    if members.iter().any(|member| member.is_admin) {
        Ok(members)
    } else {
        Err(ImportError::UnknownAdmin(options.admin.clone()))
    }
}

fn invalid(kind: EntryKind, id: i64, reason: &dyn fmt::Display) -> ImportError {
    ImportError::InvalidEntry {
        kind,
        id,
        reason: reason.to_string(),
    }
}

/// `value` as whole crowns, noting it in `rounded` if it wasn't already.
fn amount(
    kind: EntryKind,
    id: i64,
    value: f64,
    rounded: &mut Vec<Rounding>,
) -> Result<Amount, ImportError> {
    let crowns = whole_crowns(value)
        .ok_or_else(|| invalid(kind, id, &format_args!("the amount {value} isn't a number")))?;
    if value.fract().abs() > 0.0 {
        rounded.push(Rounding {
            kind,
            id,
            original: value,
            rounded: crowns,
        });
    }
    Amount::new(crowns).map_err(|err| invalid(kind, id, &format_args!("{err} ({value})")))
}

/// `value` rounded to the nearest whole crown, halves away from zero.
#[must_use]
pub fn whole_crowns(value: f64) -> Option<i64> {
    // Far beyond any amount, and well inside what an f64 holds exactly.
    const LIMIT: f64 = 1e15;
    if !value.is_finite() || value.abs() > LIMIT {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the value is finite, rounded and within ±1e15, so it converts exactly"
    )]
    Some(value.round() as i64)
}

/// The instant Heimdall stored in `column`, as Unix seconds. Heimdall's `CURRENT_TIMESTAMP`
/// defaults are UTC without an offset (`2026-01-20 09:25:07`); the Reminder dates its bot wrote
/// carry one (`2026-01-23T13:44:15+00:00`).
fn instant(
    kind: EntryKind,
    id: i64,
    column: &str,
    value: Option<&str>,
) -> Result<i64, ImportError> {
    let value = value.ok_or_else(|| invalid(kind, id, &format_args!("{column} is empty")))?;
    parse_instant(value)
        .ok_or_else(|| invalid(kind, id, &format_args!("{column} {value:?} isn't a time")))
}

/// Reads a time Heimdall stored, as Unix seconds. One without an offset is UTC.
#[must_use]
pub fn parse_instant(value: &str) -> Option<i64> {
    let value = value.trim().replacen(' ', "T", 1);
    if let Ok(timestamp) = value.parse::<Timestamp>() {
        return Some(timestamp.as_second());
    }
    let time: DateTime = value.parse().ok()?;
    time.to_zoned(TimeZone::UTC)
        .ok()
        .map(|zoned| zoned.timestamp().as_second())
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::super::source::{Expense, Payment, Quote, Reminder, User};
    use super::*;

    fn user(id: i64, name: &str) -> User {
        User {
            id,
            discord_id: format!("discord-{id}"),
            display_name: name.to_owned(),
        }
    }

    fn crew() -> Heimdall {
        Heimdall {
            users: vec![user(1, "Josef"), user(2, "ondrej_k"), user(3, "Pepa")],
            ..Heimdall::default()
        }
    }

    fn options() -> ImportOptions {
        ImportOptions {
            admin: "Josef".to_owned(),
            renames: Vec::new(),
            dry_run: false,
        }
    }

    fn expense(id: i64, owner_id: i64, amount: f64, created_at: &str) -> Expense {
        Expense {
            id,
            owner_id,
            amount,
            description: "Slad".to_owned(),
            created_at: Some(created_at.to_owned()),
        }
    }

    fn plan(heimdall: &Heimdall, options: &ImportOptions) -> Result<Plan, ImportError> {
        Plan::new(heimdall, options, date(2026, 11, 3))
    }

    #[test]
    fn reads_heimdalls_utc_timestamps_with_and_without_an_offset() {
        assert_eq!(parse_instant("1970-01-01 00:01:40"), Some(100));
        assert_eq!(parse_instant("1970-01-01T00:01:40"), Some(100));
        assert_eq!(parse_instant("1970-01-01T00:01:40+00:00"), Some(100));
        assert_eq!(parse_instant("1970-01-01 01:01:40+01:00"), Some(100));
        assert_eq!(parse_instant("1970-01-01T00:01:40.250Z"), Some(100));
        assert_eq!(parse_instant("yesterday"), None);
    }

    #[test]
    fn rounds_to_whole_crowns_halves_away_from_zero() {
        assert_eq!(whole_crowns(99.5), Some(100));
        assert_eq!(whole_crowns(99.49), Some(99));
        assert_eq!(whole_crowns(-0.5), Some(-1));
        assert_eq!(whole_crowns(f64::NAN), None);
        assert_eq!(whole_crowns(f64::INFINITY), None);
    }

    #[test]
    fn names_members_after_heimdall_or_their_rename_and_marks_the_admin() {
        let options = ImportOptions {
            admin: "ondra".to_owned(),
            renames: vec![Rename {
                from: "ondrej_k".to_owned(),
                to: " Ondra ".to_owned(),
            }],
            dry_run: false,
        };

        let members = plan(&crew(), &options).unwrap().members;

        let names: Vec<_> = members
            .iter()
            .map(|member| (member.display_name.as_str(), member.is_admin))
            .collect();
        assert_eq!(names, [("Josef", false), ("Ondra", true), ("Pepa", false)]);
    }

    #[test]
    fn an_expense_is_dated_on_the_prague_day_it_was_recorded() {
        let mut heimdall = crew();
        // 23:30 UTC in winter is 00:30 the next day in Prague; 21:30 UTC in summer is 23:30.
        heimdall
            .expenses
            .push(expense(1, 1, 100.0, "2026-01-14 23:30:00"));
        heimdall
            .expenses
            .push(expense(2, 1, 100.0, "2026-07-14 21:30:00"));

        let plan = plan(&heimdall, &options()).unwrap();

        let dates: Vec<_> = plan.expenses.iter().map(|e| e.expense_date).collect();
        assert_eq!(dates, [date(2026, 1, 15), date(2026, 7, 14)]);
        assert_eq!(plan.joined_on, date(2026, 1, 15));
    }

    #[test]
    fn members_join_on_the_earliest_entry_including_payments() {
        let mut heimdall = crew();
        heimdall
            .expenses
            .push(expense(1, 1, 100.0, "2026-02-01 10:00:00"));
        heimdall.payments.push(Payment {
            id: 1,
            owner_id: 2,
            recipient_id: 1,
            amount: 10.0,
            description: String::new(),
            created_at: Some("2026-01-20 10:00:00".to_owned()),
        });

        let plan = plan(&heimdall, &options()).unwrap();

        assert_eq!(plan.joined_on, date(2026, 1, 20));
        assert_eq!(plan.payments[0].description, None);
    }

    #[test]
    fn with_no_entries_members_join_today() {
        assert_eq!(
            plan(&crew(), &options()).unwrap().joined_on,
            date(2026, 11, 3)
        );
    }

    #[test]
    fn notes_every_amount_it_rounds() {
        let mut heimdall = crew();
        heimdall
            .expenses
            .push(expense(1, 1, 100.0, "2026-02-01 10:00:00"));
        heimdall
            .expenses
            .push(expense(2, 1, 99.5, "2026-02-01 10:00:00"));

        let plan = plan(&heimdall, &options()).unwrap();

        assert_eq!(plan.expenses[1].amount.get(), 100);
        assert_eq!(
            plan.rounded,
            [Rounding {
                kind: EntryKind::Expense,
                id: 2,
                original: 99.5,
                rounded: 100
            }]
        );
    }

    #[test]
    fn refuses_an_amount_that_rounds_out_of_range() {
        let mut heimdall = crew();
        heimdall
            .expenses
            .push(expense(7, 1, 0.4, "2026-02-01 10:00:00"));

        let err = plan(&heimdall, &options()).unwrap_err();

        assert!(
            matches!(
                err,
                ImportError::InvalidEntry {
                    kind: EntryKind::Expense,
                    id: 7,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn refuses_an_unassigned_slot() {
        let mut heimdall = crew();
        heimdall.users[2].discord_id = "emptyslot_3".to_owned();

        let err = plan(&heimdall, &options()).unwrap_err();

        assert!(
            matches!(err, ImportError::UnassignedSlot { id: 3, .. }),
            "{err}"
        );
    }

    #[test]
    fn refuses_a_reference_to_an_unknown_user_in_any_kind_of_entry() {
        let mut expenses = crew();
        expenses
            .expenses
            .push(expense(4, 9, 100.0, "2026-02-01 10:00:00"));
        let mut reminders = crew();
        reminders.reminders.push(Reminder {
            id: 5,
            text: "Stočit".to_owned(),
            date: "2026-12-01T10:00:00+00:00".to_owned(),
            created_by: 8,
            created_at: Some("2026-02-01 10:00:00".to_owned()),
        });

        for (heimdall, kind, id, user) in [
            (expenses, EntryKind::Expense, 4, 9),
            (reminders, EntryKind::Reminder, 5, 8),
        ] {
            let err = plan(&heimdall, &options()).unwrap_err();
            assert!(
                matches!(err, ImportError::UnknownUser { kind: k, id: i, user: u } if k == kind && i == id && u == user),
                "{err}"
            );
        }
    }

    #[test]
    fn refuses_two_members_with_the_same_name_after_renames() {
        let options = ImportOptions {
            renames: vec![Rename {
                from: "ondrej_k".to_owned(),
                to: "PEPA".to_owned(),
            }],
            ..options()
        };

        let err = plan(&crew(), &options).unwrap_err();

        assert!(
            matches!(&err, ImportError::DuplicateName(name) if name.as_str() == "Pepa"),
            "{err}"
        );
    }

    #[test]
    fn refuses_a_rename_or_admin_that_matches_nobody() {
        let rename = ImportOptions {
            renames: vec![Rename {
                from: "Ondra".to_owned(),
                to: "Ondřej".to_owned(),
            }],
            ..options()
        };
        assert!(matches!(
            plan(&crew(), &rename).unwrap_err(),
            ImportError::UnknownRename(name) if name == "Ondra"
        ));

        let admin = ImportOptions {
            admin: "Thor".to_owned(),
            ..options()
        };
        assert!(matches!(
            plan(&crew(), &admin).unwrap_err(),
            ImportError::UnknownAdmin(name) if name == "Thor"
        ));
    }

    #[test]
    fn keeps_a_quotes_text_author_and_time() {
        let mut heimdall = crew();
        heimdall.quotes.push(Quote {
            id: 1,
            text: "Pivo je základ.".to_owned(),
            author: "Pepa".to_owned(),
            created_at: Some("2026-01-20 09:00:00".to_owned()),
        });

        let quote = &plan(&heimdall, &options()).unwrap().quotes[0];

        assert_eq!(quote.quote.text.as_str(), "Pivo je základ.");
        assert_eq!(quote.quote.author.as_str(), "Pepa");
        assert_eq!(Some(quote.added_at), parse_instant("2026-01-20 09:00:00"));
    }
}
