use maud::{Markup, html};

use crate::views::layout::shell;
use crate::views::nav::Place;
use crate::views::push;

/// Where the list of the Member's passkeys goes. The passkey management ticket fills it in.
pub const PASSKEYS_SLOT_ID: &str = "passkeys";

/// A Member's settings: their display name, their passkeys, notifications on this device, and
/// logging out of it, which posts to `log_out`.
#[must_use]
pub fn page(name_form: &Markup, log_out: &str) -> Markup {
    shell(
        Place::Settings,
        &html! {
            h1 { "Settings" }

            section.settings-section aria-labelledby="settings-name" {
                h2 #settings-name { "Your name" }
                (name_form)
            }

            section.settings-section aria-labelledby="settings-passkeys" {
                h2 #settings-passkeys { "Passkeys" }
                div.placeholder #(PASSKEYS_SLOT_ID) {
                    "Adding, renaming and deleting passkeys is coming soon."
                }
            }

            section.settings-section aria-labelledby="settings-push" {
                h2 #settings-push { "Notifications" }
                (push::control())
            }

            section.settings-section aria-labelledby="settings-device" {
                h2 #settings-device { "This device" }
                p { "Logging out signs this device out. Your passkey stays, so you can sign back in." }
                form method="post" action=(log_out) {
                    button.button type="submit" { "Log out" }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;
    use crate::views::display_name;

    #[test]
    fn settings_page() {
        assert_html_snapshot!(page(
            &display_name::form("/settings/name", "Ondra", None),
            "/log-out"
        ));
    }

    #[test]
    fn marks_the_places_for_passkeys_and_the_push_control() {
        let html = page(&html! {}, "/log-out").into_string();

        assert!(html.contains(&format!("id=\"{PASSKEYS_SLOT_ID}\"")));
        assert!(html.contains(&format!("id=\"{}\"", push::CONTROL_ID)));
    }

    #[test]
    fn marks_settings_as_the_current_page() {
        let html = page(&html! {}, "/log-out").into_string();

        assert!(html.contains("<title>Settings | Asgard</title>"));
        assert!(html.contains("href=\"/settings\" aria-label=\"Settings\" aria-current=\"page\""));
    }
}
