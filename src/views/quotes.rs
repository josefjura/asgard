//! The Quotes tab: a random Quote on top, "+ Quote" to open the form, and the collection
//! beneath, newest first.

use maud::{Markup, html};

use crate::domain::quotes::{AUTHOR_MAX, Author, Quote, QuotePage, TEXT_MAX};
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

/// Where the Quotes tab's form posts and where "+ Quote" asks for the form. Routes match on the
/// same paths.
pub mod paths {
    /// The add form.
    pub const NEW: &str = "/quotes/new";
    /// A fresh random-Quote card.
    pub const RANDOM: &str = "/quotes/random";
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

/// The page of the collection at `page`, or the first page when `page` is the first.
fn href(page: usize) -> String {
    if page <= 1 {
        Tab::Quotes.href().to_owned()
    } else {
        format!("{}?page={page}", Tab::Quotes.href())
    }
}

/// The Quotes tab: the card showing `random`, then `adder`, which is "+ Quote" or the add form,
/// above one page of the collection.
#[must_use]
pub fn page(random: Option<&Quote>, adder: &Markup, list: &QuotePage) -> Markup {
    shell(
        Place::Tab(Tab::Quotes),
        &html! {
            h1 { "Quotes" }
            (random_card(random, false))
            div #(ADDER_ID) { (adder) }
            section.section aria-labelledby="collection" {
                h2 #collection { "Collection" }
                (quote_list(list, false))
            }
        },
    )
}

/// What htmx swaps in once a Quote is added: "+ Quote" again, and the first page of the
/// collection out of band, with the new Quote at the top.
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
                figure {
                    blockquote.random-quote-text { (quote.text.as_str()) }
                    figcaption.random-quote-author { (quote.author.as_str()) }
                }
                a.button.secondary.small href=(Tab::Quotes.href()) hx-get=(paths::RANDOM)
                    hx-target={ "#" (RANDOM_ID) } hx-swap="outerHTML" { "Another one" }
            } @else {
                p.empty { "No Quotes to pick from yet. The first one you add shows up here." }
            }
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

/// One page of the collection, newest first, or a message when there are no Quotes yet. Swapped
/// in out of band if `out_of_band`.
fn quote_list(list: &QuotePage, out_of_band: bool) -> Markup {
    html! {
        div #(LIST_ID) hx-swap-oob=[out_of_band.then_some("true")] {
            @if list.quotes.is_empty() {
                p.empty { "No Quotes yet. Add one and the Crew sees it here." }
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
                        a.button.secondary.small rel="prev" href=(href(list.page - 1)) { "Newer" }
                    }
                    span.pages-at { "Page " (list.page) " of " (list.pages) }
                    @if list.page < list.pages {
                        a.button.secondary.small rel="next" href=(href(list.page + 1)) { "Older" }
                    }
                }
            }
        }
    }
}

/// A Quote: its text with line breaks kept, its Author, and who added it when, in Prague time.
/// A Quote nobody added has no "added by" line.
#[must_use]
pub fn quote_row(quote: &Quote) -> Markup {
    let time = quote.added_at.crew_time();
    html! {
        li.quote {
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
    }
}

/// The form a Quote is added with.
///
/// It shows `values` and, beside each field, what was wrong with it. The Author field suggests
/// each of `authors`. Without htmx, it posts as a plain form. With htmx, it swaps in whatever
/// the server answers with, into the place "+ Quote" was.
#[must_use]
pub fn form(values: QuoteValues<'_>, problems: &QuoteProblems, authors: &[Author]) -> Markup {
    let action = Tab::Quotes.href();
    html! {
        form.entry-form method="post" action=(action) hx-post=(action)
            hx-target={ "#" (ADDER_ID) } hx-swap="innerHTML" {
            div.field {
                label for="quote-text" { "Quote" }
                textarea #quote-text name="text" rows="4" required maxlength=(TEXT_MAX)
                    aria-invalid=[problems.text.is_some().then_some("true")]
                    aria-describedby=[problems.text.is_some().then_some("quote-text-problem")] {
                    (values.text)
                }
                (problem("quote-text-problem", problems.text.as_deref()))
            }
            div.field {
                label for="quote-author" { "Author" }
                input #quote-author type="text" name="author" value=(values.author)
                    required maxlength=(AUTHOR_MAX) list="quote-authors"
                    aria-invalid=[problems.author.is_some().then_some("true")]
                    aria-describedby=[problems.author.is_some().then_some("quote-author-problem")];
                datalist #quote-authors {
                    @for author in authors {
                        option value=(author.as_str()) {}
                    }
                }
                (problem("quote-author-problem", problems.author.as_deref()))
            }
            button.button.large type="submit" { "Add Quote" }
        }
    }
}

/// The add form and a few Quotes, for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use super::{QuoteProblems, QuoteValues};
    use crate::domain::quotes::fixtures::quote;
    use crate::domain::quotes::{Author, Quote, QuotePage, TEXT_MAX};

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
    pub const fn empty() -> QuotePage {
        QuotePage {
            quotes: Vec::new(),
            page: 1,
            pages: 1,
        }
    }

    /// Four Quotes, newest first, on the only page.
    #[must_use]
    pub fn collection() -> QuotePage {
        QuotePage {
            quotes: vec![remembered(), imported(), multiline(), long()],
            page: 1,
            pages: 1,
        }
    }

    /// The middle of three pages.
    #[must_use]
    pub fn later_page() -> QuotePage {
        QuotePage {
            quotes: vec![long()],
            page: 2,
            pages: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{
        authors, collection, empty, imported, later_page, long, multiline, refused, remembered,
    };
    use super::*;
    use crate::views::assert_html_snapshot;

    fn empty_form() -> Markup {
        form(
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
            quotes: vec![remembered()],
            page: 1,
            pages: 1,
        };
        let html = added(&first).into_string();
        assert!(
            html.contains(r#"id="random-quote" aria-label="A random Quote" hx-swap-oob="true""#),
            "{html}"
        );

        assert!(!added(&collection()).into_string().contains("random-quote"));
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
        assert_html_snapshot!(form(values, &problems, &authors()));
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
