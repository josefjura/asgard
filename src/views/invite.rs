use maud::{Markup, html};

use crate::assets;
use crate::domain::members::DisplayName;
use crate::views::layout::document;

/// Where the passkey script sends the two halves of the registration ceremony.
pub struct CeremonyUrls {
    pub start: String,
    pub finish: String,
}

/// What an Invite link opens: whose Crew it is, the name the new Member will appear under, and
/// the button that creates their passkey.
#[must_use]
pub fn page(crew: &str, display_name: &DisplayName, ceremony: &CeremonyUrls) -> Markup {
    document(
        "Join the Crew | Asgard",
        &html! {
            main.standalone {
                h1 { "Join " (crew) }
                p.lead { "You'll appear in the Crew as " strong { (display_name) } "." }
                p {
                    "Asgard signs you in with a passkey: the fingerprint, face or screen lock you "
                    "already use on this device. There's no password to remember."
                }
                button.button type="button" data-passkey-start=(ceremony.start)
                    data-passkey-finish=(ceremony.finish) aria-describedby="passkey-status" {
                    "Create a passkey"
                }
                p.status #passkey-status role="status" {}
                script type="module" src=(assets::PASSKEY.url) {}
            }
        },
    )
}

/// What a used, expired or unknown link opens, naming the Admin to ask once there is one.
#[must_use]
pub fn link_error(admin: Option<&DisplayName>) -> Markup {
    document(
        "Link no longer works | Asgard",
        &html! {
            main.standalone {
                h1 { "This link no longer works" }
                p { "Each link works once, for 24 hours. This one has expired or was already used." }
                p {
                    "Ask "
                    @if let Some(admin) = admin {
                        strong { (admin) } ", the Admin,"
                    } @else {
                        "the Admin"
                    }
                    " to send you a new one."
                }
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
