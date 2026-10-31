//! Small inline icons; colour and stroke come from the stylesheet.

use maud::{Markup, html};

/// A right-pointing chevron, marking a row that leads somewhere.
#[must_use]
pub fn chevron() -> Markup {
    html! {
        svg.chevron viewBox="0 0 16 16" aria-hidden="true" focusable="false" {
            path d="M6 3l5 5-5 5" {}
        }
    }
}
