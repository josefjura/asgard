use maud::{Markup, html};

use crate::views::layout::standalone;
use crate::views::passkey::{self, Ceremony, CeremonyUrls, Emphasis};

/// Where a signed-out visitor is sent: one button, and no username, since the passkey says who
/// the Member is.
#[must_use]
pub fn page(ceremony: &CeremonyUrls) -> Markup {
    standalone(
        "Sign in | Asgard",
        &html! {
            h1 { "Sign in" }
            p.lead { "Welcome back to the Crew." }
            (passkey::button("Sign in with passkey", Ceremony::SignIn, Emphasis::Primary, ceremony))
            p { "New here? Open the Invite link the Admin sent you." }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn sign_in_page() {
        assert_html_snapshot!(page(&CeremonyUrls {
            start: "/sign-in/start".to_owned(),
            finish: "/sign-in/finish".to_owned(),
        }));
    }
}
