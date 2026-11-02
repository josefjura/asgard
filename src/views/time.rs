//! Dates and times as pages show them, in the Crew's time zone.

use jiff::civil::DateTime;

/// A date and time the way a `datetime-local` field and a `time` element's `datetime` hold it,
/// to the minute.
#[must_use]
pub fn field_time(time: DateTime) -> String {
    time.strftime("%Y-%m-%dT%H:%M").to_string()
}

/// A date and time the way the lists show it: `Saturday 24 October 2026, 10:00`.
#[must_use]
pub fn shown_time(time: DateTime) -> String {
    time.strftime("%A %-d %B %Y, %H:%M").to_string()
}
