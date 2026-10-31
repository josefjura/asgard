use maud::{Markup, html};

use crate::views::icon::chevron;
use crate::views::layout::shell;
use crate::views::nav::Place;
use crate::views::passkey::{self, Ceremony, CeremonyUrls, Emphasis};
use crate::views::push;

/// A Member's settings page.
///
/// It holds their display name, their passkeys and a button that adds one through
/// `add_passkey`, notifications on this device, the way to the `admin` page for the Admin
/// only, and logging out of this device, which posts to `log_out`.
#[must_use]
pub fn page(
    name_form: &Markup,
    passkeys: &Markup,
    add_passkey: &CeremonyUrls,
    admin: Option<&str>,
    log_out: &str,
) -> Markup {
    shell(
        Place::Settings,
        &html! {
            h1 { "Settings" }

            // The name field's label heads its section.
            section.section.narrow.label-heads {
                (name_form)
            }

            section.section.narrow aria-labelledby="settings-passkeys" {
                h2 #settings-passkeys { "Passkeys" }
                (passkeys)
                p.muted {
                    "Add a passkey on each device you sign in from, unless your passkeys already sync to it."
                }
                (passkey::button("Add a passkey", Ceremony::Register, Emphasis::Secondary, add_passkey))
            }

            section.section.narrow aria-labelledby="settings-push" {
                h2 #settings-push { "Notifications" }
                (push::control())
            }

            @if let Some(admin) = admin {
                section.section.narrow aria-labelledby="settings-admin" {
                    h2 #settings-admin { "Admin" }
                    a.row-link href=(admin) { "Invite links" (chevron()) }
                }
            }

            section.section.narrow aria-labelledby="settings-device" {
                h2 #settings-device { "This device" }
                p.muted { "Logging out signs this device out. Your passkey stays, so you can sign back in." }
                form method="post" action=(log_out) {
                    button.button.secondary type="submit" { "Log out" }
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
    use crate::views::nav::ADMIN_HREF;
    use crate::views::passkeys::{self, fixtures};

    const BASE: &str = "/settings/passkeys";

    fn add_passkey() -> CeremonyUrls {
        CeremonyUrls {
            start: format!("{BASE}/start"),
            finish: format!("{BASE}/finish"),
        }
    }

    fn with_passkeys(passkeys: &[crate::domain::auth::PasskeySummary]) -> Markup {
        page(
            &display_name::form("/settings/name", "Ondra", None),
            &passkeys::list(BASE, passkeys, None),
            &add_passkey(),
            None,
            "/log-out",
        )
    }

    #[test]
    fn settings_page_for_the_admin() {
        assert_html_snapshot!(page(
            &display_name::form("/settings/name", "Josef", None),
            &passkeys::list(BASE, &fixtures::one(), None),
            &add_passkey(),
            Some(ADMIN_HREF),
            "/log-out",
        ));
    }

    #[test]
    fn only_the_admin_is_shown_the_way_to_the_admin_page() {
        let member = with_passkeys(&fixtures::one()).into_string();

        assert!(!member.contains("href=\"/admin\""));
        assert!(!member.contains("<h2 id=\"settings-admin\">"));
    }

    #[test]
    fn settings_page() {
        assert_html_snapshot!(with_passkeys(&fixtures::one()));
    }

    #[test]
    fn settings_page_with_several_passkeys() {
        assert_html_snapshot!(with_passkeys(&fixtures::several()));
    }

    #[test]
    fn offers_to_add_a_passkey() {
        let html = with_passkeys(&fixtures::one()).into_string();

        assert!(html.contains("data-passkey=\"register\""));
        assert!(html.contains(&format!("data-passkey-start=\"{BASE}/start\"")));
        assert!(html.contains(&format!("data-passkey-finish=\"{BASE}/finish\"")));
    }

    #[test]
    fn marks_the_place_for_the_push_control() {
        let html = with_passkeys(&fixtures::one()).into_string();

        assert!(html.contains(&format!("id=\"{}\"", push::CONTROL_ID)));
    }

    #[test]
    fn marks_settings_as_the_current_page() {
        let html = page(&html! {}, &html! {}, &add_passkey(), None, "/log-out").into_string();

        assert!(html.contains("<title>Settings | Asgard</title>"));
        assert!(html.contains("href=\"/settings\" aria-label=\"Settings\" aria-current=\"page\""));
    }
}
