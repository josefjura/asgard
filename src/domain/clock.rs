//! The one source of the current time, so expiry logic can be tested at chosen instants.

#[cfg(test)]
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
#[cfg(test)]
use std::time::Duration;
use std::time::SystemTime;

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
    fn setting_a_manual_clock_jumps_to_that_time() {
        let manual = ManualClock::starting_at(START);
        let later = START + Duration::from_hours(24);

        manual.set(later);

        assert_eq!(manual.clock().now(), later);
    }
}
