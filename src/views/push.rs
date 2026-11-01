use maud::{Markup, html};

use crate::assets;

/// The control's element, which the push script finds by it.
pub const CONTROL_ID: &str = "push-control";

/// Whether this device gets the Crew's notifications, or why it can't yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushState {
    /// Not subscribed: one button turns notifications on.
    Off,
    /// Subscribed, with a way to turn them off.
    On,
    /// An iPhone or iPad outside the installed app, where notifications can't work.
    InstallFirst,
    /// The browser blocks Asgard's notifications.
    Blocked,
}

impl PushState {
    pub const ALL: [Self; 4] = [Self::Off, Self::On, Self::InstallFirst, Self::Blocked];

    /// The name the push script knows the state by.
    const fn attribute(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::InstallFirst => "install",
            Self::Blocked => "blocked",
        }
    }
}

/// What the push script needs from the server: the key to subscribe with and where to post.
pub struct PushUrls<'a> {
    /// The VAPID public key, base64url.
    pub public_key: &'a str,
    pub subscribe: &'a str,
    pub unsubscribe: &'a str,
}

/// The control that turns notifications on and off for this device, on the welcome and settings
/// pages.
///
/// It holds every state, with all but `state` hidden: the server knows whether this session is
/// subscribed, and the push script then shows the state that's true in this browser, such as an
/// iPhone outside the installed app or notifications blocked.
#[must_use]
pub fn control(state: PushState, urls: &PushUrls) -> Markup {
    let part = |shown: PushState, content: Markup| {
        html! {
            div.push-state data-push-state=(shown.attribute()) hidden[shown != state] { (content) }
        }
    };
    html! {
        div.push-control #(CONTROL_ID)
            data-push-key=(urls.public_key)
            data-push-subscribe=(urls.subscribe)
            data-push-unsubscribe=(urls.unsubscribe) {
            (part(PushState::Off, html! {
                p.muted { "Get the Crew's Reminders and updates on this device, even when Asgard is closed." }
                button.button type="button" data-push-action="on" aria-describedby="push-status" {
                    "Turn on notifications"
                }
            }))
            (part(PushState::On, html! {
                p { "Notifications are on for this device." }
                button.button.secondary type="button" data-push-action="off" aria-describedby="push-status" {
                    "Turn off"
                }
            }))
            (part(PushState::InstallFirst, html! {
                p {
                    "An iPhone or iPad only shows notifications once Asgard is on the Home Screen. "
                    "In Safari, tap " strong { "Share" } ", then " strong { "Add to Home Screen" }
                    ", open Asgard from there and turn notifications on."
                }
            }))
            (part(PushState::Blocked, html! {
                p {
                    "This device blocks Asgard's notifications. Allow them in the browser's site "
                    "settings for Asgard (the icon beside the address), or, for the installed app, "
                    "in the phone's settings for Asgard. Then reload this page."
                }
            }))
            p.status #push-status role="status" {}
            script type="module" src=(assets::PUSH.url) {}
        }
    }
}

/// Test data for the pages that hold the control.
#[cfg(test)]
pub(crate) mod fixtures {
    use maud::Markup;

    use super::{PushState, PushUrls, control};

    pub const URLS: PushUrls<'static> = PushUrls {
        public_key: "BPublicKey",
        subscribe: "/push/subscribe",
        unsubscribe: "/push/unsubscribe",
    };

    pub fn off() -> Markup {
        control(PushState::Off, &URLS)
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::URLS;
    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn push_control_off() {
        assert_html_snapshot!(control(PushState::Off, &URLS));
    }

    #[test]
    fn push_control_on() {
        assert_html_snapshot!(control(PushState::On, &URLS));
    }

    #[test]
    fn push_control_install_first() {
        assert_html_snapshot!(control(PushState::InstallFirst, &URLS));
    }

    #[test]
    fn push_control_blocked() {
        assert_html_snapshot!(control(PushState::Blocked, &URLS));
    }

    #[test]
    fn shows_only_the_given_state_and_holds_the_rest_for_the_script() {
        for state in PushState::ALL {
            let html = control(state, &URLS).into_string();

            for other in PushState::ALL {
                let part = format!("data-push-state=\"{}\"", other.attribute());
                let hidden = format!("{part} hidden");
                assert!(html.contains(&part), "{state:?} holds {other:?}");
                assert_eq!(
                    html.contains(&hidden),
                    other != state,
                    "{state:?} shows {other:?}"
                );
            }
        }
    }

    #[test]
    fn gives_the_script_the_key_and_where_to_post() {
        let html = control(PushState::Off, &URLS).into_string();

        assert!(html.contains("data-push-key=\"BPublicKey\""));
        assert!(html.contains("data-push-subscribe=\"/push/subscribe\""));
        assert!(html.contains("data-push-unsubscribe=\"/push/unsubscribe\""));
        assert!(html.contains(&format!("src=\"{}\"", assets::PUSH.url)));
    }
}
