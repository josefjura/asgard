//! A throwaway design prototype for `/dev/gallery`. Debug builds only; not for merging.
//!
//! It shows a display face for page titles and the Balance figure, Finance with tap-through
//! entries and the Settlement plan folded into the Crew's Balances, the Expense edit page with
//! Delete at its foot, and Settings with one weight of button per job.
//!
//! The faces load from `target/proto-fonts/` (Marcellus-Regular.ttf, Grenze.ttf, Alegreya.ttf,
//! all OFL from github.com/google/fonts). Without them the titles fall back to system-ui.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::civil::Date;
use maud::{Markup, PreEscaped, html};

use crate::domain::finance::money::{Crowns, Standing};
use crate::domain::finance::{Entry, Overview};
use crate::domain::members::Member;
use crate::views::finance::{self, ExpenseProblems, ExpenseValues, Purpose, crowns, paths};
use crate::views::gallery::Fixture;
use crate::views::layout::shell;
use crate::views::nav::{ADMIN_HREF, Place, Tab};
use crate::views::passkey::{self, Ceremony, CeremonyUrls};
use crate::views::{display_name, passkeys, push};

const FINANCE_HEIGHT: u16 = 1250;
const EDIT_HEIGHT: u16 = 900;
const SETTINGS_HEIGHT: u16 = 1500;

/// A candidate display face: its file in `target/proto-fonts/`, and how titles set in it.
struct Face {
    name: &'static str,
    file: Option<&'static str>,
    family: &'static str,
    weights: &'static str,
    css: &'static str,
}

const FACES: [Face; 4] = [
    Face {
        name: "Marcellus",
        file: Some("Marcellus-Regular.ttf"),
        family: "Marcellus",
        weights: "400",
        css: "--display: 'Marcellus', serif; --display-weight: 400; --display-track: 0.01em;",
    },
    Face {
        name: "Grenze",
        file: Some("Grenze.ttf"),
        family: "Grenze",
        weights: "100 900",
        css: "--display: 'Grenze', serif; --display-weight: 600; --display-track: 0;",
    },
    Face {
        name: "Alegreya",
        file: Some("Alegreya.ttf"),
        family: "Alegreya",
        weights: "400 900",
        css: "--display: 'Alegreya', serif; --display-weight: 800; --display-track: -0.01em;",
    },
    Face {
        name: "System (today)",
        file: None,
        family: "",
        weights: "",
        css: "--display: inherit; --display-weight: 700; --display-track: -0.01em;",
    },
];

