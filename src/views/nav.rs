use maud::{Markup, html};

/// A top-level section of the app, reachable from the bottom navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Finance,
    Shopping,
    Reminders,
    Quotes,
}

impl Tab {
    pub const ALL: [Self; 4] = [Self::Finance, Self::Shopping, Self::Reminders, Self::Quotes];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Finance => "Finance",
            Self::Shopping => "Shopping",
            Self::Reminders => "Reminders",
            Self::Quotes => "Quotes",
        }
    }

    /// Finance is the home page.
    #[must_use]
    pub const fn href(self) -> &'static str {
        match self {
            Self::Finance => "/",
            Self::Shopping => "/shopping",
            Self::Reminders => "/reminders",
            Self::Quotes => "/quotes",
        }
    }

    fn icon(self) -> Markup {
        // 24px stroke icons; colour and stroke come from the stylesheet.
        html! {
            svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" {
                @match self {
                    Self::Finance => {
                        ellipse cx="12" cy="6" rx="7" ry="3" {}
                        path d="M5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6" {}
                        path d="M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6" {}
                    }
                    Self::Shopping => {
                        path d="M3 4h2l2.4 11h10.2L20 8H6.2" {}
                        circle cx="9" cy="19.5" r="1.5" {}
                        circle cx="17" cy="19.5" r="1.5" {}
                    }
                    Self::Reminders => {
                        path d="M6 16v-5a6 6 0 0 1 12 0v5l2 2H4z" {}
                        path d="M10 20.5a2 2 0 0 0 4 0" {}
                    }
                    Self::Quotes => {
                        path d="M4 5h16v11H9l-5 4z" {}
                    }
                }
            }
        }
    }
}

/// Where a Member changes their display name and logs out.
pub const SETTINGS_HREF: &str = "/settings";

/// Where the Admin makes and revokes links. Only the Admin's settings link to it.
pub const ADMIN_HREF: &str = "/admin";

/// A page inside the app shell: one of the tabs, settings, or the admin page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Tab(Tab),
    Settings,
    Admin,
}

impl Place {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tab(tab) => tab.label(),
            Self::Settings => "Settings",
            Self::Admin => "Admin",
        }
    }
}

/// The navigation between the four tabs, with the tab at `current` marked as the current page.
#[must_use]
pub fn bottom_nav(current: Place) -> Markup {
    html! {
        nav.tabs aria-label="Sections" {
            @for tab in Tab::ALL {
                a.tab href=(tab.href()) aria-current=[(current == Place::Tab(tab)).then_some("page")] {
                    (tab.icon())
                    span { (tab.label()) }
                }
            }
        }
    }
}

/// The header's way into settings, marked when `current` is settings.
#[must_use]
pub fn settings_link(current: Place) -> Markup {
    html! {
        a.settings-link href=(SETTINGS_HREF) aria-label="Settings"
            aria-current=[(current == Place::Settings).then_some("page")] {
            // Feather's "settings" cog; colour and stroke come from the stylesheet.
            svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" {
                circle cx="12" cy="12" r="3" {}
                path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn bottom_nav_marks_only_the_active_tab() {
        for active in Tab::ALL {
            let html = bottom_nav(Place::Tab(active)).into_string();

            assert_eq!(html.matches("aria-current=\"page\"").count(), 1);
            assert!(html.contains(&format!("href=\"{}\" aria-current=\"page\"", active.href())));
        }
    }

    #[test]
    fn bottom_nav_in_each_state() {
        for tab in Tab::ALL {
            assert_html_snapshot!(
                format!("bottom_nav_{}", tab.label().to_lowercase()),
                super::bottom_nav(Place::Tab(tab))
            );
        }
    }

    #[test]
    fn on_settings_no_tab_is_current() {
        let html = bottom_nav(Place::Settings).into_string();

        assert!(!html.contains("aria-current"));
    }

    #[test]
    fn the_settings_link_is_marked_only_on_settings() {
        let on_settings = settings_link(Place::Settings).into_string();
        let on_a_tab = settings_link(Place::Tab(Tab::Finance)).into_string();

        assert!(
            on_settings
                .contains("href=\"/settings\" aria-label=\"Settings\" aria-current=\"page\"")
        );
        assert!(!on_a_tab.contains("aria-current"));
    }
}
