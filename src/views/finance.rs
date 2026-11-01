//! The Finance tab: my Balance, the latest entries, everyone's Balance with the Settlements, and
//! the Expense and Payment forms.

use jiff::civil::Date;
use maud::{Markup, html};
use url::form_urlencoded;

use crate::domain::finance::money::{Crowns, Standing};
use crate::domain::finance::settlement::Settlement;
use crate::domain::finance::{DESCRIPTION_MAX, Edit, Entry, ExpenseEntry, Overview, PaymentEntry};
use crate::domain::members::{Member, MemberId, MemberName};
use crate::views::icon::chevron;
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};

/// Where Finance's links and forms lead. The routes in `web::finance` spell out the same paths.
pub mod paths {
    use crate::domain::finance::{ExpenseId, PaymentId};

    /// Where the Expense form logs a new Expense.
    pub const EXPENSES: &str = "/expenses";

    /// The empty Expense form.
    pub const NEW_EXPENSE: &str = "/expenses/new";

    /// Where the Payment form logs a new Payment.
    pub const PAYMENTS: &str = "/payments";

    /// The empty Payment form.
    pub const NEW_PAYMENT: &str = "/payments/new";

    /// Every entry, a page at a time.
    pub const HISTORY: &str = "/history";

    /// The Expense `id`, which its edit form posts to.
    #[must_use]
    pub fn expense(id: ExpenseId) -> String {
        format!("{EXPENSES}/{}", id.get())
    }

    /// The Expense `id` in its edit form.
    #[must_use]
    pub fn edit_expense(id: ExpenseId) -> String {
        format!("{}/edit", expense(id))
    }

    /// Where the Expense `id`'s delete button posts.
    #[must_use]
    pub fn delete_expense(id: ExpenseId) -> String {
        format!("{}/delete", expense(id))
    }

    /// The Payment `id`, which its edit form posts to.
    #[must_use]
    pub fn payment(id: PaymentId) -> String {
        format!("{PAYMENTS}/{}", id.get())
    }

    /// The Payment `id` in its edit form.
    #[must_use]
    pub fn edit_payment(id: PaymentId) -> String {
        format!("{}/edit", payment(id))
    }

    /// Where the Payment `id`'s delete button posts.
    #[must_use]
    pub fn delete_payment(id: PaymentId) -> String {
        format!("{}/delete", payment(id))
    }
}

/// `path`, telling the page there to send a Member back to `back` once they are done, or to
/// Finance without a `back`.
#[must_use]
pub fn returning_to(path: String, back: Option<&str>) -> String {
    match back {
        Some(back) => {
            let query = form_urlencoded::Serializer::new(String::new())
                .append_pair("back", back)
                .finish();
            format!("{path}?{query}")
        }
        None => path,
    }
}

/// Whether a form logs a new entry or saves changes to a logged one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Log,
    Edit,
}

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

/// Where a Settlement's "I've sent it" button leads: the Payment form, pre-filled with its
/// Sender, Recipient and amount.
fn sent_href(settlement: &Settlement) -> String {
    format!(
        "{}?sender={}&recipient={}&amount={}",
        paths::NEW_PAYMENT,
        settlement.sender.id.get(),
        settlement.recipient.id.get(),
        settlement.amount.get()
    )
}

