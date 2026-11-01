//! The Reminders tab: creating a Reminder for the whole Crew, and the Reminders still pending.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Form, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::htmx::{Htmx, go_to};
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::reminders::{
    DueAt, NewReminder, ReminderError, ReminderText, TEXT_MAX, TextError,
};
use crate::views;
use crate::views::nav::Tab;
use crate::views::reminders::{CREATE, ReminderProblems, ReminderValues};

pub fn routes() -> Router<AppState> {
    debug_assert_eq!(CREATE, Tab::Reminders.href());
    Router::new().route(Tab::Reminders.href(), get(show).post(create))
}

/// The Reminders tab with an empty form.
async fn show(
    State(state): State<AppState>,
    Member(_): Member,
) -> Result<impl IntoResponse, AppError> {
    let form = views::reminders::form(
        ReminderValues::default(),
        &ReminderProblems::default(),
        state.reminders.now(),
    );
    Ok(fresh(page(&state, &form).await?))
}

#[derive(Deserialize)]
struct ReminderForm {
    text: String,
    due_at: String,
}

impl ReminderForm {
    fn values(&self) -> ReminderValues<'_> {
        ReminderValues {
            text: &self.text,
            due_at: &self.due_at,
        }
    }
}

/// Creates the Reminder and goes back to the Reminders tab, or answers with the form showing
/// what to fix: alone for htmx, inside the tab otherwise.
async fn create(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<ReminderForm>,
) -> Result<Response, AppError> {
    let problems = match parse(&input) {
        Ok(reminder) => match state.reminders.create(member.id, &reminder).await {
            Ok(_) => {
                tracing::info!(member = member.id.get(), "a Member created a Reminder");
                return Ok(go_to(htmx, Tab::Reminders.href()));
            }
            Err(ReminderError::NotInFuture) => ReminderProblems {
                due_at: Some(NOT_IN_FUTURE.to_owned()),
                ..ReminderProblems::default()
            },
            Err(ReminderError::Database(err)) => return Err(err.into()),
        },
        Err(problems) => problems,
    };
    let form = views::reminders::form(input.values(), &problems, state.reminders.now());
    let body = if htmx {
        form
    } else {
        page(&state, &form).await?
    };
    Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response())
}

/// What a Member is told when the due time they picked has already passed.
const NOT_IN_FUTURE: &str = "Pick a time that hasn't passed yet.";

/// The Reminder as typed, or what is wrong with each field.
fn parse(input: &ReminderForm) -> Result<NewReminder, ReminderProblems> {
    match (
        ReminderText::parse(&input.text),
        DueAt::parse(&input.due_at),
    ) {
        (Ok(text), Ok(due_at)) => Ok(NewReminder { text, due_at }),
        (text, due_at) => Err(ReminderProblems {
            text: text.err().map(text_message),
            due_at: due_at
                .err()
                .map(|_| "Enter the date and time it's due.".to_owned()),
        }),
    }
}

fn text_message(err: TextError) -> String {
    match err {
        TextError::Blank => "Say what to remember.".to_owned(),
        TextError::TooLong => format!("Keep it to {TEXT_MAX} characters or fewer."),
    }
}

/// The Reminders tab around `form`, with every pending Reminder.
async fn page(state: &AppState, form: &Markup) -> Result<Markup, AppError> {
    let pending = state.reminders.pending().await?;
    Ok(views::reminders::page(form, &pending))
}

