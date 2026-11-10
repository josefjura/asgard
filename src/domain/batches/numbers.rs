//! The numbers that matter on a Batch: its Brew day (brew date, OG, Volume) and Bottling
//! (bottling date, FG), the rules across them, and the ABV and apparent attenuation worked out
//! from OG and FG.
//!
//! Every number is an integer: gravities in thousandths, Volume in decilitres, percentages in
//! tenths. Nothing here is a float, so 5.25 % rounds up to 5.3 % as the Crew expects.

use std::fmt;

use jiff::civil::Date;

/// The lowest OG accepted, in thousandths: 1.000.
pub const OG_MIN: i64 = 1000;
/// The highest OG accepted, in thousandths: 1.200.
pub const OG_MAX: i64 = 1200;
/// The lowest FG accepted, in thousandths: 0.990.
pub const FG_MIN: i64 = 990;
/// The highest FG accepted, in thousandths: 1.100.
pub const FG_MAX: i64 = 1100;
/// The largest Volume accepted, in decilitres: 1000 L, far more than the Crew's fermenters hold.
pub const VOLUME_MAX: i64 = 10_000;

/// A specific gravity, such as an OG or FG, in integer thousandths: 1.052 is 1052.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gravity(i64);

impl Gravity {
    /// An OG as a Member typed it: "1.052", "1,052" or "1052", from 1.000 to 1.200.
    ///
    /// # Errors
    ///
    /// Returns [`GravityError::Malformed`] or [`GravityError::OutOfRange`].
    pub fn og(input: &str) -> Result<Self, GravityError> {
        Self::parse(input, OG_MIN, OG_MAX)
    }

    /// An FG as a Member typed it: "1.010", "1,010" or "1010", from 0.990 to 1.100.
    ///
    /// # Errors
    ///
    /// Returns [`GravityError::Malformed`] or [`GravityError::OutOfRange`].
    pub fn fg(input: &str) -> Result<Self, GravityError> {
        Self::parse(input, FG_MIN, FG_MAX)
    }

    /// A gravity read back from the database, which checked its range when it was stored.
    pub(super) const fn from_stored(thousandths: i64) -> Self {
        Self(thousandths)
    }

    /// The gravity in thousandths: 1052 for 1.052.
    #[must_use]
    pub const fn thousandths(self) -> i64 {
        self.0
    }

    fn parse(input: &str, min: i64, max: i64) -> Result<Self, GravityError> {
        let input = input.trim().replace(',', ".");
        let thousandths = match input.split_once('.') {
            // "1.052", or "1.05" meaning 1.050.
            Some((whole, fraction)) if (1..=3).contains(&fraction.len()) => {
                // Padded to three decimals, so "05" is 050.
                let padded = format!("{fraction:0<3}");
                let whole = digits(whole).ok_or(GravityError::Malformed)?;
                let fraction = digits(&padded).ok_or(GravityError::Malformed)?;
                whole
                    .checked_mul(1000)
                    .and_then(|whole| whole.checked_add(fraction))
                    .ok_or(GravityError::OutOfRange)?
            }
            Some(_) => return Err(GravityError::Malformed),
            // "1052", the gravity in thousandths as hydrometers mark it.
            None => digits(&input).ok_or(GravityError::Malformed)?,
        };
        if (min..=max).contains(&thousandths) {
            Ok(Self(thousandths))
        } else {
            Err(GravityError::OutOfRange)
        }
    }
}

/// `1.052`: as a hydrometer reads.
impl fmt::Display for Gravity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:03}", self.0 / 1000, self.0 % 1000)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GravityError {
    #[error("not a gravity")]
    Malformed,
    #[error("the gravity is outside the range it can be")]
    OutOfRange,
}

/// How much went into the fermenter, in integer decilitres: 23.5 L is 235. Always above zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Volume(i64);

