use maud::{Markup, html};

use crate::domain::auth::LINK_LIFETIME_HOURS;
use crate::domain::members::DisplayName;
use crate::views::layout::standalone;
use crate::views::passkey::{self, Ceremony, CeremonyUrls};

/// What an Invite link opens: whose Crew it is, the name the new Member will appear under, and
/// the button that creates their passkey.
#[must_use]
pub fn page(crew: &str, display_name: &DisplayName, ceremony: &CeremonyUrls) -> Markup {
    standalone(
        "Join the Crew | Asgard",
        &html! {
            h1 { "Join " (crew) }
            p.lead { "You'll appear in the Crew as " strong { (display_name) } "." }
            p {
                "Asgard signs you in with a passkey: the fingerprint, face or screen lock you "
                "already use on this device. There's no password to remember."
            }
            (passkey::button("Create a passkey", Ceremony::Register, ceremony))
        },
    )
}

/// What an Invite link opens on a device that is already signed in: who is signed in, and the
/// way to log out before the link can be used.
#[must_use]
pub fn signed_in(member: &DisplayName, log_out: &str) -> Markup {
    standalone(
        "Already signed in | Asgard",
        &html! {
            h1 { "You're already signed in" }
            p.lead { "This device is signed in as " strong { (member) } "." }
            p {
                "This link is for someone joining the Crew. If it's meant for you on this "
                "device, log out first, then use the link."
            }
            form method="post" action=(log_out) {
                button.button type="submit" { "Log out" }
            }
        },
    )
}

/// What a used, expired, revoked or unknown link opens, naming the Admin to ask once there
/// is one.
#[must_use]
pub fn link_error(admin: Option<&DisplayName>) -> Markup {
    standalone(
        "Link no longer works | Asgard",
        &html! {
            h1 { "This link no longer works" }
            p {
                "Each link works once, for " (LINK_LIFETIME_HOURS) " hours. This one has expired, "
                "was already used or was revoked."
            }
            p {
                "Ask "
                @if let Some(admin) = admin {
                    strong { (admin) } ", the Admin,"
                } @else {
                    "the Admin"
                }
                " to send you a new one."
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::members::fixtures::name;
    use crate::views::assert_html_snapshot;

    fn urls() -> CeremonyUrls {
        CeremonyUrls {
            start: "/invite/abc/start".to_owned(),
            finish: "/invite/abc/finish".to_owned(),
        }
    }

    #[test]
    fn invite_page() {
        assert_html_snapshot!(page("Asgard", &name("Ondra"), &urls()));
    }

    #[test]
    fn signed_in_page() {
        assert_html_snapshot!(signed_in(&name("Josef"), "/invite/abc/log-out"));
    }

    #[test]
    fn link_error_naming_the_admin() {
        assert_html_snapshot!(link_error(Some(&name("Josef"))));
    }

    #[test]
    fn link_error_before_there_is_an_admin() {
        assert_html_snapshot!(link_error(None));
    }

    #[test]
    fn escapes_the_display_name() {
        let html = page("Asgard", &name("<b>Ondra</b>"), &urls()).into_string();

        assert!(html.contains("&lt;b&gt;Ondra&lt;/b&gt;"));
    }
}
