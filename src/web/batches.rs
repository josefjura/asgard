//! The Batches tab, the Crew's brewery log: adding a Batch, reading the log a page at a time,
//! each Batch's own page, and editing or deleting any Batch.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::htmx::{Htmx, fragment_or_page, go_to};
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::batches::{
    Batch, BatchError, BatchName, BatchNumber, NAME_MAX, NOTES_MAX, NameError, NewBatch, Notes,
    NotesError,
};
use crate::views;
use crate::views::batches::{BatchProblems, BatchValues, Purpose, paths};
use crate::views::nav::Tab;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Batches.href(), get(show).post(add))
        .route("/batches/{number}", get(batch).post(save))
        .route("/batches/{number}/edit", get(edit))
        .route("/batches/{number}/delete", post(delete))
}

/// Which page of the log to show. Missing means the first.
#[derive(Deserialize)]
struct PageQuery {
    page: Option<usize>,
}

/// The Batches tab: the add form, empty, and one page of the log. A page past the last shows
/// the last.
async fn show(
    State(state): State<AppState>,
    Member(_): Member,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, AppError> {
    let form = views::batches::form(
        Purpose::Add,
        BatchValues::default(),
        &BatchProblems::default(),
    );
    Ok(fresh(page(&state, &form, query.page.unwrap_or(1)).await?))
}

#[derive(Deserialize)]
struct BatchForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    notes: String,
}

impl BatchForm {
    fn values(&self) -> BatchValues<'_> {
        BatchValues {
            name: &self.name,
            notes: &self.notes,
        }
    }
}

/// Adds the Batch, with the next Batch number, and goes back to the tab, where it is at the top
/// of the log. A refused form shows what to fix: alone for htmx, inside the tab otherwise.
async fn add(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<BatchForm>,
) -> Result<Response, AppError> {
    let batch = match parse(&input) {
        Ok(batch) => batch,
        Err(problems) => {
            let form = views::batches::form(Purpose::Add, input.values(), &problems);
            let body = if htmx {
                form
            } else {
                page(&state, &form, 1).await?
            };
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response());
        }
    };
    let number = state.batches.add(member.id, &batch).await?;
    tracing::info!(
        member = member.id.get(),
        batch = number.get(),
        "a Member added a Batch"
    );
    Ok(go_to(htmx, Tab::Batches.href()))
}

/// The Batch `number`'s own page.
async fn batch(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => fresh(views::batches::batch_page(&batch)).into_response(),
        Err(gone) => gone,
    })
}

/// The Batch `number` in its edit form, for any Member to edit or delete.
async fn edit(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => {
            let form = views::batches::edit_form(&batch);
            fresh(views::batches::edit_page(number, &form)).into_response()
        }
        Err(gone) => gone,
    })
}

/// Saves the edited Batch `number` and goes to its page; its number, and who added it when, stay
/// as they were. A refused form shows what to fix: alone for htmx, inside its edit page
/// otherwise.
async fn save(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
    Form(input): Form<BatchForm>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    let batch = match parse(&input) {
        Ok(batch) => batch,
        Err(problems) => {
            // A Batch that is gone is reported as gone, not as a form to fix.
            if matches!(state.batches.get(number).await, Err(BatchError::NotFound)) {
                return Ok(gone(htmx, number));
            }
            let form = views::batches::form(Purpose::Edit(number), input.values(), &problems);
            return Ok(fragment_or_page(
                htmx,
                StatusCode::UNPROCESSABLE_ENTITY,
                form,
                |form| views::batches::edit_page(number, form),
            ));
        }
    };
    match state.batches.edit(number, member.id, &batch).await {
        Err(BatchError::NotFound) => return Ok(gone(htmx, number)),
        done => done?,
    }
    tracing::info!(
        member = member.id.get(),
        batch = number.get(),
        "a Member edited a Batch"
    );
    Ok(go_to(htmx, &paths::batch(number)))
}

