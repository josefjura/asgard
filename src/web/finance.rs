//! The Finance tab, Asgard's home page: everyone's Balance and the Settlements, the latest
//! entries, and logging, editing and deleting an Expense or a Payment.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use jiff::civil::Date;
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::error::{amount_message, description_message};
use super::htmx::{Htmx, fragment_or_page, go_to};
use super::pages::fresh;
use super::{AppError, AppState, welcome};
use crate::domain::finance::history::{HistoryFilter, Month};
use crate::domain::finance::money::Amount;
use crate::domain::finance::{
    Description, ExpenseError, ExpenseId, NewExpense, NewPayment, PaymentError, PaymentId,
};
use crate::domain::members::{self, MemberId};
use crate::views;
use crate::views::finance::{
    EntryPage, ExpenseProblems, ExpenseValues, PaymentProblems, PaymentValues, Purpose, paths,
    returning_to,
};
use crate::views::history::{Filters, NOT_A_MONTH};
use crate::views::nav::Tab;

/// What a Member is told when they pick the Sender as the Recipient too.
const SAME_MEMBER: &str = "Choose someone other than the Sender.";

/// What a Member is told when a Sender or Recipient they chose has left the form's list.
const NOT_IN_CREW: &str = "That Member isn't in the Crew. Reload the page to see who is.";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Finance.href(), get(home))
        .route(paths::NEW_EXPENSE, get(new_expense))
        .route(paths::EXPENSES, post(log_expense))
        .route("/expenses/{id}/edit", get(edit_expense))
        .route("/expenses/{id}", post(save_expense))
        .route("/expenses/{id}/delete", post(delete_expense))
        .route(paths::NEW_PAYMENT, get(new_payment))
        .route(paths::PAYMENTS, post(log_payment))
        .route("/payments/{id}/edit", get(edit_payment))
        .route("/payments/{id}", post(save_payment))
        .route("/payments/{id}/delete", post(delete_payment))
        .route(paths::HISTORY, get(history))
}

/// Where a Member goes once they have edited or deleted an entry, as the entry's controls send
/// it: the page of History they came from, or Finance.
#[derive(Deserialize)]
struct Back {
    back: Option<String>,
}

impl Back {
    /// The page of History to go back to. Anything else is ignored, so a link can't send a
    /// Member off Asgard.
    fn history(&self) -> Option<&str> {
        self.back.as_deref().filter(|back| {
            back.strip_prefix(paths::HISTORY)
                .is_some_and(|query| query.is_empty() || query.starts_with('?'))
        })
    }

    /// Where to go back to.
    fn to(&self) -> &str {
        self.history().unwrap_or_else(|| Tab::Finance.href())
    }
}

/// History's filter and page, as its form and links send them.
#[derive(Deserialize)]
struct HistoryQuery {
    /// Empty for everyone.
    #[serde(default)]
    member: String,
    /// Empty for every month.
    #[serde(default)]
    month: String,
    page: Option<usize>,
}

/// A page of History with the filter applied. A month that isn't one is left out of the filter
/// and shown with what to fix.
async fn history(
    State(state): State<AppState>,
    Member(member): Member,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let (month, month_problem) = if query.month.trim().is_empty() {
        (None, None)
    } else {
        Month::parse(&query.month).map_or((None, Some(NOT_A_MONTH)), |month| (Some(month), None))
    };
    let applied = HistoryFilter {
        member: query.member.trim().parse().ok().map(MemberId::from_client),
        month,
    };
    let history = state
        .finance
        .history(&applied, query.page.unwrap_or(1))
        .await?;
    let members = state.members.all().await?;
    let filters = Filters {
        applied,
        month: &query.month,
        month_problem,
    };
    Ok(fresh(views::history::page(
        &member, &members, &filters, &history,
    )))
}

/// The home page. It sends a Member to the welcome page until they have pressed "Got it"
/// there.
async fn home(State(state): State<AppState>, Member(member): Member) -> Result<Response, AppError> {
    if !member.welcomed {
        return Ok(Redirect::to(welcome::PATH).into_response());
    }
    let overview = state.finance.overview().await?;
    Ok(fresh(views::finance::home(&member, &overview)).into_response())
}

/// The empty Expense form: the Payer is the Member asking, and the Expense date is today.
async fn new_expense(
    State(state): State<AppState>,
    Member(member): Member,
) -> Result<impl IntoResponse, AppError> {
    let today = state.finance.today().to_string();
    let values = ExpenseValues::blank(member.id, &today);
    let form = expense_form(
        &state,
        paths::EXPENSES,
        Purpose::Log,
        &values,
        &ExpenseProblems::default(),
    )
    .await?;
    Ok(fresh(views::finance::expense_page(EntryPage::New, &form)))
}

#[derive(Deserialize)]
struct ExpenseForm {
    amount: String,
    description: String,
    payer: i64,
    expense_date: String,
}

impl ExpenseForm {
    const fn payer(&self) -> MemberId {
        MemberId::from_client(self.payer)
    }

    fn values(&self) -> ExpenseValues<'_> {
        ExpenseValues {
            amount: &self.amount,
            description: &self.description,
            payer: self.payer(),
            expense_date: &self.expense_date,
        }
    }
}

/// Logs the Expense and goes back to Finance, or answers with the form showing what to fix:
/// alone for htmx, inside its page otherwise.
async fn log_expense(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<ExpenseForm>,
) -> Result<Response, AppError> {
    let problems = match parse(&input) {
        Ok(expense) => match state.finance.log_expense(member.id, &expense).await {
            Ok(_) => {
                tracing::info!(member = member.id.get(), "a Member logged an Expense");
                return Ok(go_to(htmx, Tab::Finance.href()));
            }
            Err(err) => expense_problems(err)?,
        },
        Err(problems) => problems,
    };
    let form = expense_form(
        &state,
        paths::EXPENSES,
        Purpose::Log,
        &input.values(),
        &problems,
    )
    .await?;
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| views::finance::expense_page(EntryPage::New, form),
    ))
}

/// The Expense `id` in its form, for its Payer or the Admin to edit.
async fn edit_expense(
    State(state): State<AppState>,
    Member(member): Member,
    Path(id): Path<i64>,
    Query(back): Query<Back>,
) -> Result<impl IntoResponse, AppError> {
    let id = ExpenseId::from_client(id);
    let expense = state.finance.expense_to_edit(&member, id).await?;
    let amount = expense.amount.get().to_string();
    let expense_date = expense.expense_date.to_string();
    let values = ExpenseValues {
        amount: &amount,
        description: expense.description.as_str(),
        payer: expense.payer,
        expense_date: &expense_date,
    };
    let form = expense_form(
        &state,
        &returning_to(paths::expense(id), back.history()),
        Purpose::Edit,
        &values,
        &ExpenseProblems::default(),
    )
    .await?;
    let delete = returning_to(paths::delete_expense(id), back.history());
    Ok(fresh(views::finance::expense_page(
        EntryPage::Edit { delete: &delete },
        &form,
    )))
}

