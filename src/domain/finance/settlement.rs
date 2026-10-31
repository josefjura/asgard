//! The Settlement plan: who should send whom how much for everyone to be Even. Pure, like the
//! ledger it reads the Balances of.

use std::cmp::Ordering;

use super::MemberBalance;
use super::money::{Crowns, Standing};
use crate::domain::members::MemberName;

/// One suggested Payment: the Sender sends the Recipient `amount` to settle up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub sender: MemberName,
    pub recipient: MemberName,
    /// At least a crown.
    pub amount: Crowns,
}

/// The Settlements that bring every Member who isn't Even to Even.
///
/// Members who are Even are left out, so nobody chases a single crown. The rest are matched
/// greedily, the Member who owes the most sending to the Member owed the most, until each of
/// them is at zero. That gives at most one Settlement fewer than there are Members. Equal
/// Balances are taken in display-name order (by code point, not alphabet), so the plan is the
/// same on every read.
///
/// The crowns the Even Members hold are nobody's to pass on, so once the plan is paid the rest
/// are left owing or owed that much between them, and exactly zero when it sums to nothing.
#[must_use]
pub fn settlement_plan(balances: &[MemberBalance]) -> Vec<Settlement> {
    let mut open: Vec<(&MemberName, i64)> = balances
        .iter()
        .filter(|balance| !balance.balance.is_even())
        .map(|balance| (&balance.member, balance.balance.get()))
        .collect();
    let mut plan = Vec::new();
    loop {
        let recipient = most(&open, Standing::Owed);
        let sender = most(&open, Standing::Owes);
        let (Some(recipient), Some(sender)) = (recipient, sender) else {
            return plan;
        };
        let amount = open[recipient].1.min(-open[sender].1);
        open[recipient].1 -= amount;
        open[sender].1 += amount;
        plan.push(Settlement {
            sender: open[sender].0.clone(),
            recipient: open[recipient].0.clone(),
            amount: Crowns::new(amount),
        });
    }
}

/// Where in `open` the Member who is owed the most, or owes the most, is: the first in
/// display-name order among equals.
fn most(open: &[(&MemberName, i64)], standing: Standing) -> Option<usize> {
    let size = |crowns: i64| match standing {
        Standing::Owes => -crowns,
        Standing::Owed | Standing::Even => crowns,
    };
    open.iter()
        .enumerate()
        .filter(|(_, (_, crowns))| size(*crowns) > 0)
        .max_by(|(_, (a, a_crowns)), (_, (b, b_crowns))| {
            size(*a_crowns)
                .cmp(&size(*b_crowns))
                .then_with(|| by_name(b, a))
        })
        .map(|(index, _)| index)
}

