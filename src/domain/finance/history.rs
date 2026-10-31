//! History: every Expense and Payment, filtered by Member and month, a page at a time, with the
//! totals of each month.

use std::collections::BTreeMap;
use std::fmt;

use jiff::ToSpan;
use jiff::civil::Date;

use super::Entry;
use super::money::Crowns;
use crate::domain::members::MemberId;

/// How many entries a page of History lists.
pub const HISTORY_PAGE_SIZE: usize = 25;

/// A calendar month in the Crew's time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month(Date);

impl Month {
    /// The month `date` falls in.
    #[must_use]
    pub fn of(date: Date) -> Self {
        Self(date.first_of_month())
    }

    /// Reads a month written as `YYYY-MM`, the way a month field sends it.
    ///
    /// # Errors
    ///
    /// Returns [`MonthError`] for anything else, or a month that doesn't exist.
    pub fn parse(input: &str) -> Result<Self, MonthError> {
        let (year, month) = input.trim().split_once('-').ok_or(MonthError)?;
        let digits =
            |text: &str, len| text.len() == len && text.bytes().all(|b| b.is_ascii_digit());
        if !digits(year, 4) || !digits(month, 2) {
            return Err(MonthError);
        }
        let year = year.parse().map_err(|_| MonthError)?;
        let month = month.parse().map_err(|_| MonthError)?;
        Date::new(year, month, 1).map(Self).map_err(|_| MonthError)
    }

    /// The month's first day.
    #[must_use]
    pub const fn first_day(self) -> Date {
        self.0
    }

    /// The month after this one, unless this is the last month there is.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1.month()).ok().map(Self)
    }
}

/// `YYYY-MM`, as [`Month::parse`] reads it.
impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.0.year(), self.0.month())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a month written as YYYY-MM")]
pub struct MonthError;

/// Which entries History lists. An unset filter lets every entry through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryFilter {
    /// Entries this Member paid, sent or received.
    pub member: Option<MemberId>,
    /// Entries dated in this month: an Expense by its Expense date, a Payment by the day it was
    /// logged on.
    pub month: Option<Month>,
}

impl HistoryFilter {
    /// Whether the filter lets every entry through.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.member.is_none() && self.month.is_none()
    }
}

/// What a month's Expenses and Payments added up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthlyTotal {
    pub month: Month,
    pub expenses: Crowns,
    pub payments: Crowns,
}

/// One page of History, and the monthly totals of the whole Crew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    /// The page's entries, newest first.
    pub entries: Vec<Entry>,
    /// Which page this is, from 1.
    pub page: usize,
    /// How many pages there are; at least 1, even with nothing to list.
    pub pages: usize,
    /// Each month with an entry, newest first, whoever paid, sent or received it.
    pub totals: Vec<MonthlyTotal>,
}

impl History {
    /// The `page`th page of `entries`, each with the Unix second it was logged at, newest first:
    /// by date, then by when it was logged. A page past the last is the last.
    #[must_use]
    pub fn of(mut entries: Vec<(i64, Entry)>, page: usize, totals: Vec<MonthlyTotal>) -> Self {
        // A stable sort keeps the given order between entries of the same date and second.
        entries.sort_by_key(|(logged_at, entry)| std::cmp::Reverse((entry.date(), *logged_at)));
        let pages = entries.len().div_ceil(HISTORY_PAGE_SIZE).max(1);
        let page = page.clamp(1, pages);
        let entries = entries
            .into_iter()
            .map(|(_, entry)| entry)
            .skip((page - 1) * HISTORY_PAGE_SIZE)
            .take(HISTORY_PAGE_SIZE)
            .collect();
        Self {
            entries,
            page,
            pages,
            totals,
        }
    }
}