/// Saves the edited Expense `id` and goes back to History or Finance, or answers with the form showing
/// what to fix, like [`log_expense`].
async fn save_expense(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Query(back): Query<Back>,
    Form(input): Form<ExpenseForm>,
) -> Result<Response, AppError> {
    let id = ExpenseId::from_client(id);
    let problems = match parse(&input) {
        Ok(expense) => match state.finance.edit_expense(&member, id, &expense).await {
            Ok(()) => {
                tracing::info!(member = member.id.get(), "a Member edited an Expense");
                return Ok(go_to(htmx, back.to()));
            }
            Err(err) => expense_problems(err)?,
        },
        Err(problems) => {
            // Someone who may not change the Expense is refused before hearing what to fix.
            state.finance.expense_to_edit(&member, id).await?;
            problems
        }
    };
    let form = expense_form(
        &state,
        &returning_to(paths::expense(id), back.history()),
        Purpose::Edit,
        &input.values(),
        &problems,
    )
    .await?;
    let delete = returning_to(paths::delete_expense(id), back.history());
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| views::finance::expense_page(EntryPage::Edit { delete: &delete }, form),
    ))
}

/// Deletes the Expense `id` and goes back to History or Finance.
async fn delete_expense(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Query(back): Query<Back>,
) -> Result<Response, AppError> {
    state
        .finance
        .delete_expense(&member, ExpenseId::from_client(id))
        .await?;
    tracing::info!(member = member.id.get(), "a Member deleted an Expense");
    Ok(go_to(htmx, back.to()))
}

/// The Expense as typed, or what is wrong with each field that can be checked on its own.
fn parse(input: &ExpenseForm) -> Result<NewExpense, ExpenseProblems> {
    let amount = Amount::parse(&input.amount);
    let description = Description::parse(&input.description);
    let expense_date = input.expense_date.trim().parse::<Date>();
    match (amount, description, expense_date) {
        (Ok(amount), Ok(description), Ok(expense_date)) => Ok(NewExpense {
            payer: input.payer(),
            amount,
            description,
            expense_date,
        }),
        (amount, description, expense_date) => Err(ExpenseProblems {
            amount: amount.err().map(amount_message),
            description: description.err().map(description_message),
            payer: None,
            expense_date: expense_date
                .err()
                .map(|_| "Enter the date the money was spent.".to_owned()),
        }),
    }
}

/// What to fix in an Expense the Finance service refused, or the error to answer with when
/// there is nothing to fix.
fn expense_problems(err: ExpenseError) -> Result<ExpenseProblems, AppError> {
    match err {
        ExpenseError::FutureDate => Ok(ExpenseProblems {
            expense_date: Some("The Expense date can't be in the future.".to_owned()),
            ..ExpenseProblems::default()
        }),
        ExpenseError::PayerNotInCrew => Ok(ExpenseProblems {
            payer: Some("They weren't in the Crew yet on that date.".to_owned()),
            ..ExpenseProblems::default()
        }),
        ExpenseError::Entry(err) => Err(err.into()),
        ExpenseError::Database(err) => Err(err.into()),
    }
}

/// The Expense form posting to `action`, showing `values` and `problems`, with every Member to
/// choose the Payer from.
async fn expense_form(
    state: &AppState,
    action: &str,
    purpose: Purpose,
    values: &ExpenseValues<'_>,
    problems: &ExpenseProblems,
) -> Result<Markup, AppError> {
    let members: Vec<members::MemberName> = state.members.all().await?;
    Ok(views::finance::expense_form(
        action,
        purpose,
        &members,
        values,
        problems,
        state.finance.today(),
    ))
}

/// What a Settlement's "I've sent it" button fills the Payment form in with.
#[derive(Deserialize)]
struct PaymentPrefill {
    sender: Option<i64>,
    recipient: Option<i64>,
    amount: Option<String>,
}

/// The Payment form: empty, with the Member asking as the Sender and no Recipient chosen yet,
/// or filled in with a Settlement its Sender has sent.
async fn new_payment(
    State(state): State<AppState>,
    Member(member): Member,
    Query(prefill): Query<PaymentPrefill>,
) -> Result<impl IntoResponse, AppError> {
    let values = PaymentValues {
        amount: prefill.amount.as_deref().unwrap_or_default(),
        sender: prefill.sender.map_or(member.id, MemberId::from_client),
        recipient: prefill.recipient.map(MemberId::from_client),
        ..PaymentValues::blank(member.id)
    };
    let form = payment_form(
        &state,
        paths::PAYMENTS,
        Purpose::Log,
        &values,
        &PaymentProblems::default(),
    )
    .await?;
    Ok(fresh(views::finance::payment_page(EntryPage::New, &form)))
}

#[derive(Deserialize)]
struct PaymentForm {
    amount: String,
    description: String,
    sender: i64,
    /// Missing while the form's "Choose a Member" placeholder is still selected.
    #[serde(default)]
    recipient: String,
}

impl PaymentForm {
    const fn sender(&self) -> MemberId {
        MemberId::from_client(self.sender)
    }

    fn recipient(&self) -> Option<MemberId> {
        self.recipient
            .trim()
            .parse()
            .ok()
            .map(MemberId::from_client)
    }

    fn values(&self) -> PaymentValues<'_> {
        PaymentValues {
            amount: &self.amount,
            description: &self.description,
            sender: self.sender(),
            recipient: self.recipient(),
        }
    }
}

/// Logs the Payment and goes back to Finance, or answers with the form showing what to fix,
/// like [`log_expense`].
async fn log_payment(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<PaymentForm>,
) -> Result<Response, AppError> {
    let problems = match parse_payment(&input) {
        Ok(payment) => match state.finance.log_payment(member.id, &payment).await {
            Ok(_) => {
                tracing::info!(member = member.id.get(), "a Member logged a Payment");
                return Ok(go_to(htmx, Tab::Finance.href()));
            }
            Err(err) => payment_problems(err)?,
        },
        Err(problems) => problems,
    };
    let form = payment_form(
        &state,
        paths::PAYMENTS,
        Purpose::Log,
        &input.values(),
        &problems,
    )
    .await?;
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| views::finance::payment_page(EntryPage::New, form),
    ))
}

