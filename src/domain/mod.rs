//! Services and types: finance, shopping, reminders, quotes, members.
//!
//! No axum or Maud imports here; everything is unit-testable on its own.

pub mod auth;
pub mod channel;
pub mod clock;
pub mod events;
pub mod finance;
pub mod members;
pub mod push;
pub mod reminders;
pub mod user_agent;
