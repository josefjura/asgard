use maud::{Markup, html};

use crate::views::layout::shell;
use crate::views::nav::Tab;

/// A tab's page, empty until its feature lands.
#[must_use]
pub fn page(tab: Tab) -> Markup {
    shell(
        tab,
        &html! {
            h1 { (tab.label()) }
            p.empty { (empty_text(tab)) }
        },
    )
}

const fn empty_text(tab: Tab) -> &'static str {
    match tab {
        Tab::Finance => "Expenses, Payments and everyone's Balance will show up here.",
        Tab::Shopping => "The Crew's Shopping list will show up here.",
        Tab::Reminders => "Reminders for the whole Crew will show up here.",
        Tab::Quotes => "The Crew's collection of Quotes will show up here.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn tab_page() {
        for tab in Tab::ALL {
            assert_html_snapshot!(
                format!("tab_page_{}", tab.label().to_lowercase()),
                page(tab)
            );
        }
    }
}
