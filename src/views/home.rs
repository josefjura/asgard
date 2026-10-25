use maud::{DOCTYPE, Markup, html};

/// The placeholder home page, until the app shell lands.
#[must_use]
pub fn page() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Asgard" }
            }
            body {
                main {
                    h1 { "Asgard" }
                    p { "The Crew's shared money, Shopping list, Reminders and Quotes are on their way." }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_page() {
        insta::assert_snapshot!(super::page().into_string());
    }
}