/// The totals of each month `entries` fall in, newest first.
pub fn monthly_totals<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<MonthlyTotal> {
    let mut totals = BTreeMap::new();
    for entry in entries {
        let month = Month::of(entry.date());
        let total = totals.entry(month).or_insert(MonthlyTotal {
            month,
            expenses: Crowns::ZERO,
            payments: Crowns::ZERO,
        });
        let sum = match entry {
            Entry::Expense(_) => &mut total.expenses,
            Entry::Payment(_) => &mut total.payments,
        };
        *sum = Crowns::new(sum.get() + entry.amount().get());
    }
    totals.into_values().rev().collect()
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::domain::finance::money::Amount;
    use crate::domain::finance::{Description, ExpenseEntry, ExpenseId, PaymentEntry, PaymentId};
    use crate::domain::members::MemberName;
    use crate::domain::members::fixtures::name;

    fn josef() -> MemberName {
        MemberName {
            id: MemberId::from_client(1),
            display_name: name("Josef"),
        }
    }

    fn expense(id: i64, crowns: i64, expense_date: Date) -> Entry {
        Entry::Expense(ExpenseEntry {
            id: ExpenseId::from_client(id),
            payer: josef(),
            amount: Amount::new(crowns).unwrap(),
            description: Description::parse("Malt").unwrap(),
            expense_date,
            edited: None,
        })
    }

    fn payment(id: i64, crowns: i64, logged_on: Date) -> Entry {
        Entry::Payment(PaymentEntry {
            id: PaymentId::from_client(id),
            sender: josef(),
            recipient: name("Ondra"),
            amount: Amount::new(crowns).unwrap(),
            description: None,
            logged_on,
            edited: None,
        })
    }

    fn amounts(history: &History) -> Vec<i64> {
        history
            .entries
            .iter()
            .map(|entry| entry.amount().get())
            .collect()
    }

    /// Each of `totals` as its month, Expenses and Payments, to compare at a glance.
    fn plain(totals: &[MonthlyTotal]) -> Vec<(String, i64, i64)> {
        totals
            .iter()
            .map(|total| {
                (
                    total.month.to_string(),
                    total.expenses.get(),
                    total.payments.get(),
                )
            })
            .collect()
    }

    #[test]
    fn reads_a_month_as_a_month_field_sends_it() {
        let month = Month::parse("2026-10").unwrap();

        assert_eq!(month.first_day(), date(2026, 10, 1));
        assert_eq!(month.to_string(), "2026-10");
        assert_eq!(
            Month::parse(" 2026-01 ").unwrap().first_day(),
            date(2026, 1, 1)
        );
    }

    #[test]
    fn refuses_anything_but_a_month() {
        for input in [
            "",
            "2026",
            "2026-1",
            "2026-13",
            "2026-00",
            "26-10",
            "2026-10-01",
            "October",
            "+2026-10",
        ] {
            assert_eq!(Month::parse(input), Err(MonthError), "{input:?}");
        }
    }

    #[test]
    fn a_month_is_the_one_its_days_fall_in() {
        assert_eq!(
            Month::of(date(2026, 2, 28)),
            Month::parse("2026-02").unwrap()
        );
        assert_eq!(
            Month::of(date(2026, 12, 31)).next(),
            Some(Month::parse("2027-01").unwrap())
        );
        assert_eq!(Month::parse("9999-12").unwrap().next(), None);
    }

    #[test]
    fn lists_entries_newest_first_by_date_then_by_when_they_were_logged() {
        let entries = vec![
            (100, expense(1, 1, date(2026, 3, 20))),
            (200, payment(1, 2, date(2026, 3, 21))),
            // Logged last, but backdated.
            (300, expense(2, 3, date(2026, 3, 1))),
            (400, expense(3, 4, date(2026, 3, 21))),
        ];

        let history = History::of(entries, 1, Vec::new());

        assert_eq!(amounts(&history), [4, 2, 1, 3]);
        assert_eq!((history.page, history.pages), (1, 1));
    }

    #[test]
    fn pages_through_every_entry() {
        let entries: Vec<_> = (1..=60)
            .map(|crowns| (crowns, expense(crowns, crowns, date(2026, 3, 1))))
            .collect();

        let pages: Vec<History> = (1..=3)
            .map(|page| History::of(entries.clone(), page, Vec::new()))
            .collect();

        assert_eq!(amounts(&pages[0]), (36..=60).rev().collect::<Vec<_>>());
        assert_eq!(amounts(&pages[1]), (11..=35).rev().collect::<Vec<_>>());
        assert_eq!(amounts(&pages[2]), (1..=10).rev().collect::<Vec<_>>());
        assert!(pages.iter().all(|page| page.pages == 3));
        assert_eq!(pages[2].page, 3);
    }

    #[test]
    fn a_page_out_of_range_is_the_nearest_one() {
        let entries: Vec<_> = (1..=30)
            .map(|crowns| (crowns, expense(crowns, crowns, date(2026, 3, 1))))
            .collect();

        assert_eq!(History::of(entries.clone(), 0, Vec::new()).page, 1);
        let past = History::of(entries, 9, Vec::new());
        assert_eq!((past.page, past.pages), (2, 2));
        assert_eq!(amounts(&past), [5, 4, 3, 2, 1]);
    }

    #[test]
    fn nothing_to_list_is_one_empty_page() {
        let history = History::of(Vec::new(), 1, Vec::new());

        assert_eq!(
            history,
            History {
                entries: Vec::new(),
                page: 1,
                pages: 1,
                totals: Vec::new(),
            }
        );
    }

    #[test]
    fn totals_each_months_expenses_and_payments() {
        let mut entries: Vec<_> = (1..=30)
            .map(|id| (id, expense(id, 100, date(2026, 3, 15))))
            .collect();
        entries.extend([
            (40, payment(1, 250, date(2026, 3, 31))),
            (50, expense(31, 1_000, date(2026, 4, 1))),
            (60, payment(2, 7, date(2026, 4, 30))),
            (70, payment(3, 3, date(2026, 6, 2))),
        ]);

        let totals = monthly_totals(entries.iter().map(|(_, entry)| entry));

        assert_eq!(
            plain(&totals),
            [
                ("2026-06".to_owned(), 0, 3),
                ("2026-04".to_owned(), 1_000, 7),
                ("2026-03".to_owned(), 3_000, 250),
            ]
        );
    }

    #[test]
    fn an_empty_filter_lets_everything_through() {
        assert!(HistoryFilter::default().is_empty());
        assert!(
            !HistoryFilter {
                member: Some(MemberId::from_client(1)),
                month: None
            }
            .is_empty()
        );
    }
}
