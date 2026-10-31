//! The Finance tab: everyone's Balance, the latest entries, and the Expense and Payment forms.

use jiff::civil::Date;
use maud::{Markup, html};

use crate::domain::finance::money::{Crowns, Standing};
use crate::domain::finance::{DESCRIPTION_MAX, Entry, ExpenseEntry, Overview, PaymentEntry};
use crate::domain::members::{MemberId, MemberName};
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};

/// `crowns` the way the Crew reads money: thousands grouped by a space that never breaks, and
/// "Kč". A negative amount gets a true minus sign.
#[must_use]
pub fn crowns(crowns: Crowns) -> String {
    let digits = crowns.get().unsigned_abs().to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push('\u{a0}');
        }
        grouped.push(digit);
    }
    let sign = if crowns.get() < 0 { "\u{2212}" } else { "" };
    format!("{sign}{grouped}\u{a0}Kč")
}

/// A Balance as the Crew list shows it: Even, or with its sign spelled out so owing and being
/// owed never look alike.
fn balance_text(balance: Crowns) -> String {
    match balance.standing() {
        Standing::Even => "Even".to_owned(),
        Standing::Owed => format!("+{}", crowns(balance)),
        Standing::Owes => crowns(balance),
    }
}

/// Where a Member stands, as a class the stylesheet colours.
const fn standing_class(balance: Crowns) -> &'static str {
    match balance.standing() {
        Standing::Owed => "owed",
        Standing::Owes => "owes",
        Standing::Even => "even",
    }
}

/// A date the way lists show it.
fn day(date: Date) -> String {
    date.strftime("%-d %B %Y").to_string()
}

/// The Finance home screen for `me`: their Balance, the buttons that lead to `new_expense` and
/// `new_payment`, everyone's Balance and the latest entries.
#[must_use]
pub fn home(me: MemberId, overview: &Overview, new_expense: &str, new_payment: &str) -> Markup {
    let mine = overview.balance_of(me);
    shell(
        Place::Tab(Tab::Finance),
        &html! {
            h1 { "Finance" }

            section.my-balance.(standing_class(mine)) aria-labelledby="my-balance" {
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

            div.finance-actions {
                a.button.button-large href=(new_expense) { "+ Expense" }
                a.button.button-secondary href=(new_payment) { "+ Payment" }
            }

            section.finance-section aria-labelledby="crew-balances" {
                h2 #crew-balances { "Crew Balances" }
                ul.balance-list {
                    @for balance in &overview.balances {
                        li.(standing_class(balance.balance)) {
                            span.balance-name {
                                (balance.member.display_name)
                                @if balance.member.id == me { " (you)" }
                            }
                            span.balance-amount { (balance_text(balance.balance)) }
                        }
                    }
                }
            }

            section.finance-section aria-labelledby="latest-entries" {
                h2 #latest-entries { "Latest entries" }
                @if overview.recent.is_empty() {
                    p.lead-in { "Nothing logged yet. Log the first Expense when someone buys something for the brewery." }
                } @else {
                    ul.entry-list {
                        @for entry in &overview.recent {
                            (entry_row(entry))
                        }
                    }
                }
            }
        },
    )
}

/// One Expense or Payment in a list.
#[must_use]
pub fn entry_row(entry: &Entry) -> Markup {
    match entry {
        Entry::Expense(expense) => expense_row(expense),
        Entry::Payment(payment) => payment_row(payment),
    }
}

/// An Expense: what it was for, who paid, when, and how much.
fn expense_row(entry: &ExpenseEntry) -> Markup {
    html! {
        li.entry {
            div.entry-main {
                span.entry-what { (entry.description.as_str()) }
                span.entry-meta { (entry.payer) " paid · " (day(entry.expense_date)) }
            }
            span.entry-amount { (crowns(entry.amount.crowns())) }
        }
    }
}

/// A Payment: who sent it to whom, what for if anything, when, and how much.
fn payment_row(entry: &PaymentEntry) -> Markup {
    html! {
        li.entry.payment {
            div.entry-main {
                span.entry-what { (entry.sender) " → " (entry.recipient) }
                span.entry-meta {
                    "Payment · "
                    @if let Some(description) = &entry.description {
                        (description.as_str()) " · "
                    }
                    (day(entry.logged_on))
                }
            }
            span.entry-amount { (crowns(entry.amount.crowns())) }
        }
    }
}

/// What the Expense form shows in its fields: what was typed, or the defaults.
#[derive(Debug, Clone)]
pub struct ExpenseValues<'a> {
    pub amount: &'a str,
    pub description: &'a str,
    pub payer: MemberId,
    pub expense_date: &'a str,
}

