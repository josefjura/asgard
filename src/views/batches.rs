//! The Batches tab, the Crew's brewery log: the form a Member adds a Batch with, every Batch
//! newest first, each Batch's own page, and the page it's edited or deleted on.

use maud::{Markup, html};

use crate::domain::batches::{Batch, BatchNumber, BatchPage, NAME_MAX, NOTES_MAX, Stage};
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
}

/// A Batch number as the Crew says it: `#12`.
#[must_use]
pub fn number(number: BatchNumber) -> String {
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

/// The Batches tab: `form` to add a Batch, and one page of the log, newest first. On a wide
/// screen the form sits beside the log.
#[must_use]
pub fn page(form: &Markup, log: &BatchPage) -> Markup {
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Batches" }
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

/// A Batch in the log: its number, name and Stage, and who added it on which day. It leads to
/// the Batch's own page.
#[must_use]
pub fn batch_row(batch: &Batch) -> Markup {
    let added = batch.added_at.crew_time();
    html! {
        li.batch {
            a.batch-link href=(paths::batch(batch.number)) {
                span.batch-number { (number(batch.number)) }
                span.batch-main {
                    span.batch-name { (batch.name.as_str()) }
                    span.batch-meta {
                        "Added by " (batch.added_by) " · "
                        time datetime=(field_time(added)) { (added.date().strftime("%-d %B %Y")) }
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

/// A Batch's own page: its number, name and Stage, its Notes with their line breaks, who added
/// and last edited it when, and the way to its edit page.
#[must_use]
pub fn batch_page(batch: &Batch) -> Markup {
    let added = batch.added_at.crew_time();
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            p.batch-eyebrow { "Batch " (number(batch.number)) }
            h1.batch-title { (batch.name.as_str()) }
            (stage_badge(batch.stage()))
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

/// The page a Batch is edited on: `form`, and at its foot the button that deletes the Batch
/// `number` after asking.
#[must_use]
pub fn edit_page(batch_number: BatchNumber, form: &Markup) -> Markup {
    let delete = paths::delete(batch_number);
    shell(
        Place::Tab(Tab::Batches),
        &html! {
            h1 { "Edit Batch " (number(batch_number)) }
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
            h1 { "Batch " (number(batch_number)) }
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
    html! {
        form.entry-form method="post" action=(action) hx-post=(action) hx-swap="outerHTML" {
            div.field {
                label for="batch-name" { "Name" }
                input #batch-name type="text" name="name" value=(values.name) required
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

/// A few Batches, a page of the log and refused forms, for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{BatchProblems, BatchValues};
    use crate::domain::batches::fixtures::{batch, edited as edited_by};
    use crate::domain::batches::{Batch, BatchPage, NAME_MAX, NOTES_MAX};

    /// A Batch with Notes over several lines, edited since it was added.
    #[must_use]
    pub fn pilsner() -> Batch {
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

    /// A Batch with just a name.
    #[must_use]
    pub fn bare() -> Batch {
        batch(11, "Stout", "", "Ondra", "2026-10-10T09:15")
    }

    /// A Batch whose name is long enough to wrap on a phone, with a word that has nowhere to
    /// break.
    #[must_use]
    pub fn long() -> Batch {
        batch(
            9,
            "Christmas special with honey, orange peel and Nejnezapomenutelnějšípivo",
            "",
            "Šimon",
            "2026-09-02T17:40",
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

    /// A form refused for a blank name and Notes over the limit, with the typed line break kept.
    #[must_use]
    pub fn refused() -> (BatchValues<'static>, BatchProblems) {
        (
            BatchValues {
                name: " ",
                notes: "Mashed at 66 °C.\nToo long after this.",
            },
            BatchProblems {
                name: Some("Give the Batch a name.".to_owned()),
                notes: Some(format!(
                    "Keep the Notes to {NOTES_MAX} characters or fewer."
                )),
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
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{bare, empty, later_page, log, long, pilsner, refused, refused_edit};
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(
            Purpose::Add,
            BatchValues::default(),
            &BatchProblems::default(),
        )
    }

    #[test]
    fn batches_empty() {
        assert_html_snapshot!(page(&empty_form(), &empty()));
    }

    #[test]
    fn batches_log() {
        assert_html_snapshot!(page(&empty_form(), &log()));
    }

    #[test]
    fn batches_later_page() {
        assert_html_snapshot!(page(&empty_form(), &later_page()));
    }

    #[test]
    fn batches_with_a_refused_form() {
        let (values, problems) = refused();
        assert_html_snapshot!(page(&form(Purpose::Add, values, &problems), &log()));
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
        assert_html_snapshot!(batch_page(&pilsner()));
    }

    #[test]
    fn batch_page_without_notes() {
        assert_html_snapshot!(batch_page(&bare()));
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
        let html = super::batch_row(&pilsner()).into_string();

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
    fn the_batch_page_keeps_line_breaks_and_says_who_added_and_edited_it() {
        let html = batch_page(&pilsner()).into_string();

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
        let html = batch_page(&bare()).into_string();

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
    fn pages_link_newer_and_older() {
        let html = page(&empty_form(), &later_page()).into_string();

        assert!(html.contains(r#"rel="prev" href="/batches""#), "{html}");
        assert!(html.contains(r#"rel="next" href="/batches?page=3""#));
        assert!(html.contains("Page 2 of 3"));
    }

    #[test]
    fn the_empty_log_says_so() {
        let html = page(&empty_form(), &empty()).into_string();

        assert!(html.contains("No Batches yet."), "{html}");
        assert!(!html.contains("batch-list"));
    }
}
