//! Brewing slots in the Batches tab, and each slot's own page.
//!
//! The tab lists the open slots by date, those asking "Did you brew?" first, each with its Batch
//! and the way to pick one, move or cancel it, and the form a Member plans one with. A slot's
//! page is where its Batch is picked and it is moved.

use jiff::civil::Date;
use maud::{Markup, html};

use crate::domain::batches::{PlannedBatch, Slot, SlotBatch, SlotId, SlotState};
use crate::views::batches::{number_label, paths as batch_paths};
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

    /// The slot `id`'s page, where its Batch is picked and it is moved; the move form posts back
    /// to the same path.
    #[must_use]
    pub fn slot(id: SlotId) -> String {
        format!("{}/{}", slots(), id.get())
    }

    /// Where the button that cancels the slot `id` posts.
    #[must_use]
    pub fn cancel(id: SlotId) -> String {
        format!("{}/cancel", slot(id))
    }

    /// Where the form that links a Batch to the slot `id` posts.
    #[must_use]
    pub fn link(id: SlotId) -> String {
        format!("{}/batch", slot(id))
    }

    /// Where the button that takes the Batch off the slot `id` posts.
    #[must_use]
    pub fn unlink(id: SlotId) -> String {
        format!("{}/unlink", slot(id))
    }
}

/// The id of the slot page's section where its Batch is picked.
const BATCH_ID: &str = "slot-batch";

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

/// The Batches tab's Brewing slots: `slots` by date, each with its actions, or a line saying none
/// is planned, and `plan_form` beneath.
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

