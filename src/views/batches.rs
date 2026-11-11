//! The Batches tab, the Crew's brewery log.
//!
//! The Brewing slots at the top (see `views::slots`), the form a Member adds a Batch with, every Batch newest first, each Batch's own page with its
//! Brew day, Bottling and Recipe, and the page it's edited or deleted on.

use std::fmt::Display;

use jiff::civil::Date;
use maud::{Markup, html};

use crate::domain::batches::{
    AttachedRecipe, Batch, BatchNumber, BatchPage, Bottling, BrewDay, NAME_MAX, NOTES_MAX, Percent,
    RecipeSummary, Stage, StyleSummary, Targets, Volume,
};
use crate::views::delete::{Deletion, delete_section};
use crate::views::finance::problem;
use crate::views::icon::chevron;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};
use crate::views::time::{field_time, shown_time};

/// Where a Batch's links and forms lead. The routes in `web::batches` match on the same paths.
pub mod paths {
    use crate::domain::batches::BatchNumber;
    use crate::views::nav::Tab;

    /// The Batch `number`'s page, which its edit form posts to.
    #[must_use]
    pub fn batch(number: BatchNumber) -> String {
        format!("{}/{}", Tab::Batches.href(), number.get())
    }

    /// The Batch `number` in its edit form.
    #[must_use]
    pub fn edit(number: BatchNumber) -> String {
        format!("{}/edit", batch(number))
    }

    /// Where the Batch `number`'s delete button posts.
    #[must_use]
    pub fn delete(number: BatchNumber) -> String {
        format!("{}/delete", batch(number))
    }

    /// The Batch `number`'s Brew day form, which posts back to the same path.
    #[must_use]
    pub fn brew_day(number: BatchNumber) -> String {
        format!("{}/brew-day", batch(number))
    }

    /// The Batch `number`'s Bottling form, which posts back to the same path.
    #[must_use]
    pub fn bottling(number: BatchNumber) -> String {
        format!("{}/bottling", batch(number))
    }

    /// Where the Batch `number`'s Recipe file is uploaded, to attach or replace it.
    #[must_use]
    pub fn recipe(number: BatchNumber) -> String {
        format!("{}/recipe", batch(number))
    }

    /// Where the button that removes the Batch `number`'s Recipe posts.
    #[must_use]
    pub fn remove_recipe(number: BatchNumber) -> String {
        format!("{}/delete", recipe(number))
    }
}

/// The id of the Batch page's Brew day section, which its form is swapped into and out of.
const BREW_DAY_ID: &str = "brew-day";

/// The id of the Batch page's Bottling section, which its form is swapped into and out of.
const BOTTLING_ID: &str = "bottling";

/// The id of the Recipe upload form on a Batch's page, which a refused upload is swapped into.
const RECIPE_UPLOAD_ID: &str = "recipe-upload";

/// What the Brew day form shows in its fields: what was typed, or what the Batch has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrewDayValues {
    pub brew_date: String,
    pub og: String,
    pub volume: String,
}

impl BrewDayValues {
    /// The Brew day `batch` has, with `today` as the brew date until it has one.
    #[must_use]
    pub fn of(batch: &Batch, today: Date) -> Self {
        let BrewDay { date, og, volume } = batch.brew_day;
        Self {
            brew_date: date.unwrap_or(today).to_string(),
            og: og.map(|og| og.to_string()).unwrap_or_default(),
            volume: volume.map(Volume::litres).unwrap_or_default(),
        }
    }
}

/// Why each field of the Brew day form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct BrewDayProblems {
    pub brew_date: Option<String>,
    pub og: Option<String>,
    pub volume: Option<String>,
}

/// What the Bottling form shows in its fields: what was typed, or what the Batch has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BottlingValues {
    pub bottling_date: String,
    pub fg: String,
}

impl BottlingValues {
    /// The Bottling `batch` has, with `today` as the bottling date until it has one.
    #[must_use]
    pub fn of(batch: &Batch, today: Date) -> Self {
        let Bottling { date, fg } = batch.bottling;
        Self {
            bottling_date: date.unwrap_or(today).to_string(),
            fg: fg.map(|fg| fg.to_string()).unwrap_or_default(),
        }
    }
}

/// Why each field of the Bottling form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct BottlingProblems {
    pub bottling_date: Option<String>,
    pub fg: Option<String>,
}

/// Whether a form adds a new Batch or saves changes to the Batch it edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Add,
    Edit(BatchNumber),
}

impl Purpose {
    fn action(self) -> String {
        match self {
            Self::Add => Tab::Batches.href().to_owned(),
            Self::Edit(number) => paths::batch(number),
        }
    }
}

/// What the Batch form shows in its fields: what was typed, or nothing yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct BatchValues<'a> {
    pub name: &'a str,
    pub notes: &'a str,
}

/// Why each field of the Batch form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct BatchProblems {
    pub name: Option<String>,
    pub notes: Option<String>,
    /// Only when adding: the Recipe file. A browser can't be handed a file back, so a refused
    /// form asks for it again.
    pub recipe: Option<String>,
}

/// A Batch number as the Crew says it: `#12`.
#[must_use]
pub fn number_label(number: BatchNumber) -> String {
    format!("#{}", number.get())
}

/// The tab at `page`: no query for the first.
#[must_use]
pub fn href(page: usize) -> String {
    if page > 1 {
        format!("{}?page={page}", Tab::Batches.href())
    } else {
        Tab::Batches.href().to_owned()
    }
}

/// The Batches tab: the Brewing `slots` section first, then `form` to add a Batch, and one page
/// of the log, newest first. On a wide screen the form sits beside the log.
#[must_use]
pub fn page(slots: &Markup, form: &Markup, log: &BatchPage) -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Batches" }
            (slots)
            div.columns {
                section.section aria-labelledby="new-batch" {
                    h2 #new-batch { "Add a Batch" }
                    p.muted { "A name is enough. It gets the next Batch number." }
                    (form)
                }
                section.section aria-labelledby="log" {
                    h2 #log { "Log" }
                    (batch_list(log))
                }
            }
        },
    )
}

/// One page of the log, newest first, with the way to the other pages. Without any Batches, it
/// says so.
fn batch_list(log: &BatchPage) -> Markup {
    html! {
        @if log.batches.is_empty() {
            p.empty { "No Batches yet. Add one and the Crew sees it here." }
        } @else {
            ul.batch-list {
                @for batch in &log.batches {
                    (batch_row(batch))
                }
            }
        }
        @if log.pages > 1 {
            nav.pages aria-label="Pages" {
                @if log.page > 1 {
                    a.button.secondary.small rel="prev" href=(href(log.page - 1)) { "Newer" }
                }
                span.pages-at { "Page " (log.page) " of " (log.pages) }
                @if log.page < log.pages {
                    a.button.secondary.small rel="next" href=(href(log.page + 1)) { "Older" }
                }
            }
        }
    }
}