/// The Finance home screen for `me`.
///
/// Expenses come first, since the Crew logs them all the time: my Balance, the buttons that log
/// an Expense or a Payment, and the latest entries, leading to the edit page of the ones `me` may
/// change. Settling up happens now and then, so everyone's Balance and the Settlement plan close
/// the page; `me` logs the Payment for a Settlement there once they have sent it. On a wide
/// screen the latest entries and everyone's Balance sit side by side.
#[must_use]
pub fn home(me: &Member, overview: &Overview) -> Markup {
    let mine = overview.balance_of(me.id);
    let my_sends: Vec<&Settlement> = overview.settlements_sent_by(me.id).collect();
    shell(
        Place::Tab(Tab::Finance),
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
                @if !my_sends.is_empty() {
                    p.balance-next {
                        "Send "
                        @for (index, settlement) in my_sends.iter().enumerate() {
                            @if index > 0 { " and " }
                            (settlement.recipient.display_name) " "
                            strong { (crowns(settlement.amount)) }
                        }
                        ". "
                        a href="#crew-balances" { "See Crew Balances" }
                    }
                }
            }

            div.finance-actions {
                a.button.large href=(paths::NEW_EXPENSE) { "+ Expense" }
                a.button.secondary href=(paths::NEW_PAYMENT) { "+ Payment" }
            }

            div.columns {
                section.section aria-labelledby="latest-entries" {
                    h2 #latest-entries { "Latest entries" }
                    @if overview.recent.is_empty() {
                        p.muted { "Nothing logged yet. Log the first Expense when someone buys something for the brewery." }
                    } @else {
                        ul.entry-list {
                            @for entry in &overview.recent {
                                (entry_row(me, entry, None))
                            }
                        }
                        p.history-link { a href=(paths::HISTORY) { "See every entry in History" } }
                    }
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
                                @for (index, settlement) in overview.settlements.iter().enumerate()
                                    .filter(|(_, settlement)| settlement.sender.id == balance.member.id) {
                                    (crew_settlement(me.id, settlement, index))
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// The `index`th Settlement of the plan under its Sender's Balance. Only its Sender gets the
/// button that logs its Payment, because only the Sender knows once the money has gone out.
fn crew_settlement(me: MemberId, settlement: &Settlement, index: usize) -> Markup {
    let id = format!("settlement-{index}");
    html! {
        div.balance-settlement {
            span.balance-sends #(id) {
                "Sends "
                @if settlement.recipient.id == me {
                    "you"
                } @else {
                    (settlement.recipient.display_name)
                }
                " " (crowns(settlement.amount))
            }
            @if settlement.sender.id == me {
                a.button.small.secondary href=(sent_href(settlement))
                    aria-describedby=(id) { "I've sent it" }
            }
        }
    }
}

/// One Expense or Payment in a list. If `me` may change it, the row leads to its edit page,
/// which sends `me` back to `back` afterwards, or to Finance without one.
#[must_use]
pub fn entry_row(me: &Member, entry: &Entry, back: Option<&str>) -> Markup {
    let edit = entry.may_be_changed_by(me).then(|| match entry {
        Entry::Expense(expense) => returning_to(paths::edit_expense(expense.id), back),
        Entry::Payment(payment) => returning_to(paths::edit_payment(payment.id), back),
    });
    let row = match entry {
        Entry::Expense(expense) => expense_row(expense),
        Entry::Payment(payment) => payment_row(payment),
    };
    html! {
        li.entry {
            @if let Some(edit) = edit {
                a.entry-link href=(edit) { (row) (chevron()) }
            } @else {
                div.entry-link { (row) }
            }
        }
    }
}

/// An Expense: what it was for, who paid, when, how much, and who edited it last.
fn expense_row(entry: &ExpenseEntry) -> Markup {
    html! {
        span.entry-main {
            span.entry-what #{ "expense-" (entry.id.get()) } { (entry.description.as_str()) }
            span.entry-meta {
                (entry.payer.display_name) " paid · " (day(entry.expense_date))
            }
            (edited(entry.edited.as_ref()))
        }
        span.entry-amount { (crowns(entry.amount.crowns())) }
    }
}

/// A Payment: who sent it to whom, what for if anything, when, how much, and who edited it
/// last.
fn payment_row(entry: &PaymentEntry) -> Markup {
    html! {
        span.entry-main {
            span.entry-what #{ "payment-" (entry.id.get()) } {
                (entry.sender.display_name) " → " (entry.recipient)
            }
            span.entry-meta {
                "Payment · "
                @if let Some(description) = &entry.description {
                    (description.as_str()) " · "
                }
                (day(entry.logged_on))
            }
            (edited(entry.edited.as_ref()))
        }
        span.entry-amount { (crowns(entry.amount.crowns())) }
    }
}

/// Who edited an entry last, and when, if anyone has.
fn edited(edit: Option<&Edit>) -> Markup {
    html! {
        @if let Some(edit) = edit {
            span.entry-meta.entry-edited { "edited by " (edit.by) " · " (day(edit.on)) }
        }
    }
}

/// Which page an entry's form sits on: a new entry's, or the edit page of a logged one, with the
/// button that deletes it posting to `delete`.
#[derive(Debug, Clone, Copy)]
pub enum EntryPage<'a> {
    New,
    Edit { delete: &'a str },
}

/// The foot of an edit page: what deleting the `kind` of entry takes it out of, and the button
/// that deletes it after asking. Without htmx, it posts as a plain form.
fn delete_entry(kind: &str, balances: &str, delete: &str) -> Markup {
    html! {
        section.delete-entry aria-label={ "Delete this " (kind) } {
            p { "Deleting this " (kind) " takes it out of " (balances) "." }
            form method="post" action=(delete) hx-post=(delete)
                hx-confirm={ "Delete this " (kind) "? It comes out of " (balances) "." } {
                button.button.danger type="submit" { "Delete " (kind) }
            }
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

/// The page around the Expense form, with the delete button at its foot when editing.
#[must_use]
pub fn expense_page(page: EntryPage, form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Finance),
        &html! {
            @match page {
                EntryPage::New => {
                    h1 { "New Expense" }
                    (form)
                }
                EntryPage::Edit { delete } => {
                    h1 { "Edit Expense" }
                    (form)
                    (delete_entry("Expense", "everyone's Balance", delete))
                }
            }
        },
    )
}

/// The form an Expense is logged or edited with, posting to `action`.
///
/// It shows `values` and, beside each field, what was wrong with it. The Payer is one of
/// `members`; the Expense date can't be after `today`, and while it is today it stays folded
/// away. htmx swaps the form for the one the server answers with; without htmx, it posts as a
/// plain form.
#[must_use]
pub fn expense_form(
    action: &str,
    purpose: Purpose,
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
            button.button.large type="submit" {
                @match purpose {
                    Purpose::Log => "Log Expense",
                    Purpose::Edit => "Save Expense",
                }
            }
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

/// The page around the Payment form, with the delete button at its foot when editing.
#[must_use]
pub fn payment_page(page: EntryPage, form: &Markup) -> Markup {
    shell(
        Place::Tab(Tab::Finance),
        &html! {
            @match page {
                EntryPage::New => {
                    h1 { "New Payment" }
                    (form)
                }
                EntryPage::Edit { delete } => {
                    h1 { "Edit Payment" }
                    (form)
                    (delete_entry("Payment", "both Members' Balances", delete))
                }
            }
        },
    )
}

/// The form a Payment is logged or edited with, posting to `action`.
///
/// It shows `values` and, beside each field, what was wrong with it. The Sender and the
/// Recipient are each one of `members`; the Recipient starts unchosen. It swaps and posts like
/// the Expense form.
#[must_use]
pub fn payment_form(
    action: &str,
    purpose: Purpose,
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
            button.button.large type="submit" {
                @match purpose {
                    Purpose::Log => "Log Payment",
                    Purpose::Edit => "Save Payment",
                }
            }
        }
    }
}

/// What was wrong with a field, as the element `id` its field is described by.
pub(super) fn problem(id: &str, message: Option<&str>) -> Markup {
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
    use crate::domain::finance::settlement::settlement_plan;
    use crate::domain::finance::{
        Description, Edit, Entry, ExpenseEntry, ExpenseId, MemberBalance, Overview, PaymentEntry,
        PaymentId,
    };
    use crate::domain::members::{DisplayName, Member, MemberId, MemberName};
    use crate::views::finance::PaymentValues;

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

    /// The Member of [`crew`] with `id`, signed in. Josef is the Admin.
    ///
    /// # Panics
    ///
    /// If `id` isn't in [`crew`], which is a bug in the fixtures.
    #[must_use]
    pub fn member(id: MemberId) -> Member {
        let member = crew()
            .into_iter()
            .find(|member| member.id == id)
            .expect("fixture Members are in the Crew");
        Member {
            id,
            display_name: member.display_name,
            is_admin: id == JOSEF,
            joined_on: date(2026, 10, 1),
            welcomed: true,
        }
    }

    fn member_name(display_name: &str) -> MemberName {
        crew()
            .into_iter()
            .find(|member| member.display_name.as_str() == display_name)
            .expect("fixture Members are in the Crew")
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

    /// [`entry`] after Josef, the Admin, edited it.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn edited_entry() -> Entry {
        let mut entry = entry();
        if let Entry::Expense(expense) = &mut entry {
            expense.edited = Some(Edit {
                by: name("Josef"),
                on: date(2026, 10, 30),
            });
        }
        entry
    }

    /// [`payment_entry`] after Josef, its Sender, edited it.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn edited_payment_entry() -> Entry {
        let mut entry = payment_entry(Some("For the malt"));
        if let Entry::Payment(payment) = &mut entry {
            payment.edited = Some(Edit {
                by: name("Josef"),
                on: date(2026, 10, 30),
            });
        }
        entry
    }

    fn expense(id: i64, payer: &str, crowns: i64, description: &str, expense_date: Date) -> Entry {
        Entry::Expense(ExpenseEntry {
            id: ExpenseId::from_client(id),
            payer: member_name(payer),
            amount: Amount::new(crowns).expect("fixture amounts are valid"),
            description: Description::parse(description).expect("fixture descriptions are valid"),
            expense_date,
            edited: None,
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
            sender: member_name(sender),
            recipient: name(recipient),
            amount: Amount::new(crowns).expect("fixture amounts are valid"),
            description: description
                .map(|text| Description::parse(text).expect("fixture descriptions are valid")),
            logged_on,
            edited: None,
        })
    }

    /// The Crew after a few entries: Josef owes, Ondra is owed, Šimon is Even.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn overview() -> Overview {
        let balances: Vec<MemberBalance> = crew()
            .into_iter()
            .zip([-4160, 4161, -1])
            .map(|(member, crowns)| MemberBalance {
                member,
                balance: Crowns::new(crowns),
            })
            .collect();
        Overview {
            settlements: settlement_plan(&balances),
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

    /// Josef owing 300 Kč, which he sends as 200 Kč to Ondra and 100 Kč to Šimon, with nothing
    /// in the latest entries.
    #[must_use]
    pub fn sends_to_two() -> Overview {
        let balances: Vec<MemberBalance> = crew()
            .into_iter()
            .zip([-300, 200, 100])
            .map(|(member, crowns)| MemberBalance {
                member,
                balance: Crowns::new(crowns),
            })
            .collect();
        Overview {
            settlements: settlement_plan(&balances),
            balances,
            recent: Vec::new(),
        }
    }

    /// The Payment form as Josef's Settlement with Ondra in [`overview`] fills it in.
    #[must_use]
    pub const fn prefilled_payment() -> PaymentValues<'static> {
        PaymentValues {
            amount: "4160",
            description: "",
            sender: JOSEF,
            recipient: Some(ONDRA),
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
            settlements: Vec::new(),
            recent: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{
        JOSEF, ONDRA, SIMON, TODAY, crew, edited_entry, edited_payment_entry, entry, fresh, member,
        overview, payment_entry, prefilled_payment, sends_to_two,
    };
    use super::*;
    use crate::views::assert_html_snapshot;

    const NEW_EXPENSE: &str = "/expenses/new";
    const NEW_PAYMENT: &str = "/payments/new";
    const ACTION: &str = "/expenses";
    const EDIT_ACTION: &str = "/expenses/7";
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
        assert_html_snapshot!(home(&member(JOSEF), &overview()));
    }

    #[test]
    fn finance_home_owed() {
        assert_html_snapshot!(home(&member(ONDRA), &overview()));
    }

    #[test]
    fn finance_home_even() {
        assert_html_snapshot!(home(&member(SIMON), &overview()));
    }

    #[test]
    fn finance_home_sends_to_two() {
        assert_html_snapshot!(home(&member(JOSEF), &sends_to_two()));
    }

    #[test]
    fn my_balance_names_everyone_i_send_to() {
        let html = home(&member(JOSEF), &sends_to_two()).into_string();

        assert!(html.contains(
            "<p class=\"balance-next\">Send Ondra <strong>200\u{a0}Kč</strong> and \
             Šimon <strong>100\u{a0}Kč</strong>. "
        ));
        assert_eq!(html.matches(">I've sent it</a>").count(), 2);
    }

    #[test]
    fn the_latest_entries_and_the_crew_balances_share_the_columns_under_the_buttons() {
        let html = home(&member(JOSEF), &overview()).into_string();

        let actions = html.find("class=\"finance-actions\"").unwrap();
        let columns = html.find("<div class=\"columns\">").unwrap();
        let latest = html.find("id=\"latest-entries\"").unwrap();
        let crew = html.find("id=\"crew-balances\"").unwrap();
        assert!(actions < columns && columns < latest && latest < crew);
        assert_eq!(html[columns..].matches("<section").count(), 2);
    }

    #[test]
    fn finance_home_with_nothing_logged() {
        assert_html_snapshot!(home(&member(JOSEF), &fresh()));
    }

    #[test]
    fn finance_entry_row() {
        assert_html_snapshot!(entry_row(&member(SIMON), &entry(), None));
    }

    #[test]
    fn finance_payment_row() {
        assert_html_snapshot!(entry_row(
            &member(SIMON),
            &payment_entry(Some("For the malt")),
            None
        ));
    }

    #[test]
    fn finance_edited_entry_row_with_controls() {
        assert_html_snapshot!(entry_row(&member(ONDRA), &edited_entry(), None));
    }

    #[test]
    fn finance_edited_payment_row_with_controls() {
        assert_html_snapshot!(entry_row(&member(JOSEF), &edited_payment_entry(), None));
    }

    #[test]
    fn a_payment_row_without_a_description() {
        let html = entry_row(&member(SIMON), &payment_entry(None), None).into_string();

        assert!(html.contains("<span class=\"entry-what\" id=\"payment-4\">Josef → Ondra</span>"));
        assert!(html.contains("<span class=\"entry-meta\">Payment · 29 October 2026</span>"));
    }

    #[test]
    fn only_the_payer_or_sender_and_the_admin_get_a_row_that_leads_to_editing() {
        let leads_to_editing = |me, entry: &Entry| {
            let html = entry_row(&member(me), entry, None).into_string();
            assert!(!html.contains("/delete"), "a row never deletes");
            html.contains("/edit\"")
        };

        assert!(leads_to_editing(ONDRA, &entry()));
        assert!(leads_to_editing(JOSEF, &entry()));
        assert!(!leads_to_editing(SIMON, &entry()));
        assert!(leads_to_editing(JOSEF, &payment_entry(None)));
        assert!(!leads_to_editing(ONDRA, &payment_entry(None)));
    }

    #[test]
    fn a_row_that_may_be_changed_links_to_its_edit_page() {
        let html = entry_row(&member(JOSEF), &payment_entry(None), None).into_string();

        assert!(
            html.starts_with(
                "<li class=\"entry\"><a class=\"entry-link\" href=\"/payments/4/edit\">"
            )
        );
    }

    #[test]
    fn only_the_edit_page_deletes_and_asks_first() {
        let form = html! { form {} };
        let edit = expense_page(
            EntryPage::Edit {
                delete: "/expenses/7/delete",
            },
            &form,
        )
        .into_string();
        let new = expense_page(EntryPage::New, &form).into_string();

        assert!(edit.contains("hx-post=\"/expenses/7/delete\""));
        assert!(
            edit.contains(
                "hx-confirm=\"Delete this Expense? It comes out of everyone's Balance.\""
            )
        );
        assert!(edit.contains(">Delete Expense</button>"));
        assert!(!new.contains("delete"));
    }

    #[test]
    fn an_unedited_entry_says_nothing_about_edits() {
        let html = entry_row(&member(ONDRA), &entry(), None).into_string();

        assert!(!html.contains("edited by"));
    }

    #[test]
    fn expense_edit_form() {
        let values = ExpenseValues {
            amount: "12480",
            description: "Pilsner malt, 3 sacks, and a new hydrometer",
            payer: ONDRA,
            expense_date: "2026-10-28",
        };
        assert_html_snapshot!(expense_page(
            EntryPage::Edit {
                delete: "/expenses/7/delete"
            },
            &expense_form(
                EDIT_ACTION,
                Purpose::Edit,
                &crew(),
                &values,
                &ExpenseProblems::default(),
                TODAY
            )
        ));
    }

    #[test]
    fn payment_edit_form() {
        assert_html_snapshot!(payment_page(
            EntryPage::Edit {
                delete: "/payments/4/delete"
            },
            &payment_form(
                "/payments/4",
                Purpose::Edit,
                &crew(),
                &prefilled_payment(),
                &PaymentProblems::default()
            )
        ));
    }

    #[test]
    fn payment_form_empty() {
        assert_html_snapshot!(payment_form(
            PAYMENT_ACTION,
            Purpose::Log,
            &crew(),
            &PaymentValues::blank(ONDRA),
            &PaymentProblems::default()
        ));
    }

    #[test]
    fn payment_form_prefilled() {
        assert_html_snapshot!(payment_form(
            PAYMENT_ACTION,
            Purpose::Log,
            &crew(),
            &prefilled_payment(),
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
        assert_html_snapshot!(payment_form(
            PAYMENT_ACTION,
            Purpose::Log,
            &crew(),
            &values,
            &problems
        ));
    }

    #[test]
    fn the_payment_form_starts_with_me_sending_to_nobody_yet() {
        let html = payment_form(
            PAYMENT_ACTION,
            Purpose::Log,
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
            Purpose::Log,
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
        assert_html_snapshot!(expense_form(
            ACTION,
            Purpose::Log,
            &crew(),
            &values,
            &problems,
            TODAY
        ));
    }

    #[test]
    fn the_home_screen_marks_the_member_and_their_standing() {
        let html = home(&member(ONDRA), &overview()).into_string();

        assert!(html.contains("<section class=\"card my-balance owed\""));
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
            Purpose::Log,
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
        let html = expense_form(
            ACTION,
            Purpose::Log,
            &crew(),
            &values,
            &ExpenseProblems::default(),
            TODAY,
        )
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
            Purpose::Log,
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
            Purpose::Log,
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
        let html = expense_form(
            ACTION,
            Purpose::Log,
            &crew(),
            &values,
            &ExpenseProblems::default(),
            TODAY,
        )
        .into_string();

        assert!(html.contains("value=\"&quot;&gt;&lt;b&gt;Malt\""));
    }
}
