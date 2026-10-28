use maud::{DOCTYPE, Markup, html};

use crate::assets;
use crate::views::brand::{header_logo, hero_logo};
use crate::views::nav::{Tab, bottom_nav};

/// htmx's default response handling, except that a 422 is swapped in too: it is a form
/// answering with what to fix.
const HTMX_CONFIG: &str = r#"{"responseHandling":[{"code":"204","swap":false},{"code":"[23]..","swap":true},{"code":"422","swap":true},{"code":"[45]..","swap":false,"error":true},{"code":"...","swap":false}]}"#;

/// A complete HTML document with the shared head: stylesheet, scripts, icons and manifest.
#[must_use]
pub fn document(title: &str, body: &Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                title { (title) }
                meta name="theme-color" content="#3E5B2B" media="(prefers-color-scheme: light)";
                meta name="theme-color" content="#263820" media="(prefers-color-scheme: dark)";
                meta name="mobile-web-app-capable" content="yes";
                meta name="apple-mobile-web-app-capable" content="yes";
                meta name="apple-mobile-web-app-title" content="Asgard";
                link rel="manifest" href=(assets::MANIFEST.url);
                link rel="icon" type="image/svg+xml" href=(assets::ICON_SVG.url);
                link rel="apple-touch-icon" href=(assets::APPLE_TOUCH_ICON.url);
                link rel="stylesheet" href=(assets::APP_CSS.url);
                meta name="htmx-config" content=(HTMX_CONFIG);
                script src=(assets::HTMX.url) defer {}
                script type="module" src=(assets::REGISTER_SW.url) {}
            }
            body { (body) }
        }
    }
}

/// The app shell: header band, the page content and the navigation with `active` marked.
#[must_use]
pub fn shell(active: Tab, content: &Markup) -> Markup {
    document(
        &format!("{} | Asgard", active.label()),
        &html! {
            div.shell {
                header.shell-header {
                    (header_logo())
                }
                main.shell-main { (content) }
                (bottom_nav(active))
            }
        },
    )
}

/// A page outside the app shell (sign-in, Invite, errors, offline): the hero logo above
/// `content`, centred on the page.
#[must_use]
pub fn standalone(title: &str, content: &Markup) -> Markup {
    document(
        title,
        &html! {
            main.standalone {
                (hero_logo())
                (content)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use maud::html;

    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn shell() {
        assert_html_snapshot!(super::shell(Tab::Shopping, &html! { p { "Content" } }));
    }

    #[test]
    fn shell_header_links_home_with_the_logo() {
        let html = super::shell(Tab::Quotes, &html! {}).into_string();

        assert!(html.contains(&format!(
            "<header class=\"shell-header\"><a class=\"logo\" href=\"/\"><img src=\"{}\" alt=\"Asgard\" width=\"128\" height=\"32\">",
            assets::LOGO_HEADER.url
        )));
    }

    #[test]
    fn standalone_page_shows_the_hero_above_its_content() {
        let html = standalone("Test", &html! { h1 { "Heading" } }).into_string();

        assert!(html.contains("<main class=\"standalone\"><picture class=\"hero\">"));
        assert!(html.contains("</picture><h1>Heading</h1></main>"));
    }

    #[test]
    fn document_loads_htmx_from_our_own_origin() {
        let html = document("Test", &html! {}).into_string();

        assert!(html.contains(&format!("<script src=\"{}\"", assets::HTMX.url)));
        assert!(assets::HTMX.url.starts_with("/static/"));
    }
}
