//! `/dev/gallery`: every component with fixture data in all its states, at phone and desktop
//! width. Compiled into debug builds only.

use maud::{DOCTYPE, Markup, html};

use crate::assets;
use crate::domain::auth::{LinkKind, PasskeyName};
use crate::domain::members::{CREW_NAME, DisplayName};
use crate::views::admin::{self, Outcome as AdminOutcome};
use crate::views::brand::header_logo;
use crate::views::display_name::{self, Outcome};
use crate::views::layout::document;
use crate::views::nav::{ADMIN_HREF, Place, Tab, bottom_nav};
use crate::views::passkey::CeremonyUrls;
use crate::views::passkeys::{self, Outcome as PasskeyOutcome};
use crate::views::{invite, offline, settings, sign_in, tabs, welcome};

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
/// The welcome page is long: its sections stack on a phone.
const WELCOME_HEIGHT: u16 = 1500;
/// Settings stacks its sections under the app shell's header.
const SETTINGS_HEIGHT: u16 = 1150;
/// Each extra passkey adds a card to the settings page.
const SEVERAL_PASSKEYS_HEIGHT: u16 = 1500;
/// The Admin's settings have one more section.
const ADMIN_SETTINGS_HEIGHT: u16 = 1300;
/// The admin page's two sections, with a new link or several open ones.
const ADMIN_HEIGHT: u16 = 1000;
/// Where the fixture passkey lists' forms post.
const PASSKEYS_BASE: &str = "/settings/passkeys";
const DESKTOP_WIDTH: u16 = 1280;