/// Deletes the Batch `number` for good and goes back to the tab.
async fn delete(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    match state.batches.delete(number).await {
        Err(BatchError::NotFound) => return Ok(gone(htmx, number)),
        done => done?,
    }
    tracing::info!(
        member = member.id.get(),
        batch = number.get(),
        "a Member deleted a Batch"
    );
    Ok(go_to(htmx, Tab::Batches.href()))
}

/// The Batch `number`, or the answer saying it's gone.
async fn found(
    state: &AppState,
    htmx: bool,
    number: BatchNumber,
) -> Result<Result<Batch, Response>, AppError> {
    match state.batches.get(number).await {
        Ok(batch) => Ok(Ok(batch)),
        Err(BatchError::NotFound) => Ok(Err(gone(htmx, number))),
        Err(err) => Err(err.into()),
    }
}

/// What answers a request about the Batch `number` once someone has deleted it: the page saying
/// so. htmx can't swap in a whole page, so it is told to load the Batch's page, which says so.
fn gone(htmx: bool, number: BatchNumber) -> Response {
    if htmx {
        go_to(true, &paths::batch(number))
    } else {
        (
            StatusCode::NOT_FOUND,
            fresh(views::batches::gone_page(number)),
        )
            .into_response()
    }
}

/// The Batch as typed, or what is wrong with each field.
fn parse(input: &BatchForm) -> Result<NewBatch, BatchProblems> {
    match (BatchName::parse(&input.name), Notes::parse(&input.notes)) {
        (Ok(name), Ok(notes)) => Ok(NewBatch { name, notes }),
        (name, notes) => Err(BatchProblems {
            name: name.err().map(name_message),
            notes: notes.err().map(notes_message),
        }),
    }
}

fn name_message(err: NameError) -> String {
    match err {
        NameError::Blank => "Give the Batch a name.".to_owned(),
        NameError::TooLong => format!("Keep the name to {NAME_MAX} characters or fewer."),
    }
}

fn notes_message(err: NotesError) -> String {
    match err {
        NotesError::TooLong => format!("Keep the Notes to {NOTES_MAX} characters or fewer."),
    }
}

