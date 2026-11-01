//! The foot of an edit page: what deleting the thing does, and the button that deletes it.

use maud::{Markup, html};

/// What deleting one thing does, as the foot of its edit page tells it.
#[derive(Debug, Clone, Copy)]
pub struct Deletion<'a> {
    /// What is deleted, as the button names it: "Expense", "Reminder".
    pub kind: &'a str,
    /// The sentence above the button, saying what deleting it changes.
    pub explains: &'a str,
    /// The sentence the confirmation adds after "Delete this {kind}?".
    pub warns: &'a str,
    /// Where the button posts.
    pub action: &'a str,
}

/// The foot of an edit page, ruled off from Save: what deleting `deletion` changes, and the
/// button that deletes it after asking. Without htmx, it posts as a plain form.
#[must_use]
pub fn delete_section(deletion: &Deletion) -> Markup {
    let Deletion {
        kind,
        explains,
        warns,
        action,
    } = *deletion;
    html! {
        section.delete-section aria-label={ "Delete this " (kind) } {
            p { (explains) }
            form method="post" action=(action) hx-post=(action)
                hx-confirm={ "Delete this " (kind) "? " (warns) } {
                button.button.danger type="submit" { "Delete " (kind) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_before_deleting_and_posts_without_htmx_too() {
        let html = delete_section(&Deletion {
            kind: "Reminder",
            explains: "Deleting this Reminder means nobody is notified about it.",
            warns: "Nobody will be notified about it.",
            action: "/reminders/3/delete",
        })
        .into_string();

        assert_eq!(
            html,
            r#"<section class="delete-section" aria-label="Delete this Reminder"><p>Deleting this Reminder means nobody is notified about it.</p><form method="post" action="/reminders/3/delete" hx-post="/reminders/3/delete" hx-confirm="Delete this Reminder? Nobody will be notified about it."><button class="button danger" type="submit">Delete Reminder</button></form></section>"#
        );
    }
}
