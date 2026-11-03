//! The Quotes tab: a random Quote on top, a search box, "+ Quote" to open the form, and the
//! collection beneath, newest first.

use maud::{Markup, html};
use url::form_urlencoded;

use crate::domain::quotes::{AUTHOR_MAX, Author, Quote, QuoteId, QuotePage, Search, TEXT_MAX};
use crate::views::finance::problem;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};
use crate::views::time::{field_time, shown_time};

/// The id of the control that opens the add form, which the form and the button are swapped into.
const ADDER_ID: &str = "quote-adder";

/// The id of the collection, which adding a Quote swaps in afresh.
const LIST_ID: &str = "quote-list";

/// The id of the random-Quote card, which "Another one" swaps for a fresh one.
const RANDOM_ID: &str = "random-quote";

/// The id of the search box, which adding a Quote empties again.
const SEARCH_ID: &str = "quote-search";

/// Where the Quotes tab's form posts and where "+ Quote" asks for the form. Routes match on the
/// same paths.
pub mod paths {
    use crate::domain::quotes::QuoteId;

    /// The add form.
    pub const NEW: &str = "/quotes/new";
    /// A fresh random-Quote card.
    pub const RANDOM: &str = "/quotes/random";
    /// The collection filtered by a search, alone, for the search box to swap in.
    pub const SEARCH: &str = "/quotes/search";

    /// The Quote `id`: its row, and where its edit form saves.
    #[must_use]
    pub fn quote(id: QuoteId) -> String {
        format!("/quotes/{}", id.get())
    }

    /// The Quote `id`'s edit form.
    #[must_use]
    pub fn edit(id: QuoteId) -> String {
        format!("/quotes/{}/edit", id.get())
    }

    /// Where deleting the Quote `id` posts.
    #[must_use]
    pub fn delete(id: QuoteId) -> String {
        format!("/quotes/{}/delete", id.get())
    }
}

/// What the Quote form is for: adding a Quote, or editing the Quote it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Add,
    Edit(QuoteId),
}

impl Purpose {
    fn action(self) -> String {
        match self {
            Self::Add => Tab::Quotes.href().to_owned(),
            Self::Edit(id) => paths::quote(id),
        }
    }

    /// The id of the form's `field`, unique on the tab even while a Quote is edited beneath the
    /// form that adds one.
    fn field_id(self, field: &str) -> String {
        match self {
            Self::Add => format!("quote-{field}"),
            Self::Edit(id) => format!("quote-{}-edit-{field}", id.get()),
        }
    }
}

/// What the add form shows in its fields: what was typed, or nothing yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuoteValues<'a> {
    pub text: &'a str,
    pub author: &'a str,
}

/// Why each field of the add form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct QuoteProblems {
    pub text: Option<String>,
    pub author: Option<String>,
}

/// The tab showing what `search` finds at `page`: no query for the whole collection, and no page
/// for the first.
#[must_use]
pub fn href(search: &Search, page: usize) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    if !search.is_everything() {
        query.append_pair("q", search.as_str());
    }
    if page > 1 {
        query.append_pair("page", &page.to_string());
    }
    let query = query.finish();
    if query.is_empty() {
        Tab::Quotes.href().to_owned()
    } else {
        format!("{}?{query}", Tab::Quotes.href())
    }
}

/// The Quotes tab: the card showing `random`, the search box, then `adder`, which is "+ Quote" or
/// the add form, above one page of the collection or of what the search found.
#[must_use]
pub fn page(random: Option<&Quote>, adder: &Markup, list: &QuotePage) -> Markup {
    shell(
        Place::Tab(Tab::Quotes),
        &html! {
            h1 { "Quotes" }
            (random_card(random, false))
            (search_box(&list.search, false))
            div #(ADDER_ID) { (adder) }
            section.section aria-labelledby="collection" {
                h2 #collection { "Collection" }
                (quote_list(list, false))
            }
        },
    )
}

