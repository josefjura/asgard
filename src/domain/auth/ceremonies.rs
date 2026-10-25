//! WebAuthn ceremonies between their start and finish requests.
//!
//! Asgard runs as a single replica (ADR 0004), so the state lives in memory. A restart only
//! makes someone press the passkey button again.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use super::tokens::SessionToken;

/// How long a person has between starting a ceremony and finishing it.
pub const CEREMONY_LIFETIME: Duration = Duration::from_mins(5);

/// Starting a ceremony needs no session, so the store is bounded. A Crew of three never comes
/// close.
const MAX_PENDING: usize = 64;

/// Names a pending ceremony; the browser sends it back with the finish request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CeremonyId(String);

impl CeremonyId {
    #[must_use]
    pub const fn from_client(value: String) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

struct Pending<T> {
    started: SystemTime,
    ceremony: T,
}

pub struct Ceremonies<T> {
    pending: Mutex<HashMap<CeremonyId, Pending<T>>>,
}

impl<T> Ceremonies<T> {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Keeps `ceremony` until it is taken or expires, dropping the oldest one when full.
    pub fn insert(&self, now: SystemTime, ceremony: T) -> CeremonyId {
        // Random and unguessable, like a session id, so one visitor can't finish another's.
        let id = CeremonyId(SessionToken::generate().as_str().to_owned());
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.retain(|_, entry| is_live(entry.started, now));
        if pending.len() >= MAX_PENDING
            && let Some(oldest) = pending
                .iter()
                .min_by_key(|(_, entry)| entry.started)
                .map(|(id, _)| id.clone())
        {
            pending.remove(&oldest);
        }
        pending.insert(
            id.clone(),
            Pending {
                started: now,
                ceremony,
            },
        );
        id
    }

    /// Removes and returns the ceremony, unless it is unknown or has expired.
    pub fn take(&self, id: &CeremonyId, now: SystemTime) -> Option<T> {
        let entry = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id)?;
        is_live(entry.started, now).then_some(entry.ceremony)
    }
}

fn is_live(started: SystemTime, now: SystemTime) -> bool {
    now < started + CEREMONY_LIFETIME
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: SystemTime = SystemTime::UNIX_EPOCH;

    #[test]
    fn a_ceremony_can_be_taken_once() {
        let ceremonies = Ceremonies::new();
        let id = ceremonies.insert(START, "joining");

        assert_eq!(ceremonies.take(&id, START), Some("joining"));
        assert_eq!(ceremonies.take(&id, START), None);
    }

    #[test]
    fn a_ceremony_expires() {
        let ceremonies = Ceremonies::new();
        let id = ceremonies.insert(START, "joining");

        assert_eq!(ceremonies.take(&id, START + CEREMONY_LIFETIME), None);
    }

    #[test]
    fn a_ceremony_just_inside_its_lifetime_is_live() {
        let ceremonies = Ceremonies::new();
        let id = ceremonies.insert(START, "joining");
        let just_before = START
            + CEREMONY_LIFETIME
                .checked_sub(Duration::from_secs(1))
                .unwrap();

        assert_eq!(ceremonies.take(&id, just_before), Some("joining"));
    }

    #[test]
    fn an_unknown_id_finds_nothing() {
        let ceremonies = Ceremonies::<&str>::new();

        assert_eq!(
            ceremonies.take(&CeremonyId::from_client("made-up".to_owned()), START),
            None
        );
    }

    #[test]
    fn a_full_store_drops_the_oldest_ceremony() {
        let ceremonies = Ceremonies::new();
        let oldest = ceremonies.insert(START, 0);
        let second = ceremonies.insert(START + Duration::from_secs(1), 1);
        for n in 2..=MAX_PENDING {
            ceremonies.insert(START + Duration::from_secs(2), n);
        }

        let now = START + Duration::from_secs(3);
        assert_eq!(ceremonies.take(&oldest, now), None);
        assert_eq!(ceremonies.take(&second, now), Some(1));
    }
}
