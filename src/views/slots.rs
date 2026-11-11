//! Brewing slots in the Batches tab: the upcoming slots soonest first, each with the way to move
//! or cancel it, the form a Member plans one with, and the page a slot is moved on.

use jiff::civil::Date;
use maud::{Markup, html};

use crate::domain::batches::{Slot, SlotId};
use crate::views::delete::confirm_button;
use crate::views::finance::problem;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};

/// Where a slot's links and forms lead. The routes in `web::batches` match on the same paths.
pub mod paths {
    use crate::domain::batches::SlotId;
    use crate::views::nav::Tab;

    /// Where the form that plans a slot posts.
    #[must_use]
    pub fn slots() -> String {
        format!("{}/slots", Tab::Batches.href())
    }

    /// The slot `id`'s page, where it is moved; its form posts back to the same path.
    #[must_use]
    pub fn slot(id: SlotId) -> String {
        format!("{}/{}", slots(), id.get())
    }

    /// Where the button that cancels the slot `id` posts.
    #[must_use]
    pub fn cancel(id: SlotId) -> String {
        format!("{}/cancel", slot(id))
    }
}

/// The id of the form that plans a slot, which a refused plan is swapped into.
const PLAN_ID: &str = "plan-slot";

/// The id of the form that moves a slot, which a refused move is swapped into.
const MOVE_ID: &str = "move-slot";

/// A slot's date as the Crew says it: `Sat 21 Nov`.
#[must_use]
pub fn slot_day(date: Date) -> String {
    date.strftime("%a %-d %b").to_string()
}

/// A `time` element showing `date` as a slot's day.
fn slot_time(date: Date) -> Markup {
    html! {
        time datetime=(date) { (slot_day(date)) }
    }
}

/// The Batches tab's Brewing slots: `slots` soonest first, each with Move and Cancel, or a line
/// saying none is planned, and `plan_form` beneath.
#[must_use]
pub fn slots_section(slots: &[Slot], plan_form: &Markup) -> Markup {
    html! {
        section.section.slots aria-labelledby="slots" {
            h2 #slots { "Brewing slots" }
            @if slots.is_empty() {
                p.empty { "No brew day set aside yet. Plan one and the Crew sees it here." }
            } @else {
                ul.slot-list {
                    @for slot in slots {
                        (slot_row(slot))
                    }
                }
            }
            (plan_form)
        }
    }
}

/// An upcoming slot: its day, that nothing is chosen to brew on it yet, and the way to move or
/// cancel it. Cancelling asks first.
#[must_use]
pub fn slot_row(slot: &Slot) -> Markup {
    let day = slot_day(slot.date);
    html! {
        li.slot {
            span.slot-main {
                span.slot-date { (slot_time(slot.date)) }
                span.slot-batch { "Nothing chosen yet" }
            }
            span.slot-actions {
                a.button.secondary.small href=(paths::slot(slot.id)) { "Move" }
                (confirm_button(
                    "Cancel",
                    &format!("Cancel the slot on {day}? The day is no longer set aside for brewing."),
                    &paths::cancel(slot.id),
                    "button danger small",
                ))
            }
        }
    }
}

/// The form a slot is planned with: a date from `today` on.
///
/// It shows `value` and, beneath it, what was wrong with it. htmx swaps the form for the one the
/// server answers with; without htmx, it posts as a plain form.
#[must_use]
pub fn plan_form(value: &str, problem_message: Option<&str>, today: Date) -> Markup {
    let action = paths::slots();
    html! {
        form.entry-form.slot-form #(PLAN_ID) method="post" action=(action) hx-post=(action)
            hx-swap="outerHTML" {
            (date_field("slot-date", value, problem_message, today))
            button.button type="submit" { "Plan a slot" }
        }
    }
}

/// The form that moves the slot `id` to a date from `today` on.
///
/// It shows `value` and, beneath it, what was wrong with it, and goes back to the tab.
#[must_use]
pub fn move_form(id: SlotId, value: &str, problem_message: Option<&str>, today: Date) -> Markup {
    let action = paths::slot(id);
    html! {
        form.entry-form.slot-form #(MOVE_ID) method="post" action=(action) hx-post=(action)
            hx-swap="outerHTML" {
            (date_field("move-date", value, problem_message, today))
            div.edit-actions {
                button.button type="submit" { "Move slot" }
                a.button.secondary href=(Tab::Batches.href()) { "Back" }
            }
        }
    }
}