/// What htmx swaps in once a Quote is added: "+ Quote" again, and out of band the first page of
/// the whole collection, with the new Quote at the top, and an empty search box to match.
///
/// If it's the first Quote, the random-Quote card leaves its empty state to show it, out of band
/// too.
#[must_use]
pub fn added(list: &QuotePage) -> Markup {
    let first = match list.quotes.as_slice() {
        [only] if list.pages == 1 => Some(only),
        _ => None,
    };
    html! {
        (add_button())
        (quote_list(list, true))
        (search_box(&list.search, true))
        @if let Some(quote) = first {
            (random_card(Some(quote), true))
        }
    }
}

/// The random-Quote card: `quote`'s text with its line breaks and its Author, and "Another one".
///
/// "Another one" swaps in a fresh card with htmx and reloads the tab without it. Without a Quote,
/// the card says there's nothing to pick from and offers no other. Swapped in out of band if
/// `out_of_band`.
#[must_use]
pub fn random_card(quote: Option<&Quote>, out_of_band: bool) -> Markup {
    html! {
        section.card.random-quote #(RANDOM_ID) aria-label="A random Quote"
            hx-swap-oob=[out_of_band.then_some("true")] {
            @if let Some(quote) = quote {
                (random_figure(quote, false))
                a.button.secondary.small href=(Tab::Quotes.href()) hx-get=(paths::RANDOM)
                    hx-target={ "#" (RANDOM_ID) } hx-swap="outerHTML" { "Another one" }
            } @else {
                p.empty { "No Quotes to pick from yet. The first one you add shows up here." }
            }
        }
    }
}

/// The random-Quote card's text and Author. It's named for the Quote it shows, so saving an edit
/// to that Quote can swap in the new words out of band, and leave the card alone if it shows
/// another.
fn random_figure(quote: &Quote, out_of_band: bool) -> Markup {
    html! {
        figure #(random_figure_id(quote.id)) hx-swap-oob=[out_of_band.then_some("true")] {
            blockquote.random-quote-text { (quote.text.as_str()) }
            figcaption.random-quote-author { (quote.author.as_str()) }
        }
    }
}

fn random_figure_id(id: QuoteId) -> String {
    format!("random-quote-{}", id.get())
}

/// What the search box swaps in as the Member types: one page of what `list`'s search found, or of
/// the whole collection when nothing is typed.
#[must_use]
pub fn results(list: &QuotePage) -> Markup {
    quote_list(list, false)
}

/// The search box, showing `search`. With htmx it swaps in what it finds a moment after the
/// Member stops typing; without, it loads the tab with the search. Swapped in out of band if
/// `out_of_band`.
#[must_use]
pub fn search_box(search: &Search, out_of_band: bool) -> Markup {
    html! {
        form.quote-search #(SEARCH_ID) role="search" method="get" action=(Tab::Quotes.href())
            hx-get=(paths::SEARCH) hx-trigger="input delay:300ms, submit" hx-sync="this:replace"
            hx-target={ "#" (LIST_ID) } hx-swap="outerHTML"
            hx-swap-oob=[out_of_band.then_some("true")] {
            label for="quote-search-words" { "Search Quotes" }
            input #quote-search-words type="search" name="q" value=(search.as_str())
                autocomplete="off" placeholder="A few words or a name";
        }
    }
}

/// "+ Quote", which opens the add form in its place.
#[must_use]
pub fn add_button() -> Markup {
    html! {
        a.button.large href=(paths::NEW) hx-get=(paths::NEW)
            hx-target={ "#" (ADDER_ID) } hx-swap="innerHTML" { "+ Quote" }
    }
}