/// A Batch in the log: its number, name and Stage, and when it was brewed and bottled, or who
/// added it on which day until it's brewed. It leads to the Batch's own page.
#[must_use]
pub fn batch_row(batch: &Batch) -> Markup {
    let added = batch.added_at.crew_time();
    html! {
        li.batch {
            a.batch-link href=(paths::batch(batch.number)) {
                span.batch-number { (number_label(batch.number)) }
                span.batch-main {
                    span.batch-name { (batch.name.as_str()) }
                    span.batch-meta {
                        @if let Some(brewed) = batch.brew_day.date {
                            "Brewed " (day_time(brewed))
                            @if let Some(bottled) = batch.bottling.date {
                                " · Bottled " (day_time(bottled))
                            }
                        } @else {
                            "Added by " (batch.added_by) " · "
                            time datetime=(field_time(added)) { (day(added.date())) }
                        }
                    }
                }
                (stage_badge(batch.stage()))
                (chevron())
            }
        }
    }
}

/// The badge naming `stage`. A Brewed Batch is still in the fermenter, so its badge says so and
/// stands out.
#[must_use]
pub fn stage_badge(stage: Stage) -> Markup {
    let (class, label) = match stage {
        Stage::Planned => ("stage planned", "Planned"),
        Stage::Brewed => ("stage brewed", "In the fermenter"),
        Stage::Bottled => ("stage bottled", "Bottled"),
    };
    html! {
        span class=(class) { (label) }
    }
}

/// A Batch's own page: its number, name, Stage, Brew day, Bottling, `recipe` and Notes.
///
/// Brew day and Bottling show the Recipe's targets beside what was measured, and Bottling what's
/// worked out from OG and FG. The Notes keep their line breaks, and at the foot are who added and
/// last edited it when, and the way to its edit page.
#[must_use]
pub fn batch_page(batch: &Batch, recipe: Option<&AttachedRecipe>) -> Markup {
    let added = batch.added_at.crew_time();
    let targets = recipe.and_then(AttachedRecipe::targets);
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            p.batch-eyebrow { "Batch " (number_label(batch.number)) }
            h1.batch-title { (batch.name.as_str()) }
            (stage_badge(batch.stage()))
            (brew_day_section(batch, targets))
            (bottling_section(batch, targets))
            (recipe_section(batch.number, recipe))
            section.section.narrow aria-labelledby="notes" {
                h2 #notes { "Notes" }
                @if batch.notes.is_empty() {
                    p.muted { "No Notes yet." }
                } @else {
                    p.batch-notes { (batch.notes.as_str()) }
                }
            }
            p.batch-meta {
                "Added by " (batch.added_by) " · "
                time datetime=(field_time(added)) { (shown_time(added)) }
                @if let Some(edited) = &batch.edited {
                    @let at = edited.at.crew_time();
                    br;
                    "Edited by " (edited.by) " · "
                    time datetime=(field_time(at)) { (shown_time(at)) }
                }
            }
            div.batch-actions {
                a.button.secondary href=(paths::edit(batch.number)) { "Edit Batch" }
                a href=(Tab::Batches.href()) { "All Batches" }
            }
        },
    )
}

/// The Batch page's Brew day: the brew date, OG and Volume recorded, the Recipe's target OG
/// from `targets` beside the OG, and the button that swaps in its form.
#[must_use]
pub fn brew_day_section(batch: &Batch, targets: Option<&Targets>) -> Markup {
    let BrewDay { date, og, volume } = batch.brew_day;
    let recorded = batch.brew_day != BrewDay::default();
    let target_og = targets.and_then(|targets| targets.og);
    let path = paths::brew_day(batch.number);
    html! {
        section.section.narrow #(BREW_DAY_ID) aria-labelledby="brew-day-heading" {
            h2 #brew-day-heading { "Brew day" }
            @if !recorded {
                p.muted { "Not brewed yet." }
            }
            @if recorded || target_og.is_some() {
                dl.batch-numbers {
                    @if let Some(date) = date {
                        dt { "Brewed on" }
                        dd { (day_time(date)) }
                    }
                    (measured_row("OG", og, target_og))
                    (measured_row("Volume", volume, None::<Volume>))
                }
            }
            a.button.secondary.small href=(path) hx-get=(path)
                hx-target={ "#" (BREW_DAY_ID) } hx-swap="outerHTML" {
                @if recorded { "Edit Brew day" } @else { "Record Brew day" }
            }
        }
    }
}

/// The Batch page's Bottling, with the button that swaps in its form.
///
/// It shows the bottling date and FG recorded, the ABV and attenuation once OG and FG are both
/// in, and the Recipe's target FG and ABV from `targets` beside them.
#[must_use]
pub fn bottling_section(batch: &Batch, targets: Option<&Targets>) -> Markup {
    let Bottling { date, fg } = batch.bottling;
    let recorded = batch.bottling != Bottling::default();
    let target_fg = targets.and_then(|targets| targets.fg);
    let target_abv = targets.and_then(|targets| targets.abv);
    let path = paths::bottling(batch.number);
    html! {
        section.section.narrow #(BOTTLING_ID) aria-labelledby="bottling-heading" {
            h2 #bottling-heading { "Bottling" }
            @if !recorded {
                p.muted { "Not bottled yet." }
            }
            @if recorded || target_fg.is_some() || target_abv.is_some() {
                dl.batch-numbers {
                    @if let Some(date) = date {
                        dt { "Bottled on" }
                        dd { (day_time(date)) }
                    }
                    (measured_row("FG", fg, target_fg))
                    (measured_row("ABV", batch.abv(), target_abv))
                    (measured_row("Attenuation", batch.attenuation(), None::<Percent>))
                }
            }
            a.button.secondary.small href=(path) hx-get=(path)
                hx-target={ "#" (BOTTLING_ID) } hx-swap="outerHTML" {
                @if recorded { "Edit Bottling" } @else { "Record Bottling" }
            }
        }
    }
}

/// A value as measured, beside the Recipe's target for it when there is one: `1.048` and then
/// `target 1.050`. Without a measurement it says so, so the target still has its pair.
#[must_use]
pub fn target_vs_measured<T: Display>(measured: Option<T>, target: Option<T>) -> Markup {
    html! {
        @if let Some(measured) = measured {
            span.measured { (measured) }
        } @else {
            span.measured.missing { "Not measured" }
        }
        @if let Some(target) = target {
            " "
            span.target { "target " (target) }
        }
    }
}