/// The Batches tab around `form`, with the `page_number`th page of the log.
async fn page(state: &AppState, form: &Markup, page_number: usize) -> Result<Markup, AppError> {
    let log = state.batches.page(page_number).await?;
    Ok(views::batches::page(form, &log))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::{COOKIE, LOCATION};
    use axum::http::{Method, Request, Response, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::batches::{BatchName, NAME_MAX, NOTES_MAX, NewBatch, Notes, PAGE_SIZE};
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    fn fields<'a>(pairs: &[(&'a str, &str)]) -> Vec<(&'a str, String)> {
        pairs
            .iter()
            .map(|&(name, value)| (name, value.to_owned()))
            .collect()
    }

    /// Posts the add form with `pairs`, as htmx does if `htmx`.
    async fn post_batch(
        app: &TestApp,
        cookie: &str,
        pairs: &[(&str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        post_form(app, "/batches", cookie, &fields(pairs), htmx, PUBLIC_ORIGIN).await
    }

    /// Adds a Batch called `name` with htmx, expecting it added.
    async fn add(app: &TestApp, cookie: &str, name: &str, notes: &str) {
        let response = post_batch(app, cookie, &[("name", name), ("notes", notes)], true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");
    }

    /// Posts the add form, expecting it refused, and answers with what came back.
    async fn refused(app: &TestApp, cookie: &str, pairs: &[(&str, &str)], htmx: bool) -> String {
        let response = post_batch(app, cookie, pairs, htmx).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        body
    }

    /// Posts the Batch `number`'s edit form with `pairs`, as htmx does if `htmx`.
    async fn save(
        app: &TestApp,
        cookie: &str,
        number: i64,
        pairs: &[(&str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        post_form(
            app,
            &format!("/batches/{number}"),
            cookie,
            &fields(pairs),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// Deletes the Batch `number`, as htmx does if `htmx`.
    async fn delete(app: &TestApp, cookie: &str, number: i64, htmx: bool) -> Response<Body> {
        post_form(
            app,
            &format!("/batches/{number}/delete"),
            cookie,
            &[],
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// The page at `uri`, expecting it there.
    async fn page(app: &TestApp, cookie: &str, uri: &str) -> String {
        let response = app.get_with(uri, cookie).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        body_text(response).await
    }

    /// The Batch numbers and names in the log on `html`, in order.
    fn listed(html: &str) -> Vec<(String, String)> {
        html.split(r#"<span class="batch-number">"#)
            .skip(1)
            .map(|rest| {
                let number = rest.split("</span>").next().unwrap_or_default();
                let name = rest
                    .split(r#"<span class="batch-name">"#)
                    .nth(1)
                    .and_then(|name| name.split("</span>").next())
                    .unwrap_or_default();
                (number.to_owned(), name.to_owned())
            })
            .collect()
    }

    fn htmx_get(uri: &str, cookie: &str) -> Request<Body> {
        let mut get = request(Method::GET, uri, None);
        let headers = get.headers_mut();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(HX_REQUEST, "true".parse().unwrap());
        get
    }

    #[sqlx::test]
    async fn signed_out_batches_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;

        for response in [
            app.get("/batches").await,
            app.get("/batches/1").await,
            app.get("/batches/1/edit").await,
            post_batch(&app, "", &[("name", "Stout")], false).await,
            save(&app, "", 1, &[("name", "Hacked")], false).await,
            delete(&app, "", 1, false).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
        assert_eq!(
            listed(&page(&app, &josef, "/batches").await),
            [("#1".to_owned(), "Pilsner".to_owned())]
        );
    }

    #[sqlx::test]
    async fn signed_out_htmx_is_told_to_load_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut post = request(Method::POST, "/batches", Some(PUBLIC_ORIGIN));
        post.headers_mut()
            .insert(HX_REQUEST, "true".parse().unwrap());

        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[HX_REDIRECT], "/sign-in");
    }

    #[sqlx::test]
    async fn a_new_tab_has_the_add_form_and_no_batches(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = page(&app, &cookie, "/batches").await;

        assert!(
            html.contains(r#"<form class="entry-form" method="post" action="/batches""#),
            "{html}"
        );
        assert!(html.contains("No Batches yet."));
    }

    #[sqlx::test]
    async fn a_batch_with_just_a_name_gets_the_next_number_and_is_planned(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        add(&app, &cookie, "  Pilsner ", "").await;
        let response = post_batch(&app, &cookie, &[("name", "Stout")], false).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/batches");

        let html = page(&app, &cookie, "/batches").await;
        assert_eq!(
            listed(&html),
            [
                ("#2".to_owned(), "Stout".to_owned()),
                ("#1".to_owned(), "Pilsner".to_owned())
            ]
        );
        assert_eq!(
            html.matches(r#"<span class="stage planned">Planned</span>"#)
                .count(),
            2
        );
        assert!(html.contains(r#"href="/batches/2""#));
    }

    #[sqlx::test]
    async fn a_deleted_batchs_number_is_never_given_again(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        add(&app, &cookie, "First", "").await;
        add(&app, &cookie, "Second", "").await;

        delete(&app, &cookie, 2, true).await;
        add(&app, &cookie, "Third", "").await;

        assert_eq!(
            listed(&page(&app, &cookie, "/batches").await),
            [
                ("#3".to_owned(), "Third".to_owned()),
                ("#1".to_owned(), "First".to_owned())
            ]
        );
    }

    #[sqlx::test]
    async fn the_batch_page_shows_notes_with_line_breaks_and_who_added_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        add(&app, &ondra, "Pilsner", "Saaz only.\nDry hopped on day 4.").await;

        let html = page(&app, &cookie, "/batches/1").await;

        assert!(
            html.contains("<h1 class=\"batch-title\">Pilsner</h1>"),
            "{html}"
        );
        assert!(html.contains("Batch #1"));
        assert!(html.contains("Saaz only.\nDry hopped on day 4."));
        assert!(html.contains(
            r#"Added by Ondra · <time datetime="2026-01-01T01:00">Thursday 1 January 2026, 01:00</time>"#
        ), "{html}");
        assert!(html.contains("Planned"));
        assert!(!html.contains("Edited by"));
    }

    #[sqlx::test]
    async fn a_missing_or_overlong_name_or_overlong_notes_is_refused_and_kept(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let long_name = "š".repeat(NAME_MAX + 1);
        let long_notes = "ř".repeat(NOTES_MAX + 1);

        let blank = refused(
            &app,
            &cookie,
            &[("name", " "), ("notes", "line one\nline two")],
            true,
        )
        .await;
        assert!(!blank.contains("<!DOCTYPE"), "a fragment, not the page");
        assert!(blank.starts_with("<form "), "{blank}");
        assert!(blank.contains("Give the Batch a name."), "{blank}");
        assert!(blank.contains("line one\nline two</textarea>"), "{blank}");

        let long = refused(
            &app,
            &cookie,
            &[("name", &long_name), ("notes", &long_notes)],
            false,
        )
        .await;
        assert!(long.contains("<!DOCTYPE"), "the whole tab");
        assert!(long.contains(&format!("Keep the name to {NAME_MAX} characters or fewer.")));
        assert!(long.contains(&format!(
            "Keep the Notes to {NOTES_MAX} characters or fewer."
        )));
        assert!(long.contains(&format!(r#"value="{long_name}""#)));
        assert!(long.contains(&long_notes));

        assert!(
            page(&app, &cookie, "/batches")
                .await
                .contains("No Batches yet.")
        );
    }

    #[sqlx::test]
    async fn the_log_is_newest_first_and_a_page_past_the_last_is_the_last(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let member = app.state.members.all().await.unwrap()[0].id;
        for n in 1..=PAGE_SIZE + 1 {
            let batch = NewBatch {
                name: BatchName::parse(&format!("Batch {n:02}")).unwrap(),
                notes: Notes::default(),
            };
            app.state.batches.add(member, &batch).await.unwrap();
        }

        let first = page(&app, &cookie, "/batches").await;
        assert_eq!(listed(&first).len(), PAGE_SIZE);
        assert_eq!(listed(&first)[0].0, "#26");
        assert!(
            first.contains(r#"rel="next" href="/batches?page=2""#),
            "{first}"
        );

        let last = page(&app, &cookie, "/batches?page=2").await;
        assert_eq!(listed(&last), [("#1".to_owned(), "Batch 01".to_owned())]);
        assert!(last.contains("Page 2 of 2"));

        let past = page(&app, &cookie, "/batches?page=99").await;
        assert_eq!(listed(&past), listed(&last));
    }

    #[sqlx::test]
    async fn any_member_edits_a_batch_another_added(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let simon = app.sign_in_new_member("Šimon").await;
        add(&app, &josef, "Pilsnr", "").await;
        app.clock.advance(std::time::Duration::from_hours(3));

        let form = page(&app, &simon, "/batches/1/edit").await;
        assert!(form.contains("<h1>Edit Batch #1</h1>"), "{form}");
        assert!(form.contains(r#"value="Pilsnr""#));

        let response = save(
            &app,
            &simon,
            1,
            &[
                ("name", " Pilsner "),
                ("notes", "Saaz only.\nOpen fermenter."),
            ],
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches/1");

        let html = page(&app, &josef, "/batches/1").await;
        assert!(
            html.contains("<h1 class=\"batch-title\">Pilsner</h1>"),
            "{html}"
        );
        assert!(html.contains("Saaz only.\nOpen fermenter."));
        assert!(html.contains("Added by Josef"));
        assert!(html.contains(
            r#"Edited by Šimon · <time datetime="2026-01-01T04:00">Thursday 1 January 2026, 04:00</time>"#
        ), "{html}");
    }

    #[sqlx::test]
    async fn a_plain_save_goes_to_the_batch_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Before", "").await;

        let response = save(&app, &josef, 1, &[("name", "After")], false).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/batches/1");
        assert_eq!(
            listed(&page(&app, &josef, "/batches").await),
            [("#1".to_owned(), "After".to_owned())]
        );
    }

    #[sqlx::test]
    async fn notes_can_be_cleared(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "Wrong batch's notes").await;

        save(
            &app,
            &josef,
            1,
            &[("name", "Pilsner"), ("notes", "  ")],
            true,
        )
        .await;

        let html = page(&app, &josef, "/batches/1").await;
        assert!(html.contains("No Notes yet."), "{html}");
        assert!(!html.contains("Wrong batch"));
    }

    #[sqlx::test]
    async fn a_refused_edit_keeps_what_was_typed_and_changes_nothing(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Unchanged", "").await;

        let response = save(
            &app,
            &josef,
            1,
            &[("name", ""), ("notes", "Kept\nnotes")],
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(
            html.starts_with(r#"<form class="entry-form" method="post" action="/batches/1""#),
            "{html}"
        );
        assert!(html.contains("Give the Batch a name."));
        assert!(html.contains("Kept\nnotes</textarea>"));

        let long = "š".repeat(NAME_MAX + 1);
        let response = save(&app, &josef, 1, &[("name", &long)], false).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.contains("<h1>Edit Batch #1</h1>"), "{html}");
        assert!(html.contains(&format!(r#"value="{long}""#)));

        assert_eq!(
            listed(&page(&app, &josef, "/batches").await),
            [("#1".to_owned(), "Unchanged".to_owned())]
        );
    }

    #[sqlx::test]
    async fn any_member_deletes_a_batch_and_it_is_gone_everywhere(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        add(&app, &josef, "Kept", "").await;
        add(&app, &josef, "Deleted", "").await;

        let response = delete(&app, &ondra, 2, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");

        let plain = delete(&app, &ondra, 1, false).await;
        assert_eq!(plain.status(), StatusCode::SEE_OTHER);
        assert_eq!(plain.headers()[LOCATION], "/batches");

        assert!(
            page(&app, &josef, "/batches")
                .await
                .contains("No Batches yet.")
        );
        assert_eq!(
            app.get_with("/batches/2", &josef).await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[sqlx::test]
    async fn a_batch_thats_gone_says_so_instead_of_failing(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Soon gone", "").await;
        delete(&app, &josef, 1, true).await;

        for response in [
            app.get_with("/batches/1", &josef).await,
            app.get_with("/batches/1/edit", &josef).await,
            app.get_with("/batches/99", &josef).await,
            save(&app, &josef, 1, &[("name", "New")], false).await,
            // Gone is reported as gone, not as a form to fix.
            save(&app, &josef, 1, &[("name", "")], false).await,
            delete(&app, &josef, 1, false).await,
        ] {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let html = body_text(response).await;
            assert!(html.contains("<!DOCTYPE"), "a page, not an error: {html}");
            assert!(html.contains("This Batch is gone"), "{html}");
        }

        for response in [
            save(&app, &josef, 1, &[("name", "New")], true).await,
            save(&app, &josef, 1, &[("name", "")], true).await,
            delete(&app, &josef, 1, true).await,
            app.send(htmx_get("/batches/1/edit", &josef)).await,
        ] {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[HX_REDIRECT], "/batches/1");
        }
    }

    #[sqlx::test]
    async fn posts_from_another_origin_are_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Guarded", "").await;
        let foreign = "https://evil.example";

        for uri in ["/batches", "/batches/1", "/batches/1/delete"] {
            let response = post_form(
                &app,
                uri,
                &josef,
                &fields(&[("name", "Hacked")]),
                true,
                foreign,
            )
            .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        assert_eq!(
            listed(&page(&app, &josef, "/batches").await),
            [("#1".to_owned(), "Guarded".to_owned())]
        );
    }
}