/// One page of the collection or of what a search found, newest first. Without any, a message
/// says there are no Quotes yet, or that nothing matched the search. Swapped in out of band if
/// `out_of_band`.
fn quote_list(list: &QuotePage, out_of_band: bool) -> Markup {
    html! {
        div #(LIST_ID) hx-swap-oob=[out_of_band.then_some("true")] {
            @if list.quotes.is_empty() {
                @if list.search.is_everything() {
                    p.empty { "No Quotes yet. Add one and the Crew sees it here." }
                } @else {
                    p.empty {
                        "No Quotes match “" (list.search.as_str()) "”. Try other words, or clear "
                        "the search to see them all."
                    }
                }
            } @else {
                ul.quote-list {
                    @for quote in &list.quotes {
                        (quote_row(quote))
                    }
                }
            }
            @if list.pages > 1 {
                nav.pages aria-label="Pages" {
                    @if list.page > 1 {
                        a.button.secondary.small rel="prev" href=(href(&list.search, list.page - 1)) { "Newer" }
                    }
                    span.pages-at { "Page " (list.page) " of " (list.pages) }
                    @if list.page < list.pages {
                        a.button.secondary.small rel="next" href=(href(&list.search, list.page + 1)) { "Older" }
                    }
                }
            }
        }
    }
}

/// A Quote: its text with line breaks kept, its Author, who added it when, in Prague time, and
/// the control that opens it in its edit form. A Quote nobody added has no "added by" line.
#[must_use]
pub fn quote_row(quote: &Quote) -> Markup {
    let time = quote.added_at.crew_time();
    let edit = paths::edit(quote.id);
    html! {
        li.quote #(row_id(quote.id)) {
            div.quote-main {
                p.quote-text { (quote.text.as_str()) }
                p.quote-author { (quote.author.as_str()) }
                p.quote-meta {
                    @if let Some(name) = &quote.added_by {
                        "Added by "
                        (name)
                        " · "
                    }
                    time datetime=(field_time(time)) { (shown_time(time)) }
                }
            }
            a.button.small.secondary href=(edit) hx-get=(edit)
                hx-target={ "#" (row_id(quote.id)) } hx-swap="outerHTML"
                aria-label={ "Edit the Quote by " (quote.author.as_str()) } { "Edit" }
        }
    }
}

/// The id of the Quote `id`'s row, which its edit form is swapped into and out of.
fn row_id(id: QuoteId) -> String {
    format!("quote-{}", id.get())
}

/// The Quote `id`'s row while it's edited: `form` in place of what it says.
#[must_use]
pub fn editing_row(id: QuoteId, form: &Markup) -> Markup {
    html! {
        li.quote.editing #(row_id(id)) { (form) }
    }
}

/// The Quote `id`'s row once it's gone: someone deleted it while it was on screen.
#[must_use]
pub fn gone_row(id: QuoteId) -> Markup {
    html! {
        li.quote.gone #(row_id(id)) role="status" {
            p.empty { "This Quote is gone: someone deleted it. Reload the page to see the latest Quotes." }
        }
    }
}

/// The page a Quote is edited on without htmx: `form`, which deletes it too.
#[must_use]
pub fn edit_page(form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Quotes),
        &html! {
            h1 { "Edit Quote" }
            (form)
        },
    )
}

/// What htmx swaps in once `quote` is saved: its row, showing what it says now. If the random-Quote
/// card shows it, the card shows the new words too.
#[must_use]
pub fn saved(quote: &Quote) -> Markup {
    html! {
        (quote_row(quote))
        (random_figure(quote, true))
    }
}

/// What htmx swaps in once a Quote is deleted: nothing in place of its row, and out of band a
/// random-Quote card showing `random`, so the deleted one can't stay on it.
///
/// Without `random` there are no Quotes left, and the collection says so too.
#[must_use]
pub fn deleted(random: Option<&Quote>) -> Markup {
    let nothing_left = QuotePage {
        search: Search::everything(),
        quotes: Vec::new(),
        page: 1,
        pages: 1,
    };
    html! {
        (random_card(random, true))
        @if random.is_none() {
            (quote_list(&nothing_left, true))
        }
    }
}

