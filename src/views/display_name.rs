use maud::{Markup, html};

/// What became of the last name sent from a [`form`].
#[derive(Debug, Clone, Copy)]
pub enum Outcome<'a> {
    Saved,
    /// Refused, with the sentence that says why.
    Refused(&'a str),
}

/// The form a Member changes their display name with, showing `value` and the outcome of the
/// last attempt. htmx swaps the form for the one the server answers with; without htmx, it
/// posts as a plain form.
#[must_use]
pub fn form(action: &str, value: &str, outcome: Option<Outcome>) -> Markup {
    let refused = matches!(outcome, Some(Outcome::Refused(_)));
    html! {
        form.name-form method="post" action=(action) hx-post=(action) hx-swap="outerHTML" {
            label for="display-name" { "Display name" }
            div.field-row {
                input #display-name type="text" name="display_name" value=(value) required
                    autocomplete="nickname" aria-describedby="display-name-note"
                    aria-invalid=[refused.then_some("true")];
                button.button type="submit" { "Save" }
            }
            p #display-name-note .note .refused[refused] role="status" {
                @match outcome {
                    Some(Outcome::Saved) => "Saved.",
                    Some(Outcome::Refused(message)) => (message),
                    None => "This is how the Crew sees you.",
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::assert_html_snapshot;

    #[test]
    fn display_name_form() {
        assert_html_snapshot!(form("/welcome/name", "Ondra", None));
    }

    #[test]
    fn display_name_form_saved() {
        assert_html_snapshot!(form("/welcome/name", "Ondřej", Some(Outcome::Saved)));
    }

    #[test]
    fn display_name_form_refused() {
        assert_html_snapshot!(form(
            "/welcome/name",
            "Josef",
            Some(Outcome::Refused(
                "Someone in the Crew is already called Josef."
            ))
        ));
    }

    #[test]
    fn escapes_what_was_typed() {
        let html = form("/welcome/name", "\"><b>Ondra", None).into_string();

        assert!(html.contains("value=\"&quot;&gt;&lt;b&gt;Ondra\""));
    }
}
