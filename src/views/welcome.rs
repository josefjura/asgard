use maud::{Markup, html};

use crate::views::layout::standalone;
use crate::views::nav::Tab;

/// Where the "enable notifications on this device" control goes. The Reminders and push spec
/// fills it in.
pub const PUSH_SLOT_ID: &str = "push-control";

/// What a new Member sees once, right after joining: their display name to fix, how to install
/// Asgard, a line on each tab, and "Got it", which posts to `got_it`.
#[must_use]
pub fn page(crew: &str, name_form: &Markup, got_it: &str) -> Markup {
    standalone(
        "Welcome | Asgard",
        &html! {
            h1 { "Welcome to " (crew) }
            p.lead { "You're in the Crew. Here's what to know before you start." }

            section.welcome-section aria-labelledby="welcome-name" {
                h2 #welcome-name { "Your name" }
                p { "The Admin typed it in for you. Fix it here if they got it wrong." }
                (name_form)
            }

            section.welcome-section aria-labelledby="welcome-install" {
                h2 #welcome-install { "Put Asgard on your home screen" }
                p { "It then opens like an app, without the browser around it." }
                dl.install {
                    dt { "iPhone" }
                    dd {
                        "In Safari, tap " strong { "Share" } ", then " strong { "Add to Home Screen" } ". "
                        "An iPhone only delivers Asgard's notifications once it's on the Home Screen."
                    }
                    dt { "Android" }
                    dd {
                        "In Chrome, tap the " strong { "⋮" } " menu, then " strong { "Install app" }
                        " or " strong { "Add to Home screen" } "."
                    }
                }
            }

            section.welcome-section aria-labelledby="welcome-push" {
                h2 #welcome-push { "Notifications" }
                div.placeholder #(PUSH_SLOT_ID) {
                    "Turning on notifications for this device is coming soon."
                }
            }

            section.welcome-section aria-labelledby="welcome-tour" {
                h2 #welcome-tour { "Where things are" }
                dl.tour {
                    @for tab in Tab::ALL {
                        dt { (tab.label()) }
                        dd { (tour_line(tab)) }
                    }
                }
            }

            form method="post" action=(got_it) {
                button.button type="submit" { "Got it" }
            }
        },
    )
}

const fn tour_line(tab: Tab) -> &'static str {
    match tab {
        Tab::Finance => "Log what you spent for the Crew and see who owes whom.",
        Tab::Shopping => "The list of things the Crew needs to buy, by how soon.",
        Tab::Reminders => "Notify the whole Crew at a set date and time.",
        Tab::Quotes => "The Crew's collection of memorable things people said.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;
    use crate::views::display_name;

    #[test]
    fn welcome_page() {
        assert_html_snapshot!(page(
            "Asgard",
            &display_name::form("/welcome/name", "Ondra", None),
            "/welcome/done"
        ));
    }

    #[test]
    fn marks_the_place_for_the_push_control() {
        let html = page("Asgard", &html! {}, "/welcome/done").into_string();

        assert!(html.contains(&format!("id=\"{PUSH_SLOT_ID}\"")));
    }
}
