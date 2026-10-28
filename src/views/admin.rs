use maud::{Markup, html};

use crate::assets;
use crate::domain::auth::{LINK_LIFETIME_HOURS, LinkId, OpenLink};
use crate::domain::members::DisplayName;
use crate::views::layout::shell;
use crate::views::nav::Place;

/// The element the invite and revoke forms swap for the links the server answers with.
pub const LINKS_ID: &str = "links";

/// What became of the last invite or revoke sent from [`links`].
#[derive(Debug, Clone, Copy)]
pub enum Outcome<'a> {
    /// A new Invite link for `name`, at the full `url` to send them.
    Created {
        name: &'a DisplayName,
        url: &'a str,
    },
    /// The name typed was refused, with the sentence that says why.
    Refused {
        value: &'a str,
        message: &'a str,
    },
    Revoked(&'a DisplayName),
}

/// Where the invite form posts, under `base`.
#[must_use]
pub fn invite_path(base: &str) -> String {
    format!("{base}/invites")
}

/// Where an open link's revoke form posts, under `base`.
#[must_use]
pub fn revoke_path(base: &str, id: LinkId) -> String {
    format!("{base}/links/{}/revoke", id.get())
}

/// The admin page around `links`.
#[must_use]
pub fn page(links: &Markup) -> Markup {
    shell(
        Place::Admin,
        &html! {
            h1 { "Admin" }
            (links)
            script type="module" src=(assets::COPY_LINK.url) {}
        },
    )
}

/// The form that makes an Invite link, the link just made, and the links still open.
///
/// The link just made has a copy button, and each open link a button to revoke it. The forms
/// post under `base`; htmx swaps the whole of this for the answer, and without htmx they post
/// as plain forms.
#[must_use]
pub fn links(base: &str, open: &[OpenLink], outcome: Option<Outcome>) -> Markup {
    html! {
        div.admin-links #(LINKS_ID) {
            section.settings-section aria-labelledby="admin-invite" {
                h2 #admin-invite { "Invite someone" }
                (invite_form(base, outcome))
                @if let Some(Outcome::Created { name, url }) = outcome {
                    (created(name, url))
                }
            }
            section.settings-section aria-labelledby="admin-open" {
                h2 #admin-open { "Open links" }
                (open_links(base, open))
                p.note role="status" {
                    @if let Some(Outcome::Revoked(name)) = outcome {
                        "Revoked the link for " strong { (name) } ". It no longer works."
                    }
                }
            }
        }
    }
}

fn invite_form(base: &str, outcome: Option<Outcome>) -> Markup {
    let action = invite_path(base);
    let target = format!("#{LINKS_ID}");
    let (value, note, refused) = match outcome {
        Some(Outcome::Refused { value, message }) => (value, message, true),
        _ => (
            "",
            "The name they'll appear under. They can change it after joining.",
            false,
        ),
    };
    html! {
        form.name-form method="post" action=(action) hx-post=(action)
            hx-target=(target) hx-swap="outerHTML" {
            label for="invite-name" { "Display name" }
            div.field-row {
                input #invite-name type="text" name="display_name" value=(value) required
                    autocomplete="off" aria-describedby="invite-name-note"
                    aria-invalid=[refused.then_some("true")];
                button.button type="submit" { "Make link" }
            }
            p #invite-name-note .note .refused[refused] role="status" { (note) }
        }
    }
}

/// The link just made, in a field the copy button reads from, so it can also be selected by
/// hand where the clipboard isn't available.
fn created(name: &DisplayName, url: &str) -> Markup {
    html! {
        div.new-link {
            label for="new-link" {
                "Send this link to " strong { (name) } ". It works once, for " (LINK_LIFETIME_HOURS) " hours."
            }
            div.field-row {
                input #new-link type="text" value=(url) readonly;
                button.button type="button" data-copy="new-link"
                    aria-describedby="new-link-status" { "Copy" }
            }
            p #new-link-status .note role="status" {}
        }
    }
}