/// The Payment `id` in its form, for its Sender or the Admin to edit.
async fn edit_payment(
    State(state): State<AppState>,
    Member(member): Member,
    Path(id): Path<i64>,
    Query(back): Query<Back>,
) -> Result<impl IntoResponse, AppError> {
    let id = PaymentId::from_client(id);
    let payment = state.finance.payment_to_edit(&member, id).await?;
    let amount = payment.amount.get().to_string();
    let values = PaymentValues {
        amount: &amount,
        description: payment.description.as_ref().map_or("", Description::as_str),
        sender: payment.sender,
        recipient: Some(payment.recipient),
    };
    let form = payment_form(
        &state,
        &returning_to(paths::payment(id), back.history()),
        Purpose::Edit,
        &values,
        &PaymentProblems::default(),
    )
    .await?;
    let delete = returning_to(paths::delete_payment(id), back.history());
    Ok(fresh(views::finance::payment_page(
        EntryPage::Edit { delete: &delete },
        &form,
    )))
}

/// Saves the edited Payment `id` and goes back to History or Finance, or answers with the form showing
/// what to fix, like [`save_expense`].
async fn save_payment(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Query(back): Query<Back>,
    Form(input): Form<PaymentForm>,
) -> Result<Response, AppError> {
    let id = PaymentId::from_client(id);
    let problems = match parse_payment(&input) {
        Ok(payment) => match state.finance.edit_payment(&member, id, &payment).await {
            Ok(()) => {
                tracing::info!(member = member.id.get(), "a Member edited a Payment");
                return Ok(go_to(htmx, back.to()));
            }
            Err(err) => payment_problems(err)?,
        },
        Err(problems) => {
            // Refused like an Expense in [`save_expense`].
            state.finance.payment_to_edit(&member, id).await?;
            problems
        }
    };
    let form = payment_form(
        &state,
        &returning_to(paths::payment(id), back.history()),
        Purpose::Edit,
        &input.values(),
        &problems,
    )
    .await?;
    let delete = returning_to(paths::delete_payment(id), back.history());
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| views::finance::payment_page(EntryPage::Edit { delete: &delete }, form),
    ))
}

/// Deletes the Payment `id` and goes back to History or Finance.
async fn delete_payment(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Query(back): Query<Back>,
) -> Result<Response, AppError> {
    state
        .finance
        .delete_payment(&member, PaymentId::from_client(id))
        .await?;
    tracing::info!(member = member.id.get(), "a Member deleted a Payment");
    Ok(go_to(htmx, back.to()))
}

/// The Payment as typed, or what is wrong with each field that can be checked without the
/// database.
fn parse_payment(input: &PaymentForm) -> Result<NewPayment, PaymentProblems> {
    let sender = input.sender();
    let amount = Amount::parse(&input.amount);
    let description = Description::parse_optional(&input.description);
    let recipient = match input.recipient() {
        None => Err("Choose who got the money."),
        Some(recipient) if recipient == sender => Err(SAME_MEMBER),
        Some(recipient) => Ok(recipient),
    };
    match (amount, description, recipient) {
        (Ok(amount), Ok(description), Ok(recipient)) => Ok(NewPayment {
            sender,
            recipient,
            amount,
            description,
        }),
        (amount, description, recipient) => Err(PaymentProblems {
            amount: amount.err().map(amount_message),
            description: description.err().map(description_message),
            sender: None,
            recipient: recipient.err().map(str::to_owned),
        }),
    }
}

/// What to fix in a Payment the Finance service refused, or the error to answer with when
/// there is nothing to fix.
fn payment_problems(err: PaymentError) -> Result<PaymentProblems, AppError> {
    match err {
        PaymentError::SameMember => Ok(PaymentProblems {
            recipient: Some(SAME_MEMBER.to_owned()),
            ..PaymentProblems::default()
        }),
        PaymentError::SenderNotInCrew => Ok(PaymentProblems {
            sender: Some(NOT_IN_CREW.to_owned()),
            ..PaymentProblems::default()
        }),
        PaymentError::RecipientNotInCrew => Ok(PaymentProblems {
            recipient: Some(NOT_IN_CREW.to_owned()),
            ..PaymentProblems::default()
        }),
        PaymentError::Entry(err) => Err(err.into()),
        PaymentError::Database(err) => Err(err.into()),
    }
}

