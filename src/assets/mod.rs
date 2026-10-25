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

#[cfg(test)]
mod tests {
    use super::hash::{fingerprint, hashed_file_name};
    use super::*;

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
