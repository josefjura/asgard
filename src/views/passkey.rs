use maud::{Markup, html};

use crate::assets;

/// Where the passkey script sends the two halves of a ceremony.
pub struct CeremonyUrls {
    pub start: String,
    pub finish: String,
}

/// What pressing a passkey button does.
#[derive(Debug, Clone, Copy)]
pub enum Ceremony {
    /// Creates a new passkey.
    Register,
    /// Signs in with a passkey the device already has.
    SignIn,
}

impl Ceremony {
    const fn attribute(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::SignIn => "sign-in",
        }
    }
}

/// The button that runs `ceremony` through `urls`, with the line beneath it where the script
/// says what went wrong.
#[must_use]
pub fn button(label: &str, ceremony: Ceremony, urls: &CeremonyUrls) -> Markup {
    html! {
        button.button type="button" data-passkey=(ceremony.attribute())
            data-passkey-start=(urls.start) data-passkey-finish=(urls.finish)
            aria-describedby="passkey-status" {
            (label)
        }
        p.status #passkey-status role="status" {}
        script type="module" src=(assets::PASSKEY.url) {}
    }
}