impl<'a> ExpenseValues<'a> {
    /// A new Expense paid by `payer` on `expense_date`, with nothing typed yet.
    #[must_use]
    pub const fn blank(payer: MemberId, expense_date: &'a str) -> Self {
        Self {
            amount: "",
            description: "",
            payer,
            expense_date,
        }
    }
}

/// Why each field of the Expense form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct ExpenseProblems {
    pub amount: Option<String>,
    pub description: Option<String>,
    pub payer: Option<String>,
    pub expense_date: Option<String>,
}

/// The page around the Expense form.
#[must_use]
pub fn expense_page(form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Finance),
        &html! {
            h1 { "New Expense" }
            (form)
        },
    )
}

/// The form an Expense is logged with, posting to `action`.
///
/// It shows `values` and, beside each field, what was wrong with it. The Payer is one of
/// `members`; the Expense date can't be after `today`, and while it is today it stays folded
/// away. htmx swaps the form for the one the server answers with; without htmx, it posts as a
/// plain form.
#[must_use]
pub fn expense_form(
    action: &str,
    members: &[MemberName],
    values: &ExpenseValues,
    problems: &ExpenseProblems,
    today: Date,
) -> Markup {
    let today_value = today.to_string();
    let is_today = values.expense_date == today_value;
    let spent = if is_today {
        "today".to_owned()
    } else {
        values.expense_date.parse::<Date>().map_or_else(
            |_| "on another day".to_owned(),
            |date| format!("on {}", day(date)),
        )
    };
    html! {
        form.entry-form method="post" action=(action) hx-post=(action) hx-swap="outerHTML" {
            div.field {
                label for="expense-amount" { "Amount in Kč" }
                input #expense-amount type="text" inputmode="numeric" name="amount"
                    value=(values.amount) required autocomplete="off" autofocus
                    aria-invalid=[problems.amount.is_some().then_some("true")]
                    aria-describedby=[problems.amount.is_some().then_some("expense-amount-problem")];
                (problem("expense-amount-problem", problems.amount.as_deref()))
            }
            div.field {
                label for="expense-description" { "What for" }
                input #expense-description type="text" name="description"
                    value=(values.description) required maxlength=(DESCRIPTION_MAX)
                    autocomplete="off"
                    aria-invalid=[problems.description.is_some().then_some("true")]
                    aria-describedby=[problems.description.is_some().then_some("expense-description-problem")];
                (problem("expense-description-problem", problems.description.as_deref()))
            }
            div.field {
                label for="expense-payer" { "Paid by" }
                select #expense-payer name="payer"
                    aria-invalid=[problems.payer.is_some().then_some("true")]
                    aria-describedby=[problems.payer.is_some().then_some("expense-payer-problem")] {
                    @for member in members {
                        option value=(member.id.get()) selected[member.id == values.payer] {
                            (member.display_name)
                        }
                    }
                }
                (problem("expense-payer-problem", problems.payer.as_deref()))
            }
            details.field.expense-date open[!is_today || problems.expense_date.is_some()] {
                summary { "Spent " (spent) }
                label for="expense-date" { "Expense date" }
                input #expense-date type="date" name="expense_date" value=(values.expense_date)
                    max=(today_value) required
                    aria-invalid=[problems.expense_date.is_some().then_some("true")]
                    aria-describedby=[problems.expense_date.is_some().then_some("expense-date-problem")];
                (problem("expense-date-problem", problems.expense_date.as_deref()))
            }
            button.button.button-large type="submit" { "Log Expense" }
        }
    }
}

/// What the Payment form shows in its fields: what was typed, or the defaults.
#[derive(Debug, Clone)]
pub struct PaymentValues<'a> {
    pub amount: &'a str,
    pub description: &'a str,
    pub sender: MemberId,
    /// `None` until someone is chosen.
    pub recipient: Option<MemberId>,
}

impl PaymentValues<'_> {
    /// A new Payment sent by `sender`, with nothing typed or chosen yet.
    #[must_use]
    pub const fn blank(sender: MemberId) -> Self {
        Self {
            amount: "",
            description: "",
            sender,
            recipient: None,
        }
    }
}

/// Why each field of the Payment form was refused, if it was.
#[derive(Debug, Clone, Default)]
pub struct PaymentProblems {
    pub amount: Option<String>,
    pub description: Option<String>,
    pub sender: Option<String>,
    pub recipient: Option<String>,
}

/// The page around the Payment form.
#[must_use]
pub fn payment_page(form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Finance),
        &html! {
            h1 { "New Payment" }
            (form)
        },
    )
}

