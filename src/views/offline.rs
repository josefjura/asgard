use maud::{Markup, html};

use crate::views::layout::document;

/// What the service worker shows when a page can't load for lack of network.
#[must_use]
pub fn page() -> Markup {
    document(
        "Offline | Asgard",
        &html! {
            main.offline {
                h1 { "You're offline" }
                p { "Asgard needs a connection to load the Crew's data. Check your connection, then try again." }
                p { a href="/" { "Try again" } }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::views::assert_html_snapshot;

    #[test]
    fn offline_page() {
        assert_html_snapshot!(super::page());
    }
}
