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

/// The navigation between the four tabs, with `active` marked as the current page.
#[must_use]
pub fn bottom_nav(active: Tab) -> Markup {
    html! {
        nav.tabs aria-label="Sections" {
            @for tab in Tab::ALL {
                a.tab href=(tab.href()) aria-current=[(tab == active).then_some("page")] {
                    (tab.icon())
                    span { (tab.label()) }
                }
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
            let html = bottom_nav(active).into_string();

            assert_eq!(html.matches("aria-current=\"page\"").count(), 1);
            assert!(html.contains(&format!("href=\"{}\" aria-current=\"page\"", active.href())));
        }
    }

    #[test]
    fn bottom_nav_in_each_state() {
        for tab in Tab::ALL {
            assert_html_snapshot!(
                format!("bottom_nav_{}", tab.label().to_lowercase()),
                super::bottom_nav(tab)
            );
        }
    }
}
