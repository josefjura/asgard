//! The ledger: every Member's Balance from the Crew's Expenses. Pure, with no I/O, so the
//! Finance service reads its inputs from the database and recomputes it on every read.

use jiff::civil::Date;

use super::money::{Amount, Crowns};
use crate::domain::members::MemberId;

/// A Member as the ledger sees them: who they are and from when Expenses are shared with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Participant {
    pub id: MemberId,
    pub joined_on: Date,
}

/// An Expense as the ledger sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerExpense {
    pub payer: MemberId,
    pub amount: Amount,
    pub expense_date: Date,
}

/// One Member's equal part of an Expense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    pub member: MemberId,
    pub crowns: Crowns,
}

/// One Member's net position: positive when the Crew owes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balance {
    pub member: MemberId,
    pub crowns: Crowns,
}

/// The Balances of a fixed set of Members, built up one entry at a time.
#[derive(Debug, Clone)]
pub struct Ledger {
    members: Vec<Participant>,
    /// Each Member's Balance in crowns, in the order of `members`.
    balances: Vec<i64>,
}

impl Ledger {
    /// A ledger where every one of `members` is Even.
    #[must_use]
    pub fn new(members: Vec<Participant>) -> Self {
        let balances = vec![0; members.len()];
        Self { members, balances }
    }

    /// The Shares of `expense`, one for each Member in the Crew on its Expense date: the amount
    /// divided by their number, rounded down, with the Payer absorbing the remainder.
    ///
    /// An Expense dated before anyone joined has no Shares.
    #[must_use]
    pub fn shares(&self, expense: &LedgerExpense) -> Vec<Share> {
        let split: Vec<MemberId> = self
            .members
            .iter()
            .filter(|member| member.joined_on <= expense.expense_date)
            .map(|member| member.id)
            .collect();
        let Ok(count) = i64::try_from(split.len()) else {
            return Vec::new();
        };
        if count == 0 {
            return Vec::new();
        }
        let amount = expense.amount.get();
        let share = amount / count;
        let remainder = amount - share * count;
        split
            .into_iter()
            .map(|member| Share {
                member,
                crowns: Crowns::new(if member == expense.payer {
                    share + remainder
                } else {
                    share
                }),
            })
            .collect()
    }

    /// Adds `expense`: each Member in its split owes their Share to the Payer.
    ///
    /// An Expense whose Payer isn't one of the ledger's Members is left out, so the Balances
    /// still sum to zero.
    pub fn add_expense(&mut self, expense: &LedgerExpense) {
        let Some(payer) = self.position(expense.payer) else {
            return;
        };
        for share in self.shares(expense) {
            if share.member == expense.payer {
                continue;
            }
            if let Some(member) = self.position(share.member) {
                self.balances[member] -= share.crowns.get();
                self.balances[payer] += share.crowns.get();
            }
        }
    }

    /// Every Member's Balance, in the order the ledger was given them. They always sum to zero.
    #[must_use]
    pub fn balances(&self) -> Vec<Balance> {
        self.members
            .iter()
            .zip(&self.balances)
            .map(|(member, &crowns)| Balance {
                member: member.id,
                crowns: Crowns::new(crowns),
            })
            .collect()
    }