/// A Quote's edit form, filled in with what it says now and suggesting each of `authors`.
#[must_use]
pub fn edit_form(quote: &Quote, authors: &[Author]) -> Markup {
    let values = QuoteValues {
        text: quote.text.as_str(),
        author: quote.author.as_str(),
    };
    form(
        Purpose::Edit(quote.id),
        values,
        &QuoteProblems::default(),
        authors,
    )
}

/// The form a Quote is added or edited with, as `purpose` says.
///
/// It shows `values` and, beside each field, what was wrong with it. The Author field suggests
/// each of `authors`. Without htmx, it posts as a plain form.
///
/// Adding, htmx swaps whatever the server answers with into the place "+ Quote" was. Editing, it
/// swaps the answer in place of the Quote's row; the form also cancels back to the row and deletes
/// the Quote after asking.
#[must_use]
pub fn form(
    purpose: Purpose,
    values: QuoteValues<'_>,
    problems: &QuoteProblems,
    authors: &[Author],
) -> Markup {
    let action = purpose.action();
    let id = |field: &str| purpose.field_id(field);
    let (text_id, text_problem) = (id("text"), id("text-problem"));
    let (author_id, author_problem) = (id("author"), id("author-problem"));
    let suggestions_id = id("authors");
    let (target, swap) = match purpose {
        Purpose::Add => (format!("#{ADDER_ID}"), "innerHTML"),
        Purpose::Edit(id) => (format!("#{}", row_id(id)), "outerHTML"),
    };
    html! {
        form.entry-form method="post" action=(action) hx-post=(action)
            hx-target=(target) hx-swap=(swap) {
            div.field {
                label for=(text_id) { "Quote" }
                textarea #(text_id) name="text" rows="4" required maxlength=(TEXT_MAX)
                    aria-invalid=[problems.text.is_some().then_some("true")]
                    aria-describedby=[problems.text.is_some().then_some(&text_problem)] {
                    (values.text)
                }
                (problem(&text_problem, problems.text.as_deref()))
            }
            div.field {
                label for=(author_id) { "Author" }
                input #(author_id) type="text" name="author" value=(values.author)
                    required maxlength=(AUTHOR_MAX) list=(suggestions_id)
                    aria-invalid=[problems.author.is_some().then_some("true")]
                    aria-describedby=[problems.author.is_some().then_some(&author_problem)];
                datalist #(suggestions_id) {
                    @for author in authors {
                        option value=(author.as_str()) {}
                    }
                }
                (problem(&author_problem, problems.author.as_deref()))
            }
            @match purpose {
                Purpose::Add => button.button.large type="submit" { "Add Quote" },
                Purpose::Edit(id) => (edit_actions(id)),
            }
        }
    }
}

/// The foot of the form editing the Quote `id`: Save; Cancel, which puts its row back unchanged;
/// and Delete, which asks first with htmx.
fn edit_actions(id: QuoteId) -> Markup {
    let row = paths::quote(id);
    let delete = paths::delete(id);
    html! {
        div.edit-actions {
            button.button type="submit" { "Save Quote" }
            a.button.secondary href=(Tab::Quotes.href()) hx-get=(row)
                hx-target={ "#" (row_id(id)) } hx-swap="outerHTML" { "Cancel" }
            button.button.danger type="submit" formaction=(delete) formnovalidate
                hx-post=(delete) hx-confirm="Delete this Quote? It's gone for good." {
                "Delete Quote"
            }
        }
    }
}

