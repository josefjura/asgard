//! The Batches tab, the Crew's brewery log: adding a Batch, reading the log a page at a time,
//! each Batch's own page, recording its Brew day and Bottling, and editing or deleting any Batch.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use jiff::civil::Date;
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::htmx::{Htmx, fragment_or_page, go_to};
use super::pages::fresh;
use super::{AppError, AppState};

use crate::domain::batches::{
    Batch, BatchError, BatchName, BatchNumber, Bottling, BrewDay, FG_MAX, FG_MIN, Gravity,
    GravityError, NAME_MAX, NOTES_MAX, NameError, NewBatch, Notes, NotesError, OG_MAX, OG_MIN,
    RecordError, RuleError, VOLUME_MAX, Volume, VolumeError,
};
use crate::views;
use crate::views::batches::{
    BatchProblems, BatchValues, BottlingProblems, BottlingValues, BrewDayProblems, BrewDayValues,
    Purpose, paths,
};
use crate::views::nav::Tab;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Batches.href(), get(show).post(add))
        .route("/batches/{number}", get(batch).post(save))
        .route("/batches/{number}/edit", get(edit))
        .route("/batches/{number}/delete", post(delete))
        .route(
            "/batches/{number}/brew-day",
            get(brew_day_form).post(record_brew_day),
        )
        .route(
            "/batches/{number}/bottling",
            get(bottling_form).post(record_bottling),
        )
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

/// The Batch `number`'s Brew day form, filled in with what it has and today as the brew date
/// until it has one: in place of its Brew day section for htmx, on a page of its own otherwise.
async fn brew_day_form(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => {
            let values = BrewDayValues::of(&batch, state.batches.today());
            let form = views::batches::brew_day_form(number, &values, &BrewDayProblems::default());
            numbers_form(htmx, StatusCode::OK, number, form)
        }
        Err(gone) => gone,
    })
}

/// The Batch `number`'s Bottling form, filled in with what it has and today as the bottling
/// date until it has one: in place of its Bottling section for htmx, on a page of its own
/// otherwise.
async fn bottling_form(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => {
            let values = BottlingValues::of(&batch, state.batches.today());
            let form = views::batches::bottling_form(number, &values, &BottlingProblems::default());
            numbers_form(htmx, StatusCode::OK, number, form)
        }
        Err(gone) => gone,
    })
}

#[derive(Deserialize)]
struct BrewDayForm {
    #[serde(default)]
    brew_date: String,
    #[serde(default)]
    og: String,
    #[serde(default)]
    volume: String,
}

impl BrewDayForm {
    fn values(self) -> BrewDayValues {
        BrewDayValues {
            brew_date: self.brew_date,
            og: self.og,
            volume: self.volume,
        }
    }
}

/// Records the Batch `number`'s Brew day, and goes to its page, whose Stage and numbers follow.
/// An empty field clears its value. A refused form shows what to fix: in place for htmx, on a
/// page of its own otherwise.
async fn record_brew_day(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
    Form(input): Form<BrewDayForm>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    let problems = match parse_brew_day(&input) {
        Ok(brew_day) => match state
            .batches
            .record_brew_day(number, member.id, &brew_day)
            .await
        {
            Ok(()) => {
                tracing::info!(
                    member = member.id.get(),
                    batch = number.get(),
                    "a Member recorded a Brew day"
                );
                return Ok(go_to(htmx, &paths::batch(number)));
            }
            Err(RecordError::NotFound) => return Ok(gone(htmx, number)),
            Err(RecordError::Rule(rule)) => brew_day_rule(rule),
            Err(RecordError::Database(err)) => return Err(err.into()),
        },
        Err(problems) => {
            // A Batch that is gone is reported as gone, not as a form to fix.
            if matches!(state.batches.get(number).await, Err(BatchError::NotFound)) {
                return Ok(gone(htmx, number));
            }
            problems
        }
    };
    let form = views::batches::brew_day_form(number, &input.values(), &problems);
    Ok(numbers_form(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        number,
        form,
    ))
}

#[derive(Deserialize)]
struct BottlingForm {
    #[serde(default)]
    bottling_date: String,
    #[serde(default)]
    fg: String,
}

impl BottlingForm {
    fn values(self) -> BottlingValues {
        BottlingValues {
            bottling_date: self.bottling_date,
            fg: self.fg,
        }
    }
}

