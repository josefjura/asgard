//! The Reminders tab: the form a Member creates a Reminder for the whole Crew with, and the
//! Reminders still pending.

use jiff::civil::DateTime;
use maud::{Markup, html};

use crate::domain::reminders::{PendingReminder, TEXT_MAX};
use crate::views::finance::problem;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};

/// Where the Reminders tab's form posts. The routes in `web::reminders` spell out the same path.
pub const CREATE: &str = "/reminders";

/// What the Reminder form shows in its fields: what was typed, or nothing yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReminderValues<'a> {
    pub text: &'a str,
    /// As a `datetime-local` field holds it: `2026-11-07T10:00`.
    pub due_at: &'a str,
}

/// Why each field of the Reminder form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct ReminderProblems {
    pub text: Option<String>,
    pub due_at: Option<String>,
}

/// A date and time the way a `datetime-local` field holds it, to the minute.
fn field_time(time: DateTime) -> String {
    time.strftime("%Y-%m-%dT%H:%M").to_string()
}

/// A due time the way Pending shows it, in the Crew's time zone.
fn due(time: DateTime) -> String {
    time.strftime("%A %-d %B %Y, %H:%M").to_string()
}

/// The Reminders tab: `form` to create a Reminder, and every one in `pending`, soonest first. On
/// a wide screen they sit side by side.
#[must_use]
pub fn page(form: &Markup, pending: &[PendingReminder]) -> Markup {
    shell(
        Place::Tab(Tab::Reminders),
        &html! {
            h1 { "Reminders" }
            div.columns {
                section.section aria-labelledby="new-reminder" {
                    h2 #new-reminder { "New Reminder" }
                    p.muted { "The whole Crew is notified when it's due." }
                    (form)
                }
                section.section aria-labelledby="pending" {
                    h2 #pending { "Pending" }
                    @if pending.is_empty() {
                        p.muted { "Nothing pending. Create a Reminder and it waits here until it's due." }
                    } @else {
                        ul.reminder-list {
                            @for reminder in pending {
                                (pending_row(reminder))
                            }
                        }
                    }
                }
            }
        },
    )
}

/// A pending Reminder: what it says, when it's due in Prague time, and who created it.
fn pending_row(reminder: &PendingReminder) -> Markup {
    let time = reminder.due_at.crew_time();
    html! {
        li.reminder {
            span.reminder-text #{ "reminder-" (reminder.id.get()) } { (reminder.text.as_str()) }
            span.reminder-meta {
                time datetime=(field_time(time)) { (due(time)) }
                " · " (reminder.created_by)
            }
        }
    }
}

/// The form a Reminder is created with.
///
/// It shows `values` and, beside each field, what was wrong with it. The due time is entered in
/// Prague time and can't be before `now`, Prague time too. htmx swaps the form for the one the server answers with; without htmx, it posts as a plain
/// form.
#[must_use]
pub fn form(values: ReminderValues, problems: &ReminderProblems, now: DateTime) -> Markup {
    html! {
        form.entry-form method="post" action=(CREATE) hx-post=(CREATE) hx-swap="outerHTML" {
            div.field {
                label for="reminder-text" { "What to remember" }
                input #reminder-text type="text" name="text" value=(values.text) required
                    maxlength=(TEXT_MAX) autocomplete="off"
                    aria-invalid=[problems.text.is_some().then_some("true")]
                    aria-describedby=[problems.text.is_some().then_some("reminder-text-problem")];
                (problem("reminder-text-problem", problems.text.as_deref()))
            }
            div.field {
                label for="reminder-due" { "Due, Prague time" }
                input #reminder-due type="datetime-local" name="due_at" value=(values.due_at)
                    min=(field_time(now)) required
                    aria-invalid=[problems.due_at.is_some().then_some("true")]
                    aria-describedby=[problems.due_at.is_some().then_some("reminder-due-problem")];
                (problem("reminder-due-problem", problems.due_at.as_deref()))
            }
            button.button.large type="submit" { "Create Reminder" }
        }
    }
}

/// Pending Reminders and the moment the fixture forms are filled in, for the gallery and the
/// snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::{DateTime, date};

    use crate::domain::reminders::PendingReminder;
    use crate::domain::reminders::fixtures::pending as reminder;

    /// 10:00 in Prague on Saturday 24 October 2026, the day before the clocks go back.
    pub const NOW: DateTime = date(2026, 10, 24).at(10, 0, 0, 0);

    /// Three pending Reminders, soonest first, the last after the clocks have gone back.
    #[must_use]
    pub fn pending() -> Vec<PendingReminder> {
        vec![
            reminder(1, "Bottle the stout", "2026-10-24T18:00", "Josef"),
            reminder(
                2,
                "Order 25 kg of Pilsner malt and two packs of Saaz before the shop closes for the holidays",
                "2026-10-25T09:30",
                "Šimon",
            ),
            reminder(3, "Clean the fermenter", "2026-11-07T10:00", "Ondra"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{NOW, pending};
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(ReminderValues::default(), &ReminderProblems::default(), NOW)
    }

    #[test]
    fn reminders_empty() {
        assert_html_snapshot!(page(&empty_form(), &[]));
    }

    #[test]
    fn reminders_pending() {
        assert_html_snapshot!(page(&empty_form(), &pending()));
    }

    #[test]
    fn reminder_form_with_errors() {
        let values = ReminderValues {
            text: "",
            due_at: "2026-10-23T18:00",
        };
        let problems = ReminderProblems {
            text: Some("Say what to remember.".to_owned()),
            due_at: Some("Pick a time that hasn't passed yet.".to_owned()),
        };
        assert_html_snapshot!(form(values, &problems, NOW));
    }

    #[test]
    fn shows_the_due_time_in_prague_with_who_created_it() {
        let html = page(&empty_form(), &pending()).into_string();

        assert!(html.contains(
            r#"<time datetime="2026-10-24T18:00">Saturday 24 October 2026, 18:00</time> · Josef"#
        ));
        assert!(html.contains(
            r#"<time datetime="2026-11-07T10:00">Saturday 7 November 2026, 10:00</time> · Ondra"#
        ));
    }

    #[test]
    fn a_due_time_cant_be_picked_before_now() {
        let html = empty_form().into_string();

        assert!(html.contains(r#"min="2026-10-24T10:00""#));
    }
}