/// # Panics
///
/// If a fixture's display name is invalid, which is a bug in the fixtures.
#[must_use]
pub fn fixtures() -> Vec<Fixture> {
    let nav = Tab::ALL.into_iter().map(|tab| Fixture {
        component: "Bottom navigation",
        state: format!("{} active", tab.label()),
        document: document("Bottom navigation", &bottom_nav(Place::Tab(tab))),
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
        document: invite::signed_in(&name("Josef"), LinkKind::Invite, "/invite/fixture/log-out"),
        height: PAGE_HEIGHT,
    };
    let device = [
        (
            "Valid link",
            invite::device_page(
                &name("Ondra"),
                &CeremonyUrls {
                    start: "/device/fixture/start".to_owned(),
                    finish: "/device/fixture/finish".to_owned(),
                },
            ),
        ),
        (
            "Already signed in",
            invite::signed_in(&name("Josef"), LinkKind::Device, "/device/fixture/log-out"),
        ),
    ]
    .map(|(state, document)| Fixture {
        component: "Device link page",
        state: state.to_owned(),
        document,
        height: PAGE_HEIGHT,
    });
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
        .chain(device)
        .chain(link_errors)
        .chain([sign_in])
        .chain(signed_in_pages())
        .chain(passkey_lists())
        .chain(admin_pages())
        .collect()
}

/// The admin page alone in the Crew, with open links, and after each thing the Admin can do
/// on it.
fn admin_pages() -> Vec<Fixture> {
    let open = admin::fixtures::several();
    let crew = admin::fixtures::crew();
    let karel = DisplayName::parse("Karel").expect("fixture names are valid");
    let ondra = DisplayName::parse("Ondra").expect("fixture names are valid");
    let refused = "Someone in the Crew is already called josef.";
    [
        ("Alone in the Crew", &[][..], &[][..], None),
        ("No open links", &[][..], &crew[..], None),
        ("Open links", &open[..], &crew[..], None),
        (
            "Link just made",
            &open[..],
            &crew[..],
            Some(AdminOutcome::Created {
                kind: LinkKind::Invite,
                name: &karel,
                url: admin::fixtures::URL,
            }),
        ),
        (
            "Device link just made",
            &open[..],
            &crew[..],
            Some(AdminOutcome::Created {
                kind: LinkKind::Device,
                name: &ondra,
                url: admin::fixtures::DEVICE_URL,
            }),
        ),
        (
            "Name refused",
            &[][..],
            &crew[..],
            Some(AdminOutcome::Refused {
                value: "josef",
                message: refused,
            }),
        ),
        (
            "Link revoked",
            &open[1..],
            &crew[..],
            Some(AdminOutcome::Revoked(&karel)),
        ),
    ]
    .map(|(state, open, members, outcome)| Fixture {
        component: "Admin page",
        state: state.to_owned(),
        document: admin::page(&admin::links(ADMIN_HREF, open, members, outcome)),
        height: ADMIN_HEIGHT,
    })
    .into()
}

/// The pages a signed-in Member changes their display name on, and the form itself.
fn signed_in_pages() -> Vec<Fixture> {
    let welcome = Fixture {
        component: "Welcome page",
        state: "Just joined".to_owned(),
        document: welcome::page(
            CREW_NAME,
            &display_name::form("/welcome/name", "Ondra", None),
            "/welcome/done",
        ),
        height: WELCOME_HEIGHT,
    };
    let settings = [
        (
            "One passkey",
            passkeys::fixtures::one(),
            false,
            SETTINGS_HEIGHT,
        ),
        (
            "Several passkeys",
            passkeys::fixtures::several(),
            false,
            SEVERAL_PASSKEYS_HEIGHT,
        ),
        (
            "The Admin's",
            passkeys::fixtures::one(),
            true,
            ADMIN_SETTINGS_HEIGHT,
        ),
    ]
    .map(|(state, passkeys, admin, height)| Fixture {
        component: "Settings page",
        state: state.to_owned(),
        document: settings::page(
            &display_name::form("/settings/name", "Ondra", None),
            &passkeys::list(PASSKEYS_BASE, &passkeys, None),
            &CeremonyUrls {
                start: format!("{PASSKEYS_BASE}/start"),
                finish: format!("{PASSKEYS_BASE}/finish"),
            },
            admin.then_some(ADMIN_HREF),
            "/log-out",
        ),
        height,
    });
    let refused = "Someone in the Crew is already called Josef.";
    let name_forms = [
        ("Unchanged", "Ondra", None),
        ("Saved", "Ondřej", Some(Outcome::Saved)),
        ("Refused", "Josef", Some(Outcome::Refused(refused))),
    ]
    .map(|(state, value, outcome)| Fixture {
        component: "Display name form",
        state: state.to_owned(),
        document: document(
            "Display name form",
            &html! { main.standalone { (display_name::form("/welcome/name", value, outcome)) } },
        ),
        height: 260,
    });
    std::iter::once(welcome)
        .chain(settings)
        .chain(name_forms)
        .collect()
}

/// The passkey list after each thing a Member can do to it.
fn passkey_lists() -> Vec<Fixture> {
    let passkeys = passkeys::fixtures::several();
    let lost = PasskeyName::parse("Lost phone").expect("fixture names are valid");
    let refused = "Enter a name for the passkey.";
    [
        ("Renamed", Some(PasskeyOutcome::Renamed(passkeys[1].id))),
        (
            "Rename refused",
            Some(PasskeyOutcome::RenameRefused {
                id: passkeys[1].id,
                value: "",
                message: refused,
            }),
        ),
        ("Deleted", Some(PasskeyOutcome::Deleted(&lost))),
    ]
    .map(|(state, outcome)| Fixture {
        component: "Passkey list",
        state: state.to_owned(),
        document: document(
            "Passkey list",
            &html! {
                main.standalone { (passkeys::list(PASSKEYS_BASE, &passkeys, outcome)) }
            },
        ),
        height: 640,
    })
    .into()
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
            "Device link page",
            "Link error page",
            "Sign-in page",
            "Welcome page",
            "Settings page",
            "Display name form",
            "Passkey list",
            "Admin page",
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
            "Just joined",
            "One passkey",
            "Several passkeys",
            "Renamed",
            "Rename refused",
            "Deleted",
            "Unchanged",
            "Saved",
            "Refused",
            "The Admin's",
            "No open links",
            "Open links",
            "Link just made",
            "Device link just made",
            "Alone in the Crew",
            "Name refused",
            "Link revoked",
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