/// Records the Batch `number`'s Bottling, and goes to its page, whose Stage and numbers follow.
/// An empty field clears its value. A refused form shows what to fix: in place for htmx, on a
/// page of its own otherwise.
async fn record_bottling(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
    Form(input): Form<BottlingForm>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    let problems = match parse_bottling(&input) {
        Ok(bottling) => match state
            .batches
            .record_bottling(number, member.id, &bottling)
            .await
        {
            Ok(()) => {
                tracing::info!(
                    member = member.id.get(),
                    batch = number.get(),
                    "a Member recorded a Bottling"
                );
                return Ok(go_to(htmx, &paths::batch(number)));
            }
            Err(RecordError::NotFound) => return Ok(gone(htmx, number)),
            Err(RecordError::Rule(rule)) => bottling_rule(rule),
            Err(RecordError::Database(err)) => return Err(err.into()),
        },
        Err(problems) => {
            // A Batch that is gone is reported as gone, not as a form to fix.
            if matches!(state.batches.get(number).await, Err(BatchError::NotFound)) {
                return Ok(gone(htmx, number));
            }
            problems
        }
    };
    let form = views::batches::bottling_form(number, &input.values(), &problems);
    Ok(numbers_form(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        number,
        form,
    ))
}

/// A Brew day or Bottling `form` of the Batch `number` with `status`: alone for htmx to swap in
/// place of its section, or on a page of its own otherwise.
fn numbers_form(htmx: bool, status: StatusCode, number: BatchNumber, form: Markup) -> Response {
    fragment_or_page(htmx, status, form, |form| {
        views::batches::numbers_page(number, form)
    })
}

/// The Brew day as typed, or what is wrong with each field. An empty field is a value cleared.
fn parse_brew_day(input: &BrewDayForm) -> Result<BrewDay, BrewDayProblems> {
    let date = optional(&input.brew_date, str::parse::<Date>);
    let og = optional(&input.og, Gravity::og);
    let volume = optional(&input.volume, Volume::parse);
    match (date, og, volume) {
        (Ok(date), Ok(og), Ok(volume)) => Ok(BrewDay { date, og, volume }),
        (date, og, volume) => Err(BrewDayProblems {
            brew_date: date
                .err()
                .map(|_| "Enter the brew date, or leave it empty.".to_owned()),
            og: og.err().map(og_message),
            volume: volume.err().map(volume_message),
        }),
    }
}

/// The Bottling as typed, or what is wrong with each field. An empty field is a value cleared.
fn parse_bottling(input: &BottlingForm) -> Result<Bottling, BottlingProblems> {
    let date = optional(&input.bottling_date, str::parse::<Date>);
    let fg = optional(&input.fg, Gravity::fg);
    match (date, fg) {
        (Ok(date), Ok(fg)) => Ok(Bottling { date, fg }),
        (date, fg) => Err(BottlingProblems {
            bottling_date: date
                .err()
                .map(|_| "Enter the bottling date, or leave it empty.".to_owned()),
            fg: fg.err().map(fg_message),
        }),
    }
}

/// `input` parsed by `parse`, or None if it's blank.
fn optional<T, E>(input: &str, parse: impl FnOnce(&str) -> Result<T, E>) -> Result<Option<T>, E> {
    let input = input.trim();
    if input.is_empty() {
        Ok(None)
    } else {
        parse(input).map(Some)
    }
}

/// A gravity in thousandths as a hydrometer reads it: `1.052`.
fn gravity(thousandths: i64) -> String {
    format!("{}.{:03}", thousandths / 1000, thousandths % 1000)
}

fn og_message(_: GravityError) -> String {
    format!(
        "Enter an OG from {} to {}, like 1.052.",
        gravity(OG_MIN),
        gravity(OG_MAX)
    )
}

fn fg_message(_: GravityError) -> String {
    format!(
        "Enter an FG from {} to {}, like 1.010.",
        gravity(FG_MIN),
        gravity(FG_MAX)
    )
}

fn volume_message(err: VolumeError) -> String {
    match err {
        VolumeError::Malformed => "Enter the Volume in litres, like 23.5.".to_owned(),
        VolumeError::NotPositive => "The Volume has to be more than 0 L.".to_owned(),
        VolumeError::TooBig => format!("Keep the Volume to {} L or less.", VOLUME_MAX / 10),
    }
}

/// What the Brew day form says beside the field that breaks `rule` against the Bottling.
fn brew_day_rule(rule: RuleError) -> BrewDayProblems {
    match rule {
        RuleError::FgNotBelowOg => BrewDayProblems {
            og: Some("The OG has to be above the FG recorded at Bottling.".to_owned()),
            ..BrewDayProblems::default()
        },
        RuleError::BottlingBeforeBrewing => BrewDayProblems {
            brew_date: Some("The brew date can't be after the bottling date.".to_owned()),
            ..BrewDayProblems::default()
        },
        RuleError::BottlingWithoutBrewing => BrewDayProblems {
            brew_date: Some(
                "Clear the bottling date first: a Batch can't be bottled without a brew date."
                    .to_owned(),
            ),
            ..BrewDayProblems::default()
        },
    }
}

