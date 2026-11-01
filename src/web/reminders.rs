//! The Reminders tab: creating a Reminder for the whole Crew, the Reminders still pending, and
//! editing or deleting one of them.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::error::NOT_IN_FUTURE;
use super::htmx::{Htmx, fragment_or_page, go_to};
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::reminders::{
    DueAt, NewReminder, ReminderError, ReminderId, ReminderText, TEXT_MAX, TextError,
};
use crate::views;
use crate::views::nav::Tab;
use crate::views::reminders::{FormFor, ReminderProblems, ReminderValues};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Reminders.href(), get(show).post(create))
        .route("/reminders/{id}/edit", get(edit))
        .route("/reminders/{id}", post(save))
        .route("/reminders/{id}/delete", post(delete))
}

/// The Reminders tab with an empty form.
async fn show(
    State(state): State<AppState>,
    Member(_): Member,
) -> Result<impl IntoResponse, AppError> {
    let form = views::reminders::form(
        FormFor::New,
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
            Err(err) => reminder_problems(err)?,
        },
        Err(problems) => problems,
    };
    let form = views::reminders::form(
        FormFor::New,
        input.values(),
        &problems,
        state.reminders.now(),
    );
    let body = if htmx {
        form
    } else {
        page(&state, &form).await?
    };
    Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response())
}

/// The pending Reminder `id` in its form, for any Member to edit or delete.
async fn edit(
    State(state): State<AppState>,
    Member(_): Member,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let id = ReminderId::from_client(id);
    let reminder = state.reminders.to_edit(id).await?;
    let form = views::reminders::edit_form(&reminder, state.reminders.now());
    Ok(fresh(views::reminders::edit_page(id, &form)))
}

/// Saves the edited Reminder `id` and goes back to the Reminders tab, or answers with the form
/// showing what to fix: alone for htmx, inside its edit page otherwise.
async fn save(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Form(input): Form<ReminderForm>,
) -> Result<Response, AppError> {
    let id = ReminderId::from_client(id);
    let problems = match parse(&input) {
        Ok(reminder) => match state.reminders.edit(id, &reminder).await {
            Ok(()) => {
                tracing::info!(member = member.id.get(), "a Member edited a Reminder");
                return Ok(go_to(htmx, Tab::Reminders.href()));
            }
            Err(err) => reminder_problems(err)?,
        },
        Err(problems) => {
            // A Reminder that is gone is reported as gone, not as a form to fix.
            state.reminders.to_edit(id).await?;
            problems
        }
    };
    let form = views::reminders::form(
        FormFor::Edit(id),
        input.values(),
        &problems,
        state.reminders.now(),
    );
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| views::reminders::edit_page(id, form),
    ))
}

/// Deletes the Reminder `id` and goes back to the Reminders tab.
async fn delete(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    state.reminders.delete(ReminderId::from_client(id)).await?;
    tracing::info!(member = member.id.get(), "a Member deleted a Reminder");
    Ok(go_to(htmx, Tab::Reminders.href()))
}

