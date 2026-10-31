//! Maud components and pages: pure functions from data to HTML, no DB access.

pub mod admin;
pub mod brand;
pub mod display_name;
pub mod finance;
#[cfg(debug_assertions)]
pub mod gallery;
pub mod history;
pub mod icon;
pub mod invite;
pub mod layout;
pub mod nav;
pub mod offline;
pub mod passkey;
pub mod passkeys;
pub mod push;
pub mod settings;
pub mod sign_in;
pub mod tabs;
pub mod welcome;

/// Snapshots markup one tag per line, with asset hashes masked so a CSS edit doesn't churn
/// every page snapshot.
#[cfg(test)]
macro_rules! assert_html_snapshot {
    ($markup:expr) => {
        insta::with_settings!({ filters => vec![(r"\.[0-9a-f]{16}\.", ".[hash].")] }, {
            insta::assert_snapshot!($markup.into_string().replace("><", ">\n<"));
        })
    };
    ($name:expr, $markup:expr) => {
        insta::with_settings!({ filters => vec![(r"\.[0-9a-f]{16}\.", ".[hash].")] }, {
            insta::assert_snapshot!($name, $markup.into_string().replace("><", ">\n<"));
        })
    };
}
#[cfg(test)]
pub(crate) use assert_html_snapshot;
