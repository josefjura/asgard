//! The one source of the current time, so expiry logic can be tested at chosen instants.

#[cfg(test)]
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
#[cfg(test)]
use std::time::Duration;
use std::time::SystemTime;

use jiff::civil::{Date, DateTime};
use jiff::{Timestamp, Zoned, tz};

/// Where the Crew lives: times are entered and displayed here, and stored as UTC.
pub const CREW_TIME_ZONE: &str = "Europe/Prague";

/// Tells the current time: the system clock in production, a [`ManualClock`] in tests.
#[derive(Debug, Clone)]
pub struct Clock(Source);

#[derive(Debug, Clone)]
enum Source {
    System,
    #[cfg(test)]
    Manual(Arc<Mutex<SystemTime>>),
}

impl Clock {
    #[must_use]
    pub const fn system() -> Self {
        Self(Source::System)
    }

    #[must_use]
    pub fn now(&self) -> SystemTime {
        match &self.0 {
            Source::System => SystemTime::now(),
            #[cfg(test)]
            Source::Manual(now) => *lock(now),
        }
    }

    /// Today's date in the Crew's time zone.
    ///
    /// # Panics
    ///
    /// Never in practice: the time zone database is compiled in, and the clock stays within the
    /// years 9999 BC to 9999 AD.
    #[must_use]
    pub fn today(&self) -> Date {
        crew_date(Timestamp::try_from(self.now()).expect("the clock is within jiff's range"))
    }
}

/// The date in the Crew's time zone of an instant stored as Unix seconds. An instant outside
/// jiff's range, which the database never holds, is clamped to it.
#[must_use]
pub fn crew_date_of_unix_seconds(seconds: i64) -> Date {
    crew_date(timestamp_of_unix_seconds(seconds))
}

/// The date and time on the clock in the Crew's time zone at an instant stored as Unix seconds,
/// clamped like [`crew_date_of_unix_seconds`].
#[must_use]
pub fn crew_time_of_unix_seconds(seconds: i64) -> DateTime {
    crew_zoned(timestamp_of_unix_seconds(seconds)).datetime()
}

fn timestamp_of_unix_seconds(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap_or(if seconds < 0 {
        Timestamp::MIN
    } else {
        Timestamp::MAX
    })
}

/// The instant `date` begins in the Crew's time zone, as Unix seconds.
///
/// # Panics
///
/// Never in practice: midnight of every date jiff can hold is within its range of instants.
#[must_use]
pub fn unix_seconds_at_crew_midnight(date: Date) -> i64 {
    date.to_zoned(crew_zone())
        .expect("a date's midnight is within jiff's range")
        .timestamp()
        .as_second()
}

fn crew_date(at: Timestamp) -> Date {
    crew_zoned(at).date()
}

fn crew_zoned(at: Timestamp) -> Zoned {
    at.to_zoned(crew_zone())
}

fn crew_zone() -> tz::TimeZone {
    tz::db()
        .get(CREW_TIME_ZONE)
        .expect("the bundled time zone database has the Crew's zone")
}

/// `time` as whole seconds since the Unix epoch, the way the database stores instants.
#[must_use]
pub fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |secs| -secs),
    }
}

/// A clock that stands still until a test sets or advances it.
///
/// Every [`Clock`] it hands out sees the same time, so a test can move the time of a running
/// router.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ManualClock(Arc<Mutex<SystemTime>>);

#[cfg(test)]
impl ManualClock {
    #[must_use]
    pub fn starting_at(now: SystemTime) -> Self {
        Self(Arc::new(Mutex::new(now)))
    }

    #[must_use]
    pub fn clock(&self) -> Clock {
        Clock(Source::Manual(Arc::clone(&self.0)))
    }

    pub fn set(&self, now: SystemTime) {
        *lock(&self.0) = now;
    }

    pub fn advance(&self, by: Duration) {
        *lock(&self.0) += by;
    }
}

/// A test that panicked mid-update leaves a plain timestamp behind, which is still usable.
#[cfg(test)]
fn lock(now: &Mutex<SystemTime>) -> MutexGuard<'_, SystemTime> {
    now.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: SystemTime = SystemTime::UNIX_EPOCH;

    #[test]
    fn the_system_clock_tells_the_current_time() {
        let before = SystemTime::now();
        let now = Clock::system().now();

        assert!(now >= before);
        assert!(now <= SystemTime::now());
    }

    #[test]
    fn a_manual_clock_stands_still() {
        let manual = ManualClock::starting_at(START);

        assert_eq!(manual.clock().now(), START);
        assert_eq!(manual.clock().now(), START);
    }

    #[test]
    fn advancing_a_manual_clock_moves_every_clock_it_handed_out() {
        let manual = ManualClock::starting_at(START);
        let clock = manual.clock();

        manual.advance(Duration::from_secs(90));

        assert_eq!(clock.now(), START + Duration::from_secs(90));
    }

    #[test]
    fn today_is_the_date_in_prague() {
        // 2026-01-01T22:59:59Z is still New Year's Day in Prague (UTC+1); a second later it's not.
        let manual = ManualClock::starting_at(START + Duration::from_secs(1_767_308_399));
        assert_eq!(manual.clock().today(), jiff::civil::date(2026, 1, 1));

        manual.advance(Duration::from_secs(1));
        assert_eq!(manual.clock().today(), jiff::civil::date(2026, 1, 2));
    }

    #[test]
    fn a_stored_instant_falls_on_its_date_in_prague() {
        assert_eq!(
            crew_date_of_unix_seconds(1_767_308_399),
            jiff::civil::date(2026, 1, 1)
        );
        assert_eq!(
            crew_date_of_unix_seconds(1_767_308_400),
            jiff::civil::date(2026, 1, 2)
        );
    }

    #[test]
    fn a_stored_instant_reads_as_prague_wall_clock_time() {
        // 2026-01-01T22:59:59Z is 23:59:59 in Prague (UTC+1), and 2026-07-01T12:00:00Z is 14:00
        // in summer (UTC+2).
        assert_eq!(
            crew_time_of_unix_seconds(1_767_308_399),
            jiff::civil::date(2026, 1, 1).at(23, 59, 59, 0)
        );
        assert_eq!(
            crew_time_of_unix_seconds(1_782_907_200),
            jiff::civil::date(2026, 7, 1).at(14, 0, 0, 0)
        );
    }

    #[test]
    fn a_date_begins_at_midnight_in_prague() {
        // 2026-01-01T23:00:00Z in winter (UTC+1), and 2026-04-30T22:00:00Z in summer (UTC+2).
        assert_eq!(
            unix_seconds_at_crew_midnight(jiff::civil::date(2026, 1, 2)),
            1_767_308_400
        );
        assert_eq!(
            unix_seconds_at_crew_midnight(jiff::civil::date(2026, 5, 1)),
            1_777_586_400
        );
    }

    #[test]
    fn counts_unix_seconds_either_side_of_the_epoch() {
        assert_eq!(unix_seconds(START + Duration::from_secs(90)), 90);
        assert_eq!(unix_seconds(START - Duration::from_secs(90)), -90);
    }

    #[test]
    fn setting_a_manual_clock_jumps_to_that_time() {
        let manual = ManualClock::starting_at(START);
        let later = START + Duration::from_hours(24);

        manual.set(later);

        assert_eq!(manual.clock().now(), later);
    }
}
