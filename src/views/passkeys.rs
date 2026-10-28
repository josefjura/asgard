use maud::{Markup, html};

use crate::domain::auth::{PasskeyId, PasskeyName, PasskeySummary};

/// The element each rename and delete form swaps for the list the server answers with.
pub const LIST_ID: &str = "passkeys";

/// What became of the last rename or delete sent from a [`list`].
#[derive(Debug, Clone, Copy)]
pub enum Outcome<'a> {
    Renamed(PasskeyId),
    /// The name typed for passkey `id` was refused, with the sentence that says why.
    RenameRefused {
        id: PasskeyId,
        value: &'a str,
        message: &'a str,
    },
    Deleted(&'a PasskeyName),
}

/// Where a passkey's rename form posts, under `base`.
#[must_use]
pub fn rename_path(base: &str, id: PasskeyId) -> String {
    format!("{base}/{}/name", id.get())
}

/// Where a passkey's delete form posts, under `base`.
#[must_use]
pub fn delete_path(base: &str, id: PasskeyId) -> String {
    format!("{base}/{}/delete", id.get())
}

/// The Member's passkeys, each with a form to rename it and a button to delete it.
///
/// Their only passkey has no delete button. The forms post under `base`; htmx swaps the whole
/// list for the answer, and without htmx they post as plain forms.
#[must_use]
pub fn list(base: &str, passkeys: &[PasskeySummary], outcome: Option<Outcome>) -> Markup {
    let deletable = passkeys.len() > 1;
    html! {
        div.passkeys #(LIST_ID) {
            ul.passkey-list {
                @for passkey in passkeys {
                    (item(base, passkey, deletable, outcome))
                }
            }
            p.note role="status" {
                @if let Some(Outcome::Deleted(name)) = outcome {
                    "Deleted " strong { (name) } ". Any device signed in with it is now signed out."
                } @else if !deletable {
                    "Your only passkey can't be deleted. Add another one first."
                }
            }
        }
    }
}

fn item(base: &str, passkey: &PasskeySummary, deletable: bool, outcome: Option<Outcome>) -> Markup {
    let id = passkey.id;
    let input_id = format!("passkey-{}-name", id.get());
    let note_id = format!("passkey-{}-note", id.get());
    let (value, note, refused) = match outcome {
        Some(Outcome::Renamed(renamed)) if renamed == id => {
            (passkey.name.as_str(), Some("Saved."), false)
        }
        Some(Outcome::RenameRefused {
            id: refused,
            value,
            message,
        }) if refused == id => (value, Some(message), true),
        _ => (passkey.name.as_str(), None, false),
    };
    let rename = rename_path(base, id);
    let delete = delete_path(base, id);
    let target = format!("#{LIST_ID}");
    html! {
        li.passkey {
            form.passkey-rename method="post" action=(rename) hx-post=(rename)
                hx-target=(target) hx-swap="outerHTML" {
                label for=(input_id) {
                    "Passkey added " (passkey.added_on.strftime("%-d %B %Y"))
                }
                div.field-row {
                    input #(input_id) type="text" name="name" value=(value) required
                        autocomplete="off" aria-describedby=[note.map(|_| &note_id)]
                        aria-invalid=[refused.then_some("true")];
                    button.button type="submit" { "Rename" }
                }
                @if let Some(note) = note {
                    p #(note_id) .note .refused[refused] role="status" { (note) }
                }
            }
            @if deletable {
                form.passkey-delete method="post" action=(delete) hx-post=(delete)
                    hx-target=(target) hx-swap="outerHTML"
                    hx-confirm={ "Delete the passkey \u{201c}" (passkey.name) "\u{201d}? Any device signed in with it will be signed out." } {
                    button.button.quiet type="submit" { "Delete " (passkey.name) }
                }
            }
        }
    }
}

/// Passkeys for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::date;

    use crate::domain::auth::{PasskeyId, PasskeyName, PasskeySummary};

    /// # Panics
    ///
    /// If a fixture name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn passkey(id: i64, name: &str, added_on: (i16, i8, i8)) -> PasskeySummary {
        PasskeySummary {
            id: PasskeyId::from_client(id),
            name: PasskeyName::parse(name).expect("fixture names are valid"),
            added_on: date(added_on.0, added_on.1, added_on.2),
        }
    }

    #[must_use]
    pub fn one() -> Vec<PasskeySummary> {
        vec![passkey(1, "iPhone Safari", (2026, 3, 14))]
    }

    #[must_use]
    pub fn several() -> Vec<PasskeySummary> {
        vec![
            passkey(1, "iPhone Safari", (2026, 3, 14)),
            passkey(2, "Windows Edge", (2026, 4, 2)),
            passkey(3, "Old Android", (2026, 10, 28)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{one, several};
    use super::*;
    use crate::views::assert_html_snapshot;

    const BASE: &str = "/settings/passkeys";

    #[test]
    fn passkeys_one() {
        assert_html_snapshot!(list(BASE, &one(), None));
    }

    #[test]
    fn passkeys_several() {
        assert_html_snapshot!(list(BASE, &several(), None));
    }

    #[test]
    fn the_only_passkey_has_no_delete_button() {
        let html = list(BASE, &one(), None).into_string();

        assert!(!html.contains("/delete"));
        assert!(html.contains("Your only passkey can't be deleted."));
    }

    #[test]
    fn every_one_of_several_passkeys_can_be_deleted() {
        let html = list(BASE, &several(), None).into_string();

        for id in 1..=3 {
            assert!(html.contains(&format!("hx-post=\"{BASE}/{id}/delete\"")));
            assert!(html.contains(&format!("hx-post=\"{BASE}/{id}/name\"")));
        }
    }

    #[test]
    fn shows_the_date_each_passkey_was_added() {
        let html = list(BASE, &several(), None).into_string();

        assert!(html.contains("Passkey added 14 March 2026"));
        assert!(html.contains("Passkey added 2 April 2026"));
    }

    #[test]
    fn says_a_rename_was_saved_beside_that_passkey_only() {
        let passkeys = several();
        let html = list(BASE, &passkeys, Some(Outcome::Renamed(passkeys[1].id))).into_string();

        assert_eq!(html.matches("Saved.").count(), 1);
        assert!(html.contains("id=\"passkey-2-note\""));
        assert!(html.contains("aria-describedby=\"passkey-2-note\""));
    }

    #[test]
    fn a_refused_rename_keeps_what_was_typed() {
        let passkeys = several();
        let html = list(
            BASE,
            &passkeys,
            Some(Outcome::RenameRefused {
                id: passkeys[0].id,
                value: "\"><b>",
                message: "Enter a name for the passkey.",
            }),
        )
        .into_string();

        assert!(html.contains("value=\"&quot;&gt;&lt;b&gt;\""));
        assert!(html.contains("aria-invalid=\"true\""));
        assert!(html.contains("Enter a name for the passkey."));
    }

    #[test]
    fn says_which_passkey_was_deleted() {
        let name = PasskeyName::parse("Lost phone").unwrap();
        let html = list(BASE, &several(), Some(Outcome::Deleted(&name))).into_string();

        assert!(html.contains("Deleted <strong>Lost phone</strong>."));
    }
}