/// The add form and a few Quotes, for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{QuoteProblems, QuoteValues};
    use crate::domain::quotes::fixtures::quote;
    use crate::domain::quotes::{Author, Quote, QuotePage, Search, TEXT_MAX};

    /// Authors the fixture form suggests.
    ///
    /// # Panics
    ///
    /// If a fixture Author is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn authors() -> Vec<Author> {
        ["Josef", "Ondra", "Šimon"]
            .into_iter()
            .map(|name| Author::parse(name).expect("fixture authors are valid"))
            .collect()
    }

    /// A form refused for a Quote over the limit and a blank Author, with the typed line break
    /// kept.
    #[must_use]
    pub fn refused() -> (QuoteValues<'static>, QuoteProblems) {
        (
            QuoteValues {
                text: "He said\nit twice",
                author: " ",
            },
            QuoteProblems {
                text: Some(format!("Keep the Quote to {TEXT_MAX} characters or fewer.")),
                author: Some("Say who said it.".to_owned()),
            },
        )
    }

    /// An edit refused for a blank Quote, with the Author typed kept.
    #[must_use]
    pub fn refused_edit() -> (QuoteValues<'static>, QuoteProblems) {
        (
            QuoteValues {
                text: "  ",
                author: "Pepík",
            },
            QuoteProblems {
                text: Some("Write the Quote.".to_owned()),
                author: None,
            },
        )
    }

    /// A short Quote Josef added.
    #[must_use]
    pub fn remembered() -> Quote {
        quote(
            1,
            "To pivo je spíš polévka.",
            "Pepa",
            Some("Josef"),
            "2026-10-24T18:05",
        )
    }

    /// A Quote imported with no Member to name as Added by.
    #[must_use]
    pub fn imported() -> Quote {
        quote(
            2,
            "Dej tam ještě jeden chmel.",
            "The brewer next door",
            None,
            "2026-10-24T11:30",
        )
    }

    /// A Quote whose text breaks across lines.
    #[must_use]
    pub fn multiline() -> Quote {
        quote(
            3,
            "He tasted it.\nThen he poured it back.",
            "Ondra",
            Some("Šimon"),
            "2026-10-23T21:15",
        )
    }

    /// A Quote long enough to wrap on a phone, with a word that has nowhere to break.
    #[must_use]
    pub fn long() -> Quote {
        quote(
            4,
            concat!(
                "He said the mash tasted like biscuits and then kept talking about the water ",
                "until the kettle boiled over and nobody wrote down the temperature. ",
                "Nejnezapomenutelnějšívěta which runs on without a break so a phone has to wrap it."
            ),
            "Pepa",
            Some("Josef"),
            "2026-10-22T09:40",
        )
    }

    /// Nothing listed.
    #[must_use]
    pub fn empty() -> QuotePage {
        QuotePage {
            search: Search::everything(),
            quotes: Vec::new(),
            page: 1,
            pages: 1,
        }
    }

    /// Four Quotes, newest first, on the only page.
    #[must_use]
    pub fn collection() -> QuotePage {
        QuotePage {
            search: Search::everything(),
            quotes: vec![remembered(), imported(), multiline(), long()],
            page: 1,
            pages: 1,
        }
    }

    /// The middle of three pages.
    #[must_use]
    pub fn later_page() -> QuotePage {
        QuotePage {
            search: Search::everything(),
            quotes: vec![long()],
            page: 2,
            pages: 3,
        }
    }

    /// What searching for "pepa" found: the first of two pages.
    #[must_use]
    pub fn found() -> QuotePage {
        QuotePage {
            search: Search::new("pepa"),
            quotes: vec![remembered(), long()],
            page: 1,
            pages: 2,
        }
    }

    /// A search that found nothing.
    #[must_use]
    pub fn not_found() -> QuotePage {
        QuotePage {
            search: Search::new("polévka s knedlíkem"),
            quotes: Vec::new(),
            page: 1,
            pages: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{
        authors, collection, empty, found, imported, later_page, long, multiline, not_found,
        refused, refused_edit, remembered,
    };
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(
            Purpose::Add,
            QuoteValues::default(),
            &QuoteProblems::default(),
            &authors(),
        )
    }

    #[test]
    fn quotes_empty() {
        assert_html_snapshot!(page(None, &add_button(), &empty()));
    }

    #[test]
    fn quotes_collection() {
        assert_html_snapshot!(page(Some(&remembered()), &add_button(), &collection()));
    }

    #[test]
    fn quotes_later_page() {
        assert_html_snapshot!(page(Some(&long()), &add_button(), &later_page()));
    }

    #[test]
    fn quotes_search_results() {
        assert_html_snapshot!(page(Some(&remembered()), &add_button(), &found()));
    }

    #[test]
    fn quotes_search_no_matches() {
        assert_html_snapshot!(page(Some(&remembered()), &add_button(), &not_found()));
    }

    #[test]
    fn the_search_box_sits_between_the_card_and_the_adder_and_keeps_the_query() {
        let html = page(Some(&remembered()), &add_button(), &found()).into_string();

        let card = html.find(r#"id="random-quote""#).unwrap();
        let search = html.find(r#"id="quote-search""#).unwrap();
        let adder = html.find(r#"id="quote-adder""#).unwrap();
        assert!(card < search && search < adder, "{html}");
        assert!(html.contains(r#"name="q" value="pepa""#), "{html}");
        assert!(html.contains(r##"hx-get="/quotes/search" hx-trigger="input delay:300ms, submit" hx-sync="this:replace" hx-target="#quote-list" hx-swap="outerHTML""##), "{html}");
    }

    #[test]
    fn pages_of_a_search_keep_its_query() {
        let html = results(&found()).into_string();

        assert!(
            html.contains(r#"rel="next" href="/quotes?q=pepa&amp;page=2""#),
            "{html}"
        );
        assert_eq!(href(&Search::new("pepa"), 1), "/quotes?q=pepa");
        assert_eq!(
            href(&Search::new("přišel sem"), 3),
            "/quotes?q=p%C5%99i%C5%A1el+sem&page=3"
        );
        assert_eq!(href(&Search::everything(), 1), "/quotes");
    }

    #[test]
    fn a_search_that_finds_nothing_says_so() {
        let html = results(&not_found()).into_string();

        assert!(
            html.contains("No Quotes match “polévka s knedlíkem”. Try other words"),
            "{html}"
        );
        assert!(!html.contains("No Quotes yet."));
    }

    #[test]
    fn adding_empties_the_search_box_out_of_band() {
        let html = added(&collection()).into_string();

        assert!(
            html.contains(r#"id="quote-search" role="search""#),
            "{html}"
        );
        assert!(html.contains(r#"name="q" value="""#), "{html}");
    }

    #[test]
    fn random_quote_card() {
        assert_html_snapshot!(random_card(Some(&remembered()), false));
    }

    #[test]
    fn random_quote_card_multiline() {
        assert_html_snapshot!(random_card(Some(&multiline()), false));
    }

    #[test]
    fn random_quote_card_empty() {
        assert_html_snapshot!(random_card(None, false));
    }

    #[test]
    fn the_random_card_offers_another_one_swapping_only_itself() {
        let html = random_card(Some(&multiline()), false).into_string();

        assert!(
            html.contains(
                "He tasted it.
Then he poured it back."
            ),
            "{html}"
        );
        assert!(html.contains("Ondra"));
        assert!(html.contains(
            r##"<a class="button secondary small" href="/quotes" hx-get="/quotes/random" hx-target="#random-quote" hx-swap="outerHTML">Another one</a>"##
        ), "{html}");
    }

    #[test]
    fn the_empty_random_card_offers_no_other() {
        let html = random_card(None, false).into_string();

        assert!(html.contains("No Quotes to pick from yet."), "{html}");
        assert!(!html.contains("Another one"), "{html}");
    }

    #[test]
    fn adding_the_first_quote_fills_the_random_card_and_later_ones_leave_it() {
        let first = QuotePage {
            search: Search::everything(),
            quotes: vec![remembered()],
            page: 1,
            pages: 1,
        };
        let html = added(&first).into_string();
        assert!(
            html.contains(r#"id="random-quote" aria-label="A random Quote" hx-swap-oob="true""#),
            "{html}"
        );

        assert!(
            !added(&collection())
                .into_string()
                .contains(r#"id="random-quote""#)
        );
    }

    #[test]
    fn quote_row_with_added_by() {
        assert_html_snapshot!(quote_row(&remembered()));
    }

    #[test]
    fn quote_row_without_added_by() {
        assert_html_snapshot!(quote_row(&imported()));
    }

    #[test]
    fn quote_row_long() {
        assert_html_snapshot!(quote_row(&long()));
    }

    #[test]
    fn quote_row_multiline() {
        assert_html_snapshot!(quote_row(&multiline()));
    }

    #[test]
    fn quote_form_empty() {
        assert_html_snapshot!(empty_form());
    }

    #[test]
    fn quote_form_with_errors() {
        let (values, problems) = refused();
        assert_html_snapshot!(form(Purpose::Add, values, &problems, &authors()));
    }

    #[test]
    fn quote_form_editing() {
        assert_html_snapshot!(editing_row(
            remembered().id,
            &edit_form(&remembered(), &authors())
        ));
    }

    #[test]
    fn quote_form_edit_with_errors() {
        let (values, problems) = refused_edit();
        let id = remembered().id;
        assert_html_snapshot!(editing_row(
            id,
            &form(Purpose::Edit(id), values, &problems, &authors())
        ));
    }

    #[test]
    fn quote_edit_page() {
        assert_html_snapshot!(edit_page(&edit_form(&multiline(), &authors())));
    }

    #[test]
    fn each_quote_opens_its_edit_form_in_place() {
        let html = quote_row(&remembered()).into_string();

        assert!(
            html.starts_with(r#"<li class="quote" id="quote-1">"#),
            "{html}"
        );
        assert!(html.contains(
            r##"<a class="button small secondary" href="/quotes/1/edit" hx-get="/quotes/1/edit" hx-target="#quote-1" hx-swap="outerHTML" aria-label="Edit the Quote by Pepa">Edit</a>"##
        ), "{html}");
    }

    #[test]
    fn the_edit_form_is_filled_in_and_swaps_its_row() {
        let html = edit_form(&multiline(), &authors()).into_string();

        assert!(html.starts_with(
            r##"<form class="entry-form" method="post" action="/quotes/3" hx-post="/quotes/3" hx-target="#quote-3" hx-swap="outerHTML">"##
        ), "{html}");
        assert!(
            html.contains("He tasted it.\nThen he poured it back.</textarea>"),
            "{html}"
        );
        assert!(
            html.contains(r#"id="quote-3-edit-author" type="text" name="author" value="Ondra""#),
            "{html}"
        );
        assert!(html.contains(r#"list="quote-3-edit-authors""#));
        assert!(html.contains(
            r##"hx-get="/quotes/3" hx-target="#quote-3" hx-swap="outerHTML">Cancel</a>"##
        ));
        assert!(html.contains(r#"formaction="/quotes/3/delete" formnovalidate hx-post="/quotes/3/delete" hx-confirm="Delete this Quote? It's gone for good.">Delete Quote</button>"#), "{html}");
    }

    #[test]
    fn a_refused_edit_keeps_what_was_typed_beside_its_problem() {
        let (values, problems) = refused_edit();
        let id = remembered().id;
        let html =
            editing_row(id, &form(Purpose::Edit(id), values, &problems, &authors())).into_string();

        assert!(
            html.starts_with(r#"<li class="quote editing" id="quote-1"><form "#),
            "{html}"
        );
        assert!(html.contains(r#"aria-describedby="quote-1-edit-text-problem""#));
        assert!(html.contains("Write the Quote."));
        assert!(html.contains(r#"value="Pepík""#));
    }

    #[test]
    fn saving_swaps_the_row_and_the_card_if_it_shows_the_quote() {
        let html = saved(&remembered()).into_string();

        assert!(
            html.starts_with(r#"<li class="quote" id="quote-1">"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<figure id="random-quote-1" hx-swap-oob="true">"#),
            "{html}"
        );
        assert!(
            random_card(Some(&remembered()), false)
                .into_string()
                .contains(r#"<figure id="random-quote-1">"#)
        );
    }

    #[test]
    fn deleting_refreshes_the_card_and_the_last_one_empties_the_collection() {
        let html = deleted(Some(&imported())).into_string();
        assert!(
            html.starts_with(r#"<section class="card random-quote" id="random-quote" aria-label="A random Quote" hx-swap-oob="true">"#),
            "{html}"
        );
        assert!(html.contains("Dej tam ještě jeden chmel."));
        assert!(!html.contains("quote-list"), "{html}");

        let last = deleted(None).into_string();
        assert!(last.contains("No Quotes to pick from yet."), "{last}");
        assert!(
            last.contains(r#"<div id="quote-list" hx-swap-oob="true">"#),
            "{last}"
        );
        assert!(last.contains("No Quotes yet."));
    }

    #[test]
    fn a_gone_quote_says_so_in_its_row() {
        let html = gone_row(remembered().id).into_string();

        assert!(
            html.starts_with(r#"<li class="quote gone" id="quote-1" role="status">"#),
            "{html}"
        );
        assert!(html.contains("This Quote is gone: someone deleted it."));
    }

    #[test]
    fn a_quote_shows_its_author_and_who_added_it_when_in_prague() {
        let html = quote_row(&remembered()).into_string();

        assert!(html.contains("To pivo je spíš polévka."));
        assert!(html.contains("Pepa"));
        assert!(html.contains(
            r#"Added by Josef · <time datetime="2026-10-24T18:05">Saturday 24 October 2026, 18:05</time>"#
        ), "{html}");
    }

    #[test]
    fn a_quote_without_added_by_has_no_added_by_line() {
        let html = quote_row(&imported()).into_string();

        assert!(html.contains("Dej tam ještě jeden chmel."));
        assert!(html.contains("The brewer next door"));
        assert!(!html.contains("Added by"), "{html}");
        assert!(
            html.contains(
                r#"<time datetime="2026-10-24T11:30">Saturday 24 October 2026, 11:30</time>"#
            ),
            "{html}"
        );
    }

    #[test]
    fn a_quote_keeps_line_breaks() {
        let html = quote_row(&multiline()).into_string();

        assert!(
            html.contains("He tasted it.\nThen he poured it back."),
            "{html}"
        );
    }

    #[test]
    fn the_empty_collection_says_so_and_offers_to_add_one() {
        let html = page(None, &add_button(), &empty()).into_string();

        assert!(html.contains(">+ Quote</a>"));
        assert!(html.contains("No Quotes yet."));
        assert!(html.contains(r#"hx-get="/quotes/new""#));
        assert!(!html.contains("quote-text"));
    }

    #[test]
    fn pages_link_newer_and_older() {
        let html = page(Some(&long()), &add_button(), &later_page()).into_string();

        assert!(html.contains(r#"rel="prev" href="/quotes""#), "{html}");
        assert!(
            html.contains(r#"rel="next" href="/quotes?page=3""#),
            "{html}"
        );
        assert!(html.contains("Page 2 of 3"));
    }

    #[test]
    fn adding_puts_the_button_back_and_swaps_the_list_out_of_band() {
        let html = added(&collection()).into_string();

        assert!(html.contains(">+ Quote</a>"), "{html}");
        assert!(
            html.contains(r#"id="quote-list" hx-swap-oob="true""#),
            "{html}"
        );
        assert!(html.contains("To pivo je spíš polévka."));
    }

    #[test]
    fn the_author_field_suggests_each_author() {
        let html = empty_form().into_string();

        assert!(html.contains(r#"list="quote-authors""#), "{html}");
        assert!(
            html.contains(r#"<option value="Josef"></option>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<option value="Šimon"></option>"#),
            "{html}"
        );
    }
}
