//! The app-wide event channel: domain services emit what happened, and a notifier task turns it
//! into Web Push, so a slow push never delays a request.

use tokio::sync::broadcast;

use crate::domain::finance::FinanceEvent;

/// How many events a subscriber may fall behind before it misses the oldest.
const CAPACITY: usize = 64;

/// Something a Member did that others may want to hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Finance(FinanceEvent),
}

/// Emits events to whoever subscribed, built once at startup and shared by the services.
///
/// Emitting never waits: with no subscriber an event is dropped, and a subscriber that falls
/// more than [`CAPACITY`] events behind misses the oldest.
#[derive(Debug, Clone)]
pub struct Events {
    sender: broadcast::Sender<Event>,
}

impl Events {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self { sender }
    }

    /// Every event emitted from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    pub(crate) fn emit(&self, event: Event) {
        // Sending fails only when no one subscribed, and then no one needs the event.
        let _ = self.sender.send(event);
    }
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::TryRecvError;

    use super::*;
    use crate::domain::finance::{EntryAction, EntryId, ExpenseId};
    use crate::domain::members::MemberId;

    fn event(id: i64) -> Event {
        Event::Finance(FinanceEvent {
            entry: EntryId::Expense(ExpenseId::from_client(id)),
            action: EntryAction::Added,
            actor: MemberId::from_client(1),
            notify: vec![MemberId::from_client(2)],
        })
    }

    #[test]
    fn emitting_with_no_subscriber_drops_the_event() {
        let events = Events::new();

        events.emit(event(1));

        let mut late = events.subscribe();
        assert_eq!(late.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_subscriber_that_never_reads_does_not_hold_up_emitting() {
        let events = Events::new();
        let mut idle = events.subscribe();

        for id in 0..=i64::try_from(CAPACITY).unwrap() {
            events.emit(event(id));
        }

        assert_eq!(idle.try_recv(), Err(TryRecvError::Lagged(1)));
        assert_eq!(idle.try_recv(), Ok(event(1)));
    }
}