impl Volume {
    /// A Volume in litres as a Member typed it, with at most one decimal after a dot or a comma:
    /// "23.5", "23,5" or "23".
    ///
    /// # Errors
    ///
    /// Returns [`VolumeError::Malformed`], [`VolumeError::NotPositive`] or
    /// [`VolumeError::TooBig`].
    pub fn parse(input: &str) -> Result<Self, VolumeError> {
        let input = input.trim().replace(',', ".");
        let (negative, litres) = input
            .strip_prefix('-')
            .map_or((false, input.as_str()), |rest| (true, rest.trim_start()));
        let decilitres = match litres.split_once('.') {
            Some((whole, fraction)) if fraction.len() == 1 => {
                let whole = digits(whole).ok_or(VolumeError::Malformed)?;
                let fraction = digits(fraction).ok_or(VolumeError::Malformed)?;
                whole
                    .checked_mul(10)
                    .and_then(|whole| whole.checked_add(fraction))
                    .ok_or(VolumeError::TooBig)?
            }
            Some(_) => return Err(VolumeError::Malformed),
            None => digits(litres)
                .ok_or(VolumeError::Malformed)?
                .checked_mul(10)
                .ok_or(VolumeError::TooBig)?,
        };
        if negative || decilitres == 0 {
            Err(VolumeError::NotPositive)
        } else if decilitres > VOLUME_MAX {
            Err(VolumeError::TooBig)
        } else {
            Ok(Self(decilitres))
        }
    }

    /// A Volume read back from the database, which checked its range when it was stored.
    pub(super) const fn from_stored(decilitres: i64) -> Self {
        Self(decilitres)
    }

    /// The Volume in decilitres: 235 for 23.5 L.
    #[must_use]
    pub const fn decilitres(self) -> i64 {
        self.0
    }

    /// The litres as a field holds them: `23.5`, or `23` when whole.
    #[must_use]
    pub fn litres(self) -> String {
        if self.0 % 10 == 0 {
            (self.0 / 10).to_string()
        } else {
            format!("{}.{}", self.0 / 10, self.0 % 10)
        }
    }
}

/// `23.5 L`, or `23 L` when whole.
impl fmt::Display for Volume {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} L", self.litres())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VolumeError {
    #[error("not a Volume")]
    Malformed,
    #[error("the Volume is zero or below")]
    NotPositive,
    #[error("the Volume is more than {} L", VOLUME_MAX / 10)]
    TooBig,
}

/// The number `input` spells in ASCII digits alone, if it does and fits.
fn digits(input: &str) -> Option<i64> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    input.parse().ok()
}

/// A percentage to one decimal, in integer tenths: 5.3 % is 53.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Percent(i64);

impl Percent {
    /// The percentage in tenths: 53 for 5.3 %.
    #[must_use]
    pub const fn tenths(self) -> i64 {
        self.0
    }
}

/// `5.3 %`.
impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{} %", self.0 / 10, self.0 % 10)
    }
}

/// `numerator / denominator` rounded half up, for a positive `denominator` and a numerator of
/// zero or more.
const fn round_half_up(numerator: i64, denominator: i64) -> i64 {
    (2 * numerator + denominator) / (2 * denominator)
}

/// The ABV of a beer from `og` down to `fg`: `(OG − FG) × 131.25`, to one decimal, rounded half
/// up. None unless the FG is below the OG.
#[must_use]
pub const fn abv(og: Gravity, fg: Gravity) -> Option<Percent> {
    let drop = og.0 - fg.0;
    if drop <= 0 {
        return None;
    }
    // In thousandths, (OG − FG) × 131.25 % is drop × 0.13125 %, which is drop × 13125 / 10000
    // tenths of a percent.
    Some(Percent(round_half_up(drop * 13_125, 10_000)))
}

/// The apparent attenuation from `og` down to `fg`: `(OG − FG) / (OG − 1)`.
///
/// It is to one decimal, rounded half up, and can be over 100 % when the FG is below 1.000. None
/// unless the FG is below the OG and the OG is above 1.000.
#[must_use]
pub const fn attenuation(og: Gravity, fg: Gravity) -> Option<Percent> {
    let drop = og.0 - fg.0;
    let sugar = og.0 - 1000;
    if drop <= 0 || sugar <= 0 {
        return None;
    }
    Some(Percent(round_half_up(drop * 1000, sugar)))
}

/// A Batch's Brew day: the brew date, the OG and the Volume into the fermenter. Any of them may
/// be missing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrewDay {
    pub date: Option<Date>,
    pub og: Option<Gravity>,
    pub volume: Option<Volume>,
}

/// A Batch's Bottling: the bottling date and the FG. Either may be missing; an FG read before
/// bottling is kept without a date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bottling {
    pub date: Option<Date>,
    pub fg: Option<Gravity>,
}

/// Which rule across a Batch's Brew day and Bottling a change would break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the FG isn't below the OG")]
    FgNotBelowOg,
    #[error("the bottling date is before the brew date")]
    BottlingBeforeBrewing,
    #[error("there's a bottling date but no brew date")]
    BottlingWithoutBrewing,
}

