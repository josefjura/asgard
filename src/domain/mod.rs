//! Services and types: finance, shopping, reminders, quotes, members.
//!
//! No axum or Maud imports here; everything is unit-testable on its own.

pub mod auth;
pub mod channel;
pub mod clock;
pub mod events;
pub mod finance;
pub mod heimdall;
pub mod members;
pub mod notifications;
pub mod push;
pub mod quotes;
pub mod reminders;
pub mod shopping;
pub mod user_agent;
pub mod web_push;
