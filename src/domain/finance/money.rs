//! Whole crowns: the only currency the Crew uses, and never a float.

/// The smallest amount an Expense can be for.
pub const AMOUNT_MIN: i64 = 1;

/// The largest amount an Expense can be for.
pub const AMOUNT_MAX: i64 = 1_000_000;

/// A signed number of whole crowns, such as a Balance or a Share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Crowns(i64);

impl Crowns {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(crowns: i64) -> Self {
        Self(crowns)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Whether a Balance this size is Even: within a crown of zero, so nobody chases it.
    #[must_use]
    pub const fn is_even(self) -> bool {
        self.0.abs() <= 1
    }

    /// Where a Member with a Balance this size stands with the Crew.
    #[must_use]
    pub const fn standing(self) -> Standing {
        if self.is_even() {
            Standing::Even
        } else if self.0 > 0 {
            Standing::Owed
        } else {
            Standing::Owes
        }
    }
}

/// Where a Member stands with the Crew, by their Balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The Crew owes them.
    Owed,
    /// They owe the Crew.
    Owes,
    Even,
}

/// What an Expense is for: from [`AMOUNT_MIN`] to [`AMOUNT_MAX`] whole crowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(i64);

impl Amount {
    /// Reads whole crowns as a Member typed them. Spaces between the digits, as in "1 240", are
    /// ignored.
    ///
    /// # Errors
    ///
    /// Returns [`AmountError::NotWholeCrowns`] for anything but digits, or
    /// [`AmountError::OutOfRange`].
    pub fn parse(input: &str) -> Result<Self, AmountError> {
        let digits: String = input
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err(AmountError::NotWholeCrowns);
        }
        // Too many digits to fit is out of range too.
        let crowns = digits.parse().unwrap_or(i64::MAX);
        Self::new(crowns)
    }

    /// # Errors
    ///
    /// Returns [`AmountError::OutOfRange`] outside [`AMOUNT_MIN`] to [`AMOUNT_MAX`].
    pub const fn new(crowns: i64) -> Result<Self, AmountError> {
        if crowns >= AMOUNT_MIN && crowns <= AMOUNT_MAX {
            Ok(Self(crowns))
        } else {
            Err(AmountError::OutOfRange)
        }
    }

    /// An amount read back from the database, which checked it when it was stored.
    pub(in crate::domain) const fn from_stored(crowns: i64) -> Self {
        Self(crowns)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn crowns(self) -> Crowns {
        Crowns(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AmountError {
    #[error("the amount isn't a whole number of crowns")]
    NotWholeCrowns,
    #[error("the amount is outside {AMOUNT_MIN} to {AMOUNT_MAX} crowns")]
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_whole_crowns() {
        for (input, crowns) in [
            ("1", 1),
            (" 250 ", 250),
            ("1 240", 1240),
            ("1\u{a0}000\u{a0}000", 1_000_000),
        ] {
            assert_eq!(
                Amount::parse(input).map(Amount::get),
                Ok(crowns),
                "{input:?}"
            );
        }
    }

    #[test]
    fn refuses_anything_but_whole_crowns() {
        for input in ["", "  ", "12.50", "12,50", "-5", "+5", "abc", "1e3"] {
            assert_eq!(
                Amount::parse(input),
                Err(AmountError::NotWholeCrowns),
                "{input:?}"
            );
        }
    }

    #[test]
    fn refuses_amounts_out_of_range() {
        for input in ["0", "1000001", "99999999999999999999999"] {
            assert_eq!(
                Amount::parse(input),
                Err(AmountError::OutOfRange),
                "{input:?}"
            );
        }
    }

    #[test]
    fn a_balance_within_a_crown_of_zero_is_even() {
        for crowns in [-1, 0, 1] {
            assert!(Crowns::new(crowns).is_even(), "{crowns}");
        }
        for crowns in [-2, 2, 1240] {
            assert!(!Crowns::new(crowns).is_even(), "{crowns}");
        }
    }

    #[test]
    fn a_balance_says_who_owes_whom() {
        for (crowns, standing) in [
            (-2, Standing::Owes),
            (-1, Standing::Even),
            (1, Standing::Even),
            (2, Standing::Owed),
        ] {
            assert_eq!(Crowns::new(crowns).standing(), standing, "{crowns}");
        }
    }
}
