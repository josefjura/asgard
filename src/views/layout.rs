use maud::{DOCTYPE, Markup, html};

use crate::assets;
use crate::views::nav::{Tab, bottom_nav};

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
                    a.wordmark href="/" { "Asgard" }
                }
                main.shell-main { (content) }
                (bottom_nav(active))
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
    fn document_loads_htmx_from_our_own_origin() {
        let html = document("Test", &html! {}).into_string();

        assert!(html.contains(&format!("<script src=\"{}\"", assets::HTMX.url)));
        assert!(assets::HTMX.url.starts_with("/static/"));
    }
}