/// What the Bottling form says beside the field that breaks `rule` against the Brew day.
fn bottling_rule(rule: RuleError) -> BottlingProblems {
    match rule {
        RuleError::FgNotBelowOg => BottlingProblems {
            fg: Some("The FG has to be below the OG.".to_owned()),
            ..BottlingProblems::default()
        },
        RuleError::BottlingBeforeBrewing => BottlingProblems {
            bottling_date: Some("The bottling date can't be before the brew date.".to_owned()),
            ..BottlingProblems::default()
        },
        RuleError::BottlingWithoutBrewing => BottlingProblems {
            bottling_date: Some(
                "Record the Brew day first: a Batch can't be bottled before it's brewed."
                    .to_owned(),
            ),
            ..BottlingProblems::default()
        },
    }
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

    /// Posts the Batch `number`'s form at `/batches/{number}/{form}` with `pairs`, as htmx does if
    /// `htmx`.
    async fn record(
        app: &TestApp,
        cookie: &str,
        number: i64,
        form: &str,
        pairs: &[(&str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        post_form(
            app,
            &format!("/batches/{number}/{form}"),
            cookie,
            &fields(pairs),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// Records the Batch `number`'s form with htmx, expecting it saved.
    async fn recorded(
        app: &TestApp,
        cookie: &str,
        number: i64,
        form: &str,
        pairs: &[(&str, &str)],
    ) {
        let response = record(app, cookie, number, form, pairs, true).await;
        let status = response.status();
        let redirect = response.headers().get(HX_REDIRECT).cloned();
        assert_eq!(status, StatusCode::OK, "{}", body_text(response).await);
        assert_eq!(redirect.unwrap(), format!("/batches/{number}").as_str());
    }

    /// The Stage badge on the Batch `number`'s page.
    async fn stage(app: &TestApp, cookie: &str, number: i64) -> String {
        let html = page(app, cookie, &format!("/batches/{number}")).await;
        html.split(r#"<span class="stage "#)
            .nth(1)
            .and_then(|rest| rest.split("</span>").next())
            .and_then(|badge| badge.split("\">").nth(1))
            .unwrap_or_default()
            .to_owned()
    }

    #[sqlx::test]
    async fn brew_day_and_bottling_move_a_batch_through_its_stages_and_back(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;
        assert_eq!(stage(&app, &josef, 1).await, "Planned");

        let brew_day = [
            ("brew_date", "2026-10-03"),
            ("og", "1.050"),
            ("volume", "23.5"),
        ];
        recorded(&app, &josef, 1, "brew-day", &brew_day).await;
        assert_eq!(stage(&app, &josef, 1).await, "In the fermenter");
        let log = page(&app, &josef, "/batches").await;
        assert!(
            log.contains(r#"<span class="stage brewed">In the fermenter</span>"#),
            "{log}"
        );
        assert!(log.contains(r#"Brewed <time datetime="2026-10-03">3 October 2026</time>"#));

        let bottling = [("bottling_date", "2026-10-17"), ("fg", "1.010")];
        recorded(&app, &josef, 1, "bottling", &bottling).await;
        assert_eq!(stage(&app, &josef, 1).await, "Bottled");
        assert!(
            page(&app, &josef, "/batches")
                .await
                .contains(r#" · Bottled <time datetime="2026-10-17">17 October 2026</time>"#)
        );

        // Clearing the bottling date moves it back, keeping the FG.
        recorded(
            &app,
            &josef,
            1,
            "bottling",
            &[("bottling_date", ""), ("fg", "1.010")],
        )
        .await;
        assert_eq!(stage(&app, &josef, 1).await, "In the fermenter");

        recorded(
            &app,
            &josef,
            1,
            "brew-day",
            &[("brew_date", " "), ("og", "1.050")],
        )
        .await;
        assert_eq!(stage(&app, &josef, 1).await, "Planned");
        let html = page(&app, &josef, "/batches/1").await;
        assert!(!html.contains("Volume"), "cleared: {html}");
        assert!(html.contains("5.3 %"), "OG and FG are kept: {html}");
    }

    #[sqlx::test]
    async fn the_batch_page_shows_the_numbers_with_abv_and_attenuation(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;

        let before = page(&app, &josef, "/batches/1").await;
        assert!(before.contains("Not brewed yet."), "{before}");
        assert!(before.contains("Not bottled yet."));
        assert!(before.contains("Record Brew day"));

        recorded(
            &app,
            &josef,
            1,
            "brew-day",
            &[
                ("brew_date", "2026-10-03"),
                ("og", "1,050"),
                ("volume", "23,5"),
            ],
        )
        .await;
        let brewed = page(&app, &josef, "/batches/1").await;
        assert!(
            brewed.contains(r#"<span class="measured">1.050</span>"#),
            "{brewed}"
        );
        assert!(brewed.contains("23.5 L"));
        assert!(!brewed.contains("ABV"), "not until the FG is in");

        recorded(
            &app,
            &josef,
            1,
            "bottling",
            &[("bottling_date", "2026-10-17"), ("fg", "1010")],
        )
        .await;
        let html = page(&app, &josef, "/batches/1").await;
        assert!(
            html.contains(r#"<dt>ABV</dt><dd><span class="measured">5.3 %</span></dd>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<dt>Attenuation</dt><dd><span class="measured">80.0 %</span></dd>"#)
        );
        assert!(html.contains(r#"<span class="measured">1.010</span>"#));
        assert!(
            html.contains(
                r#"Bottled on</dt><dd><time datetime="2026-10-17">17 October 2026</time>"#
            )
        );
        assert!(html.contains("Edit Bottling"));
    }

    #[sqlx::test]
    async fn both_forms_start_at_today_and_swap_in_place(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;

        let brew_day = body_text(app.send(htmx_get("/batches/1/brew-day", &josef)).await).await;
        assert!(
            brew_day.starts_with(r#"<section class="section narrow" id="brew-day""#),
            "{brew_day}"
        );
        assert!(
            brew_day.contains(r#"name="brew_date" value="2026-01-01""#),
            "{brew_day}"
        );
        assert!(brew_day.contains(r##"hx-target="#brew-day" hx-swap="outerHTML""##));

        let bottling = body_text(app.send(htmx_get("/batches/1/bottling", &josef)).await).await;
        assert!(
            bottling.starts_with(r#"<section class="section narrow" id="bottling""#),
            "{bottling}"
        );
        assert!(
            bottling.contains(r#"name="bottling_date" value="2026-01-01""#),
            "{bottling}"
        );

        let plain = page(&app, &josef, "/batches/1/brew-day").await;
        assert!(plain.contains("<!DOCTYPE"), "a page without htmx");
        assert!(plain.contains("<h1>Batch #1</h1>"));

        // A recorded date is shown, not today.
        recorded(
            &app,
            &josef,
            1,
            "brew-day",
            &[("brew_date", "2025-12-20"), ("og", "1.048")],
        )
        .await;
        let again = page(&app, &josef, "/batches/1/brew-day").await;
        assert!(again.contains(r#"value="2025-12-20""#), "{again}");
        assert!(again.contains(r#"value="1.048""#));
    }

    /// Which Batch, which form, what's typed, and what the form says.
    type Case<'a> = (i64, &'a str, &'a [(&'a str, &'a str)], &'a str);

    #[sqlx::test]
    async fn a_refused_brew_day_or_bottling_keeps_what_was_typed_and_changes_nothing(
        db: SqlitePool,
    ) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;
        add(&app, &josef, "Unbrewed", "").await;
        recorded(
            &app,
            &josef,
            1,
            "brew-day",
            &[("brew_date", "2026-10-03"), ("og", "1.050")],
        )
        .await;
        recorded(
            &app,
            &josef,
            1,
            "bottling",
            &[("bottling_date", "2026-10-17"), ("fg", "1.010")],
        )
        .await;
        let before = page(&app, &josef, "/batches/1").await;

        let cases: &[Case] = &[
            (
                1,
                "brew-day",
                &[("og", "1.250")],
                "Enter an OG from 1.000 to 1.200, like 1.052.",
            ),
            (
                1,
                "brew-day",
                &[("og", "0.999")],
                "Enter an OG from 1.000 to 1.200, like 1.052.",
            ),
            (
                1,
                "brew-day",
                &[("og", "strong")],
                "Enter an OG from 1.000 to 1.200, like 1.052.",
            ),
            (
                1,
                "brew-day",
                &[("brew_date", "2026-10-03"), ("og", "1.009")],
                "The OG has to be above the FG recorded at Bottling.",
            ),
            (
                1,
                "brew-day",
                &[("volume", "0")],
                "The Volume has to be more than 0 L.",
            ),
            (
                1,
                "brew-day",
                &[("volume", "-5")],
                "The Volume has to be more than 0 L.",
            ),
            (
                1,
                "brew-day",
                &[("volume", "lots")],
                "Enter the Volume in litres, like 23.5.",
            ),
            (
                1,
                "brew-day",
                &[("brew_date", "2026-10-18"), ("og", "1.050")],
                "The brew date can't be after the bottling date.",
            ),
            (
                1,
                "brew-day",
                &[("brew_date", ""), ("og", "1.050")],
                "Clear the bottling date first",
            ),
            (
                1,
                "brew-day",
                &[("brew_date", "3 October")],
                "Enter the brew date, or leave it empty.",
            ),
            (
                1,
                "bottling",
                &[("bottling_date", "2026-10-17"), ("fg", "1.050")],
                "The FG has to be below the OG.",
            ),
            (
                1,
                "bottling",
                &[("fg", "1.101")],
                "Enter an FG from 0.990 to 1.100, like 1.010.",
            ),
            (
                1,
                "bottling",
                &[("bottling_date", "2026-10-02")],
                "The bottling date can't be before the brew date.",
            ),
            (
                2,
                "bottling",
                &[("bottling_date", "2026-10-17")],
                "Record the Brew day first",
            ),
        ];
        for &(number, form, pairs, message) in cases {
            let response = record(&app, &josef, number, form, pairs, true).await;
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{pairs:?}"
            );
            let html = body_text(response).await;
            assert!(html.starts_with("<section "), "a fragment: {html}");
            assert!(html.contains(message), "{pairs:?}: {html}");
            for (_, typed) in pairs.iter().filter(|(_, typed)| !typed.is_empty()) {
                assert!(
                    html.contains(&format!(r#"value="{typed}""#)),
                    "{typed} kept: {html}"
                );
            }
        }

        let plain = record(&app, &josef, 1, "bottling", &[("fg", "1.200")], false).await;
        assert_eq!(plain.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(plain).await;
        assert!(
            html.contains("<h1>Batch #1</h1>"),
            "a page without htmx: {html}"
        );
        assert!(html.contains(r#"value="1.200""#));

        assert_eq!(page(&app, &josef, "/batches/1").await, before);
        assert_eq!(stage(&app, &josef, 2).await, "Planned");
    }

    #[sqlx::test]
    async fn a_plain_brew_day_post_goes_to_the_batch_page(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;

        let response = record(
            &app,
            &josef,
            1,
            "brew-day",
            &[("brew_date", "2026-10-03")],
            false,
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/batches/1");
        assert_eq!(stage(&app, &josef, 1).await, "In the fermenter");
    }

    #[sqlx::test]
    async fn brew_day_and_bottling_of_a_batch_thats_gone_say_so(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Soon gone", "").await;
        delete(&app, &josef, 1, true).await;

        for response in [
            app.get_with("/batches/1/brew-day", &josef).await,
            app.get_with("/batches/1/bottling", &josef).await,
            record(&app, &josef, 1, "brew-day", &[("og", "1.050")], false).await,
            record(&app, &josef, 1, "brew-day", &[("og", "bad")], false).await,
            record(&app, &josef, 1, "bottling", &[("fg", "1.010")], false).await,
            record(&app, &josef, 1, "bottling", &[("fg", "bad")], false).await,
        ] {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(body_text(response).await.contains("This Batch is gone"));
        }
        for response in [
            record(&app, &josef, 1, "brew-day", &[("og", "1.050")], true).await,
            record(&app, &josef, 1, "bottling", &[("fg", "bad")], true).await,
        ] {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[HX_REDIRECT], "/batches/1");
        }
    }

    #[sqlx::test]
    async fn signed_out_brew_day_and_bottling_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;

        for response in [
            app.get("/batches/1/brew-day").await,
            app.get("/batches/1/bottling").await,
            record(
                &app,
                "",
                1,
                "brew-day",
                &[("brew_date", "2026-10-03")],
                false,
            )
            .await,
            record(&app, "", 1, "bottling", &[("fg", "1.010")], false).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
        assert_eq!(stage(&app, &josef, 1).await, "Planned");
    }

    #[sqlx::test]
    async fn posts_from_another_origin_are_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Guarded", "").await;
        let foreign = "https://evil.example";

        for uri in ["/batches/1/brew-day", "/batches/1/bottling"] {
            let response = post_form(
                &app,
                uri,
                &josef,
                &fields(&[("brew_date", "2026-10-03"), ("bottling_date", "2026-10-03")]),
                true,
                foreign,
            )
            .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        assert_eq!(stage(&app, &josef, 1).await, "Planned");

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
