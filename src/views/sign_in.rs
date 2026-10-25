use maud::{Markup, html};

use crate::views::layout::document;

/// Where a signed-out visitor is sent. Signing in with a passkey arrives in a later ticket.
#[must_use]
pub fn placeholder() -> Markup {
    document(
        "Sign in | Asgard",
        &html! {
            main.standalone {
                h1 { "Sign in" }
                p { "Signing in with a passkey is on its way. To join the Crew, open the Invite link the Admin sent you." }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::views::assert_html_snapshot;

    #[test]
    fn sign_in_placeholder() {
        assert_html_snapshot!(super::placeholder());
    }
}