/// A row of a Batch's numbers named `label`, if there's a measured or target value to show.
fn measured_row<T: Display>(label: &str, measured: Option<T>, target: Option<T>) -> Markup {
    html! {
        @if measured.is_some() || target.is_some() {
            dt { (label) }
            dd { (target_vs_measured(measured, target)) }
        }
    }
}

/// The Brew day form of the Batch `number`, in place of its Brew day section.
///
/// It shows `values` and, beside each field, what was wrong with it. htmx swaps the answer in
/// place of the section; without htmx, it posts as a plain form. It cancels back to the Batch's
/// page.
#[must_use]
pub fn brew_day_form(
    batch_number: BatchNumber,
    values: &BrewDayValues,
    problems: &BrewDayProblems,
) -> Markup {
    let action = paths::brew_day(batch_number);
    html! {
        section.section.narrow #(BREW_DAY_ID) aria-labelledby="brew-day-heading" {
            h2 #brew-day-heading { "Brew day" }
            form.entry-form method="post" action=(action) hx-post=(action)
                hx-target={ "#" (BREW_DAY_ID) } hx-swap="outerHTML" {
                (field("brew-date", "brew_date", "Brew date", Input::Date, &values.brew_date,
                    problems.brew_date.as_deref()))
                (field("brew-og", "og", "OG", Input::Number("1.052"), &values.og,
                    problems.og.as_deref()))
                (field("brew-volume", "volume", "Volume in litres", Input::Number("23.5"),
                    &values.volume, problems.volume.as_deref()))
                p.note { "Leave a field empty to clear it." }
                div.edit-actions {
                    button.button type="submit" { "Save Brew day" }
                    a.button.secondary href=(paths::batch(batch_number)) { "Cancel" }
                }
            }
        }
    }
}

/// The Bottling form of the Batch `number`, in place of its Bottling section.
///
/// It shows `values` and, beside each field, what was wrong with it. htmx swaps the answer in
/// place of the section; without htmx, it posts as a plain form. It cancels back to the Batch's
/// page.
#[must_use]
pub fn bottling_form(
    batch_number: BatchNumber,
    values: &BottlingValues,
    problems: &BottlingProblems,
) -> Markup {
    let action = paths::bottling(batch_number);
    html! {
        section.section.narrow #(BOTTLING_ID) aria-labelledby="bottling-heading" {
            h2 #bottling-heading { "Bottling" }
            form.entry-form method="post" action=(action) hx-post=(action)
                hx-target={ "#" (BOTTLING_ID) } hx-swap="outerHTML" {
                (field("bottling-date", "bottling_date", "Bottling date", Input::Date,
                    &values.bottling_date, problems.bottling_date.as_deref()))
                (field("bottling-fg", "fg", "FG", Input::Number("1.010"), &values.fg,
                    problems.fg.as_deref()))
                p.note { "Leave a field empty to clear it. To record an FG before Bottling, clear the bottling date." }
                div.edit-actions {
                    button.button type="submit" { "Save Bottling" }
                    a.button.secondary href=(paths::batch(batch_number)) { "Cancel" }
                }
            }
        }
    }
}

/// The page a Brew day, Bottling or Recipe `form` of the Batch `number` is on without htmx.
#[must_use]
pub fn numbers_page(batch_number: BatchNumber, form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Batch " (number_label(batch_number)) }
            (form)
        },
    )
}

/// What a Brew day or Bottling field holds.
#[derive(Debug, Clone, Copy)]
enum Input {
    Date,
    /// A decimal number, like the example it shows.
    Number(&'static str),
}

/// A field of the Brew day or Bottling form: `label` over the control named `name`, holding
/// `value`, with `problem` beneath. None of them is required, so each can be cleared.
fn field(
    id: &str,
    name: &str,
    label: &str,
    input: Input,
    value: &str,
    problem_message: Option<&str>,
) -> Markup {
    let problem_id = format!("{id}-problem");
    let refused = problem_message.is_some();
    html! {
        div.field {
            label for=(id) { (label) " " span.optional { "(optional)" } }
            @match input {
                Input::Date => input #(id) type="date" name=(name) value=(value)
                    aria-invalid=[refused.then_some("true")]
                    aria-describedby=[refused.then_some(&problem_id)];,
                Input::Number(example) => input #(id) type="text" inputmode="decimal" name=(name)
                    value=(value) placeholder=(example) autocomplete="off"
                    aria-invalid=[refused.then_some("true")]
                    aria-describedby=[refused.then_some(&problem_id)];,
            }
            (problem(&problem_id, problem_message))
        }
    }
}

/// A day as the Batches tab shows it: `3 October 2026`.
fn day(date: Date) -> String {
    date.strftime("%-d %B %Y").to_string()
}

/// A `time` element showing `date` as a day.
fn day_time(date: Date) -> Markup {
    html! {
        time datetime=(date) { (day(date)) }
    }
}

/// The page a Batch is edited on: `form`, and at its foot the button that deletes the Batch
/// `number` after asking.
#[must_use]
pub fn edit_page(batch_number: BatchNumber, form: &Markup) -> Markup {
    let delete = paths::delete(batch_number);
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Edit Batch " (number_label(batch_number)) }
            (form)
            (delete_section(&Deletion {
                kind: "Batch",
                explains: "Deleting this Batch removes it from the log for everyone. Its number isn't used again.",
                warns: "It's gone for good.",
                action: &delete,
            }))
        },
    )
}

/// The page shown in place of the Batch `number` once someone has deleted it.
#[must_use]
pub fn gone_page(batch_number: BatchNumber) -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Batch " (number_label(batch_number)) }
            p.empty role="status" {
                "This Batch is gone: someone deleted it, or it never existed."
            }
            a.button.secondary href=(Tab::Batches.href()) { "All Batches" }
        },
    )
}

/// The form that edits `batch`, filled in with its name and Notes.
#[must_use]
pub fn edit_form(batch: &Batch) -> Markup {
    form(
        Purpose::Edit(batch.number),
        BatchValues {
            name: batch.name.as_str(),
            notes: batch.notes.as_str(),
        },
        &BatchProblems::default(),
    )
}