/// Checks `brew_day` and `bottling` agree: the FG is below the OG when both are in, and a
/// bottling date needs a brew date no later than it.
///
/// # Errors
///
/// Returns the first rule broken.
pub fn check(brew_day: &BrewDay, bottling: &Bottling) -> Result<(), RuleError> {
    if let (Some(og), Some(fg)) = (brew_day.og, bottling.fg)
        && fg >= og
    {
        return Err(RuleError::FgNotBelowOg);
    }
    match (brew_day.date, bottling.date) {
        (None, Some(_)) => Err(RuleError::BottlingWithoutBrewing),
        (Some(brewed), Some(bottled)) if bottled < brewed => Err(RuleError::BottlingBeforeBrewing),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    fn og(input: &str) -> Gravity {
        Gravity::og(input).unwrap()
    }

    fn fg(input: &str) -> Gravity {
        Gravity::fg(input).unwrap()
    }

    #[test]
    fn a_gravity_is_typed_with_a_dot_a_comma_or_in_thousandths() {
        for typed in ["1.052", "1,052", "1052", " 1.052 "] {
            assert_eq!(og(typed).thousandths(), 1052, "{typed}");
        }
    }

    #[test]
    fn a_gravity_with_fewer_decimals_means_the_rest_are_zero() {
        assert_eq!(og("1.05").thousandths(), 1050);
        assert_eq!(og("1.1").thousandths(), 1100);
    }

    #[test]
    fn a_gravity_reads_as_a_hydrometer_does() {
        assert_eq!(og("1052").to_string(), "1.052");
        assert_eq!(og("1.1").to_string(), "1.100");
        assert_eq!(fg("0.998").to_string(), "0.998");
    }

    #[test]
    fn the_og_is_from_1_000_to_1_200() {
        assert_eq!(og("1.000").thousandths(), 1000);
        assert_eq!(og("1.200").thousandths(), 1200);
        assert_eq!(Gravity::og("0.999"), Err(GravityError::OutOfRange));
        assert_eq!(Gravity::og("1.201"), Err(GravityError::OutOfRange));
        assert_eq!(Gravity::og("52"), Err(GravityError::OutOfRange));
    }

    #[test]
    fn the_fg_is_from_0_990_to_1_100() {
        assert_eq!(fg("0.990").thousandths(), 990);
        assert_eq!(fg("990").thousandths(), 990);
        assert_eq!(fg("1.100").thousandths(), 1100);
        assert_eq!(Gravity::fg("0.989"), Err(GravityError::OutOfRange));
        assert_eq!(Gravity::fg("1.101"), Err(GravityError::OutOfRange));
    }

    #[test]
    fn a_gravity_that_isnt_a_number_is_malformed() {
        for typed in [
            "",
            " ",
            "abc",
            "1.0525",
            "1.",
            ".052",
            "1.05.2",
            "-1.010",
            "1.0e3",
            "1 052",
            "99999999999999999999",
        ] {
            assert_eq!(
                Gravity::og(typed),
                Err(GravityError::Malformed),
                "{typed:?}"
            );
        }
    }

    #[test]
    fn a_volume_is_typed_in_litres_with_a_dot_or_a_comma() {
        for typed in ["23.5", "23,5", " 23.5 "] {
            assert_eq!(Volume::parse(typed).unwrap().decilitres(), 235, "{typed}");
        }
        assert_eq!(Volume::parse("23").unwrap().decilitres(), 230);
        assert_eq!(Volume::parse("0.1").unwrap().decilitres(), 1);
    }

    #[test]
    fn a_volume_shows_its_decimal_only_when_it_has_one() {
        assert_eq!(Volume::parse("23,5").unwrap().to_string(), "23.5 L");
        assert_eq!(Volume::parse("23.0").unwrap().to_string(), "23 L");
        assert_eq!(Volume::parse("23.0").unwrap().litres(), "23");
    }

    #[test]
    fn a_volume_is_above_zero() {
        for typed in ["0", "0.0", "0,0", "-1", "-23.5", "- 5"] {
            assert_eq!(
                Volume::parse(typed),
                Err(VolumeError::NotPositive),
                "{typed}"
            );
        }
    }

    #[test]
    fn a_volume_is_at_most_1000_litres() {
        assert_eq!(Volume::parse("1000").unwrap().decilitres(), VOLUME_MAX);
        assert_eq!(Volume::parse("1000.1"), Err(VolumeError::TooBig));
        assert_eq!(Volume::parse("99999999999999999"), Err(VolumeError::TooBig));
    }

    #[test]
    fn a_volume_that_isnt_a_number_is_malformed() {
        for typed in [
            "",
            "abc",
            "23.55",
            "23.",
            ".5",
            "23 L",
            "2.3.5",
            "99999999999999999999",
        ] {
            assert_eq!(
                Volume::parse(typed),
                Err(VolumeError::Malformed),
                "{typed:?}"
            );
        }
    }

    #[test]
    fn abv_and_attenuation_of_a_typical_beer() {
        // (1.050 − 1.010) × 131.25 is exactly 5.25, which rounds up.
        assert_eq!(abv(og("1.050"), fg("1.010")).unwrap().to_string(), "5.3 %");
        assert_eq!(
            attenuation(og("1.050"), fg("1.010")).unwrap().to_string(),
            "80.0 %"
        );
    }

    #[test]
    fn abv_and_attenuation_against_known_values() {
        let cases = [
            // OG, FG, ABV, attenuation
            ("1.044", "1.010", 45, 773),
            ("1.060", "1.012", 63, 800),
            ("1.080", "1.015", 85, 813),
            ("1.012", "0.998", 18, 1167),
            ("1.001", "1.000", 1, 1000),
        ];
        for (og_typed, fg_typed, abv_tenths, attenuation_tenths) in cases {
            let (og, fg) = (og(og_typed), fg(fg_typed));
            assert_eq!(
                abv(og, fg).unwrap().tenths(),
                abv_tenths,
                "{og_typed} {fg_typed}"
            );
            assert_eq!(
                attenuation(og, fg).unwrap().tenths(),
                attenuation_tenths,
                "{og_typed} {fg_typed}"
            );
        }
    }

    #[test]
    fn nothing_is_worked_out_unless_the_fg_is_below_the_og() {
        assert_eq!(abv(og("1.010"), fg("1.010")), None);
        assert_eq!(attenuation(og("1.010"), fg("1.020")), None);
        // OG 1.000 has no sugar to attenuate.
        assert_eq!(attenuation(og("1.000"), fg("0.995")), None);
        assert_eq!(abv(og("1.000"), fg("0.995")).unwrap().to_string(), "0.7 %");
    }

    fn brew_day(on: Option<Date>, og_typed: Option<&str>) -> BrewDay {
        BrewDay {
            date: on,
            og: og_typed.map(og),
            volume: None,
        }
    }

    fn bottling(on: Option<Date>, fg_typed: Option<&str>) -> Bottling {
        Bottling {
            date: on,
            fg: fg_typed.map(fg),
        }
    }

    #[test]
    fn the_fg_must_be_below_the_og() {
        let brewed = brew_day(None, Some("1.050"));

        assert_eq!(check(&brewed, &bottling(None, Some("1.049"))), Ok(()));
        assert_eq!(
            check(&brewed, &bottling(None, Some("1.050"))),
            Err(RuleError::FgNotBelowOg)
        );
        assert_eq!(
            check(&brewed, &bottling(None, Some("1.060"))),
            Err(RuleError::FgNotBelowOg)
        );
        // Either alone is fine.
        assert_eq!(check(&brewed, &Bottling::default()), Ok(()));
        assert_eq!(
            check(&BrewDay::default(), &bottling(None, Some("1.060"))),
            Ok(())
        );
    }

    #[test]
    fn bottling_needs_a_brew_date_no_later_than_it() {
        let brewed = brew_day(Some(date(2026, 10, 3)), None);

        assert_eq!(
            check(&brewed, &bottling(Some(date(2026, 10, 3)), None)),
            Ok(())
        );
        assert_eq!(
            check(&brewed, &bottling(Some(date(2026, 10, 17)), None)),
            Ok(())
        );
        assert_eq!(
            check(&brewed, &bottling(Some(date(2026, 10, 2)), None)),
            Err(RuleError::BottlingBeforeBrewing)
        );
        assert_eq!(
            check(
                &BrewDay::default(),
                &bottling(Some(date(2026, 10, 17)), None)
            ),
            Err(RuleError::BottlingWithoutBrewing)
        );
    }

    #[test]
    fn an_fg_may_be_read_before_bottling() {
        let brewed = brew_day(Some(date(2026, 10, 3)), Some("1.050"));

        assert_eq!(check(&brewed, &bottling(None, Some("1.012"))), Ok(()));
    }
}