/// The form a Payment is logged with, posting to `action`.
///
/// It shows `values` and, beside each field, what was wrong with it. The Sender and the
/// Recipient are each one of `members`; the Recipient starts unchosen. It swaps and posts like
/// the Expense form.
#[must_use]
pub fn payment_form(
    action: &str,
    members: &[MemberName],
    values: &PaymentValues,
    problems: &PaymentProblems,
) -> Markup {
    html! {
        form.entry-form method="post" action=(action) hx-post=(action) hx-swap="outerHTML" {
            div.field {
                label for="payment-amount" { "Amount in Kč" }
                input #payment-amount type="text" inputmode="numeric" name="amount"
                    value=(values.amount) required autocomplete="off" autofocus
                    aria-invalid=[problems.amount.is_some().then_some("true")]
                    aria-describedby=[problems.amount.is_some().then_some("payment-amount-problem")];
                (problem("payment-amount-problem", problems.amount.as_deref()))
            }
            div.field {
                label for="payment-sender" { "Sent by" }
                select #payment-sender name="sender"
                    aria-invalid=[problems.sender.is_some().then_some("true")]
                    aria-describedby=[problems.sender.is_some().then_some("payment-sender-problem")] {
                    @for member in members {
                        option value=(member.id.get()) selected[member.id == values.sender] {
                            (member.display_name)
                        }
                    }
                }
                (problem("payment-sender-problem", problems.sender.as_deref()))
            }
            div.field {
                label for="payment-recipient" { "Sent to" }
                select #payment-recipient name="recipient" required
                    aria-invalid=[problems.recipient.is_some().then_some("true")]
                    aria-describedby=[problems.recipient.is_some().then_some("payment-recipient-problem")] {
                    option value="" disabled selected[values.recipient.is_none()] { "Choose a Member" }
                    @for member in members {
                        option value=(member.id.get()) selected[Some(member.id) == values.recipient] {
                            (member.display_name)
                        }
                    }
                }
                (problem("payment-recipient-problem", problems.recipient.as_deref()))
            }
            div.field {
                label for="payment-description" { "What for " span.optional { "(optional)" } }
                input #payment-description type="text" name="description"
                    value=(values.description) maxlength=(DESCRIPTION_MAX) autocomplete="off"
                    aria-invalid=[problems.description.is_some().then_some("true")]
                    aria-describedby=[problems.description.is_some().then_some("payment-description-problem")];
                (problem("payment-description-problem", problems.description.as_deref()))
            }
            button.button.button-large type="submit" { "Log Payment" }
        }
    }
}

fn problem(id: &str, message: Option<&str>) -> Markup {
    html! {
        @if let Some(message) = message {
            p.field-problem id=(id) { (message) }
        }
    }
}