/// The form a Batch is added or edited with, as `purpose` says.
///
/// It shows `values` and, beside each field, what was wrong with it. htmx swaps the form for the
/// one the server answers with; without htmx, it posts as a plain form. Editing, it also cancels
/// back to the Batch's page.
#[must_use]
pub fn form(purpose: Purpose, values: BatchValues<'_>, problems: &BatchProblems) -> Markup {
    let action = purpose.action();
    let adding = purpose == Purpose::Add;
    html! {
        // Adding uploads the Recipe file too; htmx sends a multipart form as one as well.
        form.entry-form method="post" action=(action)
            enctype=[adding.then_some("multipart/form-data")] hx-post=(action) hx-swap="outerHTML" {
            div.field {
                label for="batch-name" { "Name" }
                // Adding, a blank name takes the Recipe's, so the browser mustn't insist on one.
                input #batch-name type="text" name="name" value=(values.name) required[!adding]
                    maxlength=(NAME_MAX) autocomplete="off"
                    aria-invalid=[problems.name.is_some().then_some("true")]
                    aria-describedby=[problems.name.is_some().then_some("batch-name-problem")];
                (problem("batch-name-problem", problems.name.as_deref()))
            }
            div.field {
                label for="batch-notes" { "Notes " span.optional { "(optional)" } }
                textarea #batch-notes name="notes" rows="5" maxlength=(NOTES_MAX)
                    aria-invalid=[problems.notes.is_some().then_some("true")]
                    aria-describedby=[problems.notes.is_some().then_some("batch-notes-problem")] {
                    (values.notes)
                }
                (problem("batch-notes-problem", problems.notes.as_deref()))
            }
            // Last, so the name and Notes arrive before it and are kept when the file is too big.
            @if adding {
                (recipe_field(problems.recipe.as_deref()))
            }
            @match purpose {
                Purpose::Add => button.button.large type="submit" { "Add Batch" },
                Purpose::Edit(number) => div.edit-actions {
                    button.button type="submit" { "Save Batch" }
                    a.button.secondary href=(paths::batch(number)) { "Cancel" }
                },
            }
        }
    }
}

/// The add form's Recipe file field, with `problem_message` beneath and a note on what it takes.
///
/// It accepts any file: phones label BeerXML exports inconsistently, so the server reads
/// whatever comes and says if it isn't BeerXML.
fn recipe_field(problem_message: Option<&str>) -> Markup {
    let refused = problem_message.is_some();
    html! {
        div.field {
            label for="batch-recipe" { "Recipe " span.optional { "(optional)" } }
            input #batch-recipe type="file" name="recipe"
                aria-invalid=[refused.then_some("true")]
                aria-describedby=[refused.then_some("batch-recipe-problem")];
            (problem("batch-recipe-problem", problem_message))
            p.note {
                "A BeerXML file from Brewfather, BeerSmith or similar. With a Recipe, the name can be left empty to take the Recipe's."
            }
        }
    }
}

/// The Batch page's Recipe: the Recipe viewer if `recipe` reads, a line saying so if it is
/// attached but doesn't, or that there's none; and beneath, the upload form.
#[must_use]
pub fn recipe_section(batch_number: BatchNumber, recipe: Option<&AttachedRecipe>) -> Markup {
    html! {
        section.section.narrow #recipe aria-labelledby="recipe-heading" {
            h2 #recipe-heading { "Recipe" }
            @match recipe {
                Some(AttachedRecipe::Readable(summary)) => (recipe_viewer(summary)),
                Some(AttachedRecipe::Unreadable) => p.muted {
                    "A Recipe file is attached, but it can't be read. Upload it again, or remove it."
                },
                None => p.muted {
                    "No Recipe yet. Attach a BeerXML file exported from Brewfather, BeerSmith or similar."
                },
            }
            (recipe_upload(batch_number, recipe.is_some(), None))
        }
    }
}

/// The form that attaches a Recipe to the Batch `number`, or replaces the one it `has`, with
/// `problem_message` under its file field; and, when it has one, the button that removes it after
/// asking.
///
/// htmx swaps a refused upload in place of the form; without htmx, it posts as a plain form.
#[must_use]
pub fn recipe_upload(
    batch_number: BatchNumber,
    has: bool,
    problem_message: Option<&str>,
) -> Markup {
    let action = paths::recipe(batch_number);
    let remove = paths::remove_recipe(batch_number);
    let refused = problem_message.is_some();
    html! {
        div.recipe-upload #(RECIPE_UPLOAD_ID) {
            form.entry-form method="post" action=(action) enctype="multipart/form-data"
                hx-post=(action) hx-target={ "#" (RECIPE_UPLOAD_ID) } hx-swap="outerHTML" {
                div.field {
                    label for="recipe-file" {
                        @if has { "Replace with another BeerXML file" } @else { "BeerXML file" }
                    }
                    // Any file, as on the add form.
                    input #recipe-file type="file" name="recipe" required
                        aria-invalid=[refused.then_some("true")]
                        aria-describedby=[refused.then_some("recipe-file-problem")];
                    (problem("recipe-file-problem", problem_message))
                }
                button.button.secondary.small type="submit" {
                    @if has { "Replace Recipe" } @else { "Attach Recipe" }
                }
            }
            @if has {
                form.recipe-remove method="post" action=(remove) hx-post=(remove)
                    hx-confirm="Remove the Recipe from this Batch? The file is gone for good." {
                    button.button.secondary.small type="submit" { "Remove Recipe" }
                }
            }
        }
    }
}

/// The Recipe viewer: a read-only view of `recipe` in metric units, readable on a phone.
///
/// It shows the Recipe's name, style, type, batch size, boil time and efficiency; its targets;
/// the style's ranges; the fermentables with each one's share of the grist; the hops; the
/// yeasts; and the mash steps. Whatever the file leaves out, it leaves out too.
#[must_use]
pub fn recipe_viewer(recipe: &RecipeSummary) -> Markup {
    html! {
        div.recipe {
            p.recipe-name { (recipe.name) }
            @if let Some(style) = &recipe.style {
                p.recipe-style { (style_line(style)) }
            }
            (recipe_facts(recipe))
            (recipe_table(
                "Fermentables",
                &["Fermentable", "Amount", "Share"],
                recipe.fermentables.iter().map(|line| html! {
                    tr {
                        th scope="row" { (line.name) }
                        td { (or_dash(line.amount)) }
                        td { (or_dash(line.share)) }
                    }
                }),
            ))
            (recipe_table(
                "Hops",
                &["Hop", "Amount", "Alpha", "Use", "Time"],
                recipe.hops.iter().map(|hop| html! {
                    tr {
                        th scope="row" { (hop.name) (aside(hop.form.as_deref())) }
                        td { (or_dash(hop.amount)) }
                        td { (or_dash(hop.alpha)) }
                        td { (or_dash(hop.usage.as_ref())) }
                        td { (or_dash(hop.time)) }
                    }
                }),
            ))
            (yeasts(recipe))
            (recipe_table(
                "Mash",
                &["Step", "Temperature", "Time"],
                recipe.mash_steps.iter().map(|step| html! {
                    tr {
                        th scope="row" { (step.name) (aside(step.kind.as_deref())) }
                        td { (or_dash(step.temperature)) }
                        td { (or_dash(step.time)) }
                    }
                }),
            ))
        }
    }
}

