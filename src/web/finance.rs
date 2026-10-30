//! The Finance tab, Asgard's home page: everyone's Balance, the latest entries, and logging an
//! Expense.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use jiff::civil::Date;
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::error::{amount_message, description_message};
use super::htmx::{HX_REDIRECT, Htmx};
use super::pages::fresh;
use super::{AppError, AppState, welcome};
use crate::domain::finance::money::Amount;
use crate::domain::finance::{Description, ExpenseError, NewExpense};
use crate::domain::members::{self, MemberId};
use crate::views;
use crate::views::finance::{ExpenseProblems, ExpenseValues};
use crate::views::nav::Tab;

/// Where the "+ Expense" button leads.
pub const NEW_EXPENSE_PATH: &str = "/expenses/new";

/// Where the Expense form posts.
const EXPENSES_PATH: &str = "/expenses";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Finance.href(), get(home))
        .route(NEW_EXPENSE_PATH, get(new_expense))
        .route(EXPENSES_PATH, post(log_expense))
}

/// The home page. It sends a Member to the welcome page until they have pressed "Got it"
/// there.
async fn home(State(state): State<AppState>, Member(member): Member) -> Result<Response, AppError> {
    if !member.welcomed {
        return Ok(Redirect::to(welcome::PATH).into_response());
    }
    let overview = state.finance.overview().await?;
    Ok(fresh(views::finance::home(member.id, &overview, NEW_EXPENSE_PATH)).into_response())
}

/// The empty Expense form: the Payer is the Member asking, and the Expense date is today.
async fn new_expense(
    State(state): State<AppState>,
    Member(member): Member,
) -> Result<impl IntoResponse, AppError> {
    let today = state.finance.today().to_string();
    let values = ExpenseValues::blank(member.id, &today);
    let form = expense_form(&state, &values, &ExpenseProblems::default()).await?;
    Ok(fresh(views::finance::expense_page(&form)))
}

#[derive(Deserialize)]
struct ExpenseForm {
    amount: String,
    description: String,
    payer: i64,
    expense_date: String,
}

/// Logs the Expense and goes back to Finance, or answers with the form showing what to fix:
/// alone for htmx, inside its page otherwise.
async fn log_expense(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<ExpenseForm>,
) -> Result<Response, AppError> {
    let payer = MemberId::from_client(input.payer);
    let problems = match parse(&input, payer) {
        Ok(expense) => match state.finance.log_expense(member.id, &expense).await {
            Ok(_) => {
                tracing::info!(member = member.id.get(), "a Member logged an Expense");
                return Ok(back_to_finance(htmx));
            }
            Err(ExpenseError::FutureDate) => ExpenseProblems {
                expense_date: Some("The Expense date can't be in the future.".to_owned()),
                ..ExpenseProblems::default()
            },
            Err(ExpenseError::PayerNotInCrew) => ExpenseProblems {
                payer: Some("They weren't in the Crew yet on that date.".to_owned()),
                ..ExpenseProblems::default()
            },
            Err(ExpenseError::Database(err)) => return Err(err.into()),
        },
        Err(problems) => problems,
    };
    let values = ExpenseValues {
        amount: &input.amount,
        description: &input.description,
        payer,
        expense_date: &input.expense_date,
    };
    let form = expense_form(&state, &values, &problems).await?;
    let body = if htmx {
        form
    } else {
        views::finance::expense_page(&form)
    };
    Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response())
}

/// htmx can't follow a redirect to a whole page, so it is told to load Finance instead.
fn back_to_finance(htmx: bool) -> Response {
    let home = Tab::Finance.href();
    if htmx {
        (StatusCode::OK, [(HX_REDIRECT, home)]).into_response()
    } else {
        Redirect::to(home).into_response()
    }
}

/// The Expense as typed, or what is wrong with each field that can be checked on its own.
fn parse(input: &ExpenseForm, payer: MemberId) -> Result<NewExpense, ExpenseProblems> {
    let amount = Amount::parse(&input.amount);
    let description = Description::parse(&input.description);
    let expense_date = input.expense_date.trim().parse::<Date>();
    match (amount, description, expense_date) {
        (Ok(amount), Ok(description), Ok(expense_date)) => Ok(NewExpense {
            payer,
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

/// The Expense form showing `values` and `problems`, with every Member to choose the Payer from.
async fn expense_form(
    state: &AppState,
    values: &ExpenseValues<'_>,
    problems: &ExpenseProblems,
) -> Result<Markup, AppError> {
    let members: Vec<members::MemberName> = state.members.all().await?;
    Ok(views::finance::expense_form(
        EXPENSES_PATH,
        &members,
        values,
        problems,
        state.finance.today(),
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::header::{CACHE_CONTROL, COOKIE, LOCATION};
    use axum::http::{Method, Response, StatusCode};
    use sqlx::SqlitePool;
    use url::form_urlencoded;

    use crate::domain::finance::DESCRIPTION_MAX;
    use crate::domain::finance::money::Crowns;
    use crate::views::finance::crowns;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, request};

    /// The fields of a 1 000 Kč Malt Expense paid by the Member `payer` on 1 January 2026, the
    /// test app's first day, with `changes` made to them.
    fn fields<'a>(payer: i64, changes: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
        let mut fields = vec![
            ("amount", "1000".to_owned()),
            ("description", "Malt".to_owned()),
            ("payer", payer.to_string()),
            ("expense_date", "2026-01-01".to_owned()),
        ];
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
        let mut post = request(Method::POST, "/expenses", Some(origin));
        let headers = post.headers_mut();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        if htmx {
            headers.insert(HX_REQUEST, "true".parse().unwrap());
        }
        *post.body_mut() = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().map(|(name, value)| (name, value.as_str())))
            .finish()
            .into();
        app.send(post).await
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
        assert!(html.contains("<section class=\"my-balance owed\""));
        assert!(html.contains(&balance_row("Josef (you)", "+500 Kč")));
        assert!(html.contains(&balance_row("Ondra", "−500 Kč")));
        assert!(html.contains("<span class=\"entry-what\">Malt</span>"));
        assert!(html.contains("Josef paid · 1 January 2026"));
        let html = page(&app, "/", &ondra).await;
        assert!(html.contains("<section class=\"my-balance owes\""));
        assert!(html.contains("You owe the Crew this."));
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
}
