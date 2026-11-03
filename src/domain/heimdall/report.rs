//! The verification report: what Heimdall had next to what Asgard now holds, and whether the two
//! agree closely enough to commit.

use std::fmt;

use super::plan::{EntryKind, Rounding};
use crate::domain::finance::money::Crowns;
use crate::domain::members::DisplayName;

/// How far apart, in hundredths of a crown, a Member's Balance may be in Heimdall and Asgard.
/// Heimdall's float Shares drift; Asgard's whole-crown ones don't.
pub const BALANCE_TOLERANCE_HUNDREDTHS: i64 = 100;

/// How many entries of one kind each system has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountRow {
    pub kind: EntryKind,
    pub heimdall: i64,
    pub asgard: i64,
}

/// What the entries of one kind add up to in each system, in whole crowns. Heimdall's side is
/// after each amount is converted to whole crowns, as the import stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotalRow {
    pub kind: EntryKind,
    pub heimdall: i64,
    pub asgard: i64,
}

/// One Member's Balance as Heimdall computes it, in hundredths of a crown, and as Asgard's
/// ledger does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRow {
    pub member: DisplayName,
    pub heimdall_hundredths: i64,
    pub asgard: Crowns,
}

impl BalanceRow {
    /// Asgard's Balance less Heimdall's, in hundredths of a crown.
    #[must_use]
    pub const fn difference_hundredths(&self) -> i64 {
        self.asgard.get() * 100 - self.heimdall_hundredths
    }

    #[must_use]
    pub const fn within_tolerance(&self) -> bool {
        self.difference_hundredths().abs() <= BALANCE_TOLERANCE_HUNDREDTHS
    }
}

/// Everything the import checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub counts: Vec<CountRow>,
    pub totals: Vec<TotalRow>,
    pub balances: Vec<BalanceRow>,
    pub rounded: Vec<Rounding>,
    pub dry_run: bool,
}

impl Report {
    /// Every check that failed, as a line of the verdict.
    #[must_use]
    pub fn failures(&self) -> Vec<String> {
        let counts = self
            .counts
            .iter()
            .filter(|row| row.heimdall != row.asgard)
            .map(|row| {
                format!(
                    "{}: {} in Heimdall but {} in Asgard",
                    row.kind.plural(),
                    row.heimdall,
                    row.asgard
                )
            });
        let totals = self
            .totals
            .iter()
            .filter(|row| row.heimdall != row.asgard)
            .map(|row| {
                format!(
                    "{} add up to {} Kč in Heimdall but {} Kč in Asgard",
                    row.kind.plural(),
                    row.heimdall,
                    row.asgard
                )
            });
        let balances = self
            .balances
            .iter()
            .filter(|row| !row.within_tolerance())
            .map(|row| {
                format!(
                    "{}'s Balance differs by {} Kč, more than 1 Kč",
                    row.member,
                    hundredths(row.difference_hundredths())
                )
            });
        counts.chain(totals).chain(balances).collect()
    }

    /// Whether counts and totals match exactly and every Balance is within 1 Kč.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures().is_empty()
    }
}

/// Plain-text tables for a terminal, then the verdict.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .balances
            .iter()
            .map(|row| row.member.as_str().chars().count())
            .chain([14])
            .max()
            .unwrap_or(14);

        writeln!(
            f,
            "{:<width$}  {:>12}  {:>12}",
            "Counts", "Heimdall", "Asgard"
        )?;
        for row in &self.counts {
            writeln!(
                f,
                "{:<width$}  {:>12}  {:>12}",
                row.kind.plural(),
                row.heimdall,
                row.asgard
            )?;
        }

        writeln!(f)?;
        writeln!(
            f,
            "{:<width$}  {:>12}  {:>12}",
            "Totals (Kč)", "Heimdall", "Asgard"
        )?;
        for row in &self.totals {
            writeln!(
                f,
                "{:<width$}  {:>12}  {:>12}",
                row.kind.plural(),
                row.heimdall,
                row.asgard
            )?;
        }

        writeln!(f)?;
        writeln!(
            f,
            "{:<width$}  {:>12}  {:>12}  {:>12}",
            "Balances (Kč)", "Heimdall", "Asgard", "Difference"
        )?;
        for row in &self.balances {
            writeln!(
                f,
                "{:<width$}  {:>12}  {:>12}  {:>12}",
                row.member.as_str(),
                hundredths(row.heimdall_hundredths),
                row.asgard.get(),
                hundredths(row.difference_hundredths())
            )?;
        }

        writeln!(f)?;
        writeln!(f, "Rounded to whole crowns")?;
        if self.rounded.is_empty() {
            writeln!(f, "  none")?;
        }
        for rounding in &self.rounded {
            writeln!(
                f,
                "  {} {}: {} -> {}",
                rounding.kind, rounding.id, rounding.original, rounding.rounded
            )?;
        }

        writeln!(f)?;
        let failures = self.failures();
        match (failures.is_empty(), self.dry_run) {
            (true, false) => writeln!(f, "Verdict: PASS. The import is committed.")?,
            (true, true) => writeln!(f, "Verdict: PASS. Dry run, so nothing was committed.")?,
            (false, _) => {
                writeln!(f, "Verdict: FAIL. Nothing was committed:")?;
                for failure in failures {
                    writeln!(f, "  - {failure}")?;
                }
            }
        }
        Ok(())
    }
}