/// The Recipe's facts, its targets and its style's ranges, each as name–value pairs, leaving
/// out whatever the file doesn't have.
fn recipe_facts(recipe: &RecipeSummary) -> Markup {
    let targets = &recipe.targets;
    let has_facts = recipe.kind.is_some()
        || recipe.batch_size.is_some()
        || recipe.boil_time.is_some()
        || recipe.efficiency.is_some();
    html! {
        @if has_facts {
            dl.recipe-facts {
                (fact("Type", recipe.kind.as_ref()))
                (fact("Batch size", recipe.batch_size))
                (fact("Boil time", recipe.boil_time))
                (fact("Efficiency", recipe.efficiency))
            }
        }
        @if *targets != Targets::default() {
            h3 { "Targets" }
            dl.recipe-facts {
                (fact("OG", targets.og))
                (fact("FG", targets.fg))
                (fact("ABV", targets.abv))
                (fact("IBU", targets.ibu))
                (fact("Colour", targets.colour))
            }
        }
        @if let Some(style) = recipe.style.as_ref().filter(|style| has_ranges(style)) {
            h3 { "Style ranges" }
            dl.recipe-facts {
                (fact("OG", style.og))
                (fact("FG", style.fg))
                (fact("IBU", style.ibu))
                (fact("Colour", style.colour))
            }
        }
    }
}

/// A table of the Recipe headed `heading`, with `columns` and `rows`, or nothing without rows.
fn recipe_table(
    heading: &str,
    columns: &[&str],
    rows: impl ExactSizeIterator<Item = Markup>,
) -> Markup {
    html! {
        @if rows.len() > 0 {
            h3 { (heading) }
            div.recipe-table-wrap {
                table.recipe-table {
                    thead {
                        tr {
                            @for column in columns {
                                th scope="col" { (column) }
                            }
                        }
                    }
                    tbody {
                        @for row in rows {
                            (row)
                        }
                    }
                }
            }
        }
    }
}

