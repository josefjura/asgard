//! The Reminders tab: the form a Member creates a Reminder for the whole Crew with, the
//! Reminders still pending, those recently sent, and the page a pending Reminder is edited or
//! deleted on.

use jiff::civil::DateTime;
use maud::{Markup, html};

use crate::domain::reminders::{PendingReminder, ReminderId, SentReminder, TEXT_MAX};
use crate::views::delete::{Deletion, delete_section};
use crate::views::finance::problem;
use crate::views::icon::chevron;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};

/// Where a Reminder's links and forms lead. The routes in `web::reminders` spell out the same
/// paths.
pub mod paths {
    use crate::domain::reminders::ReminderId;
    use crate::views::nav::Tab;

    /// The Reminder `id`, which its edit form posts to.
    #[must_use]
    pub fn reminder(id: ReminderId) -> String {
        format!("{}/{}", Tab::Reminders.href(), id.get())
    }

    /// The Reminder `id` in its edit form.
    #[must_use]
    pub fn edit_reminder(id: ReminderId) -> String {
        format!("{}/edit", reminder(id))
    }

    /// Where the Reminder `id`'s delete button posts.
    #[must_use]
    pub fn delete_reminder(id: ReminderId) -> String {
        format!("{}/delete", reminder(id))
    }
}

/// Whether a form creates a new Reminder or saves changes to the pending Reminder it edits.
#[derive(Debug, Clone, Copy)]
pub enum Purpose {
    Create,
    Edit(ReminderId),
}

impl Purpose {
    fn action(self) -> String {
        match self {
            Self::Create => Tab::Reminders.href().to_owned(),
            Self::Edit(id) => paths::reminder(id),
        }
    }

    const fn submit(self) -> &'static str {
        match self {
            Self::Create => "Create Reminder",
            Self::Edit(_) => "Save Reminder",
        }
    }
}

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

/// A due or sent time the way the lists show it, in the Crew's time zone.
fn shown_time(time: DateTime) -> String {
    time.strftime("%A %-d %B %Y, %H:%M").to_string()
}

