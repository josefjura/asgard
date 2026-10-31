use maud::{Markup, html};

use crate::domain::auth::{LINK_LIFETIME_HOURS, LinkKind};
use crate::domain::members::DisplayName;
use crate::views::layout::standalone;
use crate::views::passkey::{self, Ceremony, CeremonyUrls, Emphasis};

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
            (passkey::button("Create a passkey", Ceremony::Register, Emphasis::Primary, ceremony))
        },
    )
}

/// What a Device link opens: whose Crew it is, the Member it is for, and the button that
/// creates a passkey for them on this device.
#[must_use]
pub fn device_page(crew: &str, display_name: &DisplayName, ceremony: &CeremonyUrls) -> Markup {
    standalone(
        "Set up this device | Asgard",
        &html! {
            h1 { "Set up this device" }
            p.lead { "You'll sign in to " (crew) " as " strong { (display_name) } "." }
            p {
                "Create a passkey on this device: the fingerprint, face or screen lock you "
                "already use on it. Your other passkeys keep working."
            }
            (passkey::button("Create a passkey", Ceremony::Register, Emphasis::Primary, ceremony))
        },
    )
}

/// What an Invite or Device link opens on a device that is already signed in: who is signed
/// in, and the way to log out before the link can be used.
#[must_use]
pub fn signed_in(member: &DisplayName, kind: LinkKind, log_out: &str) -> Markup {
    let purpose = match kind {
        LinkKind::Invite => "This link is for someone joining the Crew.",
        LinkKind::Device => "This link sets up a new device for a Member of the Crew.",
    };
    standalone(
        "Already signed in | Asgard",
        &html! {
            h1 { "You're already signed in" }
            p.lead { "This device is signed in as " strong { (member) } "." }
            p {
                (purpose) " If it's meant for you on this device, log out first, then use the link."
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
        assert_html_snapshot!(signed_in(
            &name("Josef"),
            LinkKind::Invite,
            "/invite/abc/log-out"
        ));
    }

    #[test]
    fn device_link_page() {
        let urls = CeremonyUrls {
            start: "/device/abc/start".to_owned(),
            finish: "/device/abc/finish".to_owned(),
        };
        assert_html_snapshot!(device_page("Asgard", &name("Ondra"), &urls));
    }

    #[test]
    fn signed_in_with_a_device_link_says_what_the_link_is_for() {
        let html = signed_in(&name("Josef"), LinkKind::Device, "/device/abc/log-out").into_string();

        assert!(html.contains("This link sets up a new device for a Member of the Crew."));
        assert!(html.contains("action=\"/device/abc/log-out\""));
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
