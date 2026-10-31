use maud::{DOCTYPE, Markup, PreEscaped, html};

use crate::assets;
use crate::views::brand::{header_logo, hero_logo};
use crate::views::nav::{Place, bottom_nav, settings_link};

/// htmx's default response handling, except that a 422 is swapped in too: it is a form
/// answering with what to fix.
const HTMX_CONFIG: &str = r#"{"responseHandling":[{"code":"204","swap":false},{"code":"[23]..","swap":true},{"code":"422","swap":true},{"code":"[45]..","swap":false,"error":true},{"code":"...","swap":false}]}"#;

/// Grenze, the display face of page titles and the Balance figure: its Latin subset, and the
/// extended one that holds Czech letters such as "č" and "Š". Each loads only when a page uses
/// one of its characters. The ranges are the ones Fontsource cut the files with.
fn display_faces() -> String {
    [
        (
            assets::GRENZE_LATIN.url,
            "U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, \
             U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, \
             U+FFFD",
        ),
        (
            assets::GRENZE_LATIN_EXT.url,
            "U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, \
             U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, \
             U+2113, U+2C60-2C7F, U+A720-A7FF",
        ),
    ]
    .map(|(url, range)| {
        format!(
            "@font-face {{ font-family: \"Grenze\"; font-weight: 600; font-display: swap; \
             src: url(\"{url}\") format(\"woff2\"); unicode-range: {range}; }}"
        )
    })
    .join("\n")
}

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
                link rel="preload" href=(assets::GRENZE_LATIN.url) as="font" type="font/woff2" crossorigin;
                link rel="preload" href=(assets::GRENZE_LATIN_EXT.url) as="font" type="font/woff2" crossorigin;
                style { (PreEscaped(display_faces())) }
                link rel="stylesheet" href=(assets::APP_CSS.url);
                meta name="htmx-config" content=(HTMX_CONFIG);
                script src=(assets::HTMX.url) defer {}
                script type="module" src=(assets::REGISTER_SW.url) {}
            }
            body { (body) }
        }
    }
}

/// The app shell: header band with the way into settings, the page content and the
/// navigation, with `current` marked.
#[must_use]
pub fn shell(current: Place, content: &Markup) -> Markup {
    document(
        &format!("{} | Asgard", current.label()),
        &html! {
            div.shell {
                header.shell-header {
                    (header_logo())
                    (settings_link(current))
                }
                main.shell-main { (content) }
                (bottom_nav(current))
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
    use crate::views::nav::Tab;

    #[test]
    fn shell() {
        assert_html_snapshot!(super::shell(
            Place::Tab(Tab::Shopping),
            &html! { p { "Content" } }
        ));
    }

    #[test]
    fn shell_header_links_home_with_the_logo() {
        let html = super::shell(Place::Tab(Tab::Quotes), &html! {}).into_string();

        assert!(html.contains(&format!(
            "<header class=\"shell-header\"><a class=\"logo\" href=\"/\"><img src=\"{}\" alt=\"Asgard\" width=\"128\" height=\"32\">",
            assets::LOGO_HEADER.url
        )));
    }

    #[test]
    fn shell_header_leads_to_settings() {
        let html = super::shell(Place::Tab(Tab::Quotes), &html! {}).into_string();

        assert!(html.contains("<a class=\"settings-link\" href=\"/settings\""));
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
