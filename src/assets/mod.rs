//! Static files, embedded at compile time by `build.rs`.
//!
//! Most are served under a content-hash URL, so editing a file changes its URL and an old URL
//! is never served stale. The service worker and the web manifest need fixed URLs.

pub mod hash;

/// An embedded static file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub url: &'static str,
    pub content_type: &'static str,
    pub body: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// Finds the hashed asset served at `url`.
#[must_use]
pub fn hashed(url: &str) -> Option<Asset> {
    HASHED.iter().copied().find(|asset| asset.url == url)
}

/// The width and height of an SVG's `viewBox`, so tests can tie markup sizes to the artwork.
///
/// # Panics
///
/// If the asset isn't an SVG with a four-number `viewBox`.
#[cfg(test)]
#[must_use]
pub fn view_box_size(asset: Asset) -> (f64, f64) {
    let svg = std::str::from_utf8(asset.body).expect("SVGs are UTF-8");
    let start = svg.find("viewBox=\"").expect("SVG has a viewBox") + "viewBox=\"".len();
    let numbers: Vec<f64> = svg[start..]
        .split('"')
        .next()
        .unwrap()
        .split([' ', ','])
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().expect("viewBox numbers"))
        .collect();
    assert_eq!(numbers.len(), 4, "{}", asset.url);
    (numbers[2], numbers[3])
}

#[cfg(test)]
mod tests {
    use super::hash::{fingerprint, hashed_file_name};
    use super::*;

    /// What a PNG's `IHDR` chunk says about the image.
    #[derive(Debug, PartialEq, Eq)]
    struct PngHeader {
        width: u32,
        height: u32,
        bit_depth: u8,
        has_alpha: bool,
    }

    fn png_header(asset: Asset) -> PngHeader {
        let body = asset.body;
        assert_eq!(&body[..8], b"\x89PNG\r\n\x1a\n", "{}", asset.url);
        assert_eq!(&body[12..16], b"IHDR", "{}", asset.url);
        let number = |at: usize| u32::from_be_bytes(body[at..at + 4].try_into().unwrap());
        // Colour types 4 and 6 carry an alpha channel; a tRNS chunk adds one to the others.
        let colour_type = body[25];
        PngHeader {
            width: number(16),
            height: number(20),
            bit_depth: body[24],
            has_alpha: matches!(colour_type, 4 | 6) || body.windows(4).any(|w| w == b"tRNS"),
        }
    }

    #[test]
    fn pngs_are_8_bit_at_their_exact_size_and_opaque_where_the_platform_needs_it() {
        for (asset, size, has_alpha) in [
            (ICON_192, 192, true),
            (ICON_512, 512, true),
            (ICON_MASKABLE_512, 512, false),
            (APPLE_TOUCH_ICON, 180, false),
            (BADGE_96, 96, true),
        ] {
            assert_eq!(
                png_header(asset),
                PngHeader {
                    width: size,
                    height: size,
                    bit_depth: 8,
                    has_alpha
                },
                "{}",
                asset.url
            );
        }
    }

    #[test]
    fn svgs_have_the_inventory_artboard_and_nothing_but_shapes() {
        assert_eq!(view_box_size(ICON_SVG), (512.0, 512.0));
        assert_eq!(view_box_size(LOGO_HERO_LIGHT), (180.0, 180.0));
        assert_eq!(view_box_size(LOGO_HERO_DARK), (180.0, 180.0));
        let (width, height) = view_box_size(LOGO_HEADER);
        assert_eq!(height, 32.0);
        assert!(width <= 160.0, "{width}");

        for asset in [ICON_SVG, LOGO_HEADER, LOGO_HERO_LIGHT, LOGO_HERO_DARK] {
            let svg = std::str::from_utf8(asset.body).unwrap();
            for tag in ["<image", "<script", "<text", "<filter", "<foreignObject"] {
                assert!(!svg.contains(tag), "{} contains {tag}", asset.url);
            }
        }
    }

    #[test]
    fn hashed_name_goes_before_the_extension() {
        assert_eq!(
            hashed_file_name("app.css", b"body {}"),
            format!("app.{:016x}.css", fingerprint(b"body {}"))
        );
        assert_eq!(
            hashed_file_name("LICENSE", b"x"),
            format!("LICENSE.{:016x}", fingerprint(b"x"))
        );
    }

    #[test]
    fn editing_a_file_changes_its_url() {
        assert_ne!(
            hashed_file_name("app.css", b"body { color: green; }"),
            hashed_file_name("app.css", b"body { color: olive; }")
        );
    }

    #[test]
    fn every_hashed_url_carries_the_fingerprint_of_its_body() {
        for asset in HASHED {
            let fingerprint = format!(".{:016x}.", fingerprint(asset.body));
            assert!(asset.url.starts_with("/static/"), "{}", asset.url);
            assert!(asset.url.contains(&fingerprint), "{}", asset.url);
        }
    }

    #[test]
    fn service_worker_precaches_every_hashed_asset_and_the_offline_page() {
        let worker = std::str::from_utf8(SERVICE_WORKER.body).unwrap();

        for asset in HASHED {
            assert!(
                worker.contains(&format!("\"{}\"", asset.url)),
                "{}",
                asset.url
            );
        }
        assert!(worker.contains("\"/offline\""));
        assert!(!worker.contains("{{"));
    }

    #[test]
    fn manifest_is_json_pointing_at_hashed_icons() {
        let manifest: serde_json::Value = serde_json::from_slice(MANIFEST.body).unwrap();

        assert_eq!(manifest["display"], "standalone");
        assert_eq!(manifest["start_url"], "/");
        let icons: Vec<&str> = manifest["icons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|icon| icon["src"].as_str().unwrap())
            .collect();
        assert_eq!(icons, [ICON_192.url, ICON_512.url, ICON_MASKABLE_512.url]);
    }

    #[test]
    fn finds_assets_by_their_own_url_only() {
        assert_eq!(hashed(APP_CSS.url), Some(APP_CSS));
        assert_eq!(hashed("/static/app.0000000000000000.css"), None);
        assert_eq!(hashed(SERVICE_WORKER.url), None);
    }
}