    fn position(&self, id: MemberId) -> Option<usize> {
        self.members.iter().position(|member| member.id == id)
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    const JOSEF: MemberId = MemberId::from_client(1);
    const ONDRA: MemberId = MemberId::from_client(2);
    const SIMON: MemberId = MemberId::from_client(3);
    const KAREL: MemberId = MemberId::from_client(4);

    /// Josef, Ondra and Šimon, who all joined on 2026-01-01.
    fn crew() -> Vec<Participant> {
        [JOSEF, ONDRA, SIMON]
            .map(|id| Participant {
                id,
                joined_on: date(2026, 1, 1),
            })
            .into()
    }

    /// The Crew and Karel, who joined on 2026-06-01.
    fn crew_and_karel() -> Vec<Participant> {
        let mut members = crew();
        members.push(Participant {
            id: KAREL,
            joined_on: date(2026, 6, 1),
        });
        members
    }

    fn expense(payer: MemberId, crowns: i64, expense_date: Date) -> LedgerExpense {
        LedgerExpense {
            payer,
            amount: Amount::new(crowns).unwrap(),
            expense_date,
        }
    }

    fn balances_after(
        members: Vec<Participant>,
        expenses: &[LedgerExpense],
    ) -> Vec<(MemberId, i64)> {
        let mut ledger = Ledger::new(members);
        for expense in expenses {
            ledger.add_expense(expense);
        }
        ledger
            .balances()
            .into_iter()
            .map(|balance| (balance.member, balance.crowns.get()))
            .collect()
    }

    fn shares_of(members: Vec<Participant>, expense: &LedgerExpense) -> Vec<(MemberId, i64)> {
        Ledger::new(members)
            .shares(expense)
            .into_iter()
            .map(|share| (share.member, share.crowns.get()))
            .collect()
    }

    #[test]
    fn splits_an_expense_equally_with_the_payer_absorbing_the_remainder() {
        let cases = [
            (JOSEF, 100, [(JOSEF, 34), (ONDRA, 33), (SIMON, 33)]),
            (ONDRA, 100, [(JOSEF, 33), (ONDRA, 34), (SIMON, 33)]),
            (SIMON, 101, [(JOSEF, 33), (ONDRA, 33), (SIMON, 35)]),
            (JOSEF, 99, [(JOSEF, 33), (ONDRA, 33), (SIMON, 33)]),
            (JOSEF, 1, [(JOSEF, 1), (ONDRA, 0), (SIMON, 0)]),
        ];
        for (payer, crowns, shares) in cases {
            let expense = expense(payer, crowns, date(2026, 3, 1));

            assert_eq!(shares_of(crew(), &expense), shares, "{crowns} Kč");
        }
    }

    #[test]
    fn the_payer_is_owed_everyone_elses_shares() {
        let balances = balances_after(crew(), &[expense(JOSEF, 100, date(2026, 3, 1))]);

        assert_eq!(balances, [(JOSEF, 66), (ONDRA, -33), (SIMON, -33)]);
    }

    #[test]
    fn a_single_members_expense_changes_nothing() {
        let alone = vec![Participant {
            id: JOSEF,
            joined_on: date(2026, 1, 1),
        }];
        let expense = expense(JOSEF, 1240, date(2026, 3, 1));

        assert_eq!(shares_of(alone.clone(), &expense), [(JOSEF, 1240)]);
        assert_eq!(balances_after(alone, &[expense]), [(JOSEF, 0)]);
    }

    #[test]
    fn a_member_joining_later_leaves_earlier_shares_unchanged() {
        let before = expense(ONDRA, 300, date(2026, 5, 31));

        assert_eq!(
            shares_of(crew_and_karel(), &before),
            shares_of(crew(), &before)
        );
        assert_eq!(
            balances_after(crew_and_karel(), &[before]),
            [(JOSEF, -100), (ONDRA, 200), (SIMON, -100), (KAREL, 0)]
        );
    }

    #[test]
    fn an_expense_dated_on_a_members_join_date_includes_them() {
        let on_the_day = expense(ONDRA, 400, date(2026, 6, 1));

        assert_eq!(
            shares_of(crew_and_karel(), &on_the_day),
            [(JOSEF, 100), (ONDRA, 100), (SIMON, 100), (KAREL, 100)]
        );
    }

    #[test]
    fn an_expense_dated_before_anyone_joined_has_no_shares() {
        let early = expense(JOSEF, 100, date(2025, 12, 31));

        assert_eq!(shares_of(crew(), &early), []);
        assert_eq!(
            balances_after(crew(), &[early]),
            [(JOSEF, 0), (ONDRA, 0), (SIMON, 0)]
        );
    }

    #[test]
    fn an_expense_by_someone_outside_the_ledger_is_left_out() {
        let balances = balances_after(crew(), &[expense(KAREL, 300, date(2026, 6, 1))]);

        assert_eq!(balances, [(JOSEF, 0), (ONDRA, 0), (SIMON, 0)]);
    }

    #[test]
    fn balances_sum_to_zero() {
        let expenses = [
            expense(JOSEF, 100, date(2026, 3, 1)),
            expense(ONDRA, 1_000_000, date(2026, 4, 2)),
            expense(SIMON, 7, date(2026, 5, 31)),
            expense(KAREL, 1001, date(2026, 6, 1)),
            expense(KAREL, 999, date(2026, 7, 15)),
            expense(JOSEF, 1, date(2026, 8, 8)),
            expense(SIMON, 2, date(2026, 9, 9)),
        ];
        for count in 0..=expenses.len() {
            let total: i64 = balances_after(crew_and_karel(), &expenses[..count])
                .iter()
                .map(|(_, crowns)| crowns)
                .sum();

            assert_eq!(total, 0, "after {count} Expenses");
        }
    }
}