/// A slot's date field: `value` in a date control that starts at `today`, with `problem_message`
/// beneath.
fn date_field(id: &str, value: &str, problem_message: Option<&str>, today: Date) -> Markup {
    let problem_id = format!("{id}-problem");
    let refused = problem_message.is_some();
    html! {
        div.field {
            label for=(id) { "Date" }
            input #(id) type="date" name="date" value=(value) min=(today) required
                aria-invalid=[refused.then_some("true")]
                aria-describedby=[refused.then_some(&problem_id)];
            (problem(&problem_id, problem_message))
        }
    }
}

/// The page the slot on `date` is moved on, around `form`.
#[must_use]
pub fn move_page(date: Date, form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Move the slot on " (slot_time(date)) }
            p.muted { "Pick the day the Crew brews instead." }
            (form)
        },
    )
}

/// The page shown in place of a slot once someone has cancelled it.
#[must_use]
pub fn gone_page() -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Brewing slot" }
            p.empty role="status" {
                "This slot is gone: someone cancelled it, or it never existed."
            }
            a.button.secondary href=(Tab::Batches.href()) { "All Batches" }
        },
    )
}

/// Slots and forms for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::{Date, date};

    use crate::domain::batches::Slot;
    use crate::domain::batches::slot_fixtures::slot;

    /// The day the fixtures are planned on: Wednesday 11 November 2026.
    pub const TODAY: Date = date(2026, 11, 11);

    /// One slot, on Saturday 21 November.
    #[must_use]
    pub fn one() -> Vec<Slot> {
        vec![slot(3, "2026-11-21")]
    }

    /// Three slots, soonest first, the first of them today.
    #[must_use]
    pub fn several() -> Vec<Slot> {
        vec![
            slot(5, "2026-11-11"),
            slot(3, "2026-11-21"),
            slot(4, "2026-12-05"),
        ]
    }

    /// A date typed into the plan form, and why it was refused.
    pub const REFUSED: (&str, &str) = ("2026-11-21", "There's already a slot on Sat 21 Nov.");
}

#[cfg(test)]
mod tests {
    use super::fixtures::{REFUSED, TODAY, one, several};
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_plan_form() -> Markup {
        plan_form("", None, TODAY)
    }

    #[test]
    fn slots_empty() {
        assert_html_snapshot!(slots_section(&[], &empty_plan_form()));
    }

    #[test]
    fn slots_one() {
        assert_html_snapshot!(slots_section(&one(), &empty_plan_form()));
    }

    #[test]
    fn slots_several() {
        assert_html_snapshot!(slots_section(&several(), &empty_plan_form()));
    }

    #[test]
    fn plan_form_with_errors() {
        let (value, message) = REFUSED;
        assert_html_snapshot!(plan_form(value, Some(message), TODAY));
    }

    #[test]
    fn slot_move_page() {
        let slot = &one()[0];
        let form = move_form(slot.id, "2026-11-21", None, TODAY);
        assert_html_snapshot!(move_page(slot.date, &form));
    }

    #[test]
    fn slot_move_page_with_errors() {
        let slot = &one()[0];
        let form = move_form(
            slot.id,
            "2026-11-10",
            Some("Pick today or a later date."),
            TODAY,
        );
        assert_html_snapshot!(move_page(slot.date, &form));
    }

    #[test]
    fn slot_gone_page() {
        assert_html_snapshot!(gone_page());
    }

    #[test]
    fn a_slot_shows_its_day_and_that_no_batch_is_chosen() {
        let html = slot_row(&one()[0]).into_string();

        assert!(
            html.contains(r#"<time datetime="2026-11-21">Sat 21 Nov</time>"#),
            "{html}"
        );
        assert!(html.contains("Nothing chosen yet"));
        assert!(html.contains(r#"href="/batches/slots/3""#));
    }

    #[test]
    fn cancelling_a_slot_asks_first() {
        let html = slot_row(&one()[0]).into_string();

        assert!(
            html.contains(r#"hx-post="/batches/slots/3/cancel" hx-confirm="Cancel the slot on Sat 21 Nov? The day is no longer set aside for brewing.""#),
            "{html}"
        );
    }

    #[test]
    fn the_forms_start_at_today_and_keep_what_was_typed() {
        let html = plan_form("2026-11-21", Some("Taken."), TODAY).into_string();

        assert!(html.contains(r#"min="2026-11-11""#), "{html}");
        assert!(html.contains(r#"value="2026-11-21""#));
        assert!(html.contains(r#"aria-invalid="true""#));
        assert!(html.contains("Taken."));
    }

    #[test]
    fn no_slot_says_so() {
        let html = slots_section(&[], &empty_plan_form()).into_string();

        assert!(html.contains("No brew day set aside yet."), "{html}");
        assert!(!html.contains("slot-list"));
    }
}
