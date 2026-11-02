//! The Shopping tab: the form a Member adds an Item with, the open Items grouped by Urgency, and
//! those bought lately.

use maud::{Markup, html};

use crate::domain::shopping::{
    BoughtItem, ItemId, NAME_MAX, NOTE_MAX, OpenItem, RECENTLY_BOUGHT_FOR, Urgency,
};
use crate::views::finance::problem;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};
use crate::views::time::{field_time, shown_time};

/// The id of the open list, which adding, buying and undoing swap in afresh.
const OPEN_ITEMS_ID: &str = "open-items";

/// The id of Recently bought, which buying and undoing swap in afresh.
const RECENTLY_BOUGHT_ID: &str = "recently-bought";

/// Whether Recently bought shows its Items or only its heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    Folded,
    Unfolded,
}

/// Where the Shopping tab's buttons post. Routes match on the same paths.
pub mod paths {
    use crate::domain::shopping::ItemId;
    use crate::views::nav::Tab;

    /// Marks the open Item `id` bought.
    #[must_use]
    pub fn bought(id: ItemId) -> String {
        format!("{}/{}/bought", Tab::Shopping.href(), id.get())
    }

    /// Takes back marking the Item `id` bought.
    #[must_use]
    pub fn undo(id: ItemId) -> String {
        format!("{}/{}/undo", Tab::Shopping.href(), id.get())
    }
}

/// What the Item form shows in its fields: what was typed, or nothing yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct ItemValues<'a> {
    pub name: &'a str,
    pub note: &'a str,
    pub urgency: Urgency,
}

/// Why each field of the Item form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct ItemProblems {
    pub name: Option<String>,
    pub note: Option<String>,
    pub urgency: Option<String>,
}

/// How a page names `urgency`.
const fn label(urgency: Urgency) -> &'static str {
    match urgency {
        Urgency::Asap => "ASAP",
        Urgency::NextBrewday => "Next brewday",
        Urgency::NiceToHave => "Nice to have",
    }
}

/// The Shopping tab: `form` to add an Item above every `open` one, grouped by Urgency, and the
/// Items `bought` lately, folded away beneath.
#[must_use]
pub fn page(form: &Markup, open: &[OpenItem], bought: &[BoughtItem]) -> Markup {
    shell(
        Place::Tab(Tab::Shopping),
        &html! {
            h1 { "Shopping" }
            section.section aria-labelledby="new-item" {
                h2 #new-item { "Add an Item" }
                (form)
            }
            (open_list(open, false))
            (recently_bought_section(bought, Fold::Folded, false))
        },
    )
}

/// What htmx swaps in once an Item is marked bought or back: the `open` list in place of the old
/// one, and Recently bought out of band, folded as `fold`.
#[must_use]
pub fn changed_lists(open: &[OpenItem], bought: &[BoughtItem], fold: Fold) -> Markup {
    html! {
        (open_list(open, false))
        (recently_bought_section(bought, fold, true))
    }
}

/// What htmx swaps in once an Item is added: `form` emptied for the next one, and the open list
/// with the new Item in its group.
#[must_use]
pub fn added(form: &Markup, open: &[OpenItem]) -> Markup {
    html! {
        (form)
        (open_list(open, true))
    }
}