/// A slot: its day, its Batch leading to the Batch's page or that nothing is chosen yet, and
/// what can be done with it. Cancelling asks first.
///
/// An upcoming slot offers to pick a Batch, or to unlink the one it has, and to move it. A slot
/// whose day has passed with nothing brewed asks "Did you brew?" and offers to pick the Batch
/// brewed, which goes to that Batch's Brew day form with the slot's date filled in.
#[must_use]
pub fn slot_row(slot: &Slot) -> Markup {
    let day = slot_day(slot.date);
    let passed = slot.state == SlotState::DidYouBrew;
    html! {
        li.slot.passed[passed] {
            span.slot-main {
                span.slot-date { (slot_time(slot.date)) }
                @if passed {
                    span.slot-ask { "Did you brew?" }
                }
                span.slot-batch {
                    @if let Some(batch) = &slot.batch {
                        a href=(batch_paths::batch(batch.number)) { (batch_label(batch)) }
                    } @else {
                        "Nothing chosen yet"
                    }
                }
            }
            span.slot-actions {
                @if passed {
                    @let pick = slot.batch.as_ref().map_or_else(
                        || paths::slot(slot.id),
                        |batch| batch_paths::brew_day_on(batch.number, slot.date),
                    );
                    a.button.small href=(pick) { "Pick the Batch" }
                } @else if slot.batch.is_some() {
                    (unlink_button(&paths::unlink(slot.id)))
                } @else {
                    a.button.secondary.small href={ (paths::slot(slot.id)) "#" (BATCH_ID) } {
                        "Pick a Batch"
                    }
                }
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

/// A Batch as a slot names it: `#14 Vzor 10°`.
#[must_use]
pub fn batch_label(batch: &SlotBatch) -> String {
    format!("{} {}", number_label(batch.number), batch.name.as_str())
}

/// The button that posts to `action` to take a Batch off its slot. It asks nothing: linking it
/// again is as quick.
#[must_use]
pub fn unlink_button(action: &str) -> Markup {
    html! {
        form method="post" action=(action) {
            button.button.secondary.small type="submit" { "Unlink" }
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

/// The slot's page: what's brewed on it, and `move_form` beneath.
///
/// An upcoming slot shows its Batch with the way to unlink it, and the form that picks one of
/// `planned`, the Planned Batches, saying `link_problem` beneath it. A slot whose day has passed
/// asks "Did you brew?" and lists `planned`, each leading to its Brew day form with the slot's
/// date filled in.
#[must_use]
pub fn slot_page(
    slot: &Slot,
    planned: &[PlannedBatch],
    link_problem: Option<&str>,
    move_form: &Markup,
) -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Brewing slot on " (slot_time(slot.date)) }
            @if slot.state == SlotState::DidYouBrew {
                (did_you_brew(slot, planned))
            } @else {
                (batch_section(slot, planned, link_problem))
            }
            section.section.narrow aria-labelledby="move-heading" {
                h2 #move-heading { "Move" }
                p.muted { "Pick the day the Crew brews instead." }
                (move_form)
            }
        },
    )
}

/// What's brewed on an upcoming `slot`: its Batch with Unlink, or that nothing is chosen, and
/// the form that picks another of `planned`.
fn batch_section(slot: &Slot, planned: &[PlannedBatch], link_problem: Option<&str>) -> Markup {
    let others: Vec<&PlannedBatch> = planned
        .iter()
        .filter(|batch| {
            slot.batch
                .as_ref()
                .is_none_or(|on| on.number != batch.number)
        })
        .collect();
    html! {
        section.section.narrow #(BATCH_ID) aria-labelledby="slot-batch-heading" {
            h2 #slot-batch-heading { "What's brewed" }
            @if let Some(batch) = &slot.batch {
                div.slot-chosen {
                    a href=(batch_paths::batch(batch.number)) { (batch_label(batch)) }
                    (unlink_button(&paths::unlink(slot.id)))
                }
            } @else {
                p.muted { "Nothing chosen yet." }
            }
            @if others.is_empty() {
                p.muted {
                    @if slot.batch.is_some() { "No other Planned Batch to brew instead." }
                    @else { "No Planned Batch to brew. Add one to the log first." }
                }
            } @else {
                (link_form(slot, &others, link_problem))
            }
        }
    }
}

/// The form that links one of `planned` to `slot`, saying `problem_message` beneath. A Batch
/// on another slot says so, since picking it moves it here.
fn link_form(slot: &Slot, planned: &[&PlannedBatch], problem_message: Option<&str>) -> Markup {
    let action = paths::link(slot.id);
    let id = "link-batch";
    let problem_id = format!("{id}-problem");
    let refused = problem_message.is_some();
    html! {
        form.entry-form.slot-form method="post" action=(action) {
            div.field {
                label for=(id) {
                    @if slot.batch.is_some() { "Brew instead" } @else { "Batch" }
                }
                select #(id) name="batch" required
                    aria-invalid=[refused.then_some("true")]
                    aria-describedby=[refused.then_some(&problem_id)] {
                    option value="" { "Pick a Planned Batch" }
                    @for batch in planned {
                        option value=(batch.number.get()) {
                            (number_label(batch.number)) " " (batch.name.as_str())
                            @if let Some(on) = batch.slot {
                                " · on " (slot_day(on))
                            }
                        }
                    }
                }
                (problem(&problem_id, problem_message))
            }
            button.button type="submit" { "Link Batch" }
        }
    }
}

/// What a `slot` whose day has passed asks: which of `planned` was brewed on it, each leading
/// to its Brew day form with the slot's date filled in.
///
/// Only its own Batch and those waiting for a slot are offered: recording the Brew day links a
/// Batch to the slot only if it has none, so a Batch on another slot would leave this one asking.
fn did_you_brew(slot: &Slot, planned: &[PlannedBatch]) -> Markup {
    let own = slot.batch.as_ref().map(|batch| batch.number);
    let planned: Vec<&PlannedBatch> = planned
        .iter()
        .filter(|batch| batch.slot.is_none() || Some(batch.number) == own)
        .collect();
    html! {
        section.section.narrow #(BATCH_ID) aria-labelledby="slot-batch-heading" {
            h2 #slot-batch-heading { "Did you brew?" }
            p.muted {
                "The day has passed and no Brew day is recorded on it. Pick the Batch you brewed, \
                 or move or cancel the slot."
            }
            @if planned.is_empty() {
                p.muted { "No Planned Batch to pick. Add the one you brewed to the log first." }
            } @else {
                ul.slot-picks {
                    @for batch in planned {
                        li {
                            a href=(batch_paths::brew_day_on(batch.number, slot.date)) {
                                (number_label(batch.number)) " " (batch.name.as_str())
                            }
                        }
                    }
                }
            }
        }
    }
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

    use crate::domain::batches::slot_fixtures::{passed, slot, with_batch};
    use crate::domain::batches::{BatchName, BatchNumber, PlannedBatch, Slot};

    /// The day the fixtures are planned on: Wednesday 11 November 2026.
    pub const TODAY: Date = date(2026, 11, 11);

    /// One slot, on Saturday 21 November, with nothing chosen.
    #[must_use]
    pub fn one() -> Vec<Slot> {
        vec![slot(3, "2026-11-21")]
    }

    /// The slot on Saturday 21 November, with #14 chosen for it.
    #[must_use]
    pub fn linked() -> Slot {
        with_batch(slot(3, "2026-11-21"), 14, "Vzor 10°")
    }

    /// A slot on Sunday 8 November that passed with nothing brewed and nothing chosen.
    #[must_use]
    pub fn asking() -> Slot {
        passed(slot(2, "2026-11-08"))
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

    /// The upcoming slots with nothing chosen, soonest first: those of [`several`] but the one
    /// on Saturday 21 November, which the slotted Pilsner is on.
    #[must_use]
    pub fn free() -> Vec<Slot> {
        vec![slot(5, "2026-11-11"), slot(4, "2026-12-05")]
    }

    /// A slot in each state shown, by date: two passed with nothing brewed, one of them with a
    /// Batch chosen; one today with nothing chosen; and one later with a Batch.
    #[must_use]
    pub fn every_state() -> Vec<Slot> {
        vec![
            passed(with_batch(slot(1, "2026-11-01"), 13, "Desítka")),
            asking(),
            slot(5, "2026-11-11"),
            linked(),
        ]
    }

    fn planned_batch(number: i64, name: &str, slot: Option<Date>) -> PlannedBatch {
        PlannedBatch {
            number: BatchNumber::from_client(number),
            name: BatchName::parse(name).expect("fixture names are valid"),
            slot,
        }
    }

    /// The Planned Batches, newest first: one waiting for a slot, #14 on the slot on Saturday 21
    /// November, and one on a slot in December.
    #[must_use]
    pub fn planned() -> Vec<PlannedBatch> {
        vec![
            planned_batch(15, "Winter Stout", None),
            planned_batch(14, "Vzor 10°", Some(date(2026, 11, 21))),
            planned_batch(10, "Hoppy Lager", Some(date(2026, 12, 5))),
        ]
    }

    /// Why picking a Batch for a slot was refused.
    pub const LINK_REFUSED: &str = "That Batch is brewed already. Pick a Planned Batch.";

    /// A date typed into the plan form, and why it was refused.
    pub const REFUSED: (&str, &str) = ("2026-11-21", "There's already a slot on Sat 21 Nov.");
}

#[cfg(test)]
mod tests {
    use super::fixtures::{
        LINK_REFUSED, REFUSED, TODAY, asking, every_state, linked, one, planned, several,
    };
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_plan_form() -> Markup {
        plan_form("", None, TODAY)
    }

    fn page_of(slot: &Slot, planned: &[PlannedBatch], link_problem: Option<&str>) -> Markup {
        let form = move_form(slot.id, &slot.date.to_string(), None, TODAY);
        slot_page(slot, planned, link_problem, &form)
    }

    #[test]
    fn slots_in_every_state() {
        assert_html_snapshot!(slots_section(&every_state(), &empty_plan_form()));
    }

    #[test]
    fn slot_page_with_nothing_chosen() {
        assert_html_snapshot!(page_of(&one()[0], &planned(), None));
    }

    #[test]
    fn slot_page_with_a_batch() {
        assert_html_snapshot!(page_of(&linked(), &planned(), None));
    }

    #[test]
    fn slot_page_with_a_refused_link() {
        assert_html_snapshot!(page_of(&one()[0], &planned(), Some(LINK_REFUSED)));
    }

    #[test]
    fn slot_page_without_planned_batches() {
        assert_html_snapshot!(page_of(&one()[0], &[], None));
    }

    #[test]
    fn slot_page_did_you_brew() {
        assert_html_snapshot!(page_of(&asking(), &planned(), None));
    }

    #[test]
    fn a_slot_shows_its_batch_leading_to_it_and_unlinks_without_asking() {
        let html = slot_row(&linked()).into_string();

        assert!(
            html.contains(r#"<a href="/batches/14">#14 Vzor 10°</a>"#),
            "{html}"
        );
        assert!(!html.contains("Nothing chosen yet"));
        assert!(
            html.contains(
                r#"<form method="post" action="/batches/slots/3/unlink"><button class="button secondary small" type="submit">Unlink</button></form>"#
            ),
            "{html}"
        );
        assert!(!html.contains("Pick a Batch"));
    }

    #[test]
    fn a_slot_with_nothing_chosen_offers_to_pick_a_batch() {
        let html = slot_row(&one()[0]).into_string();

        assert!(
            html.contains(r#"href="/batches/slots/3#slot-batch">Pick a Batch</a>"#),
            "{html}"
        );
        assert!(!html.contains("Unlink"));
    }

    #[test]
    fn a_passed_slot_asks_did_you_brew_and_picks_the_batch_on_its_date() {
        let rows = every_state();

        let chosen = slot_row(&rows[0]).into_string();
        assert!(chosen.contains("Did you brew?"), "{chosen}");
        assert!(
            chosen.contains(r#"href="/batches/13/brew-day?date=2026-11-01">Pick the Batch</a>"#),
            "{chosen}"
        );
        assert!(chosen.contains(r#"href="/batches/slots/1">Move</a>"#));
        assert!(chosen.contains("/batches/slots/1/cancel"));
        assert!(
            !chosen.contains("Unlink"),
            "nothing to unlink once it's passed"
        );

        let unchosen = slot_row(&rows[1]).into_string();
        assert!(
            unchosen.contains(r#"href="/batches/slots/2">Pick the Batch</a>"#),
            "{unchosen}"
        );

        let upcoming = slot_row(&rows[2]).into_string();
        assert!(!upcoming.contains("Did you brew?"));
    }

    #[test]
    fn the_slot_page_offers_the_other_planned_batches_saying_which_would_move() {
        let html = page_of(&linked(), &planned(), None).into_string();

        assert!(
            html.contains(r#"action="/batches/slots/3/batch""#),
            "{html}"
        );
        assert!(html.contains(r#"<option value="15">#15 Winter Stout</option>"#));
        assert!(html.contains(r#"<option value="10">#10 Hoppy Lager · on Sat 5 Dec</option>"#));
        assert!(
            !html.contains(r#"<option value="14">"#),
            "not the Batch it has"
        );
        assert!(html.contains("Brew instead"));
        assert!(html.contains("/batches/slots/3/unlink"));
    }

    #[test]
    fn a_passed_slots_page_leads_to_the_brew_day_of_batches_waiting_for_a_slot() {
        let html = page_of(&asking(), &planned(), None).into_string();

        assert!(html.contains("Did you brew?"), "{html}");
        assert!(html.contains(r#"href="/batches/15/brew-day?date=2026-11-08""#));
        assert!(
            !html.contains("/batches/14/brew-day"),
            "not one on another slot"
        );
        assert!(!html.contains("/batches/slots/2/batch"), "no link form");

        let own = crate::domain::batches::slot_fixtures::passed(linked());
        let html = page_of(&own, &planned(), None).into_string();
        assert!(
            html.contains(r#"href="/batches/14/brew-day?date=2026-11-21""#),
            "its own Batch: {html}"
        );
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
    fn slot_move_page_with_errors() {
        let slot = &one()[0];
        let form = move_form(
            slot.id,
            "2026-11-10",
            Some("Pick today or a later date."),
            TODAY,
        );
        assert_html_snapshot!(slot_page(slot, &planned(), None, &form));
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