/// The Payment form posting to `action`, showing `values` and `problems`, with every Member to
/// choose the Sender and the Recipient from.
async fn payment_form(
    state: &AppState,
    action: &str,
    purpose: Purpose,
    values: &PaymentValues<'_>,
    problems: &PaymentProblems,
) -> Result<Markup, AppError> {
    let members = state.members.all().await?;
    Ok(views::finance::payment_form(
        action, purpose, &members, values, problems,
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, COOKIE, LOCATION};
    use axum::http::{Method, Response, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::events::Event;
    use crate::domain::finance::money::Crowns;
    use crate::domain::finance::{DESCRIPTION_MAX, EntryAction, EntryId, ExpenseId, FinanceEvent};
    use crate::domain::members::MemberId;
    use crate::views::finance::crowns;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    /// The fields of a 1 000 Kč Malt Expense paid by the Member `payer` on 1 January 2026, the
    /// test app's first day, with `changes` made to them.
    fn fields<'a>(payer: i64, changes: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
        let fields = vec![
            ("amount", "1000".to_owned()),
            ("description", "Malt".to_owned()),
            ("payer", payer.to_string()),
            ("expense_date", "2026-01-01".to_owned()),
        ];
        changed(fields, changes)
    }

    /// The fields of a 300 Kč Payment from the Member `sender` to `recipient`, with no
    /// description, and with `changes` made to them.
    fn payment_fields<'a>(
        sender: i64,
        recipient: i64,
        changes: &[(&'a str, &'a str)],
    ) -> Vec<(&'a str, String)> {
        let fields = vec![
            ("amount", "300".to_owned()),
            ("description", String::new()),
            ("sender", sender.to_string()),
            ("recipient", recipient.to_string()),
        ];
        changed(fields, changes)
    }

    fn changed<'a>(
        mut fields: Vec<(&'a str, String)>,
        changes: &[(&'a str, &'a str)],
    ) -> Vec<(&'a str, String)> {
        for (name, value) in changes {
            let field = fields.iter_mut().find(|(field, _)| field == name).unwrap();
            field.1 = (*value).to_owned();
        }
        fields
    }

    async fn post_expense(
        app: &TestApp,
        cookie: &str,
        fields: &[(&str, String)],
        htmx: bool,
        origin: &str,
    ) -> Response<Body> {
        post_form(app, "/expenses", cookie, fields, htmx, origin).await
    }

    async fn post_payment(
        app: &TestApp,
        cookie: &str,
        fields: &[(&str, String)],
        htmx: bool,
        origin: &str,
    ) -> Response<Body> {
        post_form(app, "/payments", cookie, fields, htmx, origin).await
    }

    /// Josef, the Admin, and Ondra, who both joined on 1 January 2026, and their browsers'
    /// session cookies.
    async fn josef_and_ondra(app: &TestApp) -> (String, String) {
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        (josef, ondra)
    }

    async fn page(app: &TestApp, uri: &str, cookie: &str) -> String {
        let response = app.get_with(uri, cookie).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
        body_text(response).await
    }

    /// A Balance as the Crew list on Finance shows it.
    fn balance_row(name: &str, balance: &str) -> String {
        format!(
            "<span class=\"balance-name\">{name}</span><span class=\"balance-amount\">{}</span>",
            balance.replace(' ', "\u{a0}")
        )
    }

    #[sqlx::test]
    async fn signed_out_finance_and_the_expense_form_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/").await,
            app.get("/expenses/new").await,
            post_expense(&app, "", &fields(1, &[]), false, PUBLIC_ORIGIN).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn finance_shows_everyone_even_before_anything_is_logged(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let html = page(&app, "/", &josef).await;

        assert!(html.contains("<h1>Finance</h1>"));
        assert!(html.contains("<p class=\"balance-figure\">Even</p>"));
        assert!(html.contains(&balance_row("Josef (you)", "Even")));
        assert!(html.contains(&balance_row("Ondra", "Even")));
        assert!(html.contains("Nothing logged yet."));
        assert!(html.contains("href=\"/expenses/new\""));
        assert!(html.contains("href=\"/payments/new\""));
    }

    #[sqlx::test]
    async fn the_form_defaults_to_me_paying_today(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, ondra) = josef_and_ondra(&app).await;

        let html = page(&app, "/expenses/new", &ondra).await;

        assert!(html.contains("<h1>New Expense</h1>"));
        assert!(html.contains("<option value=\"2\" selected>Ondra</option>"));
        assert!(html.contains("<option value=\"1\">Josef</option>"));
        assert!(html.contains("<summary>Spent today</summary>"));
        assert!(html.contains("value=\"2026-01-01\" max=\"2026-01-01\""));
    }

    #[sqlx::test]
    async fn today_follows_the_clock_in_prague(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        // 23:00 UTC on New Year's Day is midnight in Prague.
        app.clock.advance(Duration::from_hours(23));
        let html = page(&app, "/expenses/new", &josef).await;

        assert!(html.contains("value=\"2026-01-02\" max=\"2026-01-02\""));
    }

    #[sqlx::test]
    async fn logging_an_expense_updates_the_balances_on_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra) = josef_and_ondra(&app).await;

        let response = post_expense(&app, &ondra, &fields(1, &[]), true, PUBLIC_ORIGIN).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/");
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("<section class=\"card my-balance owed\""));
        assert!(html.contains(&balance_row("Josef (you)", "+500 Kč")));
        assert!(html.contains(&balance_row("Ondra", "−500 Kč")));
        assert!(html.contains("<span class=\"entry-what\" id=\"expense-1\">Malt</span>"));
        assert!(html.contains("Josef paid · 1 January 2026"));
        let html = page(&app, "/", &ondra).await;
        assert!(html.contains("<section class=\"card my-balance owes\""));
        assert!(html.contains("You owe the Crew this."));
    }

    #[sqlx::test]
    async fn logging_an_expense_tells_the_rest_of_the_crew(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, ondra) = josef_and_ondra(&app).await;
        let mut events = app.state.events.subscribe();

        post_expense(&app, &ondra, &fields(1, &[]), true, PUBLIC_ORIGIN).await;

        assert_eq!(
            events.try_recv(),
            Ok(Event::Finance(FinanceEvent {
                entry: EntryId::Expense(ExpenseId::from_client(1)),
                action: EntryAction::Added,
                actor: MemberId::from_client(2),
                notify: vec![MemberId::from_client(1)],
            }))
        );
    }

    #[sqlx::test]
    async fn a_plain_form_post_goes_back_to_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let response = post_expense(&app, &josef, &fields(1, &[]), false, PUBLIC_ORIGIN).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/");
        let html = page(&app, "/", &josef).await;
        assert!(html.contains(&balance_row("Josef (you)", "+500 Kč")));
    }

    #[sqlx::test]
    async fn each_mistake_is_refused_in_the_form_with_what_was_typed(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;
        let range = format!(
            "Enter an amount from {} to {}.",
            crowns(Crowns::new(1)),
            crowns(Crowns::new(1_000_000))
        );
        let too_long = "x".repeat(DESCRIPTION_MAX + 1);
        let cases = [
            (("amount", "0"), range.as_str()),
            (("amount", "1000001"), range.as_str()),
            (("amount", "12.50"), "Enter the amount in whole crowns."),
            (("description", "  "), "Say what the money was for."),
            (
                ("description", too_long.as_str()),
                "Keep the description to 500 characters or fewer.",
            ),
            (
                ("expense_date", "2026-01-02"),
                "The Expense date can't be in the future.",
            ),
            (
                ("expense_date", "next week"),
                "Enter the date the money was spent.",
            ),
            // Josef and Ondra joined on 1 January 2026.
            (
                ("expense_date", "2025-12-31"),
                "They weren't in the Crew yet on that date.",
            ),
        ];
        for ((field, value), message) in cases {
            let changes = [(field, value)];
            let response =
                post_expense(&app, &josef, &fields(2, &changes), true, PUBLIC_ORIGIN).await;

            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{field}={value}"
            );
            let html = body_text(response).await;
            assert!(
                html.starts_with("<form class=\"entry-form\""),
                "{field}={value}"
            );
            assert!(html.contains(message), "{field}={value}: {html}");
            assert!(html.contains("aria-invalid=\"true\""), "{field}={value}");
            assert!(
                html.contains(&format!("value=\"{value}\"")),
                "{field}={value}"
            );
            assert!(
                html.contains("<option value=\"2\" selected>Ondra</option>"),
                "{field}={value}"
            );
        }
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("Nothing logged yet."));
    }

    #[sqlx::test]
    async fn a_refused_plain_form_post_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let changes = [("amount", "")];
        let response = post_expense(&app, &josef, &fields(1, &changes), false, PUBLIC_ORIGIN).await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>New Expense</h1>"));
        assert!(html.contains("Enter the amount in whole crowns."));
    }

    #[sqlx::test]
    async fn logging_an_expense_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let response =
            post_expense(&app, &josef, &fields(1, &[]), true, "https://evil.example").await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("Nothing logged yet."));
    }

    #[sqlx::test]
    async fn signed_out_the_payment_form_redirects_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/payments/new").await,
            post_payment(&app, "", &payment_fields(1, 2, &[]), false, PUBLIC_ORIGIN).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn the_payment_form_defaults_to_me_sending(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, ondra) = josef_and_ondra(&app).await;

        let html = page(&app, "/payments/new", &ondra).await;

        assert!(html.contains("<h1>New Payment</h1>"));
        let sender = &html[html.find("name=\"sender\"").unwrap()..];
        let sender = &sender[..sender.find("</select>").unwrap()];
        assert!(sender.contains("<option value=\"2\" selected>Ondra</option>"));
        assert!(sender.contains("<option value=\"1\">Josef</option>"));
        assert!(html.contains("<option value=\"\" disabled selected>Choose a Member</option>"));
    }

    #[sqlx::test]
    async fn logging_a_payment_updates_both_balances_on_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra) = josef_and_ondra(&app).await;
        post_expense(&app, &josef, &fields(1, &[]), true, PUBLIC_ORIGIN).await;

        let response = post_payment(
            &app,
            &ondra,
            &payment_fields(2, 1, &[]),
            true,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/");
        let html = page(&app, "/", &josef).await;
        assert!(html.contains(&balance_row("Josef (you)", "+200 Kč")));
        assert!(html.contains(&balance_row("Ondra", "−200 Kč")));
        assert!(html.contains("<span class=\"entry-what\" id=\"payment-1\">Ondra → Josef</span>"));
        assert!(html.contains("<span class=\"entry-meta\">Payment · 1 January 2026</span>"));
        assert!(html.contains(&format!("{}</span>", crowns(Crowns::new(300)))));
    }

    #[sqlx::test]
    async fn a_payment_logged_for_someone_else_keeps_its_description(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let changes = [("description", "For the malt")];
        let response = post_payment(
            &app,
            &josef,
            &payment_fields(2, 1, &changes),
            false,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/");
        let html = page(&app, "/", &josef).await;
        assert!(html.contains(&balance_row("Josef (you)", "−300 Kč")));
        assert!(html.contains("Payment · For the malt · 1 January 2026"));
    }

    #[sqlx::test]
    async fn each_payment_mistake_is_refused_in_the_form_with_what_was_typed(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;
        let range = format!(
            "Enter an amount from {} to {}.",
            crowns(Crowns::new(1)),
            crowns(Crowns::new(1_000_000))
        );
        let too_long = "x".repeat(DESCRIPTION_MAX + 1);
        let cases = [
            (("amount", "0"), range.as_str()),
            (("amount", "1000001"), range.as_str()),
            (("amount", "12.50"), "Enter the amount in whole crowns."),
            (
                ("description", too_long.as_str()),
                "Keep the description to 500 characters or fewer.",
            ),
            (("recipient", "2"), "Choose someone other than the Sender."),
            (("recipient", ""), "Choose who got the money."),
            (
                ("recipient", "99"),
                "That Member isn't in the Crew. Reload the page to see who is.",
            ),
        ];
        for ((field, value), message) in cases {
            let changes = [(field, value)];
            let response = post_payment(
                &app,
                &josef,
                &payment_fields(2, 1, &changes),
                true,
                PUBLIC_ORIGIN,
            )
            .await;

            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{field}={value}"
            );
            let html = body_text(response).await;
            assert!(
                html.starts_with("<form class=\"entry-form\""),
                "{field}={value}"
            );
            assert!(html.contains(message), "{field}={value}: {html}");
            assert!(html.contains("aria-invalid=\"true\""), "{field}={value}");
            let sender = &html[html.find("name=\"sender\"").unwrap()..];
            assert!(
                sender.contains("<option value=\"2\" selected>Ondra</option>"),
                "{field}={value}"
            );
            if field != "recipient" {
                assert!(
                    html.contains(&format!("value=\"{value}\"")),
                    "{field}={value}"
                );
            }
        }
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("Nothing logged yet."));
    }

    #[sqlx::test]
    async fn a_payment_to_oneself_is_refused_along_with_other_mistakes(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let changes = [("amount", "0"), ("recipient", "1")];
        let response = post_payment(
            &app,
            &josef,
            &payment_fields(1, 2, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.contains("Enter an amount from"));
        assert!(html.contains("Choose someone other than the Sender."));
    }

    #[sqlx::test]
    async fn a_payment_with_no_recipient_chosen_is_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;
        // The browser sends nothing for a select left on its disabled placeholder.
        let fields: Vec<_> = payment_fields(1, 2, &[])
            .into_iter()
            .filter(|(name, _)| *name != "recipient")
            .collect();

        let response = post_payment(&app, &josef, &fields, true, PUBLIC_ORIGIN).await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.contains("Choose who got the money."));
        assert!(html.contains("<option value=\"\" disabled selected>Choose a Member</option>"));
    }

    #[sqlx::test]
    async fn a_refused_plain_payment_post_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let changes = [("amount", "")];
        let response = post_payment(
            &app,
            &josef,
            &payment_fields(1, 2, &changes),
            false,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>New Payment</h1>"));
        assert!(html.contains("Enter the amount in whole crowns."));
    }

    /// Josef, Ondra and Šimon after Josef paid 900 Kč for Malt and Ondra 3 Kč for a bottle cap:
    /// Josef is owed 599 Kč, Ondra owes 298 Kč and Šimon 301 Kč.
    async fn three_after_malt(app: &TestApp) -> (String, String, String) {
        let (josef, ondra) = josef_and_ondra(app).await;
        let simon = app.sign_in_new_member("Šimon").await;
        let changes = [("amount", "900")];
        post_expense(app, &josef, &fields(1, &changes), true, PUBLIC_ORIGIN).await;
        let changes = [("amount", "3"), ("description", "Bottle cap")];
        post_expense(app, &ondra, &fields(2, &changes), true, PUBLIC_ORIGIN).await;
        (josef, ondra, simon)
    }

    /// Where the "I've sent it" buttons on a Finance page lead, in order.
    fn sent_links(html: &str) -> Vec<String> {
        html.match_indices(">I've sent it</a>")
            .filter_map(|(end, _)| {
                let before = &html[..end];
                let href = &before[before.rfind("href=\"")? + "href=\"".len()..];
                Some(href[..href.find('"')?].replace("&amp;", "&"))
            })
            .collect()
    }

    /// `sender`'s row in the Crew's Balances, owing `amount` and sending it all to `recipient`
    /// as the `index`th Settlement of the plan.
    fn sends(sender: &str, recipient: &str, amount: &str, index: usize) -> String {
        format!(
            "{}<div class=\"balance-settlement\"><span class=\"balance-sends\" \
             id=\"settlement-{index}\">Sends {recipient} {}</span>",
            balance_row(sender, &format!("\u{2212}{amount}")),
            amount.replace(' ', "\u{a0}")
        )
    }

    /// My Balance's hint at whom I send how much.
    fn hint(recipient: &str, amount: &str) -> String {
        format!(
            "<p class=\"balance-next\">Send {recipient} <strong>{}</strong>.",
            amount.replace(' ', "\u{a0}")
        )
    }

    #[sqlx::test]
    async fn only_the_sender_logs_the_payment_for_a_settlement(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra, simon) = three_after_malt(&app).await;

        let html = page(&app, "/", &ondra).await;
        assert!(html.contains(&sends("Šimon", "Josef", "301 Kč", 0)));
        assert!(html.contains(&sends("Ondra (you)", "Josef", "298 Kč", 1)));
        assert_eq!(
            sent_links(&html),
            ["/payments/new?sender=2&recipient=1&amount=298"]
        );
        assert!(html.contains("aria-describedby=\"settlement-1\">I've sent it</a>"));

        let html = page(&app, "/", &josef).await;
        assert!(html.contains(&sends("Šimon", "you", "301 Kč", 0)));
        assert!(html.contains(&sends("Ondra", "you", "298 Kč", 1)));
        assert_eq!(sent_links(&html), Vec::<String>::new());

        let html = page(&app, "/", &simon).await;
        assert_eq!(
            sent_links(&html),
            ["/payments/new?sender=3&recipient=1&amount=301"]
        );
    }

    #[sqlx::test]
    async fn finance_leads_with_expenses_and_ends_with_the_crew_balances(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, ondra, _) = three_after_malt(&app).await;

        let html = page(&app, "/", &ondra).await;
        let at = |text: &str| html.find(text).unwrap_or_else(|| panic!("no {text}"));
        assert!(at("Your Balance") < at("+ Expense"));
        assert!(at("+ Expense") < at("+ Payment"));
        assert!(at("+ Payment") < at("Latest entries"));
        assert!(at("Latest entries") < at("Crew Balances</h2>"));
        assert!(!html.contains("Settle up"));
        assert!(!html.contains(">Record</a>"));
    }

    #[sqlx::test]
    async fn my_balance_says_whom_to_send_how_much_only_while_i_owe(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra, _) = three_after_malt(&app).await;

        let html = page(&app, "/", &ondra).await;
        assert!(html.contains(&hint("Josef", "298 Kč")));
        assert!(html.contains("<a href=\"#crew-balances\">See Crew Balances</a>"));

        let html = page(&app, "/", &josef).await;
        assert!(!html.contains("balance-next"));
    }

    #[sqlx::test]
    async fn logging_the_payment_for_a_sent_settlement_makes_both_members_even(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra, _) = three_after_malt(&app).await;
        let html = page(&app, "/", &ondra).await;
        let sent = sent_links(&html).remove(0);

        let html = page(&app, &sent, &ondra).await;
        assert!(html.contains("<h1>New Payment</h1>"));
        assert!(html.contains("name=\"amount\" value=\"298\""));
        let sender = &html[html.find("name=\"sender\"").unwrap()..];
        let sender = &sender[..sender.find("</select>").unwrap()];
        assert!(sender.contains("<option value=\"2\" selected>Ondra</option>"));
        let recipient = &html[html.find("name=\"recipient\"").unwrap()..];
        let recipient = &recipient[..recipient.find("</select>").unwrap()];
        assert!(recipient.contains("<option value=\"1\" selected>Josef</option>"));
        assert!(recipient.contains("<option value=\"\" disabled>Choose a Member</option>"));

        let changes = [("amount", "298")];
        let response = post_payment(
            &app,
            &ondra,
            &payment_fields(2, 1, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let html = page(&app, "/", &ondra).await;
        assert!(html.contains("<p class=\"balance-figure\">Even</p>"));
        assert!(!html.contains("balance-next"));
        assert_eq!(sent_links(&html), Vec::<String>::new());
        let html = page(&app, "/", &josef).await;
        assert!(html.contains(&balance_row("Ondra", "Even")));
        assert!(html.contains(&sends("Šimon", "you", "301 Kč", 0)));
        assert!(!html.contains("Sends you 298"));
    }

    #[sqlx::test]
    async fn a_member_within_a_crown_of_zero_is_even_with_no_settlement(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra) = josef_and_ondra(&app).await;
        // 3 Kč split two ways: Josef is owed a crown and Ondra owes it.
        let changes = [("amount", "3")];
        post_expense(&app, &josef, &fields(1, &changes), true, PUBLIC_ORIGIN).await;

        let html = page(&app, "/", &ondra).await;
        assert!(html.contains("<p class=\"balance-figure\">Even</p>"));
        assert!(html.contains(&balance_row("Ondra (you)", "Even")));
        assert!(html.contains(&balance_row("Josef", "Even")));
        assert!(!html.contains("balance-next"));
        assert!(!html.contains("balance-sends"));
    }

    #[sqlx::test]
    async fn logging_a_payment_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;

        let response = post_payment(
            &app,
            &josef,
            &payment_fields(1, 2, &[]),
            true,
            "https://evil.example",
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("Nothing logged yet."));
    }

    /// Josef the Admin, Ondra and Šimon, after Ondra logged a 900 Kč Malt Expense that Šimon
    /// paid: the Crew's first Expense.
    async fn simon_paid_ondra_logged(app: &TestApp) -> (String, String, String) {
        let (josef, ondra) = josef_and_ondra(app).await;
        let simon = app.sign_in_new_member("Šimon").await;
        let changes = [("amount", "900")];
        let response = post_expense(app, &ondra, &fields(3, &changes), true, PUBLIC_ORIGIN).await;
        assert_eq!(response.status(), StatusCode::OK);
        (josef, ondra, simon)
    }

    /// Whether Finance leads the browser holding `cookie` to the edit page of the entry at
    /// `path`. Finance itself never deletes: that is on the edit page.
    async fn sees_controls(app: &TestApp, cookie: &str, path: &str) -> bool {
        let html = page(app, "/", cookie).await;
        assert!(!html.contains(&format!("{path}/delete")), "{path}");
        html.contains(&format!("href=\"{path}/edit\""))
    }

    #[sqlx::test]
    async fn signed_out_editing_and_deleting_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/expenses/1/edit").await,
            post_form(
                &app,
                "/expenses/1",
                "",
                &fields(1, &[]),
                false,
                PUBLIC_ORIGIN,
            )
            .await,
            app.post_with("/expenses/1/delete", "").await,
            app.get("/payments/1/edit").await,
            app.post_with("/payments/1/delete", "").await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn the_payer_edits_an_expense_from_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _, simon) = simon_paid_ondra_logged(&app).await;
        assert!(sees_controls(&app, &simon, "/expenses/1").await);

        let html = page(&app, "/expenses/1/edit", &simon).await;
        assert!(html.contains("<h1>Edit Expense</h1>"));
        assert!(html.contains("action=\"/expenses/1\""));
        assert!(html.contains("name=\"amount\" value=\"900\""));
        assert!(html.contains("name=\"description\" value=\"Malt\""));
        assert!(html.contains("<option value=\"3\" selected>Šimon</option>"));
        assert!(html.contains(">Save Expense</button>"));
        assert!(html.contains("action=\"/expenses/1/delete\""));

        // Josef paid it, not Šimon, and on New Year's Eve, before anyone had joined: refused.
        let changes = [("amount", "600"), ("expense_date", "2025-12-31")];
        let response = post_form(
            &app,
            "/expenses/1",
            &simon,
            &fields(1, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.starts_with("<form class=\"entry-form\""));
        assert!(html.contains("action=\"/expenses/1\""));
        assert!(html.contains("They weren't in the Crew yet on that date."));
        assert!(html.contains("value=\"600\""));
        assert!(html.contains("<option value=\"1\" selected>Josef</option>"));

        let changes = [("amount", "600")];
        let response = post_form(
            &app,
            "/expenses/1",
            &simon,
            &fields(1, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/");

        let html = page(&app, "/", &josef).await;
        assert!(html.contains(&balance_row("Josef (you)", "+400 Kč")));
        assert!(html.contains(&balance_row("Šimon", "−200 Kč")));
        assert!(html.contains("Josef paid · 1 January 2026"));
        assert!(html.contains("edited by Šimon · 1 January 2026"));
        // Šimon handed the Expense to Josef, so it is no longer his to change.
        assert!(!sees_controls(&app, &simon, "/expenses/1").await);
    }

    #[sqlx::test]
    async fn only_the_payer_and_the_admin_may_change_an_expense(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra, _) = simon_paid_ondra_logged(&app).await;

        // Ondra logged it, but Šimon paid.
        assert!(!sees_controls(&app, &ondra, "/expenses/1").await);
        for response in [
            app.get_with("/expenses/1/edit", &ondra).await,
            post_form(
                &app,
                "/expenses/1",
                &ondra,
                &fields(3, &[]),
                true,
                PUBLIC_ORIGIN,
            )
            .await,
            post_form(
                &app,
                "/expenses/1",
                &ondra,
                &fields(3, &[("amount", "")]),
                true,
                PUBLIC_ORIGIN,
            )
            .await,
            app.post_with("/expenses/1/delete", &ondra).await,
        ] {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(
                body_text(response).await,
                "Only whoever paid or sent the money, or the Admin, can change this entry."
            );
        }
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("<span class=\"entry-amount\">900\u{a0}Kč</span>"));

        assert!(sees_controls(&app, &josef, "/expenses/1").await);
        page(&app, "/expenses/1/edit", &josef).await;
        let changes = [("amount", "300")];
        let response = post_form(
            &app,
            "/expenses/1",
            &josef,
            &fields(3, &changes),
            false,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("edited by Josef · 1 January 2026"));
        assert!(html.contains(&balance_row("Šimon", "+200 Kč")));
    }

    #[sqlx::test]
    async fn a_refused_plain_edit_gets_the_whole_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, _, simon) = simon_paid_ondra_logged(&app).await;

        let changes = [("description", " ")];
        let response = post_form(
            &app,
            "/expenses/1",
            &simon,
            &fields(3, &changes),
            false,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Edit Expense</h1>"));
        assert!(html.contains("Say what the money was for."));
    }

    #[sqlx::test]
    async fn a_deleted_expense_is_gone_from_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra, simon) = simon_paid_ondra_logged(&app).await;

        let mut delete = request(Method::POST, "/expenses/1/delete", Some(PUBLIC_ORIGIN));
        delete.headers_mut().insert(COOKIE, simon.parse().unwrap());
        delete
            .headers_mut()
            .insert(HX_REQUEST, "true".parse().unwrap());
        let response = app.send(delete).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/");

        let html = page(&app, "/", &josef).await;
        assert!(html.contains("Nothing logged yet."));
        assert!(html.contains(&balance_row("Šimon", "Even")));
        assert!(!html.contains("balance-sends"));
        for response in [
            app.post_with("/expenses/1/delete", &simon).await,
            app.get_with("/expenses/1/edit", &josef).await,
        ] {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        assert!(!sees_controls(&app, &ondra, "/expenses/1").await);
    }

    #[sqlx::test]
    async fn editing_and_deleting_from_history_goes_back_to_the_same_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, _, simon) = simon_paid_ondra_logged(&app).await;
        let back = "back=%2Fhistory%3Fmember%3D3";

        let html = page(&app, "/history?member=3", &simon).await;
        assert!(html.contains(&format!("href=\"/expenses/1/edit?{back}\"")));
        let html = page(&app, &format!("/expenses/1/edit?{back}"), &simon).await;
        assert!(html.contains(&format!("action=\"/expenses/1?{back}\"")));
        assert!(html.contains(&format!("action=\"/expenses/1/delete?{back}\"")));

        let edit = format!("/expenses/1?{back}");
        let changes = [("amount", "600")];
        let response = post_form(
            &app,
            &edit,
            &simon,
            &fields(3, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/history?member=3");

        let delete = format!("/expenses/1/delete?{back}");
        let response = post_form(&app, &delete, &simon, &[], false, PUBLIC_ORIGIN).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/history?member=3");
    }

    #[sqlx::test]
    async fn going_back_anywhere_but_history_goes_to_finance(db: SqlitePool) {
        let app = TestApp::new(db);
        let (_, _, simon) = simon_paid_ondra_logged(&app).await;

        for back in [
            "https%3A%2F%2Fevil.example",
            "%2F%2Fevil.example",
            "%2Fhistoryx",
        ] {
            let html = page(&app, &format!("/expenses/1/edit?back={back}"), &simon).await;
            assert!(html.contains("action=\"/expenses/1\""), "{back}");
            let edit = format!("/expenses/1?back={back}");
            let response =
                post_form(&app, &edit, &simon, &fields(3, &[]), false, PUBLIC_ORIGIN).await;
            assert_eq!(response.headers()[LOCATION], "/", "{back}");
        }
    }

    #[sqlx::test]
    async fn deleting_an_entry_needs_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _, simon) = simon_paid_ondra_logged(&app).await;

        let mut delete = request(
            Method::POST,
            "/expenses/1/delete",
            Some("https://evil.example"),
        );
        delete.headers_mut().insert(COOKIE, simon.parse().unwrap());
        let response = app.send(delete).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("<span class=\"entry-what\" id=\"expense-1\">Malt</span>"));
    }

    #[sqlx::test]
    async fn signed_out_history_redirects_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for uri in ["/history", "/history?member=1&month=2026-01&page=2"] {
            let response = app.get(uri).await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
            assert_eq!(response.headers()[LOCATION], "/sign-in", "{uri}");
        }
    }

    /// The ids of the Expenses a page lists, in order.
    fn expense_ids(html: &str) -> Vec<i64> {
        html.match_indices("id=\"expense-")
            .filter_map(|(start, found)| {
                let id = &html[start + found.len()..];
                id[..id.find('"')?].parse().ok()
            })
            .collect()
    }

    #[sqlx::test]
    async fn history_lists_every_entry_but_the_deleted_across_pages(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;
        for crowns in 1..=27 {
            let amount = crowns.to_string();
            let changes = [("amount", amount.as_str())];
            post_expense(&app, &josef, &fields(1, &changes), true, PUBLIC_ORIGIN).await;
        }
        app.post_with("/expenses/27/delete", &josef).await;

        let html = page(&app, "/", &josef).await;
        assert!(html.contains("<a href=\"/history\">See every entry in History</a>"));

        let first = page(&app, "/history", &josef).await;
        assert!(first.contains("<h1>History</h1>"));
        assert_eq!(expense_ids(&first), (2..=26).rev().collect::<Vec<_>>());
        assert!(first.contains("Page 1 of 2"));
        assert!(first.contains("rel=\"next\" href=\"/history?page=2\">Older</a>"));
        // The Admin may change every entry.
        assert!(first.contains("href=\"/expenses/26/edit?back=%2Fhistory\""));
        let second = page(&app, "/history?page=2", &josef).await;
        assert_eq!(expense_ids(&second), [1]);
        assert!(second.contains("rel=\"prev\" href=\"/history\">Newer</a>"));
        // 1 + 2 + … + 26 Kč, without the deleted 27 Kč.
        assert!(second.contains(&format!(
            "January 2026</a></th><td>{}</td><td>{}</td>",
            crowns(Crowns::new(351)),
            crowns(Crowns::ZERO)
        )));
    }

    #[sqlx::test]
    async fn history_filters_by_payer_sender_or_recipient(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra, _) = simon_paid_ondra_logged(&app).await;
        post_payment(
            &app,
            &josef,
            &payment_fields(2, 1, &[]),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        let lists = |html: &str| {
            (
                html.contains("id=\"expense-1\""),
                html.contains("id=\"payment-1\""),
            )
        };

        // Šimon paid the Malt; Ondra sent Josef the Payment and logged the Malt.
        let simons = page(&app, "/history?member=3", &ondra).await;
        assert_eq!(lists(&simons), (true, false));
        assert!(simons.contains("<option value=\"3\" selected>Šimon</option>"));
        assert_eq!(
            lists(&page(&app, "/history?member=2", &ondra).await),
            (false, true)
        );
        assert_eq!(
            lists(&page(&app, "/history?member=1", &ondra).await),
            (false, true)
        );
        assert_eq!(
            lists(&page(&app, "/history?member=&month=", &ondra).await),
            (true, true)
        );
        let nobodys = page(&app, "/history?member=99", &ondra).await;
        assert_eq!(lists(&nobodys), (false, false));
        assert!(nobodys.contains("No entries match."));
        assert!(nobodys.contains(">Clear</a>"));
    }

    #[sqlx::test]
    async fn history_months_turn_at_midnight_in_prague(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra) = josef_and_ondra(&app).await;
        // 2026-01-31T22:59:59Z is 23:59:59 on 31 January in Prague (UTC+1); a second later it's
        // February.
        app.clock.advance(Duration::from_secs(
            30 * 24 * 60 * 60 + 22 * 60 * 60 + 59 * 60 + 59,
        ));
        post_payment(
            &app,
            &ondra,
            &payment_fields(2, 1, &[]),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        app.clock.advance(Duration::from_secs(1));
        let changes = [("amount", "400")];
        post_payment(
            &app,
            &ondra,
            &payment_fields(2, 1, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        let changes = [("expense_date", "2026-01-31")];
        post_expense(&app, &josef, &fields(1, &changes), true, PUBLIC_ORIGIN).await;

        let january = page(&app, "/history?month=2026-01", &josef).await;
        assert!(january.contains("id=\"payment-1\""));
        assert!(!january.contains("id=\"payment-2\""));
        assert!(january.contains("id=\"expense-1\""));
        assert!(january.contains("value=\"2026-01\""));
        assert!(january.contains(&format!(
            "January 2026</a></th><td>{}</td><td>{}</td>",
            crowns(Crowns::new(1_000)),
            crowns(Crowns::new(300))
        )));
        assert!(!january.contains("February 2026"));

        let february = page(&app, "/history?month=2026-02", &josef).await;
        assert!(february.contains("id=\"payment-2\""));
        assert!(!february.contains("id=\"payment-1\""));
        assert!(!february.contains("id=\"expense-1\""));

        let everything = page(&app, "/history", &josef).await;
        assert!(everything.contains("<a href=\"/history?month=2026-02\">February 2026</a>"));
        assert!(everything.contains("<a href=\"/history?month=2026-01\">January 2026</a>"));
    }

    #[sqlx::test]
    async fn a_mistyped_month_is_shown_with_what_to_fix(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, _) = josef_and_ondra(&app).await;
        post_expense(&app, &josef, &fields(1, &[]), true, PUBLIC_ORIGIN).await;

        let html = page(&app, "/history?month=Oct", &josef).await;

        assert!(html.contains("value=\"Oct\""));
        assert!(html.contains("aria-invalid=\"true\""));
        assert!(html.contains("Enter a month as YYYY-MM, like 2026-10."));
        assert!(html.contains("id=\"expense-1\""));
    }

    #[sqlx::test]
    async fn the_sender_edits_and_deletes_a_payment(db: SqlitePool) {
        let app = TestApp::new(db);
        let (josef, ondra) = josef_and_ondra(&app).await;
        let simon = app.sign_in_new_member("Šimon").await;
        // Josef logs that Ondra sent him 300 Kč.
        post_payment(
            &app,
            &josef,
            &payment_fields(2, 1, &[]),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert!(sees_controls(&app, &ondra, "/payments/1").await);
        assert!(!sees_controls(&app, &simon, "/payments/1").await);
        assert_eq!(
            app.get_with("/payments/1/edit", &simon).await.status(),
            StatusCode::FORBIDDEN
        );

        let html = page(&app, "/payments/1/edit", &ondra).await;
        assert!(html.contains("<h1>Edit Payment</h1>"));
        assert!(html.contains("name=\"amount\" value=\"300\""));
        let recipient = &html[html.find("name=\"recipient\"").unwrap()..];
        assert!(recipient.contains("<option value=\"1\" selected>Josef</option>"));

        app.clock.advance(Duration::from_hours(24));
        let changes = [("amount", "250"), ("description", "For the hops")];
        let response = post_form(
            &app,
            "/payments/1",
            &ondra,
            &payment_fields(2, 3, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("<span class=\"entry-what\" id=\"payment-1\">Ondra → Šimon</span>"));
        // The Payment keeps the day it was logged on; the edit has its own.
        assert!(html.contains("Payment · For the hops · 1 January 2026"));
        assert!(html.contains("edited by Ondra · 2 January 2026"));
        assert!(html.contains(&balance_row("Josef (you)", "Even")));

        let changes = [("recipient", "2")];
        let response = post_form(
            &app,
            "/payments/1",
            &ondra,
            &payment_fields(2, 3, &changes),
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            body_text(response)
                .await
                .contains("Choose someone other than the Sender.")
        );

        let response = app.post_with("/payments/1/delete", &ondra).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/");
        let html = page(&app, "/", &josef).await;
        assert!(html.contains("Nothing logged yet."));
        assert!(html.contains(&balance_row("Šimon", "Even")));
    }
}