/// Every `open` Item, most urgent first, under a heading for each Urgency that has any; or a
/// message saying there's nothing to buy. Swapped in out of band if `out_of_band`.
fn open_list(open: &[OpenItem], out_of_band: bool) -> Markup {
    html! {
        div #(OPEN_ITEMS_ID) hx-swap-oob=[out_of_band.then_some("true")] {
            @if open.is_empty() {
                p.empty { "Nothing to buy. Add an Item and the Crew sees it here." }
            }
            @for urgency in Urgency::ALL {
                @let items: Vec<&OpenItem> = open.iter().filter(|item| item.urgency == urgency).collect();
                @if !items.is_empty() {
                    @let heading = format!("urgency-{}", urgency.key());
                    section.section aria-labelledby=(heading) {
                        h2 #(heading) { (label(urgency)) }
                        ul.item-list {
                            @for item in items {
                                (item_row(item))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// An open Item: its name, its note, who added it when, in Prague time, and the button that
/// marks it bought.
#[must_use]
pub fn item_row(item: &OpenItem) -> Markup {
    let time = item.added_at.crew_time();
    let name_id = name_id(item.id);
    html! {
        li.item #{ "item-" (item.id.get()) } {
            div.item-main {
                span.item-name #(name_id) { (item.name.as_str()) }
                @if let Some(note) = &item.note {
                    p.item-note { (note_text(note.as_str())) }
                }
                span.item-meta {
                    time datetime=(field_time(time)) { (shown_time(time)) }
                    " · " (item.added_by)
                }
            }
            (list_button(&paths::bought(item.id), "Bought", &name_id))
        }
    }
}

/// An Item bought lately: its name, who bought it when, in Prague time, and the button that takes
/// it back.
#[must_use]
pub fn bought_row(item: &BoughtItem) -> Markup {
    let time = item.bought_at.crew_time();
    let name_id = name_id(item.id);
    html! {
        li.item #{ "item-" (item.id.get()) } {
            div.item-main {
                span.item-name #(name_id) { (item.name.as_str()) }
                span.item-meta {
                    "Bought " time datetime=(field_time(time)) { (shown_time(time)) }
                    " · " (item.bought_by)
                }
            }
            (list_button(&paths::undo(item.id), "Undo", &name_id))
        }
    }
}

/// The id of the name of the Item `id`, which its button is described by.
fn name_id(id: ItemId) -> String {
    format!("item-{}-name", id.get())
}

/// A button on an Item row that posts to `action` and, with htmx, swaps in the lists it answers
/// with. Without htmx, it posts as a plain form.
fn list_button(action: &str, text: &str, name_id: &str) -> Markup {
    html! {
        form method="post" action=(action) hx-post=(action)
            hx-target={ "#" (OPEN_ITEMS_ID) } hx-swap="outerHTML" {
            button.button.small.secondary type="submit" aria-describedby=(name_id) { (text) }
        }
    }
}

/// Recently bought, folded as `fold`: the Items `bought` lately, newest first, or a message
/// saying nothing was. Swapped in out of band if `out_of_band`.
fn recently_bought_section(bought: &[BoughtItem], fold: Fold, out_of_band: bool) -> Markup {
    html! {
        details.section.recently-bought #(RECENTLY_BOUGHT_ID) open[fold == Fold::Unfolded]
            hx-swap-oob=[out_of_band.then_some("true")] {
            summary {
                h2 { "Recently bought" }
                @if !bought.is_empty() {
                    " " span.count { "(" (bought.len()) ")" }
                }
            }
            @if bought.is_empty() {
                p.muted { "Nothing bought in the last " (bought_days()) " days." }
            } @else {
                p.muted { "The last " (bought_days()) " days, newest first." }
                ul.item-list {
                    @for item in bought {
                        (bought_row(item))
                    }
                }
            }
        }
    }
}

/// Recently bought on its own, folded as `fold`, as the gallery shows it.
#[must_use]
pub fn recently_bought(bought: &[BoughtItem], fold: Fold) -> Markup {
    recently_bought_section(bought, fold, false)
}

/// How many days back Recently bought reaches.
const fn bought_days() -> u64 {
    RECENTLY_BOUGHT_FOR.as_secs() / (24 * 3600)
}

/// `note` as plain text, with each web address in it a link that opens in a new tab.
fn note_text(note: &str) -> Markup {
    html! {
        @for part in note_parts(note) {
            @match part {
                NotePart::Text(text) => (text),
                NotePart::Link(url) => a href=(url) target="_blank" rel="noopener noreferrer" { (url) },
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum NotePart<'a> {
    Text(&'a str),
    Link(&'a str),
}

/// `note` split into plain text and the `http` and `https` addresses in it. An address runs to
/// the next space; punctuation that ends a sentence, and a closing bracket it didn't open, stay
/// text.
fn note_parts(note: &str) -> Vec<NotePart<'_>> {
    let mut parts = Vec::new();
    let mut rest = note;
    while let Some(start) = next_address(rest) {
        let candidate = &rest[start..];
        let end = candidate
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
            .unwrap_or(candidate.len());
        let url = trim_address(&candidate[..end]);
        let scheme = if url.starts_with("https://") { 8 } else { 7 };
        if url.len() <= scheme {
            // A scheme with nothing after it is just text.
            parts.push(NotePart::Text(&rest[..start + scheme]));
            rest = &rest[start + scheme..];
            continue;
        }
        if start > 0 {
            parts.push(NotePart::Text(&rest[..start]));
        }
        parts.push(NotePart::Link(url));
        rest = &rest[start + url.len()..];
    }
    if !rest.is_empty() {
        parts.push(NotePart::Text(rest));
    }
    parts
}

/// Where the next `http://` or `https://` in `text` starts.
fn next_address(text: &str) -> Option<usize> {
    text.match_indices("http")
        .map(|(at, _)| at)
        .find(|&at| text[at..].starts_with("http://") || text[at..].starts_with("https://"))
}

/// `address` without the punctuation after it, such as the full stop of the sentence it ends.
fn trim_address(mut address: &str) -> &str {
    loop {
        let Some(last) = address.chars().last() else {
            return address;
        };
        let unmatched_bracket =
            last == ')' && address.matches(')').count() > address.matches('(').count();
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'') || unmatched_bracket {
            address = &address[..address.len() - last.len_utf8()];
        } else {
            return address;
        }
    }
}

/// The form an Item is added with.
///
/// It shows `values` and, beside each field, what was wrong with it. The note folds away until
/// it's opened, or has something in it. htmx swaps the form for the one the server answers with;
/// without htmx, it posts as a plain form.
#[must_use]
pub fn form(values: ItemValues, problems: &ItemProblems) -> Markup {
    let action = Tab::Shopping.href();
    let note_open = !values.note.trim().is_empty() || problems.note.is_some();
    html! {
        form.entry-form method="post" action=(action) hx-post=(action) hx-swap="outerHTML" {
            div.field {
                label for="item-name" { "What's needed" }
                input #item-name type="text" name="name" value=(values.name) required
                    maxlength=(NAME_MAX) autocomplete="off"
                    aria-invalid=[problems.name.is_some().then_some("true")]
                    aria-describedby=[problems.name.is_some().then_some("item-name-problem")];
                (problem("item-name-problem", problems.name.as_deref()))
            }
            fieldset.field.urgency-choice
                aria-describedby=[problems.urgency.is_some().then_some("item-urgency-problem")] {
                legend { "How soon" }
                div.urgency-options {
                    @for urgency in Urgency::ALL {
                        label.urgency-option {
                            input type="radio" name="urgency" value=(urgency.key())
                                checked[urgency == values.urgency];
                            (label(urgency))
                        }
                    }
                }
                (problem("item-urgency-problem", problems.urgency.as_deref()))
            }
            details.field.item-note-field open[note_open] {
                summary { "Add a note" }
                label for="item-note" { "Note " span.optional { "(optional)" } }
                textarea #item-note name="note" rows="3" maxlength=(NOTE_MAX)
                    aria-invalid=[problems.note.is_some().then_some("true")]
                    aria-describedby=[problems.note.is_some().then_some("item-note-problem")] {
                    (values.note)
                }
                (problem("item-note-problem", problems.note.as_deref()))
            }
            button.button.large type="submit" { "Add Item" }
        }
    }
}

/// Open Items in every group and a refused form, for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{ItemProblems, ItemValues};
    use crate::domain::shopping::fixtures::{bought as bought_item, open as item};
    use crate::domain::shopping::{BoughtItem, OpenItem, Urgency};

    /// Three Items bought lately, newest first.
    #[must_use]
    pub fn bought() -> Vec<BoughtItem> {
        vec![
            bought_item(9, "Bottle caps", "2026-10-24T08:30", "Šimon"),
            bought_item(8, "Irish moss", "2026-10-22T17:05", "Josef"),
            bought_item(
                10,
                "A spare tap for the fermenter, the one with the long spout",
                "2026-10-18T11:20",
                "Ondra",
            ),
        ]
    }

    /// A form refused for a blank name and a note over the limit.
    #[must_use]
    pub fn refused() -> (ItemValues<'static>, ItemProblems) {
        (
            ItemValues {
                name: "  ",
                note: "Two packs of Saaz,\nfrom the usual shop",
                urgency: Urgency::Asap,
            },
            ItemProblems {
                name: Some("Say what's needed.".to_owned()),
                note: Some("Keep the note to 500 characters or fewer.".to_owned()),
                urgency: None,
            },
        )
    }

    /// Open Items in all three groups, most urgent first, oldest first within each.
    #[must_use]
    pub fn open() -> Vec<OpenItem> {
        vec![
            item(
                1,
                "Star San",
                None,
                Urgency::Asap,
                "2026-10-20T08:00",
                "Ondra",
            ),
            item(
                4,
                "CO₂ bottle refill",
                Some("The 10 kg one"),
                Urgency::Asap,
                "2026-10-23T19:42",
                "Josef",
            ),
            item(
                2,
                "Pilsner malt",
                Some("25 kg\nhttps://www.example.cz/slady/pilsner?bag=25"),
                Urgency::NextBrewday,
                "2026-10-21T12:15",
                "Šimon",
            ),
            item(
                5,
                "Saaz",
                None,
                Urgency::NextBrewday,
                "2026-10-24T09:00",
                "Josef",
            ),
            item(
                3,
                "A new hydrometer",
                None,
                Urgency::NiceToHave,
                "2026-10-22T21:05",
                "Ondra",
            ),
        ]
    }

    /// Items with names and notes as long as they may get, and a word too long for a phone.
    #[must_use]
    pub fn long() -> Vec<OpenItem> {
        vec![
            item(
                6,
                &"Mangrove Jack's Bavarian Wheat yeast, or any other dry wheat yeast if they don't have it ".repeat(2),
                Some(&"Ask at the counter whether they can order more for next month. ".repeat(7)),
                Urgency::Asap,
                "2026-10-24T09:30",
                "Šimon",
            ),
            item(
                7,
                "Dezinfekčnípřípravekprorozlévárnupiva",
                Some("https://www.example.cz/a/very/long/address/that/goes/on/and/on/without/a/single/space/to/break/at"),
                Urgency::NiceToHave,
                "2026-10-24T09:45",
                "Josef",
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{bought, long, open, refused};
    use super::*;
    use crate::domain::shopping::fixtures::open as item;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(ItemValues::default(), &ItemProblems::default())
    }

    #[test]
    fn shopping_empty() {
        assert_html_snapshot!(page(&empty_form(), &[], &[]));
    }

    #[test]
    fn shopping_all_groups() {
        assert_html_snapshot!(page(&empty_form(), &open(), &bought()));
    }

    #[test]
    fn shopping_long_names_and_notes() {
        assert_html_snapshot!(page(&empty_form(), &long(), &[]));
    }

    #[test]
    fn open_item_row() {
        assert_html_snapshot!(item_row(&open()[2]));
    }

    #[test]
    fn bought_item_row() {
        assert_html_snapshot!(bought_row(&bought()[0]));
    }

    #[test]
    fn recently_bought_empty() {
        assert_html_snapshot!(recently_bought(&[], Fold::Folded));
    }

    #[test]
    fn recently_bought_with_items() {
        assert_html_snapshot!(recently_bought(&bought(), Fold::Unfolded));
    }

    #[test]
    fn an_open_item_is_marked_bought_in_place() {
        let html = item_row(&open()[0]).into_string();

        assert!(html.contains(
            r##"<form method="post" action="/shopping/1/bought" hx-post="/shopping/1/bought" hx-target="#open-items" hx-swap="outerHTML"><button class="button small secondary" type="submit" aria-describedby="item-1-name">Bought</button></form>"##
        ), "{html}");
        assert!(html.contains(r#"<span class="item-name" id="item-1-name">Star San</span>"#));
    }

    #[test]
    fn a_bought_item_shows_who_bought_it_when_in_prague_and_can_be_undone() {
        let html = bought_row(&bought()[0]).into_string();

        assert!(html.contains(
            r#"Bought <time datetime="2026-10-24T08:30">Saturday 24 October 2026, 08:30</time> · Šimon"#
        ), "{html}");
        assert!(html.contains(
            r##"action="/shopping/9/undo" hx-post="/shopping/9/undo" hx-target="#open-items""##
        ));
        assert!(html.contains(">Undo</button>"));
    }

    #[test]
    fn recently_bought_is_folded_away_on_the_tab() {
        let html = page(&empty_form(), &open(), &bought()).into_string();

        assert!(html.contains(r#"<details class="section recently-bought" id="recently-bought">"#));
        assert!(html.contains(r#"<h2>Recently bought</h2> <span class="count">(3)</span>"#));
        let open_list = html.find(r#"id="open-items""#).unwrap();
        assert!(open_list < html.find(r#"id="recently-bought""#).unwrap());
    }

    #[test]
    fn nothing_bought_says_so() {
        let html = recently_bought(&[], Fold::Folded).into_string();

        assert!(html.contains("Nothing bought in the last 7 days."));
        assert!(!html.contains("count"));
    }

    #[test]
    fn buying_swaps_in_the_open_list_and_recently_bought_out_of_band() {
        let html = changed_lists(&open(), &bought(), Fold::Folded).into_string();

        assert!(html.starts_with(r#"<div id="open-items">"#), "{html}");
        assert!(html.contains(
            r#"<details class="section recently-bought" id="recently-bought" hx-swap-oob="true">"#
        ));

        let html = changed_lists(&open(), &bought(), Fold::Unfolded).into_string();

        assert!(
            html.contains(r#"id="recently-bought" open hx-swap-oob="true">"#),
            "{html}"
        );
    }

    #[test]
    fn item_form_empty() {
        assert_html_snapshot!(empty_form());
    }

    #[test]
    fn item_form_with_errors() {
        let (values, problems) = refused();
        assert_html_snapshot!(form(values, &problems));
    }

    #[test]
    fn groups_most_urgent_first_and_skips_empty_groups() {
        let only_later = [item(
            1,
            "Saaz",
            None,
            Urgency::NiceToHave,
            "2026-10-20T08:00",
            "Josef",
        )];
        let html = page(&empty_form(), &only_later, &[]).into_string();

        assert!(html.contains(r#"<h2 id="urgency-nice-to-have">Nice to have</h2>"#));
        assert!(!html.contains("urgency-asap"));
        assert!(!html.contains("Nothing to buy"));

        let html = page(&empty_form(), &open(), &[]).into_string();
        let asap = html.find(">ASAP</h2>").unwrap();
        let next = html.find(">Next brewday</h2>").unwrap();
        let nice = html.find(">Nice to have</h2>").unwrap();
        assert!(asap < next && next < nice);
    }

    #[test]
    fn shows_who_added_an_item_and_when_in_prague() {
        let html = item_row(&open()[0]).into_string();

        assert!(html.contains(
            r#"<time datetime="2026-10-20T08:00">Tuesday 20 October 2026, 08:00</time> · Ondra"#
        ));
    }

    #[test]
    fn an_empty_list_says_theres_nothing_to_buy() {
        let html = page(&empty_form(), &[], &[]).into_string();

        assert!(html.contains("Nothing to buy."));
    }

    #[test]
    fn the_form_defaults_to_next_brewday_with_the_note_folded() {
        let html = empty_form().into_string();

        assert!(html.contains(r#"value="next-brewday" checked>"#));
        assert_eq!(html.matches(" checked").count(), 1);
        assert!(html.contains(r#"<details class="field item-note-field">"#));
    }

    #[test]
    fn a_refused_form_keeps_what_was_typed_and_opens_the_note() {
        let (values, problems) = refused();
        let html = form(values, &problems).into_string();

        assert!(html.contains(r#"value="asap" checked>"#));
        assert!(html.contains(">Two packs of Saaz,\nfrom the usual shop</textarea>"));
        assert!(html.contains(r#"<details class="field item-note-field" open>"#));
        assert!(html.contains("Say what's needed."));
    }

    #[test]
    fn adding_swaps_in_the_open_list_out_of_band() {
        let html = added(&empty_form(), &open()).into_string();

        assert!(html.starts_with("<form "));
        assert!(html.contains(r#"<div id="open-items" hx-swap-oob="true">"#));
    }

    #[test]
    fn a_link_in_a_note_opens_in_a_new_tab_and_the_rest_is_escaped() {
        let note = "<b>2 bags</b> from https://shop.example.cz/malt?a=1&b=2.";
        let row = item(
            1,
            "Malt",
            Some(note),
            Urgency::Asap,
            "2026-10-20T08:00",
            "Josef",
        );

        let html = item_row(&row).into_string();

        assert!(html.contains(
            r#"<p class="item-note">&lt;b&gt;2 bags&lt;/b&gt; from <a href="https://shop.example.cz/malt?a=1&amp;b=2" target="_blank" rel="noopener noreferrer">https://shop.example.cz/malt?a=1&amp;b=2</a>.</p>"#
        ), "{html}");
    }

    #[test]
    fn splits_a_note_into_text_and_addresses() {
        use NotePart::{Link, Text};

        for (note, parts) in [
            ("2 packs", vec![Text("2 packs")]),
            ("https://a.cz", vec![Link("https://a.cz")]),
            (
                "see http://a.cz, or https://b.cz/x!",
                vec![
                    Text("see "),
                    Link("http://a.cz"),
                    Text(", or "),
                    Link("https://b.cz/x"),
                    Text("!"),
                ],
            ),
            (
                "(https://a.cz/wiki/Malt_(grain))",
                vec![Text("("), Link("https://a.cz/wiki/Malt_(grain)"), Text(")")],
            ),
            (
                "line\nhttps://a.cz\nline",
                vec![Text("line\n"), Link("https://a.cz"), Text("\nline")],
            ),
            (
                "just https:// and http",
                vec![Text("just https://"), Text(" and http")],
            ),
            ("ftp://a.cz", vec![Text("ftp://a.cz")]),
        ] {
            assert_eq!(note_parts(note), parts, "{note:?}");
        }
    }
}