fn open_links(base: &str, open: &[OpenLink]) -> Markup {
    let target = format!("#{LINKS_ID}");
    html! {
        @if open.is_empty() {
            p.lead-in { "No open links. Each link works once, for " (LINK_LIFETIME_HOURS) " hours." }
        } @else {
            ul.link-list {
                @for link in open {
                    @let revoke = revoke_path(base, link.id);
                    li.open-link {
                        p {
                            "Invite for " strong { (link.display_name) }
                            br;
                            span.expires { "Works until " (link.expires.strftime("%-d %B, %H:%M")) }
                        }
                        form method="post" action=(revoke) hx-post=(revoke)
                            hx-target=(target) hx-swap="outerHTML"
                            hx-confirm={ "Revoke the link for " (link.display_name) "? It will stop working." } {
                            button.button.quiet type="submit" { "Revoke" }
                        }
                    }
                }
            }
        }
    }
}

/// Open links for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::date;

    use crate::domain::auth::{LinkId, OpenLink};
    use crate::domain::members::DisplayName;

    /// # Panics
    ///
    /// If a fixture name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn several() -> Vec<OpenLink> {
        [(1, "Ondra", 9), (2, "Karel", 14)]
            .map(|(id, name, hour)| OpenLink {
                id: LinkId::from_client(id),
                display_name: DisplayName::parse(name).expect("fixture names are valid"),
                expires: date(2026, 10, 29).at(hour, 5, 0, 0),
            })
            .into()
    }

    /// A link made just now, as the admin page shows it.
    pub const URL: &str =
        "https://asgard.joseforge.cz/invite/q7Xb2kLmNp4RsT9vWy0zA1cD3eF5gH6iJ8kL0mN2oP4";
}

#[cfg(test)]
mod tests {
    use super::fixtures::{URL, several};
    use super::*;
    use crate::domain::members::fixtures::name;
    use crate::views::assert_html_snapshot;

    const BASE: &str = "/admin";

    #[test]
    fn admin_page_without_links() {
        assert_html_snapshot!(page(&links(BASE, &[], None)));
    }

    #[test]
    fn admin_page_with_links() {
        assert_html_snapshot!(page(&links(BASE, &several(), None)));
    }

    #[test]
    fn admin_links_just_made() {
        let name = name("Karel");
        let outcome = Outcome::Created {
            name: &name,
            url: URL,
        };
        assert_html_snapshot!(links(BASE, &several(), Some(outcome)));
    }

    #[test]
    fn admin_links_refused() {
        let outcome = Outcome::Refused {
            value: "josef",
            message: "Someone in the Crew is already called josef.",
        };
        assert_html_snapshot!(links(BASE, &[], Some(outcome)));
    }

    #[test]
    fn admin_links_revoked() {
        let name = name("Ondra");
        assert_html_snapshot!(links(BASE, &several(), Some(Outcome::Revoked(&name))));
    }

    #[test]
    fn the_new_link_has_a_copy_button_for_its_field() {
        let name = name("Karel");
        let html = links(
            BASE,
            &[],
            Some(Outcome::Created {
                name: &name,
                url: URL,
            }),
        )
        .into_string();

        assert!(html.contains(&format!(
            "<input id=\"new-link\" type=\"text\" value=\"{URL}\" readonly>"
        )));
        assert!(html.contains("data-copy=\"new-link\""));
    }

    #[test]
    fn every_open_link_can_be_revoked() {
        let html = links(BASE, &several(), None).into_string();

        for id in 1..=2 {
            assert!(html.contains(&format!("hx-post=\"{BASE}/links/{id}/revoke\"")));
        }
        assert!(html.contains("Works until 29 October, 09:05"));
    }

    #[test]
    fn a_refused_name_keeps_what_was_typed() {
        let html = links(
            BASE,
            &[],
            Some(Outcome::Refused {
                value: "\"><b>",
                message: "Enter a display name.",
            }),
        )
        .into_string();

        assert!(html.contains("value=\"&quot;&gt;&lt;b&gt;\""));
        assert!(html.contains("aria-invalid=\"true\""));
        assert!(!html.contains("data-copy"));
    }

    #[test]
    fn loads_the_copy_script() {
        let html = page(&links(BASE, &[], None)).into_string();

        assert!(html.contains(&format!("src=\"{}\"", assets::COPY_LINK.url)));
        assert!(html.contains("<title>Admin | Asgard</title>"));
    }
}
