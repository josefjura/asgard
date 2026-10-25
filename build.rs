//! Embeds `static/` into the binary: hashes each file into its URL and fills the
//! `{{...}}` placeholders in the files served at fixed URLs (service worker, manifest).

#[path = "src/assets/hash.rs"]
mod hash;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

/// Served at `/static/<name>.<hash>.<ext>` and cached forever.
struct Hashed {
    constant: &'static str,
    file: &'static str,
    content_type: &'static str,
}

/// Served at a fixed URL and revalidated on every request.
struct Fixed {
    constant: &'static str,
    file: &'static str,
    url: &'static str,
    content_type: &'static str,
}

const JAVASCRIPT: &str = "text/javascript; charset=utf-8";

const HASHED: &[Hashed] = &[
    Hashed {
        constant: "APP_CSS",
        file: "app.css",
        content_type: "text/css; charset=utf-8",
    },
    Hashed {
        constant: "HTMX",
        file: "htmx-2.0.11.min.js",
        content_type: JAVASCRIPT,
    },
    Hashed {
        constant: "REGISTER_SW",
        file: "register-sw.js",
        content_type: JAVASCRIPT,
    },
    Hashed {
        constant: "PASSKEY",
        file: "passkey.js",
        content_type: JAVASCRIPT,
    },
    Hashed {
        constant: "ICON_SVG",
        file: "icon.svg",
        content_type: "image/svg+xml",
    },
    Hashed {
        constant: "ICON_192",
        file: "icon-192.png",
        content_type: "image/png",
    },
    Hashed {
        constant: "ICON_512",
        file: "icon-512.png",
        content_type: "image/png",
    },
    Hashed {
        constant: "ICON_MASKABLE_512",
        file: "icon-maskable-512.png",
        content_type: "image/png",
    },
    Hashed {
        constant: "APPLE_TOUCH_ICON",
        file: "apple-touch-icon.png",
        content_type: "image/png",
    },
];

const FIXED: &[Fixed] = &[
    Fixed {
        constant: "SERVICE_WORKER",
        file: "sw.js",
        url: "/sw.js",
        content_type: JAVASCRIPT,
    },
    Fixed {
        constant: "MANIFEST",
        file: "manifest.webmanifest",
        url: "/manifest.webmanifest",
        content_type: "application/manifest+json",
    },
];

fn main() {
    println!("cargo:rerun-if-changed=static");
    println!("cargo:rerun-if-changed=src/assets/hash.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let static_dir = manifest_dir.join("static");

    let mut code = String::new();
    let mut placeholders = Vec::new();
    let mut hashed_urls = Vec::new();

    for asset in HASHED {
        let path = static_dir.join(asset.file);
        let bytes =
            fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", asset.file));
        let url = format!("/static/{}", hash::hashed_file_name(asset.file, &bytes));
        write_asset(&mut code, asset.constant, &url, asset.content_type, &path);
        placeholders.push((format!("{{{{{}}}}}", asset.constant), url.clone()));
        hashed_urls.push(url);
    }

    let version = format!(
        "{}-{:016x}",
        env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION"),
        hash::fingerprint(hashed_urls.join("\n").as_bytes())
    );
    placeholders.push(("{{VERSION}}".to_owned(), version));
    placeholders.push(("{{PRECACHE}}".to_owned(), json_array(&hashed_urls)));

    for asset in FIXED {
        let file = asset.file;
        let mut text = fs::read_to_string(static_dir.join(file))
            .unwrap_or_else(|err| panic!("cannot read {file}: {err}"));
        for (placeholder, value) in &placeholders {
            text = text.replace(placeholder, value);
        }
        assert!(!text.contains("{{"), "{file} has an unknown placeholder");
        let path = out_dir.join(file);
        fs::write(&path, text).unwrap_or_else(|err| panic!("cannot write {file}: {err}"));
        write_asset(
            &mut code,
            asset.constant,
            asset.url,
            asset.content_type,
            &path,
        );
    }

    let hashed: Vec<&str> = HASHED.iter().map(|asset| asset.constant).collect();
    let fixed: Vec<&str> = FIXED.iter().map(|asset| asset.constant).collect();
    let _ = writeln!(
        code,
        "/// Assets served under `/static/` with a content-hash name.\n\
         pub const HASHED: &[Asset] = &[{}];\n\
         /// Assets served at a fixed URL.\n\
         pub const FIXED: &[Asset] = &[{}];",
        hashed.join(", "),
        fixed.join(", "),
    );

    fs::write(out_dir.join("assets.rs"), code).expect("cannot write assets.rs");
}

fn write_asset(code: &mut String, constant: &str, url: &str, content_type: &str, path: &Path) {
    let path = path.to_str().expect("asset paths are UTF-8");
    let _ = writeln!(
        code,
        "pub const {constant}: Asset = Asset {{ url: {url:?}, content_type: {content_type:?}, \
         body: include_bytes!({path:?}) }};"
    );
}

fn json_array(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|value| format!("{value:?}")).collect();
    format!("[{}]", items.join(", "))
}
