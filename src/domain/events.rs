//! The app-wide event channel: domain services emit what happened, and a notifier task turns it
//! into Web Push, so a slow push never delays a request.
//!
//! Each service holds only an [`Emitter`](crate::domain::channel::Emitter) of its own events, so
//! this module depends on the services and none of them on it.

use crate::domain::channel::Channel;
use crate::domain::finance::FinanceEvent;

/// Something a Member did that others may want to hear about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Finance(FinanceEvent),
}

/// The app-wide channel of [`Event`]s, built once at startup.
pub type Events = Channel<Event>;