/// Hundredths of a crown as crowns with two decimals: `-1234.05`.
fn hundredths(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let value = value.unsigned_abs();
    format!("{sign}{}.{:02}", value / 100, value % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str) -> DisplayName {
        DisplayName::parse(name).unwrap()
    }

    fn passing() -> Report {
        Report {
            counts: EntryKind::ALL
                .into_iter()
                .map(|kind| CountRow {
                    kind,
                    heimdall: 2,
                    asgard: 2,
                })
                .collect(),
            totals: vec![
                TotalRow {
                    kind: EntryKind::Expense,
                    heimdall: 180_000,
                    asgard: 180_000,
                },
                TotalRow {
                    kind: EntryKind::Payment,
                    heimdall: 5_000,
                    asgard: 5_000,
                },
            ],
            balances: vec![
                BalanceRow {
                    member: name("Josef"),
                    heimdall_hundredths: 6667,
                    asgard: Crowns::new(66),
                },
                BalanceRow {
                    member: name("Ondra"),
                    heimdall_hundredths: -3333,
                    asgard: Crowns::new(-33),
                },
                BalanceRow {
                    member: name("Pepa"),
                    heimdall_hundredths: -3334,
                    asgard: Crowns::new(-33),
                },
            ],
            rounded: Vec::new(),
            dry_run: false,
        }
    }

    #[test]
    fn formats_hundredths_with_two_decimals_and_a_sign() {
        assert_eq!(hundredths(0), "0.00");
        assert_eq!(hundredths(5), "0.05");
        assert_eq!(hundredths(-123_405), "-1234.05");
    }

    #[test]
    fn passes_when_counts_and_totals_match_and_balances_are_within_a_crown() {
        assert!(passing().passed());
    }

    #[test]
    fn a_balance_exactly_a_crown_apart_still_passes() {
        let mut report = passing();
        report.balances[0].heimdall_hundredths = 6700;
        report.balances[0].asgard = Crowns::new(68);

        assert!(report.passed());
    }

    #[test]
    fn fails_on_any_count_total_or_balance_that_disagrees() {
        let mut report = passing();
        report.counts[2].asgard = 1;
        report.totals[0].asgard = 179_999;
        report.balances[1].asgard = Crowns::new(-35);

        assert_eq!(
            report.failures(),
            [
                "Reminders: 2 in Heimdall but 1 in Asgard",
                "Expenses add up to 180000 Kč in Heimdall but 179999 Kč in Asgard",
                "Ondra's Balance differs by -1.67 Kč, more than 1 Kč",
            ]
        );
    }

    #[test]
    fn prints_the_tables_rounded_amounts_and_verdict() {
        let mut report = passing();
        report.rounded.push(Rounding {
            kind: EntryKind::Payment,
            id: 4,
            original: 99.5,
            rounded: 100,
        });
        report.balances[2].asgard = Crowns::new(-40);

        insta::assert_snapshot!(report.to_string());
    }

    #[test]
    fn a_dry_run_says_nothing_was_committed() {
        let report = Report {
            dry_run: true,
            ..passing()
        };

        assert!(
            report
                .to_string()
                .ends_with("Verdict: PASS. Dry run, so nothing was committed.\n")
        );
    }
}