/// The Reminders tab: `form` to create a Reminder, every one in `pending`, soonest first, and
/// those recently `sent`, newest first. On a wide screen the form sits beside the lists.
#[must_use]
pub fn page(form: &Markup, pending: &[PendingReminder], sent: &[SentReminder]) -> Markup {
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
                div {
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
                    section.section aria-labelledby="sent" {
                        h2 #sent { "Sent" }
                        @if sent.is_empty() {
                            p.muted { "Nothing sent in the last 30 days." }
                        } @else {
                            p.muted { "The last 30 days, newest first." }
                            ul.reminder-list {
                                @for reminder in sent {
                                    (sent_row(reminder))
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// A pending Reminder: what it says, when it's due in Prague time, and who created it. Any
/// Member may change it, so it leads to its edit page.
fn pending_row(reminder: &PendingReminder) -> Markup {
    let time = reminder.due_at.crew_time();
    html! {
        li.reminder {
            a.reminder-link href=(paths::edit_reminder(reminder.id)) {
                span.reminder-main {
                    span.reminder-text #{ "reminder-" (reminder.id.get()) } {
                        (reminder.text.as_str())
                    }
                    span.reminder-meta {
                        time datetime=(field_time(time)) { (shown_time(time)) }
                        " · " (reminder.created_by)
                    }
                }
                (chevron())
            }
        }
    }
}

/// A sent Reminder: what it said, when it went out in Prague time, and who created it. It can't
/// be edited, so history isn't rewritten, but any Member may delete it after asking.
fn sent_row(reminder: &SentReminder) -> Markup {
    let time = reminder.sent_at.crew_time();
    let text_id = format!("reminder-{}", reminder.id.get());
    let delete = paths::delete_reminder(reminder.id);
    html! {
        li.reminder {
            div.reminder-sent {
                span.reminder-main {
                    span.reminder-text #(text_id) { (reminder.text.as_str()) }
                    span.reminder-meta {
                        "Sent " time datetime=(field_time(time)) { (shown_time(time)) }
                        " · " (reminder.created_by)
                    }
                }
                form method="post" action=(delete) hx-post=(delete)
                    hx-confirm="Delete this sent Reminder? It leaves the Sent list for everyone." {
                    button.button.small.danger type="submit" aria-describedby=(text_id) {
                        "Delete"
                    }
                }
            }
        }
    }
}

/// The page a pending Reminder is edited on: `form`, and at its foot the button that deletes
/// the Reminder `id` after asking.
#[must_use]
pub fn edit_page(id: ReminderId, form: &Markup) -> Markup {
    let delete = paths::delete_reminder(id);
    shell(
        Place::Tab(Tab::Reminders),
        &html! {
            h1 { "Edit Reminder" }
            (form)
            (delete_section(&Deletion {
                kind: "Reminder",
                explains: "Deleting this Reminder means nobody is notified about it.",
                warns: "Nobody will be notified about it.",
                action: &delete,
            }))
        },
    )
}

/// The form that edits `reminder`, filled in with what it says and when it's due, as its edit
/// page opens. The due time can't be before `now`.
#[must_use]
pub fn edit_form(reminder: &PendingReminder, now: DateTime) -> Markup {
    let due_at = field_time(reminder.due_at.crew_time());
    let values = ReminderValues {
        text: reminder.text.as_str(),
        due_at: &due_at,
    };
    form(
        Purpose::Edit(reminder.id),
        values,
        &ReminderProblems::default(),
        now,
    )
}

/// The form a Reminder is created or edited with, as `purpose` says.
///
/// It shows `values` and, beside each field, what was wrong with it. The due time is entered in
/// Prague time and can't be before `now`, Prague time too. htmx swaps the form for the one the
/// server answers with; without htmx, it posts as a plain form.
#[must_use]
pub fn form(
    purpose: Purpose,
    values: ReminderValues,
    problems: &ReminderProblems,
    now: DateTime,
) -> Markup {
    let action = purpose.action();
    html! {
        form.entry-form method="post" action=(action) hx-post=(action) hx-swap="outerHTML" {
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
            button.button.large type="submit" { (purpose.submit()) }
        }
    }
}

/// Pending Reminders, a refused form and the moment the forms are filled in, for the gallery and
/// the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{ReminderProblems, ReminderValues};
    use jiff::civil::{DateTime, date};

    use crate::domain::reminders::fixtures::{pending as reminder, sent as sent_reminder};
    use crate::domain::reminders::{PendingReminder, SentReminder};

    /// 10:00 in Prague on Saturday 24 October 2026, the day before the clocks go back.
    pub const NOW: DateTime = date(2026, 10, 24).at(10, 0, 0, 0);

    /// A form refused for a blank text and a due time that has passed.
    #[must_use]
    pub fn refused() -> (ReminderValues<'static>, ReminderProblems) {
        (
            ReminderValues {
                text: "",
                due_at: "2026-10-23T18:00",
            },
            ReminderProblems {
                text: Some("Say what to remember.".to_owned()),
                due_at: Some("Pick a time that hasn't passed yet.".to_owned()),
            },
        )
    }

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

    /// Two sent Reminders, newest first, the older one sent late.
    #[must_use]
    pub fn sent() -> Vec<SentReminder> {
        vec![
            sent_reminder(
                5,
                "Transfer the beer to the keg",
                "2026-10-24T09:00",
                "Šimon",
            ),
            sent_reminder(4, "Pick up the CO₂ bottle", "2026-10-17T19:42", "Josef"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{NOW, pending, refused, sent};
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(
            Purpose::Create,
            ReminderValues::default(),
            &ReminderProblems::default(),
            NOW,
        )
    }

    fn edit_form_with_errors() -> Markup {
        let (values, problems) = refused();
        form(
            Purpose::Edit(ReminderId::from_client(3)),
            values,
            &problems,
            NOW,
        )
    }

    #[test]
    fn reminder_edit_page_with_errors() {
        assert_html_snapshot!(edit_page(
            ReminderId::from_client(3),
            &edit_form_with_errors()
        ));
    }

    #[test]
    fn the_edit_form_saves_the_reminder_and_its_page_deletes_it_after_asking() {
        let html = edit_page(ReminderId::from_client(3), &edit_form_with_errors()).into_string();

        assert!(html.contains(r#"action="/reminders/3" hx-post="/reminders/3""#));
        assert!(html.contains("Save Reminder"));
        assert!(html.contains(r#"action="/reminders/3/delete" hx-post="/reminders/3/delete""#));
        assert!(html.contains("hx-confirm="));
    }

    #[test]
    fn each_pending_reminder_leads_to_its_edit_page() {
        let html = page(&empty_form(), &pending(), &[]).into_string();

        for id in 1..=3 {
            assert!(html.contains(&format!(r#"href="/reminders/{id}/edit""#)));
        }
    }

    #[test]
    fn reminders_empty() {
        assert_html_snapshot!(page(&empty_form(), &[], &[]));
    }

    #[test]
    fn reminders_pending() {
        assert_html_snapshot!(page(&empty_form(), &pending(), &[]));
    }

    #[test]
    fn reminders_pending_and_sent() {
        assert_html_snapshot!(page(&empty_form(), &pending(), &sent()));
    }

    #[test]
    fn shows_when_each_sent_reminder_went_out_in_prague_and_who_created_it() {
        let html = page(&empty_form(), &[], &sent()).into_string();

        assert!(html.contains(
            r#"Sent <time datetime="2026-10-24T09:00">Saturday 24 October 2026, 09:00</time> · Šimon"#
        ));
    }

    #[test]
    fn a_sent_reminder_cant_be_edited_but_can_be_deleted_after_asking() {
        let html = page(&empty_form(), &[], &sent()).into_string();

        assert!(!html.contains("/edit"));
        assert!(html.contains(r#"action="/reminders/5/delete" hx-post="/reminders/5/delete""#));
        assert!(html.contains(r#"aria-describedby="reminder-5""#));
        assert!(html.contains("hx-confirm="));
    }

    #[test]
    fn nothing_sent_says_so() {
        let html = page(&empty_form(), &pending(), &[]).into_string();

        assert!(html.contains("Nothing sent in the last 30 days."));
    }

    #[test]
    fn reminder_form_with_errors() {
        let (values, problems) = refused();
        assert_html_snapshot!(form(Purpose::Create, values, &problems, NOW));
    }

    #[test]
    fn shows_the_due_time_in_prague_with_who_created_it() {
        let html = page(&empty_form(), &pending(), &[]).into_string();

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