fn font_face(face: &Face) -> String {
    let Some(file) = face.file else {
        return String::new();
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/proto-fonts")
        .join(file);
    std::fs::read(path).map_or_else(
        |_| String::new(),
        |bytes| {
            format!(
                "@font-face {{ font-family: '{}'; font-weight: {}; src: url(data:font/ttf;base64,{}) format('truetype'); }}",
                face.family,
                face.weights,
                STANDARD.encode(bytes)
            )
        },
    )
}

/// The prototype's styles, layered over `app.css`.
const PROTO_CSS: &str = "
/* The one bold move: a display face for page titles and the Balance figure, nowhere else. */
.shell-main h1 {
  font-family: var(--display);
  font-weight: var(--display-weight);
  letter-spacing: var(--display-track);
  font-size: 2rem;
}
.balance-figure {
  font-family: var(--display);
  font-weight: var(--display-weight);
  letter-spacing: var(--display-track);
  font-size: 3rem;
  font-variant-numeric: lining-nums tabular-nums;
}

/* The Crew's Balances, each sender with what they send beneath. */
.balance-list li { flex-wrap: wrap; }
.crew-sends {
  flex-basis: 100%;
  margin-top: calc(-1 * var(--space-1));
  color: var(--muted);
  font-size: 0.875rem;
}

/* Entries: a row its Member may change is a link to its edit page, marked with a chevron. The
   chevron's column is kept on every row so the amounts line up. */
.entry-list .entry { display: block; padding: 0; }
.entry-link {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto 1rem;
  gap: var(--space-2);
  align-items: baseline;
  padding: var(--space-2) 0;
  color: inherit;
  text-decoration: none;
}
a.entry-link:hover .entry-what { text-decoration: underline; }
.entry-link .entry-amount { padding-left: var(--space-2); }
.chevron {
  width: 1rem;
  height: 1rem;
  /* Level with the first line, beside the amount. */
  align-self: start;
  margin-top: 0.25rem;
  fill: none;
  stroke: var(--muted);
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

/* Delete sits at the foot of the edit page, away from Save. */
.delete-zone {
  max-width: 28rem;
  margin-top: var(--space-5);
  padding-top: var(--space-4);
  border-top: 1px solid var(--line);
}
.delete-zone p { margin: 0 0 var(--space-3); color: var(--muted); font-size: 0.875rem; }

/* Settings: one primary at most, a submit beside its field is secondary, helper text is muted,
   and sections sit an even distance apart. */

.section .name-form label { font-size: 1.125rem; margin-bottom: var(--space-2); }
.field-row .button,
.outline-buttons .button:not(.danger) {
  border: 1px solid var(--edge);
  background: transparent;
  color: var(--ink);
}

.row-link {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 3rem;
  padding: var(--space-2) 0;
  border-block: 1px solid var(--line);
  color: var(--ink);
  font-weight: 600;
  text-decoration: none;
}
";

fn chevron() -> Markup {
    html! {
        svg.chevron viewBox="0 0 16 16" aria-hidden="true" focusable="false" {
            path d="M6 3l5 5-5 5" {}
        }
    }
}

/// `content` in the shell, with the prototype's styles and `face`'s title settings.
fn page(place: Place, face: &Face, content: &Markup) -> Markup {
    shell(
        place,
        &html! {
            style {
                (PreEscaped(font_face(face)))
                ":root { " (PreEscaped(face.css)) " }"
                (PreEscaped(PROTO_CSS))
            }
            (content)
        },
    )
}

fn day(date: Date) -> String {
    date.strftime("%-d %B %Y").to_string()
}

fn balance_text(balance: Crowns) -> String {
    match balance.standing() {
        Standing::Even => "Even".to_owned(),
        Standing::Owed => format!("+{}", crowns(balance)),
        Standing::Owes => crowns(balance),
    }
}

const fn standing_class(balance: Crowns) -> &'static str {
    match balance.standing() {
        Standing::Owed => "owed",
        Standing::Owes => "owes",
        Standing::Even => "even",
    }
}

/// Finance with the Settlement plan folded into the Crew's Balances and tap-through entries.
fn finance_home(me: &Member, overview: &Overview, face: &Face) -> Markup {
    let mine = overview.balance_of(me.id);
    let my_settlements: Vec<_> = overview.settlements_of(me.id).collect();
    page(
        Place::Tab(Tab::Finance),
        face,
        &html! {
            h1 { "Finance" }

            section.card.my-balance.(standing_class(mine)) aria-labelledby="my-balance" {
                h2 #my-balance { "Your Balance" }
                p.balance-figure { (balance_text(mine)) }
                p.balance-says {
                    (match mine.standing() {
                        Standing::Owed => "The Crew owes you this.",
                        Standing::Owes => "You owe the Crew this.",
                        Standing::Even => "Nobody owes you and you owe nobody.",
                    })
                }
            }

            @if !my_settlements.is_empty() {
                section.section aria-labelledby="my-settlements" {
                    h2 #my-settlements { "Settle up" }
                    ul.settlement-list {
                        @for settlement in &my_settlements {
                            li.card.settlement {
                                span.settlement-what {
                                    @if settlement.sender.id == me.id {
                                        "Send " (settlement.recipient.display_name) " "
                                        strong { (crowns(settlement.amount)) }
                                    } @else {
                                        (settlement.sender.display_name) " should send you "
                                        strong { (crowns(settlement.amount)) }
                                    }
                                }
                                a.button.secondary href=(paths::NEW_PAYMENT) { "Record" }
                            }
                        }
                    }
                }
            }

            div.finance-actions {
                a.button.large href=(paths::NEW_EXPENSE) { "+ Expense" }
                a.button.secondary href=(paths::NEW_PAYMENT) { "+ Payment" }
            }

            section.section aria-labelledby="crew-balances" {
                h2 #crew-balances { "Crew Balances" }
                ul.balance-list {
                    @for balance in &overview.balances {
                        li.(standing_class(balance.balance)) {
                            span.balance-name {
                                (balance.member.display_name)
                                @if balance.member.id == me.id { " (you)" }
                            }
                            span.balance-amount { (balance_text(balance.balance)) }
                            @for settlement in overview.settlements.iter()
                                .filter(|settlement| settlement.sender.id == balance.member.id) {
                                span.crew-sends {
                                    "Sends " (settlement.recipient.display_name) " "
                                    (crowns(settlement.amount))
                                }
                            }
                        }
                    }
                }
            }

            section.section aria-labelledby="latest-entries" {
                h2 #latest-entries { "Latest entries" }
                ul.entry-list {
                    @for entry in &overview.recent {
                        (entry_row(me, entry))
                    }
                }
                p.history-link { a href=(paths::HISTORY) { "See every entry in History" } }
            }
        },
    )
}

/// An entry as one row, a link to its edit page if `me` may change it.
fn entry_row(me: &Member, entry: &Entry) -> Markup {
    let (href, what, meta, edited, amount) = match entry {
        Entry::Expense(expense) => (
            paths::edit_expense(expense.id),
            expense.description.as_str().to_owned(),
            format!(
                "{} paid · {}",
                expense.payer.display_name,
                day(expense.expense_date)
            ),
            expense.edited.as_ref(),
            expense.amount.crowns(),
        ),
        Entry::Payment(payment) => (
            paths::edit_payment(payment.id),
            format!("{} → {}", payment.sender.display_name, payment.recipient),
            payment.description.as_ref().map_or_else(
                || format!("Payment · {}", day(payment.logged_on)),
                |description| {
                    format!(
                        "Payment · {} · {}",
                        description.as_str(),
                        day(payment.logged_on)
                    )
                },
            ),
            payment.edited.as_ref(),
            payment.amount.crowns(),
        ),
    };
    let body = html! {
        span.entry-main {
            span.entry-what { (what) }
            span.entry-meta { (meta) }
            @if let Some(edit) = edited {
                span.entry-meta.entry-edited { "edited by " (edit.by) " · " (day(edit.on)) }
            }
        }
        span.entry-amount { (crowns(amount)) }
    };
    html! {
        li.entry {
            @if entry.may_be_changed_by(me) {
                a.entry-link href=(href) { (body) (chevron()) }
            } @else {
                div.entry-link { (body) span {} }
            }
        }
    }
}

/// The Expense edit page with Delete at its foot.
fn edit_expense(face: &Face) -> Markup {
    use finance::fixtures::{ONDRA, TODAY, crew};

    let values = ExpenseValues {
        amount: "12480",
        description: "Pilsner malt, 3 sacks, and a new hydrometer",
        payer: ONDRA,
        expense_date: "2026-10-28",
    };
    let delete = "/expenses/7/delete";
    page(
        Place::Tab(Tab::Finance),
        face,
        &html! {
            h1 { "Edit Expense" }
            (finance::expense_form(
                "/expenses/7",
                Purpose::Edit,
                &crew(),
                &values,
                &ExpenseProblems::default(),
                TODAY,
            ))
            section.delete-zone aria-label="Delete" {
                p { "Deleting takes it out of everyone's Balance." }
                form method="post" action=(delete) hx-post=(delete)
                    hx-confirm="Delete the Expense \u{201c}Pilsner malt, 3 sacks, and a new hydrometer\u{201d}?" {
                    button.button.danger type="submit" { "Delete Expense" }
                }
            }
        },
    )
}

/// Settings with one weight of button per job.
fn settings(face: &Face) -> Markup {
    let base = "/settings/passkeys";
    let urls = CeremonyUrls {
        start: format!("{base}/start"),
        finish: format!("{base}/finish"),
    };
    page(
        Place::Settings,
        face,
        &html! {
            h1 { "Settings" }

            section.section.narrow {
                (display_name::form("/settings/name", "Ondra", None))
            }

            section.section.narrow.outline-buttons aria-labelledby="settings-passkeys" {
                h2 #settings-passkeys { "Passkeys" }
                (passkeys::list(base, &passkeys::fixtures::several(), None))
                p.muted {
                    "Add a passkey on each device you sign in from, unless your passkeys already sync to it."
                }
                (passkey::button("Add a passkey", Ceremony::Register, &urls))
            }

            section.section.narrow aria-labelledby="settings-push" {
                h2 #settings-push { "Notifications" }
                (push::control())
            }

            section.section.narrow aria-labelledby="settings-admin" {
                h2 #settings-admin { "Admin" }
                a.row-link href=(ADMIN_HREF) { "Invite links" (chevron()) }
            }

            section.section.narrow.outline-buttons aria-labelledby="settings-device" {
                h2 #settings-device { "This device" }
                p.muted { "Logging out signs this device out. Your passkey stays, so you can sign back in." }
                form method="post" action="/log-out" {
                    button.button type="submit" { "Log out" }
                }
            }
        },
    )
}

/// The prototype's fixtures: Finance in each candidate face, then the edit page and Settings.
#[must_use]
pub fn fixtures() -> Vec<Fixture> {
    use finance::fixtures::{JOSEF, member, overview};

    let homes = FACES.iter().map(|face| Fixture {
        component: "Prototype: Finance home",
        state: face.name.to_owned(),
        document: finance_home(&member(JOSEF), &overview(), face),
        height: FINANCE_HEIGHT,
    });
    let today = &FACES[3];
    homes
        .chain([
            Fixture {
                component: "Prototype: Edit Expense",
                state: "Delete at the foot".to_owned(),
                document: edit_expense(today),
                height: EDIT_HEIGHT,
            },
            Fixture {
                component: "Prototype: Settings",
                state: "One weight per job".to_owned(),
                document: settings(today),
                height: SETTINGS_HEIGHT,
            },
        ])
        .collect()
}