/// The yeasts, each with its laboratory, product and form beside it, or nothing without any.
fn yeasts(recipe: &RecipeSummary) -> Markup {
    html! {
        @if !recipe.yeasts.is_empty() {
            h3 { "Yeast" }
            ul.recipe-yeasts {
                @for yeast in &recipe.yeasts {
                    @let maker = [
                        yeast.laboratory.as_deref(),
                        yeast.product_id.as_deref(),
                        yeast.form.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                    li {
                        (yeast.name)
                        (aside((!maker.is_empty()).then_some(maker.as_str())))
                    }
                }
            }
        }
    }
}

/// `text` quieter beside a name, after a space, if there is any.
fn aside(text: Option<&str>) -> Markup {
    html! {
        @if let Some(text) = text {
            " " span.recipe-aside { (text) }
        }
    }
}

/// The style's name, then its category and guide: `Czech Premium Pale Lager · Pale Czech Lager
/// · BJCP 2015`.
fn style_line(style: &StyleSummary) -> String {
    [
        Some(style.name.as_str()),
        style.category.as_deref(),
        style.guide.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

/// Whether the file has any of the style's ranges.
const fn has_ranges(style: &StyleSummary) -> bool {
    style.og.is_some() || style.fg.is_some() || style.ibu.is_some() || style.colour.is_some()
}

/// A row of the Recipe's facts named `label`, if the file has its `value`.
fn fact<T: Display>(label: &str, value: Option<T>) -> Markup {
    html! {
        @if let Some(value) = value {
            dt { (label) }
            dd { (value) }
        }
    }
}

/// `value` in a table cell, or a dash where the file leaves it out.
fn or_dash<T: Display>(value: Option<T>) -> Markup {
    html! {
        @if let Some(value) = value {
            (value)
        } @else {
            span.recipe-aside { "–" }
        }
    }
}

/// A few Batches, a page of the log and refused forms, for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{
        BatchProblems, BatchValues, BottlingProblems, BottlingValues, BrewDayProblems,
        BrewDayValues,
    };
    use crate::domain::batches::fixtures::{batch, bottled, brewed, edited as edited_by};
    use crate::domain::batches::{Batch, BatchPage, NAME_MAX, NOTES_MAX};

    /// A Bottled Batch with every number in and Notes over several lines, edited since it was
    /// added. OG 1.050 and FG 1.010 give 5.3 % ABV and 80.0 % attenuation.
    #[must_use]
    pub fn pilsner() -> Batch {
        bottled(
            brewed(planned_pilsner(), "2026-10-03", Some("1.050"), Some("23.5")),
            "2026-10-17",
            Some("1.010"),
        )
    }

    /// The Pilsner before its Brew day.
    #[must_use]
    pub fn planned_pilsner() -> Batch {
        edited_by(
            batch(
                12,
                "Únětický Vzor 10.7°",
                "Saaz only, open fermenter.\nDry hopped on day 4.\n\nTasted grassy at bottling.",
                "Josef",
                "2026-10-24T18:05",
            ),
            "Šimon",
            "2026-10-26T20:30",
        )
    }

    /// The Pilsner in the fermenter, with an FG read before Bottling.
    #[must_use]
    pub fn fermenting_pilsner() -> Batch {
        let mut batch = brewed(planned_pilsner(), "2026-10-03", Some("1.050"), Some("23.5"));
        batch.bottling.fg = crate::domain::batches::Gravity::fg("1.014").ok();
        batch
    }

    /// A Batch with just a name.
    #[must_use]
    pub fn bare() -> Batch {
        batch(11, "Stout", "", "Ondra", "2026-10-10T09:15")
    }

    /// A Batch in the fermenter whose name is long enough to wrap on a phone, with a word that
    /// has nowhere to break. Only its brew date is in.
    #[must_use]
    pub fn long() -> Batch {
        brewed(
            batch(
                9,
                "Christmas special with honey, orange peel and Nejnezapomenutelnějšípivo",
                "",
                "Šimon",
                "2026-09-02T17:40",
            ),
            "2026-11-07",
            None,
            None,
        )
    }

    /// Nothing logged.
    #[must_use]
    pub const fn empty() -> BatchPage {
        BatchPage {
            batches: Vec::new(),
            page: 1,
            pages: 1,
        }
    }

    /// Three Batches, newest first, on the only page.
    #[must_use]
    pub fn log() -> BatchPage {
        BatchPage {
            batches: vec![pilsner(), bare(), long()],
            page: 1,
            pages: 1,
        }
    }

    /// The middle of three pages.
    #[must_use]
    pub fn later_page() -> BatchPage {
        BatchPage {
            batches: vec![long()],
            page: 2,
            pages: 3,
        }
    }

    /// A form refused for a blank name, Notes over the limit and a file that isn't BeerXML, with
    /// the typed line break kept.
    #[must_use]
    pub fn refused() -> (BatchValues<'static>, BatchProblems) {
        (
            BatchValues {
                name: " ",
                notes: "Mashed at 66 °C.\nToo long after this.",
            },
            BatchProblems {
                name: Some(
                    "Give the Batch a name, or attach a Recipe to take its name from.".to_owned(),
                ),
                notes: Some(format!(
                    "Keep the Notes to {NOTES_MAX} characters or fewer."
                )),
                recipe: Some(
                    "This isn't a BeerXML file. Export the recipe as BeerXML and try again."
                        .to_owned(),
                ),
            },
        )
    }

    /// A Brew day refused for an OG out of range and a Volume of zero, with what was typed kept.
    #[must_use]
    pub fn refused_brew_day() -> (BrewDayValues, BrewDayProblems) {
        (
            BrewDayValues {
                brew_date: "2026-10-03".to_owned(),
                og: "1.250".to_owned(),
                volume: "0".to_owned(),
            },
            BrewDayProblems {
                brew_date: None,
                og: Some("Enter an OG from 1.000 to 1.200, like 1.052.".to_owned()),
                volume: Some("The Volume has to be more than 0 L.".to_owned()),
            },
        )
    }

    /// A Bottling refused for a date before the brew date and an FG not below the OG.
    #[must_use]
    pub fn refused_bottling() -> (BottlingValues, BottlingProblems) {
        (
            BottlingValues {
                bottling_date: "2026-10-01".to_owned(),
                fg: "1,060".to_owned(),
            },
            BottlingProblems {
                bottling_date: Some("The bottling date can't be before the brew date.".to_owned()),
                fg: Some("The FG has to be below the OG.".to_owned()),
            },
        )
    }

    /// An edit refused for a name over the limit, with the Notes typed kept.
    #[must_use]
    pub fn refused_edit() -> (BatchValues<'static>, BatchProblems) {
        (
            BatchValues {
                name: "Únětický Vzor 10.7° with a name that goes on and on",
                notes: "Saaz only.",
            },
            BatchProblems {
                name: Some(format!("Keep the name to {NAME_MAX} characters or fewer.")),
                notes: None,
                recipe: None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::fixtures::{
        bare, empty, fermenting_pilsner, later_page, log, long, pilsner, planned_pilsner, refused,
        refused_bottling, refused_brew_day, refused_edit,
    };
    use super::*;
    use crate::domain::batches::recipe_fixtures;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(
            Purpose::Add,
            BatchValues::default(),
            &BatchProblems::default(),
        )
    }

    fn no_slots() -> Markup {
        crate::views::slots::slots_section(
            &[],
            &crate::views::slots::plan_form("", None, crate::views::slots::fixtures::TODAY),
        )
    }

    #[test]
    fn batches_with_slots() {
        use crate::views::slots::{fixtures, plan_form, slots_section};
        let slots = slots_section(&fixtures::several(), &plan_form("", None, fixtures::TODAY));
        assert_html_snapshot!(page(&slots, &empty_form(), &log()));
    }

    #[test]
    fn batches_empty() {
        assert_html_snapshot!(page(&no_slots(), &empty_form(), &empty()));
    }

    #[test]
    fn batches_log() {
        assert_html_snapshot!(page(&no_slots(), &empty_form(), &log()));
    }

    #[test]
    fn batches_later_page() {
        assert_html_snapshot!(page(&no_slots(), &empty_form(), &later_page()));
    }

    #[test]
    fn batches_with_a_refused_form() {
        let (values, problems) = refused();
        assert_html_snapshot!(page(
            &no_slots(),
            &form(Purpose::Add, values, &problems),
            &log()
        ));
    }

    #[test]
    fn batch_row() {
        assert_html_snapshot!(super::batch_row(&pilsner()));
    }

    #[test]
    fn batch_row_long() {
        assert_html_snapshot!(super::batch_row(&long()));
    }

    #[test]
    fn batch_page_with_notes_edited() {
        assert_html_snapshot!(batch_page(&pilsner(), None));
    }

    #[test]
    fn batch_page_without_notes() {
        assert_html_snapshot!(batch_page(&bare(), None));
    }

    #[test]
    fn batch_form_empty() {
        assert_html_snapshot!(empty_form());
    }

    #[test]
    fn batch_form_with_errors() {
        let (values, problems) = refused();
        assert_html_snapshot!(form(Purpose::Add, values, &problems));
    }

    #[test]
    fn batch_edit_page() {
        assert_html_snapshot!(edit_page(pilsner().number, &edit_form(&pilsner())));
    }

    #[test]
    fn batch_edit_page_with_errors() {
        let (values, problems) = refused_edit();
        let number = pilsner().number;
        assert_html_snapshot!(edit_page(
            number,
            &form(Purpose::Edit(number), values, &problems)
        ));
    }

    #[test]
    fn batch_gone_page() {
        assert_html_snapshot!(gone_page(BatchNumber::from_client(7)));
    }

    #[test]
    fn each_stage_has_its_badge_and_a_brewed_batch_is_in_the_fermenter() {
        assert_eq!(
            stage_badge(Stage::Planned).into_string(),
            r#"<span class="stage planned">Planned</span>"#
        );
        assert_eq!(
            stage_badge(Stage::Brewed).into_string(),
            r#"<span class="stage brewed">In the fermenter</span>"#
        );
        assert_eq!(
            stage_badge(Stage::Bottled).into_string(),
            r#"<span class="stage bottled">Bottled</span>"#
        );
    }

    #[test]
    fn a_row_shows_number_name_stage_and_leads_to_the_batch() {
        let html = super::batch_row(&planned_pilsner()).into_string();

        assert!(html.contains(r#"href="/batches/12""#), "{html}");
        assert!(html.contains(r#"<span class="batch-number">#12</span>"#));
        assert!(html.contains("Únětický Vzor 10.7°"));
        assert!(html.contains(r#"<span class="stage planned">Planned</span>"#));
        assert!(
            html.contains(
                r#"Added by Josef · <time datetime="2026-10-24T18:05">24 October 2026</time>"#
            ),
            "{html}"
        );
    }

    #[test]
    fn a_brewed_row_shows_its_dates_in_place_of_who_added_it() {
        let fermenting = super::batch_row(&long()).into_string();
        assert!(
            fermenting.contains(r#"<span class="stage brewed">In the fermenter</span>"#),
            "{fermenting}"
        );
        assert!(
            fermenting
                .contains(r#"Brewed <time datetime="2026-11-07">7 November 2026</time></span>"#)
        );
        assert!(!fermenting.contains("Added by"));

        let bottled = super::batch_row(&pilsner()).into_string();
        assert!(bottled.contains(
            r#"Brewed <time datetime="2026-10-03">3 October 2026</time> · Bottled <time datetime="2026-10-17">17 October 2026</time>"#
        ), "{bottled}");
    }

    #[test]
    fn a_bottled_batch_shows_its_numbers_with_abv_and_attenuation() {
        let html = batch_page(&pilsner(), None).into_string();

        assert!(
            html.contains(r#"<span class="stage bottled">Bottled</span>"#),
            "{html}"
        );
        assert!(html.contains(r#"<dt>OG</dt><dd><span class="measured">1.050</span></dd>"#));
        assert!(html.contains(r#"<dt>Volume</dt><dd><span class="measured">23.5 L</span></dd>"#));
        assert!(html.contains(r#"<dt>FG</dt><dd><span class="measured">1.010</span></dd>"#));
        assert!(html.contains(r#"<dt>ABV</dt><dd><span class="measured">5.3 %</span></dd>"#));
        assert!(
            html.contains(r#"<dt>Attenuation</dt><dd><span class="measured">80.0 %</span></dd>"#)
        );
        assert!(html.contains("Edit Brew day"));
        assert!(html.contains("Edit Bottling"));
    }

    #[test]
    fn a_planned_batch_has_nothing_recorded_yet() {
        let html = batch_page(&bare(), None).into_string();

        assert!(html.contains("Not brewed yet."), "{html}");
        assert!(html.contains("Not bottled yet."));
        assert!(html.contains(
            r##"href="/batches/11/brew-day" hx-get="/batches/11/brew-day" hx-target="#brew-day" hx-swap="outerHTML">Record Brew day"##
        ));
        assert!(html.contains(
            r##"href="/batches/11/bottling" hx-get="/batches/11/bottling" hx-target="#bottling" hx-swap="outerHTML">Record Bottling"##
        ));
    }

    #[test]
    fn an_fg_before_bottling_is_shown_with_the_abv_but_not_a_date() {
        let html = bottling_section(&fermenting_pilsner(), None).into_string();

        assert!(!html.contains("Bottled on"), "{html}");
        assert!(html.contains(r#"<span class="measured">1.014</span>"#));
        assert!(
            html.contains(r#"<span class="measured">4.7 %</span>"#),
            "{html}"
        );
        assert!(html.contains("Edit Bottling"));
    }

    #[test]
    fn a_target_sits_beside_what_was_measured() {
        let og = |typed| crate::domain::batches::Gravity::og(typed).ok();

        assert_eq!(
            target_vs_measured(og("1.048"), og("1.050")).into_string(),
            r#"<span class="measured">1.048</span> <span class="target">target 1.050</span>"#
        );
        assert_eq!(
            target_vs_measured(None, og("1.050")).into_string(),
            r#"<span class="measured missing">Not measured</span> <span class="target">target 1.050</span>"#
        );
        assert_eq!(
            target_vs_measured(og("1.048"), None).into_string(),
            r#"<span class="measured">1.048</span>"#
        );
    }

    #[test]
    fn the_forms_start_at_today_until_a_date_is_recorded() {
        let today = date(2026, 11, 10);

        assert_eq!(
            BrewDayValues::of(&bare(), today),
            BrewDayValues {
                brew_date: "2026-11-10".to_owned(),
                og: String::new(),
                volume: String::new(),
            }
        );
        assert_eq!(
            BrewDayValues::of(&pilsner(), today),
            BrewDayValues {
                brew_date: "2026-10-03".to_owned(),
                og: "1.050".to_owned(),
                volume: "23.5".to_owned(),
            }
        );
        assert_eq!(
            BottlingValues::of(&fermenting_pilsner(), today),
            BottlingValues {
                bottling_date: "2026-11-10".to_owned(),
                fg: "1.014".to_owned(),
            }
        );
    }

    #[test]
    fn batch_row_planned() {
        assert_html_snapshot!(super::batch_row(&planned_pilsner()));
    }

    #[test]
    fn batch_page_planned() {
        assert_html_snapshot!(batch_page(&planned_pilsner(), None));
    }

    #[test]
    fn batch_page_in_the_fermenter_with_an_early_fg() {
        assert_html_snapshot!(batch_page(&fermenting_pilsner(), None));
    }

    #[test]
    fn brew_day_form_empty() {
        let values = BrewDayValues::of(&bare(), date(2026, 11, 10));
        assert_html_snapshot!(brew_day_form(
            bare().number,
            &values,
            &BrewDayProblems::default()
        ));
    }

    #[test]
    fn brew_day_form_with_errors() {
        let (values, problems) = refused_brew_day();
        assert_html_snapshot!(brew_day_form(pilsner().number, &values, &problems));
    }

    #[test]
    fn bottling_form_filled_in() {
        let values = BottlingValues::of(&pilsner(), date(2026, 11, 10));
        assert_html_snapshot!(bottling_form(
            pilsner().number,
            &values,
            &BottlingProblems::default()
        ));
    }

    #[test]
    fn bottling_form_with_errors() {
        let (values, problems) = refused_bottling();
        assert_html_snapshot!(bottling_form(pilsner().number, &values, &problems));
    }

    #[test]
    fn numbers_page_without_htmx() {
        let (values, problems) = refused_bottling();
        let number = pilsner().number;
        assert_html_snapshot!(numbers_page(
            number,
            &bottling_form(number, &values, &problems)
        ));
    }

    #[test]
    fn the_batch_page_keeps_line_breaks_and_says_who_added_and_edited_it() {
        let html = batch_page(&pilsner(), None).into_string();

        assert!(html.contains("Batch #12"), "{html}");
        assert!(html.contains(
            "Saaz only, open fermenter.\nDry hopped on day 4.\n\nTasted grassy at bottling."
        ));
        assert!(html.contains(
            r#"Added by Josef · <time datetime="2026-10-24T18:05">Saturday 24 October 2026, 18:05</time>"#
        ), "{html}");
        assert!(html.contains("Edited by Šimon"));
        assert!(html.contains(r#"href="/batches/12/edit""#));
    }

    #[test]
    fn a_batch_never_edited_has_no_edited_line() {
        let html = batch_page(&bare(), None).into_string();

        assert!(html.contains("No Notes yet."), "{html}");
        assert!(!html.contains("Edited by"), "{html}");
    }

    #[test]
    fn the_edit_page_saves_the_batch_and_deletes_it_after_asking() {
        let html = edit_page(pilsner().number, &edit_form(&pilsner())).into_string();

        assert!(
            html.contains(r#"action="/batches/12" hx-post="/batches/12""#),
            "{html}"
        );
        assert!(html.contains(r#"value="Únětický Vzor 10.7°""#));
        assert!(html.contains("Dry hopped on day 4.\n\nTasted grassy at bottling.</textarea>"));
        assert!(html.contains(r#"action="/batches/12/delete" hx-post="/batches/12/delete""#));
        assert!(html.contains(r#"hx-confirm="Delete this Batch? It's gone for good.""#));
    }

    #[test]
    fn recipe_viewer_full() {
        assert_html_snapshot!(recipe_viewer(&recipe_fixtures::full()));
    }

    #[test]
    fn recipe_viewer_sparse() {
        assert_html_snapshot!(recipe_viewer(&recipe_fixtures::sparse()));
    }

    #[test]
    fn batch_page_with_a_recipe() {
        assert_html_snapshot!(batch_page(&pilsner(), Some(&recipe_fixtures::attached())));
    }

    #[test]
    fn recipe_upload_refused() {
        assert_html_snapshot!(recipe_upload(
            pilsner().number,
            true,
            Some("This BeerXML file has no recipe in it.")
        ));
    }

    #[test]
    fn the_viewer_shows_every_section_the_recipe_has() {
        let html = recipe_viewer(&recipe_fixtures::full()).into_string();

        for heading in [
            "<h3>Targets</h3>",
            "<h3>Style ranges</h3>",
            "<h3>Fermentables</h3>",
            "<h3>Hops</h3>",
            "<h3>Yeast</h3>",
            "<h3>Mash</h3>",
        ] {
            assert!(html.contains(heading), "{heading}: {html}");
        }
        assert!(html.contains("<dt>Efficiency</dt><dd>75.0 %</dd>"));
        assert!(html.contains("<dt>IBU</dt><dd>36 IBU</dd>"));
        assert!(html.contains("<dt>Colour</dt><dd>9 EBC</dd>"));
        assert!(html.contains("<dt>IBU</dt><dd>30–45 IBU</dd>"));
    }

    #[test]
    fn the_viewer_leaves_out_what_the_recipe_doesnt_have() {
        let html = recipe_viewer(&recipe_fixtures::sparse()).into_string();

        assert_eq!(
            html,
            r#"<div class="recipe"><p class="recipe-name">Just a name</p></div>"#
        );
    }

    #[test]
    fn the_batch_page_puts_the_recipes_targets_beside_what_was_measured() {
        let html = batch_page(&pilsner(), Some(&recipe_fixtures::attached())).into_string();

        assert!(html.contains(
            r#"<dt>OG</dt><dd><span class="measured">1.050</span> <span class="target">target 1.043</span></dd>"#
        ), "{html}");
        assert!(html.contains(
            r#"<dt>FG</dt><dd><span class="measured">1.010</span> <span class="target">target 1.010</span></dd>"#
        ));
        assert!(html.contains(
            r#"<dt>ABV</dt><dd><span class="measured">5.3 %</span> <span class="target">target 4.4 %</span></dd>"#
        ));
        assert!(html.contains("Replace Recipe"));
    }

    #[test]
    fn a_planned_batch_with_a_recipe_shows_the_targets_still_to_measure() {
        let html = batch_page(&bare(), Some(&recipe_fixtures::attached())).into_string();

        assert!(html.contains("Not brewed yet."), "{html}");
        assert!(html.contains(
            r#"<dt>OG</dt><dd><span class="measured missing">Not measured</span> <span class="target">target 1.043</span></dd>"#
        ), "{html}");
        assert!(!html.contains("<dt>Volume</dt>"));
        assert!(!html.contains("<dt>Attenuation</dt>"));
    }

    #[test]
    fn a_batch_without_a_recipe_offers_to_attach_one() {
        let html = batch_page(&bare(), None).into_string();

        assert!(html.contains("No Recipe yet."), "{html}");
        assert!(html.contains(
            r##"action="/batches/11/recipe" enctype="multipart/form-data" hx-post="/batches/11/recipe" hx-target="#recipe-upload""##
        ), "{html}");
        assert!(html.contains("Attach Recipe"));
        assert!(!html.contains("Remove Recipe"));
    }

    #[test]
    fn an_unreadable_recipe_says_it_is_attached() {
        let html = batch_page(&bare(), Some(&AttachedRecipe::Unreadable)).into_string();

        assert!(
            html.contains("A Recipe file is attached, but it can't be read."),
            "{html}"
        );
        assert!(html.contains(r#"action="/batches/11/recipe/delete""#));
    }

    #[test]
    fn the_add_form_uploads_a_recipe_and_lets_the_name_be_left_empty() {
        let add = empty_form().into_string();
        assert!(add.contains(r#"enctype="multipart/form-data""#), "{add}");
        assert!(add.contains(r#"<input id="batch-recipe" type="file" name="recipe""#));
        assert!(!add.contains("required"), "{add}");

        let edit = edit_form(&pilsner()).into_string();
        assert!(!edit.contains("enctype"), "{edit}");
        assert!(!edit.contains(r#"type="file""#));
        assert!(edit.contains(" required "), "{edit}");
    }

    #[test]
    fn pages_link_newer_and_older() {
        let html = page(&no_slots(), &empty_form(), &later_page()).into_string();

        assert!(html.contains(r#"rel="prev" href="/batches""#), "{html}");
        assert!(html.contains(r#"rel="next" href="/batches?page=3""#));
        assert!(html.contains("Page 2 of 3"));
    }

    #[test]
    fn the_empty_log_says_so() {
        let html = page(&no_slots(), &empty_form(), &empty()).into_string();

        assert!(html.contains("No Batches yet."), "{html}");
        assert!(!html.contains("batch-list"));
    }
}