/// What to tell the Member when the service refuses a Reminder: a problem with the form they can
/// fix, or an error to answer with.
fn reminder_problems(err: ReminderError) -> Result<ReminderProblems, AppError> {
    match err {
        ReminderError::NotInFuture => Ok(ReminderProblems {
            due_at: Some(NOT_IN_FUTURE.to_owned()),
            ..ReminderProblems::default()
        }),
        err => Err(err.into()),
    }
}

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
    use axum::body::Body;
    use axum::http::header::LOCATION;
    use axum::http::{Method, Response, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::reminders::TEXT_MAX;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    /// The test app's clock starts at 01:00 on 1 January 2026 in Prague.
    const TOMORROW_MORNING: &str = "2026-01-02T09:00";

    fn fields(text: &str, due_at: &str) -> Vec<(&'static str, String)> {
        vec![("text", text.to_owned()), ("due_at", due_at.to_owned())]
    }

    /// Posts the Reminder form with `text` and `due_at`, as htmx does if `htmx`.
    async fn post_reminder(
        app: &TestApp,
        cookie: &str,
        text: &str,
        due_at: &str,
        htmx: bool,
    ) -> Response<Body> {
        post_form(
            app,
            "/reminders",
            cookie,
            &fields(text, due_at),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// Posts the Reminder form, expecting it refused, and answers with what came back.
    async fn refused(app: &TestApp, cookie: &str, text: &str, due_at: &str, htmx: bool) -> String {
        let response = post_reminder(app, cookie, text, due_at, htmx).await;
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
            post_reminder(&app, "", "Bottle the stout", TOMORROW_MORNING, false).await,
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

        let response =
            post_reminder(&app, &ondra, "  Bottle the stout ", TOMORROW_MORNING, false).await;

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

        let response =
            post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;

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
            post_reminder(&app, &cookie, text, due_at, true).await;
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
            post_reminder(&app, &cookie, text, due_at, true).await;
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

        let html = refused(&app, &cookie, " \n ", TOMORROW_MORNING, true).await;

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

        let html = refused(
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
        let html = refused(&app, &cookie, "Too late", "2026-01-01T00:59", true).await;

        assert!(html.contains("Pick a time that hasn't passed yet."));
        assert!(html.contains(r#"aria-describedby="reminder-due-problem""#));
        assert!(tab(&app, &cookie).await.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn refuses_what_isnt_a_date_and_time(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = refused(&app, &cookie, "Bottle the stout", "", true).await;

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

    /// Posts the edit form of the Reminder `id` with `text` and `due_at`, as htmx does if
    /// `htmx`.
    async fn post_edit(
        app: &TestApp,
        cookie: &str,
        id: i64,
        text: &str,
        due_at: &str,
        htmx: bool,
    ) -> Response<Body> {
        post_form(
            app,
            &format!("/reminders/{id}"),
            cookie,
            &fields(text, due_at),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
    }

    async fn edit_page(app: &TestApp, cookie: &str, id: i64) -> String {
        let response = app.get_with(&format!("/reminders/{id}/edit"), cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    #[sqlx::test]
    async fn signed_out_editing_and_deleting_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/reminders/1/edit").await,
            post_edit(&app, "", 1, "Bottle the stout", TOMORROW_MORNING, false).await,
            app.post_with("/reminders/1/delete", "").await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn a_pending_reminder_leads_to_its_form_filled_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let ondra = app.sign_in_new_member("Ondra").await;
        post_reminder(&app, &ondra, "Bottle the stout", TOMORROW_MORNING, true).await;

        assert!(
            tab(&app, &ondra)
                .await
                .contains(r#"href="/reminders/1/edit""#)
        );
        let html = edit_page(&app, &ondra, 1).await;
        assert!(html.contains("<h1>Edit Reminder</h1>"));
        assert!(html.contains(r#"action="/reminders/1""#));
        assert!(html.contains(r#"value="Bottle the stout""#));
        assert!(html.contains(r#"value="2026-01-02T09:00""#));
        assert!(html.contains(r#"action="/reminders/1/delete""#));
    }

    #[sqlx::test]
    async fn a_member_edits_another_members_reminder_and_pending_reorders(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        post_reminder(&app, &josef, "Order hops", TOMORROW_MORNING, true).await;
        post_reminder(&app, &josef, "Bottle the stout", "2026-01-03T18:00", true).await;

        let response = post_edit(
            &app,
            &ondra,
            1,
            " Order more hops ",
            "2026-01-05T08:30",
            false,
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/reminders");
        let html = tab(&app, &josef).await;
        let at = |text: &str| html.find(&format!(">{text}</span>")).unwrap();
        assert!(at("Bottle the stout") < at("Order more hops"), "{html}");
        // Its creator stays who they were.
        assert!(html.contains(
            r#"<time datetime="2026-01-05T08:30">Monday 5 January 2026, 08:30</time> · Josef"#
        ));
    }

    #[sqlx::test]
    async fn htmx_is_told_to_load_the_tab_once_saved(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;

        let response = post_edit(&app, &cookie, 1, "Bottle it", TOMORROW_MORNING, true).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/reminders");
    }

    #[sqlx::test]
    async fn an_edit_to_a_blank_text_or_a_past_time_is_refused_and_changes_nothing(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;

        // The clock stands at 01:00 in Prague.
        for (text, due_at, problem) in [
            (" ", TOMORROW_MORNING, "Say what to remember."),
            (
                "Too late",
                "2026-01-01T00:59",
                "Pick a time that hasn't passed yet.",
            ),
        ] {
            let response = post_edit(&app, &cookie, 1, text, due_at, true).await;
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let html = body_text(response).await;
            assert!(html.starts_with("<form"), "a fragment: {html}");
            assert!(html.contains(problem), "{html}");
            assert!(html.contains("Save Reminder"));
        }
        let html = tab(&app, &cookie).await;
        assert!(html.contains(">Bottle the stout</span>"));
        assert!(html.contains(r#"<time datetime="2026-01-02T09:00">"#));
    }

    #[sqlx::test]
    async fn a_refused_plain_edit_gets_its_edit_page_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;

        let response = post_edit(&app, &cookie, 1, "", TOMORROW_MORNING, false).await;

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.contains("<h1>Edit Reminder</h1>"), "{html}");
        assert!(html.contains("Say what to remember."));
        assert!(html.contains(r#"action="/reminders/1/delete""#));
    }

    #[sqlx::test]
    async fn a_member_deletes_a_reminder_and_it_leaves_pending(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        post_reminder(&app, &josef, "Bottle the stout", TOMORROW_MORNING, true).await;

        let response = app.post_with("/reminders/1/delete", &ondra).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/reminders");
        assert!(tab(&app, &josef).await.contains("Nothing pending."));
    }

    #[sqlx::test]
    async fn htmx_is_told_to_load_the_tab_once_deleted(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;

        let response = post_form(
            &app,
            "/reminders/1/delete",
            &cookie,
            &[],
            true,
            PUBLIC_ORIGIN,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/reminders");
    }

    #[sqlx::test]
    async fn a_reminder_that_doesnt_exist_is_not_found(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;
        app.post_with("/reminders/1/delete", &cookie).await;

        for response in [
            app.get_with("/reminders/1/edit", &cookie).await,
            app.get_with("/reminders/999/edit", &cookie).await,
            post_edit(&app, &cookie, 1, "Bottle it", TOMORROW_MORNING, true).await,
            // Gone is reported before what's wrong with the form.
            post_edit(&app, &cookie, 1, "", "", true).await,
            app.post_with("/reminders/1/delete", &cookie).await,
        ] {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(
                body_text(response)
                    .await
                    .contains("can't be changed any more")
            );
        }
    }

    #[sqlx::test]
    async fn editing_and_deleting_need_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        post_reminder(&app, &cookie, "Bottle the stout", TOMORROW_MORNING, true).await;

        for (uri, fields) in [
            ("/reminders/1", fields("Bottle it", TOMORROW_MORNING)),
            ("/reminders/1/delete", Vec::new()),
        ] {
            let response = post_form(&app, uri, &cookie, &fields, false, "https://evil.test").await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        assert!(
            tab(&app, &cookie)
                .await
                .contains(">Bottle the stout</span>")
        );
    }
}