/// Display-name order, with the id settling two names that can't be told apart.
fn by_name(a: &MemberName, b: &MemberName) -> Ordering {
    a.display_name
        .as_str()
        .cmp(b.display_name.as_str())
        .then_with(|| a.id.get().cmp(&b.id.get()))
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan;
    use jiff::civil::{Date, date};
    use proptest::prelude::*;

    use super::*;
    use crate::domain::finance::ledger::{Ledger, LedgerExpense, LedgerPayment, Participant};
    use crate::domain::finance::money::Amount;
    use crate::domain::members::{DisplayName, MemberId};

    fn member(id: i64, name: &str) -> MemberName {
        MemberName {
            id: MemberId::from_client(id),
            display_name: DisplayName::parse(name).unwrap(),
        }
    }

    /// Members called by `names`, in that order, with `crowns` as their Balances.
    fn balances(members: &[(&str, i64)]) -> Vec<MemberBalance> {
        (1..)
            .zip(members)
            .map(|(id, &(name, crowns))| MemberBalance {
                member: member(id, name),
                balance: Crowns::new(crowns),
            })
            .collect()
    }

    fn plan_of(members: &[(&str, i64)]) -> Vec<(String, String, i64)> {
        settlement_plan(&balances(members))
            .into_iter()
            .map(|settlement| {
                (
                    settlement.sender.display_name.to_string(),
                    settlement.recipient.display_name.to_string(),
                    settlement.amount.get(),
                )
            })
            .collect()
    }

    fn settlement(sender: &str, recipient: &str, crowns: i64) -> (String, String, i64) {
        (sender.to_owned(), recipient.to_owned(), crowns)
    }

    #[test]
    fn a_debtor_sends_a_creditor_what_they_owe() {
        assert_eq!(
            plan_of(&[("Josef", -1240), ("Ondra", 1240)]),
            [settlement("Josef", "Ondra", 1240)]
        );
    }

    #[test]
    fn members_within_a_crown_of_zero_are_left_out() {
        for members in [
            [("Josef", 1), ("Ondra", -1), ("Šimon", 0)],
            [("Josef", 2), ("Ondra", -1), ("Šimon", -1)],
        ] {
            assert_eq!(plan_of(&members), [], "{members:?}");
        }
        assert_eq!(
            plan_of(&[("Josef", 2), ("Ondra", -2), ("Šimon", 0)]),
            [settlement("Ondra", "Josef", 2)]
        );
        assert_eq!(
            plan_of(&[("Josef", 3), ("Ondra", -2), ("Šimon", -1)]),
            [settlement("Ondra", "Josef", 2)]
        );
    }

    #[test]
    fn settles_the_rest_down_to_zero_even_by_a_single_crown() {
        assert_eq!(
            plan_of(&[("Josef", 5), ("Ondra", -4), ("Šimon", -3), ("Karel", 2)]),
            [
                settlement("Ondra", "Josef", 4),
                settlement("Šimon", "Karel", 2),
                settlement("Šimon", "Josef", 1),
            ]
        );
    }

    #[test]
    fn matches_the_largest_debtor_with_the_largest_creditor() {
        assert_eq!(
            plan_of(&[
                ("Josef", -300),
                ("Ondra", 500),
                ("Šimon", -700),
                ("Karel", 500)
            ]),
            [
                settlement("Šimon", "Karel", 500),
                settlement("Josef", "Ondra", 300),
                settlement("Šimon", "Ondra", 200),
            ]
        );
    }

    #[test]
    fn breaks_ties_by_display_name_whatever_the_order() {
        let crew = [
            ("Ondra", 100),
            ("Josef", 100),
            ("Šimon", -100),
            ("Karel", -100),
        ];
        let mut reversed = crew;
        reversed.reverse();
        let plan = [
            settlement("Karel", "Josef", 100),
            settlement("Šimon", "Ondra", 100),
        ];

        assert_eq!(plan_of(&crew), plan);
        assert_eq!(plan_of(&reversed), plan);
    }

    #[test]
    fn nobody_settles_when_everyone_is_even() {
        assert_eq!(plan_of(&[]), []);
        assert_eq!(plan_of(&[("Josef", 0)]), []);
        assert_eq!(plan_of(&[("Josef", 0), ("Ondra", 0)]), []);
    }

    /// The first day a generated Member can join on.
    const START: Date = date(2026, 1, 1);

    /// A generated Crew: when each Member joined, and the Expenses and Payments they logged.
    #[derive(Debug, Clone)]
    struct Generated {
        joined: Vec<i64>,
        expenses: Vec<(usize, i64, i64)>,
        payments: Vec<(usize, usize, i64)>,
    }

    /// Crews of one to six Members with up to ten of each entry. Each amount is at most a
    /// twentieth of the largest so no Balance, nor any Settlement, can exceed what a Payment
    /// can be for.
    fn crews() -> impl Strategy<Value = Generated> {
        (1..=6_usize).prop_flat_map(|count| {
            let amount = 1..=50_000_i64;
            (
                prop::collection::vec(0..60_i64, count),
                prop::collection::vec((0..count, amount.clone(), 0..90_i64), 0..=10),
                prop::collection::vec((0..count, 0..count, amount), 0..=10),
            )
                .prop_map(|(joined, expenses, payments)| Generated {
                    joined,
                    expenses,
                    payments,
                })
        })
    }

    fn id(index: usize) -> MemberId {
        MemberId::from_client(i64::try_from(index).unwrap() + 1)
    }

    fn payment(sender: MemberId, recipient: MemberId, crowns: i64) -> LedgerPayment {
        LedgerPayment {
            sender,
            recipient,
            amount: Amount::new(crowns).unwrap(),
        }
    }

    impl Generated {
        fn ledger(&self) -> Ledger {
            let mut ledger = Ledger::new(
                self.joined
                    .iter()
                    .enumerate()
                    .map(|(index, &days)| Participant {
                        id: id(index),
                        joined_on: START + days.days(),
                    })
                    .collect(),
            );
            for &(payer, crowns, days) in &self.expenses {
                ledger.add_expense(&LedgerExpense {
                    payer: id(payer),
                    amount: Amount::new(crowns).unwrap(),
                    expense_date: START + days.days(),
                });
            }
            for &(sender, recipient, crowns) in &self.payments {
                ledger.add_payment(&payment(id(sender), id(recipient), crowns));
            }
            ledger
        }
    }

    /// The ledger's Balances under names that tie-break in an order unlike the ids'.
    fn named(ledger: &Ledger) -> Vec<MemberBalance> {
        ledger
            .balances()
            .into_iter()
            .map(|balance| MemberBalance {
                member: member(
                    balance.member.get(),
                    &format!("M{}", 9 - balance.member.get()),
                ),
                balance: balance.crowns,
            })
            .collect()
    }

    proptest! {
        #[test]
        fn balances_always_sum_to_zero(crew in crews()) {
            let total: i64 = crew.ledger().balances().iter().map(|b| b.crowns.get()).sum();

            prop_assert_eq!(total, 0);
        }

        #[test]
        fn paying_the_plan_leaves_only_what_even_members_hold(crew in crews()) {
            let mut ledger = crew.ledger();
            let before = named(&ledger);
            let plan = settlement_plan(&before);

            for settlement in &plan {
                prop_assert!(settlement.amount.get() >= 1, "{settlement:?}");
                for member in [&settlement.sender, &settlement.recipient] {
                    let balance = before.iter().find(|b| b.member == *member).unwrap();
                    prop_assert!(!balance.balance.is_even(), "{settlement:?}");
                }
                ledger.add_payment(&payment(
                    settlement.sender.id,
                    settlement.recipient.id,
                    settlement.amount.get(),
                ));
            }

            let after = ledger.balances();
            let held: i64 = before
                .iter()
                .filter(|b| b.balance.is_even())
                .map(|b| b.balance.get())
                .sum();
            let left: i64 = before
                .iter()
                .zip(&after)
                .filter(|(b, _)| !b.balance.is_even())
                .map(|(_, a)| a.crowns.get().abs())
                .sum();
            prop_assert!(left <= held.abs(), "{before:?} paying {plan:?} left {after:?}");
            if held == 0 {
                prop_assert!(
                    after.iter().all(|a| a.crowns.is_even()),
                    "{before:?} paying {plan:?} left {after:?}"
                );
            }
        }

        #[test]
        fn the_plan_has_fewer_settlements_than_members(crew in crews()) {
            let plan = settlement_plan(&named(&crew.ledger()));

            prop_assert!(plan.len() < crew.joined.len(), "{plan:?}");
        }

        #[test]
        fn a_payment_never_makes_the_plan_longer_than_that(
            crew in crews(),
            sender in 0..6_usize,
            recipient in 0..6_usize,
            crowns in 1..=50_000_i64,
        ) {
            let count = crew.joined.len();
            let mut ledger = crew.ledger();
            ledger.add_payment(&payment(id(sender % count), id(recipient % count), crowns));

            let plan = settlement_plan(&named(&ledger));

            prop_assert!(plan.len() < count, "{plan:?}");
        }
    }
}