/// Members, Balances and entries for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use jiff::civil::{Date, date};

    use crate::domain::finance::money::{Amount, Crowns};
    use crate::domain::finance::{
        Description, Entry, ExpenseEntry, ExpenseId, MemberBalance, Overview, PaymentEntry,
        PaymentId,
    };
    use crate::domain::members::{DisplayName, MemberId, MemberName};

    pub const JOSEF: MemberId = MemberId::from_client(1);
    pub const ONDRA: MemberId = MemberId::from_client(2);
    pub const SIMON: MemberId = MemberId::from_client(3);

    /// The day the fixture forms are filled in.
    pub const TODAY: Date = date(2026, 10, 30);

    fn name(name: &str) -> DisplayName {
        DisplayName::parse(name).expect("fixture names are valid")
    }

    /// Josef, Ondra and Šimon, in the order they joined.
    ///
    /// # Panics
    ///
    /// If a fixture name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn crew() -> Vec<MemberName> {
        [(JOSEF, "Josef"), (ONDRA, "Ondra"), (SIMON, "Šimon")]
            .map(|(id, display_name)| MemberName {
                id,
                display_name: name(display_name),
            })
            .into()
    }

    /// A Malt Expense Ondra paid for.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn entry() -> Entry {
        expense(
            7,
            "Ondra",
            12_480,
            "Pilsner malt, 3 sacks, and a new hydrometer",
            date(2026, 10, 28),
        )
    }

    fn expense(id: i64, payer: &str, crowns: i64, description: &str, expense_date: Date) -> Entry {
        Entry::Expense(ExpenseEntry {
            id: ExpenseId::from_client(id),
            payer: name(payer),
            amount: Amount::new(crowns).expect("fixture amounts are valid"),
            description: Description::parse(description).expect("fixture descriptions are valid"),
            expense_date,
        })
    }

    /// Josef paying Ondra back, with or without a description.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn payment_entry(description: Option<&str>) -> Entry {
        payment(4, "Josef", "Ondra", 4_160, description, date(2026, 10, 29))
    }

    fn payment(
        id: i64,
        sender: &str,
        recipient: &str,
        crowns: i64,
        description: Option<&str>,
        logged_on: Date,
    ) -> Entry {
        Entry::Payment(PaymentEntry {
            id: PaymentId::from_client(id),
            sender: name(sender),
            recipient: name(recipient),
            amount: Amount::new(crowns).expect("fixture amounts are valid"),
            description: description
                .map(|text| Description::parse(text).expect("fixture descriptions are valid")),
            logged_on,
        })
    }

    /// The Crew after a few entries: Josef owes, Ondra is owed, Šimon is Even.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn overview() -> Overview {
        let balances = crew()
            .into_iter()
            .zip([-4160, 4161, -1])
            .map(|(member, crowns)| MemberBalance {
                member,
                balance: Crowns::new(crowns),
            })
            .collect();
        Overview {
            balances,
            recent: vec![
                payment_entry(None),
                entry(),
                expense(6, "Šimon", 380, "Saaz hops", date(2026, 10, 21)),
                expense(
                    5,
                    "Josef",
                    1_240,
                    "Bottle caps and sanitiser",
                    date(2026, 10, 2),
                ),
            ],
        }
    }

    /// A Crew that hasn't logged anything yet.
    ///
    /// # Panics
    ///
    /// If a fixture name is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn fresh() -> Overview {
        Overview {
            balances: crew()
                .into_iter()
                .map(|member| MemberBalance {
                    member,
                    balance: Crowns::ZERO,
                })
                .collect(),
            recent: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{
        JOSEF, ONDRA, SIMON, TODAY, crew, entry, fresh, overview, payment_entry,
    };
    use super::*;
    use crate::views::assert_html_snapshot;

    const NEW_EXPENSE: &str = "/expenses/new";
    const NEW_PAYMENT: &str = "/payments/new";
    const ACTION: &str = "/expenses";
    const PAYMENT_ACTION: &str = "/payments";

    fn empty_values(today: &str) -> ExpenseValues<'_> {
        ExpenseValues::blank(ONDRA, today)
    }

    #[test]
    fn groups_thousands_with_a_space_that_never_breaks() {
        for (amount, shown) in [
            (0, "0 Kč"),
            (7, "7 Kč"),
            (999, "999 Kč"),
            (1_000, "1 000 Kč"),
            (12_480, "12 480 Kč"),
            (1_000_000, "1 000 000 Kč"),
            (-4_160, "−4 160 Kč"),
        ] {
            assert_eq!(
                crowns(Crowns::new(amount)),
                shown.replace(' ', "\u{a0}"),
                "{amount}"
            );
        }
    }

    #[test]
    fn finance_home_owes() {
        assert_html_snapshot!(home(JOSEF, &overview(), NEW_EXPENSE, NEW_PAYMENT));
    }

    #[test]
    fn finance_home_owed() {
        assert_html_snapshot!(home(ONDRA, &overview(), NEW_EXPENSE, NEW_PAYMENT));
    }

    #[test]
    fn finance_home_even() {
        assert_html_snapshot!(home(SIMON, &overview(), NEW_EXPENSE, NEW_PAYMENT));
    }

    #[test]
    fn finance_home_with_nothing_logged() {
        assert_html_snapshot!(home(JOSEF, &fresh(), NEW_EXPENSE, NEW_PAYMENT));
    }

    #[test]
    fn finance_entry_row() {
        assert_html_snapshot!(entry_row(&entry()));
    }

    #[test]
    fn finance_payment_row() {
        assert_html_snapshot!(entry_row(&payment_entry(Some("For the malt"))));
    }

    #[test]
    fn a_payment_row_without_a_description() {
        let html = entry_row(&payment_entry(None)).into_string();

        assert!(html.contains("<span class=\"entry-what\">Josef → Ondra</span>"));
        assert!(html.contains("<span class=\"entry-meta\">Payment · 29 October 2026</span>"));
    }

    #[test]
    fn payment_form_empty() {
        assert_html_snapshot!(payment_form(
            PAYMENT_ACTION,
            &crew(),
            &PaymentValues::blank(ONDRA),
            &PaymentProblems::default()
        ));
    }

    #[test]
    fn payment_form_with_errors() {
        let values = PaymentValues {
            amount: "0",
            description: "",
            sender: SIMON,
            recipient: Some(SIMON),
        };
        let problems = PaymentProblems {
            amount: Some("Enter an amount from 1 to 1 000 000 Kč.".to_owned()),
            description: None,
            sender: None,
            recipient: Some("Choose someone other than the Sender.".to_owned()),
        };
        assert_html_snapshot!(payment_form(PAYMENT_ACTION, &crew(), &values, &problems));
    }

    #[test]
    fn the_payment_form_starts_with_me_sending_to_nobody_yet() {
        let html = payment_form(
            PAYMENT_ACTION,
            &crew(),
            &PaymentValues::blank(ONDRA),
            &PaymentProblems::default(),
        )
        .into_string();

        let first_input = html.find("<input").unwrap();
        assert!(
            html[first_input..]
                .starts_with("<input id=\"payment-amount\" type=\"text\" inputmode=\"numeric\"")
        );
        assert!(html.contains("<option value=\"2\" selected>Ondra</option>"));
        assert!(html.contains("<option value=\"\" disabled selected>Choose a Member</option>"));
        assert_eq!(html.matches(" selected").count(), 2);
    }

    #[test]
    fn expense_form_empty() {
        let today = TODAY.to_string();
        assert_html_snapshot!(expense_form(
            ACTION,
            &crew(),
            &empty_values(&today),
            &ExpenseProblems::default(),
            TODAY
        ));
    }

    #[test]
    fn expense_form_with_errors() {
        let values = ExpenseValues {
            amount: "0",
            description: "",
            payer: SIMON,
            expense_date: "2026-11-02",
        };
        let problems = ExpenseProblems {
            amount: Some("Enter an amount from 1 to 1 000 000 Kč.".to_owned()),
            description: Some("Say what the money was for.".to_owned()),
            payer: Some("They weren't in the Crew yet on that date.".to_owned()),
            expense_date: Some("The Expense date can't be in the future.".to_owned()),
        };
        assert_html_snapshot!(expense_form(ACTION, &crew(), &values, &problems, TODAY));
    }

    #[test]
    fn the_home_screen_marks_the_member_and_their_standing() {
        let html = home(ONDRA, &overview(), NEW_EXPENSE, NEW_PAYMENT).into_string();

        assert!(html.contains("<section class=\"my-balance owed\""));
        assert!(html.contains("+4\u{a0}161\u{a0}Kč"));
        assert!(html.contains("Ondra (you)"));
        assert!(html.contains(&format!("href=\"{NEW_EXPENSE}\"")));
        assert!(html.contains(&format!("href=\"{NEW_PAYMENT}\"")));
    }

    #[test]
    fn the_expense_date_stays_folded_while_it_is_today() {
        let today = TODAY.to_string();
        let html = expense_form(
            ACTION,
            &crew(),
            &empty_values(&today),
            &ExpenseProblems::default(),
            TODAY,
        )
        .into_string();

        assert!(
            html.contains("<details class=\"field expense-date\"><summary>Spent today</summary>")
        );
        assert!(html.contains("max=\"2026-10-30\""));
    }

    #[test]
    fn an_earlier_expense_date_shows_unfolded() {
        let values = ExpenseValues {
            expense_date: "2026-10-28",
            ..empty_values("")
        };
        let html = expense_form(ACTION, &crew(), &values, &ExpenseProblems::default(), TODAY)
            .into_string();

        assert!(html.contains(
            "<details class=\"field expense-date\" open><summary>Spent on 28 October 2026</summary>"
        ));
    }

    #[test]
    fn the_amount_comes_first_with_a_numeric_keyboard() {
        let today = TODAY.to_string();
        let html = expense_form(
            ACTION,
            &crew(),
            &empty_values(&today),
            &ExpenseProblems::default(),
            TODAY,
        )
        .into_string();

        let first_input = html.find("<input").unwrap();
        assert!(
            html[first_input..]
                .starts_with("<input id=\"expense-amount\" type=\"text\" inputmode=\"numeric\"")
        );
    }

    #[test]
    fn the_payer_is_preselected() {
        let today = TODAY.to_string();
        let html = expense_form(
            ACTION,
            &crew(),
            &empty_values(&today),
            &ExpenseProblems::default(),
            TODAY,
        )
        .into_string();

        assert!(html.contains("<option value=\"2\" selected>Ondra</option>"));
        assert_eq!(html.matches(" selected").count(), 1);
    }

    #[test]
    fn escapes_what_was_typed() {
        let values = ExpenseValues {
            description: "\"><b>Malt",
            ..empty_values("2026-10-30")
        };
        let html = expense_form(ACTION, &crew(), &values, &ExpenseProblems::default(), TODAY)
            .into_string();

        assert!(html.contains("value=\"&quot;&gt;&lt;b&gt;Malt\""));
    }
}