#[cfg(test)]
mod tests {
    use axum::http::header::LOCATION;
    use axum::http::{Method, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::reminders::TEXT_MAX;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    /// The test app's clock starts at 01:00 on 1 January 2026 in Prague.
    const TOMORROW_MORNING: &str = "2026-01-02T09:00";

    fn fields(text: &str, due_at: &str) -> Vec<(&'static str, String)> {
        vec![("text", text.to_owned()), ("due_at", due_at.to_owned())]
    }

    async fn create(app: &TestApp, cookie: &str, text: &str, due_at: &str, htmx: bool) -> String {
        let response = post_form(
            app,
            "/reminders",
            cookie,
            &fields(text, due_at),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        body
    }

    async fn tab(app: &TestApp, cookie: &str) -> String {
        let response = app.get_with("/reminders", cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    #[sqlx::test]
    async fn signed_out_the_reminders_tab_redirects_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/reminders").await,
            post_form(
                &app,
                "/reminders",
                "",
                &fields("Bottle the stout", TOMORROW_MORNING),
                false,
                PUBLIC_ORIGIN,
            )
            .await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn signed_out_htmx_is_told_to_load_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut post = request(Method::POST, "/reminders", Some(PUBLIC_ORIGIN));
        post.headers_mut()
            .insert(HX_REQUEST, "true".parse().unwrap());

        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[HX_REDIRECT], "/sign-in");
    }

    #[sqlx::test]
    async fn a_new_tab_has_the_form_and_nothing_pending(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = tab(&app, &cookie).await;

        assert!(html.contains(r#"<form class="entry-form" method="post" action="/reminders""#));
        assert!(html.contains(r#"min="2026-01-01T01:00""#));
        assert!(html.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn a_member_creates_a_reminder_and_sees_it_pending(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;

        let response = post_form(
            &app,
            "/reminders",
            &ondra,
            &fields("  Bottle the stout ", TOMORROW_MORNING),
            false,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/reminders");
        // The whole Crew sees it, with its Prague due time and its creator.
        let html = tab(&app, &josef).await;
        assert!(html.contains(">Bottle the stout</span>"), "{html}");
        assert!(html.contains(
            r#"<time datetime="2026-01-02T09:00">Friday 2 January 2026, 09:00</time> · Ondra"#
        ));
    }

    #[sqlx::test]
    async fn htmx_is_told_to_load_the_tab_once_created(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = post_form(
            &app,
            "/reminders",
            &cookie,
            &fields("Bottle the stout", TOMORROW_MORNING),
            true,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/reminders");
    }

    #[sqlx::test]
    async fn pending_is_soonest_first(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        for (text, due_at) in [
            ("Third", "2026-02-01T10:00"),
            ("First", "2026-01-01T12:00"),
            ("Second", "2026-01-15T08:00"),
        ] {
            post_form(
                &app,
                "/reminders",
                &cookie,
                &fields(text, due_at),
                true,
                PUBLIC_ORIGIN,
            )
            .await;
        }

        let html = tab(&app, &cookie).await;

        let at = |text: &str| html.find(&format!(">{text}</span>")).unwrap();
        assert!(at("First") < at("Second"));
        assert!(at("Second") < at("Third"));
    }

    #[sqlx::test]
    async fn prague_time_is_kept_across_the_clocks_going_forward(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        for (text, due_at) in [
            ("Before", "2026-03-28T10:00"),
            ("After", "2026-03-30T10:00"),
            ("Skipped", "2026-03-29T02:30"),
        ] {
            post_form(
                &app,
                "/reminders",
                &cookie,
                &fields(text, due_at),
                true,
                PUBLIC_ORIGIN,
            )
            .await;
        }

        let html = tab(&app, &cookie).await;

        assert!(html.contains(r#"<time datetime="2026-03-28T10:00">"#));
        assert!(html.contains(r#"<time datetime="2026-03-30T10:00">"#));
        // There is no 02:30 that night; it's read as 03:30 summer time.
        assert!(html.contains(r#"<time datetime="2026-03-29T03:30">"#));
    }

    #[sqlx::test]
    async fn refuses_a_blank_text_with_htmx_and_creates_nothing(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = create(&app, &cookie, " \n ", TOMORROW_MORNING, true).await;

        assert!(html.starts_with("<form"), "a fragment: {html}");
        assert!(html.contains("Say what to remember."));
        // What was typed stays in the form.
        assert!(html.contains(r#"value="2026-01-02T09:00""#));
        assert!(tab(&app, &cookie).await.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn refuses_a_text_too_long_without_htmx_inside_the_tab(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = create(
            &app,
            &cookie,
            &"a".repeat(TEXT_MAX + 1),
            TOMORROW_MORNING,
            false,
        )
        .await;

        assert!(html.contains("<h1>Reminders</h1>"), "the whole tab: {html}");
        assert!(html.contains("Keep it to 500 characters or fewer."));
        assert!(html.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn refuses_a_due_time_in_the_past(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        // The clock stands at 01:00 in Prague.
        let html = create(&app, &cookie, "Too late", "2026-01-01T00:59", true).await;

        assert!(html.contains("Pick a time that hasn't passed yet."));
        assert!(html.contains(r#"aria-describedby="reminder-due-problem""#));
        assert!(tab(&app, &cookie).await.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn refuses_what_isnt_a_date_and_time(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = create(&app, &cookie, "Bottle the stout", "", true).await;

        assert!(html.contains("Enter the date and time it"), "{html}");
        assert!(tab(&app, &cookie).await.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn refuses_a_post_from_another_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = post_form(
            &app,
            "/reminders",
            &cookie,
            &fields("Bottle the stout", TOMORROW_MORNING),
            false,
            "https://evil.test",
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(tab(&app, &cookie).await.contains("Nothing pending."));
    }
}
