//! `/dev/gallery`: every component with fixture data in all its states, at phone and desktop
//! width. Compiled into debug builds only.

use maud::{DOCTYPE, Markup, html};

use crate::assets;
use crate::domain::members::{CREW_NAME, DisplayName};
use crate::views::brand::header_logo;
use crate::views::layout::document;
use crate::views::nav::{Tab, bottom_nav};
use crate::views::passkey::CeremonyUrls;
use crate::views::{invite, offline, sign_in, tabs};

/// One component in one state, rendered as a complete document.
pub struct Fixture {
    pub component: &'static str,
    pub state: String,
    pub document: Markup,
    /// How tall the frames need to be to show the whole fixture, at both widths.
    pub height: u16,
}

const PHONE_WIDTH: u16 = 390;
const PAGE_HEIGHT: u16 = 720;
const DESKTOP_WIDTH: u16 = 1280;

/// # Panics
///
/// If a fixture's display name is invalid, which is a bug in the fixtures.
#[must_use]
pub fn fixtures() -> Vec<Fixture> {
    let nav = Tab::ALL.into_iter().map(|tab| Fixture {
        component: "Bottom navigation",
        state: format!("{} active", tab.label()),
        document: document("Bottom navigation", &bottom_nav(tab)),
        height: 80,
    });
    let pages = Tab::ALL.into_iter().map(|tab| Fixture {
        component: "App shell",
        state: format!("{} tab", tab.label()),
        document: tabs::page(tab),
        height: PAGE_HEIGHT,
    });
    let offline = Fixture {
        component: "Offline page",
        state: "No network".to_owned(),
        document: offline::page(),
        height: PAGE_HEIGHT,
    };
    let name = |name: &str| DisplayName::parse(name).expect("fixture names are valid");
    let invite = Fixture {
        component: "Invite page",
        state: "Valid link".to_owned(),
        document: invite::page(
            CREW_NAME,
            &name("Ondra"),
            &CeremonyUrls {
                start: "/invite/fixture/start".to_owned(),
                finish: "/invite/fixture/finish".to_owned(),
            },
        ),
        height: PAGE_HEIGHT,
    };
    let link_errors = [
        ("Naming the Admin", Some(name("Josef"))),
        ("Before there is an Admin", None),
    ]
    .map(|(state, admin)| Fixture {
        component: "Link error page",
        state: state.to_owned(),
        document: invite::link_error(admin.as_ref()),
        height: PAGE_HEIGHT,
    });
    let signed_in = Fixture {
        component: "Invite page",
        state: "Already signed in".to_owned(),
        document: invite::signed_in(&name("Josef"), "/invite/fixture/sign-out"),
        height: PAGE_HEIGHT,
    };
    let sign_in = Fixture {
        component: "Sign-in page",
        state: "Signed out".to_owned(),
        document: sign_in::page(&CeremonyUrls {
            start: "/sign-in/start".to_owned(),
            finish: "/sign-in/finish".to_owned(),
        }),
        height: PAGE_HEIGHT,
    };
    let brand = Fixture {
        component: "Brand",
        state: "Logos on every background".to_owned(),
        document: brand(),
        height: 560,
    };
    nav.chain(pages)
        .chain([brand, offline, invite, signed_in])
        .chain(link_errors)
        .chain([sign_in])
        .collect()
}

/// Each logo on every background it was drawn for, whatever scheme the viewer's browser is in.
fn brand() -> Markup {
    let hero = |asset: assets::Asset, background: &str| {
        html! {
            div style={ "padding: 1rem; background: " (background) } {
                img src=(asset.url) alt="" width="180" height="180" style="display: block";
            }
        }
    };
    document(
        "Brand",
        &html! {
            @for band in ["#3E5B2B", "#263820"] {
                header.shell-header style={ "background: " (band) } { (header_logo()) }
            }
            div style="display: flex; flex-wrap: wrap" {
                (hero(assets::LOGO_HERO_LIGHT, "#EEF0E4"))
                (hero(assets::LOGO_HERO_DARK, "#15180F"))
            }
        },
    )
}

#[must_use]
pub fn page(fixtures: &[Fixture]) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Gallery | Asgard" }
                style { (GALLERY_CSS) }
            }
            body {
                h1 { "Component gallery" }
                @for (index, fixture) in fixtures.iter().enumerate() {
                    @if index == 0 || fixtures[index - 1].component != fixture.component {
                        h2 { (fixture.component) }
                    }
                    section {
                        h3 { (fixture.state) }
                        div.frames {
                            figure {
                                figcaption { "Phone, " (PHONE_WIDTH) "px" }
                                iframe title={ (fixture.component) ", " (fixture.state) ", phone" }
                                    width=(PHONE_WIDTH) height=(fixture.height)
                                    srcdoc=(fixture.document.0) {}
                            }
                            figure {
                                figcaption { "Desktop, " (DESKTOP_WIDTH) "px, shown at half size" }
                                div.desktop style={ "height: " (fixture.height / 2) "px" } {
                                    iframe title={ (fixture.component) ", " (fixture.state) ", desktop" }
                                        width=(DESKTOP_WIDTH) height=(fixture.height)
                                        srcdoc=(fixture.document.0) {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The gallery's own chrome. Debug-only, so it stays out of the app's stylesheet.
const GALLERY_CSS: &str = "
body { margin: 0; padding: 1.5rem; font-family: system-ui, sans-serif; background: #e4e6da; color: #22261b; }
h2 { margin-top: 2.5rem; }
.frames { display: flex; flex-wrap: wrap; gap: 1.5rem; align-items: flex-start; }
figure { margin: 0; }
figcaption { font-size: 0.875rem; color: #5e6450; margin-bottom: 0.5rem; }
iframe { display: block; border: 1px solid #b9bea6; background: #fff; }
.desktop { width: 640px; overflow: hidden; }
.desktop iframe { transform: scale(0.5); transform-origin: top left; }
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_component_in_every_state() {
        let html = page(&fixtures()).into_string();

        for component in [
            "Bottom navigation",
            "App shell",
            "Brand",
            "Offline page",
            "Invite page",
            "Link error page",
            "Sign-in page",
        ] {
            assert!(
                html.contains(&format!("<h2>{component}</h2>")),
                "{component}"
            );
        }
        for tab in Tab::ALL {
            assert!(html.contains(&format!("<h3>{} active</h3>", tab.label())));
            assert!(html.contains(&format!("<h3>{} tab</h3>", tab.label())));
        }
        for state in [
            "Logos on every background",
            "No network",
            "Valid link",
            "Already signed in",
            "Naming the Admin",
            "Before there is an Admin",
            "Signed out",
        ] {
            assert!(html.contains(&format!("<h3>{state}</h3>")), "{state}");
        }
    }

    #[test]
    fn shows_each_fixture_at_phone_and_desktop_width() {
        let fixtures = fixtures();
        let html = page(&fixtures).into_string();

        assert_eq!(
            html.matches(&format!("width=\"{PHONE_WIDTH}\"")).count(),
            fixtures.len()
        );
        assert_eq!(
            html.matches(&format!("width=\"{DESKTOP_WIDTH}\"")).count(),
            fixtures.len()
        );
    }
}
