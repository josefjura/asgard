//! The Asgard logo, as the header's home link and as the hero above standalone pages.

use maud::{Markup, html};

use crate::assets;

/// The header lockup's artboard, so the band keeps its height before the SVG arrives.
const HEADER_WIDTH: u16 = 128;
const HEADER_HEIGHT: u16 = 32;
/// The hero's artboard, the same for its light and dark versions.
const HERO_SIZE: u16 = 180;

/// The header lockup linking home. One file serves both schemes: the band is green in both.
#[must_use]
pub fn header_logo() -> Markup {
    html! {
        a.logo href="/" {
            img src=(assets::LOGO_HEADER.url) alt="Asgard" width=(HEADER_WIDTH) height=(HEADER_HEIGHT);
        }
    }
}

/// The large logo above a standalone page. Decorative: the page's heading names the page.
#[must_use]
pub fn hero_logo() -> Markup {
    html! {
        picture.hero {
            source srcset=(assets::LOGO_HERO_DARK.url) media="(prefers-color-scheme: dark)";
            img src=(assets::LOGO_HERO_LIGHT.url) alt="" width=(HERO_SIZE) height=(HERO_SIZE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn hero_logo() {
        assert_html_snapshot!(super::hero_logo());
    }

    #[test]
    fn header_logo_is_a_home_link_named_asgard() {
        let html = header_logo().into_string();

        assert!(html.starts_with("<a class=\"logo\" href=\"/\"><img "));
        assert!(html.contains("alt=\"Asgard\""));
    }

    #[test]
    fn markup_sizes_match_the_artwork() {
        assert_eq!(
            assets::view_box_size(assets::LOGO_HEADER),
            (f64::from(HEADER_WIDTH), f64::from(HEADER_HEIGHT))
        );
        for hero in [assets::LOGO_HERO_LIGHT, assets::LOGO_HERO_DARK] {
            assert_eq!(
                assets::view_box_size(hero),
                (f64::from(HERO_SIZE), f64::from(HERO_SIZE))
            );
        }
        assert!(header_logo().into_string().contains(&format!(
            "width=\"{HEADER_WIDTH}\" height=\"{HEADER_HEIGHT}\""
        )));
    }
}
