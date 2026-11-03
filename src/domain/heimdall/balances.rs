//! Heimdall's own Balance maths, reproduced only so the import can show each Member's Balance as
//! Heimdall computes it next to Asgard's. Asgard's Balances always come from its ledger.

use std::collections::BTreeMap;

use super::source::Heimdall;

/// Heimdall rounds every Share to hundredths of a crown.
const SHARE_ROUNDING_FACTOR: f64 = 100.0;

/// Every Heimdall user's Balance, as Heimdall computes it, by user id.
///
/// Heimdall sums each payer's Expenses first and divides the sum, in `f64`, between all its
/// users, rounding the Share to hundredths. The payer gains the sum minus their Share, and every
/// other user loses one Share. A Payment raises the Sender's Balance and lowers the
/// Recipient's.
#[must_use]
pub fn heimdall_balances(heimdall: &Heimdall) -> BTreeMap<i64, f64> {
    let mut balances: BTreeMap<i64, f64> =
        heimdall.users.iter().map(|user| (user.id, 0.0)).collect();
    // Heimdall reads the users from the database as f64 too.
    let user_count = f64::from(u32::try_from(heimdall.users.len()).unwrap_or(u32::MAX));

    let mut paid_by: BTreeMap<i64, f64> = BTreeMap::new();
    for expense in &heimdall.expenses {
        *paid_by.entry(expense.owner_id).or_insert(0.0) += expense.amount;
    }
    for (&payer, &amount) in &paid_by {
        let share = share(amount, user_count);
        if let Some(balance) = balances.get_mut(&payer) {
            *balance += amount - share;
        }
        for (&user, balance) in &mut balances {
            if user != payer {
                *balance -= share;
            }
        }
    }

    let mut sent_between: BTreeMap<(i64, i64), f64> = BTreeMap::new();
    for payment in &heimdall.payments {
        *sent_between
            .entry((payment.owner_id, payment.recipient_id))
            .or_insert(0.0) += payment.amount;
    }
    for (&(sender, recipient), &amount) in &sent_between {
        if let Some(balance) = balances.get_mut(&sender) {
            *balance += amount;
        }
        if let Some(balance) = balances.get_mut(&recipient) {
            *balance -= amount;
        }
    }
    balances
}

fn share(amount: f64, user_count: f64) -> f64 {
    if user_count.abs() < f64::EPSILON {
        return 0.0;
    }
    (amount / user_count * SHARE_ROUNDING_FACTOR).round() / SHARE_ROUNDING_FACTOR
}

#[cfg(test)]
mod tests {
    use super::super::source::{Expense, Payment, User};
    use super::*;

    fn crew() -> Heimdall {
        Heimdall {
            users: [(1, "Josef"), (2, "Ondra"), (3, "Pepa")]
                .into_iter()
                .map(|(id, name)| User {
                    id,
                    discord_id: format!("discord-{id}"),
                    display_name: name.to_owned(),
                })
                .collect(),
            ..Heimdall::default()
        }
    }

    fn expense(id: i64, owner_id: i64, amount: f64) -> Expense {
        Expense {
            id,
            owner_id,
            amount,
            description: "Slad".to_owned(),
            created_at: None,
        }
    }

    fn payment(id: i64, owner_id: i64, recipient_id: i64, amount: f64) -> Payment {
        Payment {
            id,
            owner_id,
            recipient_id,
            amount,
            description: "Vyrovnání".to_owned(),
            created_at: None,
        }
    }

    /// Each user's Balance in hundredths, so the hand-worked figures compare exactly.
    fn hundredths(heimdall: &Heimdall) -> Vec<i64> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test Balances are small and already rounded to hundredths"
        )]
        heimdall_balances(heimdall)
            .into_values()
            .map(|balance| (balance * 100.0).round() as i64)
            .collect()
    }

    #[test]
    fn everyone_is_even_with_nothing_logged() {
        assert_eq!(hundredths(&crew()), [0, 0, 0]);
    }

    #[test]
    fn the_payer_gains_the_amount_less_their_share_rounded_to_hundredths() {
        let mut heimdall = crew();
        heimdall.expenses.push(expense(1, 1, 100.0));

        // 100 / 3 = 33.333… rounds to 33.33.
        assert_eq!(hundredths(&heimdall), [6667, -3333, -3333]);
    }

    #[test]
    fn a_payers_expenses_are_summed_before_they_are_divided() {
        let mut heimdall = crew();
        heimdall.expenses.push(expense(1, 1, 1.0));
        heimdall.expenses.push(expense(2, 1, 1.0));

        // 2 / 3 = 0.666… rounds to 0.67; two Shares of 1 / 3 would round to 0.33 + 0.33.
        assert_eq!(hundredths(&heimdall), [133, -67, -67]);
    }

    #[test]
    fn each_payer_is_divided_on_their_own() {
        let mut heimdall = crew();
        heimdall.expenses.push(expense(1, 1, 100.0));
        heimdall.expenses.push(expense(2, 2, 50.0));
        heimdall.expenses.push(expense(3, 1, 200.0));

        // Josef's 300 is 100 a Share; Ondra's 50 is 16.67.
        assert_eq!(hundredths(&heimdall), [18333, -6667, -11667]);
    }

    #[test]
    fn a_payment_raises_the_sender_and_lowers_the_recipient() {
        let mut heimdall = crew();
        heimdall.expenses.push(expense(1, 1, 300.0));
        heimdall.payments.push(payment(1, 2, 1, 60.0));
        heimdall.payments.push(payment(2, 2, 1, 40.0));
        heimdall.payments.push(payment(3, 3, 1, 25.5));

        assert_eq!(hundredths(&heimdall), [7450, 0, -7450]);
    }

    #[test]
    fn a_fraction_of_a_crown_is_kept() {
        let mut heimdall = crew();
        heimdall.expenses.push(expense(1, 2, 10.5));

        // 10.5 / 3 = 3.5.
        assert_eq!(hundredths(&heimdall), [-350, 700, -350]);
    }
}
