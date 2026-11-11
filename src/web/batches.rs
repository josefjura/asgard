//! The Batches tab, the Crew's brewery log: planning, moving and cancelling Brewing slots, adding
//! a Batch, reading the log a page at a time, each Batch's own page, recording its Brew day and
//! Bottling, attaching its Recipe, and editing or deleting any Batch.

use axum::extract::multipart::{Field, MultipartError};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
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
    GravityError, LinkError, MoveError, NAME_MAX, NOTES_MAX, NameError, NewBatch, Notes,
    NotesError, OG_MAX, OG_MIN, RECIPE_MAX_BYTES, Recipe, RecipeError, RecordError, RuleError,
    SlotError, SlotId, VOLUME_MAX, Volume, VolumeError,
};
use crate::views;
use crate::views::batches::{
    BatchProblems, BatchValues, BottlingProblems, BottlingValues, BrewDayProblems, BrewDayValues,
    Purpose, SlotChoice, paths,
};
use crate::views::nav::Tab;
use crate::views::slots::{self as slot_views, slot_day};

/// The most a request that uploads a Recipe may carry: the file's cap, plus room for the name,
/// the Notes and the multipart framing. It's only a backstop: the handlers stop reading a file
/// at the cap and say it's too big in the form, which a bare 413 can't, since htmx doesn't swap
/// one in.
const UPLOAD_LIMIT: usize = RECIPE_MAX_BYTES + 64 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            Tab::Batches.href(),
            get(show)
                .post(add)
                .layer(DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
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
        .route(
            "/batches/{number}/recipe",
            post(attach_recipe).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
        .route("/batches/{number}/recipe/delete", post(remove_recipe))
        .route("/batches/{number}/slot", post(link_batch))
        .route("/batches/{number}/slot/unlink", post(unlink_batch))
        .route("/batches/slots", post(plan_slot))
        .route("/batches/slots/{id}", get(slot_page).post(move_slot))
        .route("/batches/slots/{id}/cancel", post(cancel_slot))
        .route("/batches/slots/{id}/batch", post(link_slot))
        .route("/batches/slots/{id}/unlink", post(unlink_slot))
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
    let plan = plan_form(&state);
    Ok(fresh(
        page(&state, &plan, &add_form(), query.page.unwrap_or(1)).await?,
    ))
}

/// The empty form a Batch is added with.
fn add_form() -> Markup {
    views::batches::form(
        Purpose::Add,
        BatchValues::default(),
        &BatchProblems::default(),
    )
}

/// The empty form a Brewing slot is planned with.
fn plan_form(state: &AppState) -> Markup {
    slot_views::plan_form("", None, state.batches.today())
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

/// Adds the Batch, with the next Batch number and the Recipe uploaded with it, if any, and goes
/// back to the tab, where it is at the top of the log. A blank name takes the Recipe's. A
/// refused form shows what to fix: alone for htmx, inside the tab otherwise.
async fn add(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let Uploaded {
        name,
        notes,
        recipe: upload,
    } = read_upload(&mut multipart).await?;
    let input = BatchForm { name, notes };
    let (batch, recipe) = match parse_new(&input, upload) {
        Ok(parsed) => parsed,
        Err(problems) => {
            let form = views::batches::form(Purpose::Add, input.values(), &problems);
            let body = if htmx {
                form
            } else {
                page(&state, &plan_form(&state), &form, 1).await?
            };
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response());
        }
    };
    let number = state
        .batches
        .add(member.id, &batch, recipe.as_ref())
        .await?;
    tracing::info!(
        member = member.id.get(),
        batch = number.get(),
        recipe = recipe.is_some(),
        "a Member added a Batch"
    );
    Ok(go_to(htmx, Tab::Batches.href()))
}

/// The Batch `number`'s own page, with its Recipe read from the file as it was uploaded.
async fn batch(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => fresh(batch_page(&state, &batch, None).await?).into_response(),
        Err(gone) => gone,
    })
}

/// The Batch's own page, with its Recipe and the free slots it can be linked to, saying
/// `link_problem` beneath the slot form.
async fn batch_page(
    state: &AppState,
    batch: &Batch,
    link_problem: Option<&str>,
) -> Result<Markup, AppError> {
    let recipe = state.batches.recipe(batch.number).await?;
    let free = state.batches.free_slots().await?;
    let slots = SlotChoice {
        free: &free,
        problem: link_problem,
    };
    Ok(views::batches::batch_page(batch, recipe.as_ref(), &slots))
}

#[derive(Deserialize)]
struct LinkSlotForm {
    #[serde(default)]
    slot: String,
}

/// Links the Batch `number` to the slot picked in the form, moving it off any slot it was on,
/// and goes to its page. A refused pick says why on the Batch's page.
async fn link_batch(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
    Form(input): Form<LinkSlotForm>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    let message = match input.slot.trim().parse() {
        Ok(id) => match state.batches.link(SlotId::from_client(id), number).await {
            Ok(()) => {
                tracing::info!(
                    member = member.id.get(),
                    batch = number.get(),
                    slot = id,
                    "a Member linked a Batch to a Brewing slot"
                );
                return Ok(go_to(htmx, &paths::batch(number)));
            }
            Err(LinkError::BatchNotFound) => return Ok(gone(htmx, number)),
            // Its page shows it's brewed, with no slot to pick.
            Err(LinkError::NotPlanned) => return Ok(go_to(htmx, &paths::batch(number))),
            Err(LinkError::SlotNotFound) => {
                "That slot is gone: someone cancelled it. Pick another."
            }
            Err(LinkError::SlotDone) => "That slot's Batch is brewed already. Pick another.",
            Err(LinkError::Database(err)) => return Err(err.into()),
        },
        Err(_) => "Pick a slot to brew it on.",
    };
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            fresh(batch_page(&state, &batch, Some(message)).await?),
        )
            .into_response(),
        Err(gone) => gone,
    })
}

/// Takes the Batch `number` off its slot and goes to its page, where it waits for a slot.
async fn unlink_batch(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    match state.batches.unlink_batch(number).await {
        Ok(()) => tracing::info!(
            member = member.id.get(),
            batch = number.get(),
            "a Member took a Batch off its Brewing slot"
        ),
        Err(LinkError::BatchNotFound) => return Ok(gone(htmx, number)),
        // Its page shows it's brewed, with no slot to unlink.
        Err(LinkError::NotPlanned | LinkError::SlotNotFound | LinkError::SlotDone) => {}
        Err(LinkError::Database(err)) => return Err(err.into()),
    }
    Ok(go_to(htmx, &paths::batch(number)))
}

/// Attaches the uploaded Recipe to the Batch `number`, in place of any it had, and goes to its
/// page. A refused file shows what's wrong in the upload form: in place for htmx, on a page of
/// its own otherwise.
async fn attach_recipe(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    let problem = match read_upload(&mut multipart).await?.recipe.into_recipe() {
        Ok(None) => "Choose a BeerXML file to attach.".to_owned(),
        Ok(Some(recipe)) => {
            match state
                .batches
                .attach_recipe(number, member.id, &recipe)
                .await
            {
                Err(BatchError::NotFound) => return Ok(gone(htmx, number)),
                done => done?,
            }
            tracing::info!(
                member = member.id.get(),
                batch = number.get(),
                "a Member attached a Recipe"
            );
            return Ok(go_to(htmx, &paths::batch(number)));
        }
        Err(err) => recipe_message(err),
    };
    // A Batch that is gone is reported as gone, not as a form to fix.
    let has = match state.batches.recipe(number).await {
        Ok(recipe) => recipe.is_some(),
        Err(BatchError::NotFound) => return Ok(gone(htmx, number)),
        Err(err) => return Err(err.into()),
    };
    let form = views::batches::recipe_upload(number, has, Some(&problem));
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| views::batches::numbers_page(number, form),
    ))
}

/// Removes the Batch `number`'s Recipe for good and goes to its page.
async fn remove_recipe(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    match state.batches.remove_recipe(number, member.id).await {
        Err(BatchError::NotFound) => return Ok(gone(htmx, number)),
        done => done?,
    }
    tracing::info!(
        member = member.id.get(),
        batch = number.get(),
        "a Member removed a Recipe"
    );
    Ok(go_to(htmx, &paths::batch(number)))
}

/// A Recipe file as a multipart form brought it.
enum Upload {
    /// No file was chosen: a browser sends the field empty, without a file name.
    Missing,
    File(Vec<u8>),
    /// Over [`RECIPE_MAX_BYTES`]. Reading stopped there.
    TooBig,
}

impl Upload {
    /// The Recipe in the file, None if none was chosen, or why the file can't be one.
    fn into_recipe(self) -> Result<Option<Recipe>, RecipeError> {
        match self {
            Self::Missing => Ok(None),
            Self::TooBig => Err(RecipeError::TooBig),
            Self::File(bytes) => Recipe::parse(bytes).map(Some),
        }
    }
}

/// What a multipart form brought: its text fields, and the file in its `recipe` field.
struct Uploaded {
    name: String,
    notes: String,
    recipe: Upload,
}

/// Reads the fields of a multipart form. A Recipe file over the cap stops the reading there, so
/// the forms put the file last, after the fields they keep when it's refused.
///
/// A request over the backstop is refused as a file too big too, wherever the reading meets it.
/// The reader takes in whatever has already arrived, so a file far over the cap can be met before
/// the name and Notes are read, and they are lost; a file just over the cap never is.
///
/// # Errors
///
/// [`AppError::Upload`] if the form didn't arrive whole, other than for being too big.
async fn read_upload(multipart: &mut Multipart) -> Result<Uploaded, AppError> {
    let mut uploaded = Uploaded {
        name: String::new(),
        notes: String::new(),
        recipe: Upload::Missing,
    };
    let read = async {
        while let Some(field) = multipart.next_field().await? {
            match field.name() {
                Some("name") => uploaded.name = field.text().await?,
                Some("notes") => uploaded.notes = field.text().await?,
                Some("recipe") => {
                    uploaded.recipe = read_file(field).await?;
                    if matches!(uploaded.recipe, Upload::TooBig) {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok::<_, MultipartError>(())
    };
    match read.await {
        Ok(()) => {}
        Err(err) if err.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            uploaded.recipe = Upload::TooBig;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(uploaded)
}

/// The file in `field`, read up to [`RECIPE_MAX_BYTES`] and no further.
async fn read_file(mut field: Field<'_>) -> Result<Upload, MultipartError> {
    let chosen = field.file_name().is_some_and(|name| !name.is_empty());
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await? {
        if bytes.len() + chunk.len() > RECIPE_MAX_BYTES {
            return Ok(Upload::TooBig);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(if !chosen && bytes.is_empty() {
        Upload::Missing
    } else {
        Upload::File(bytes)
    })
}

/// The new Batch and the Recipe uploaded with it, or what is wrong with each field. A blank name
/// takes the Recipe's.
fn parse_new(
    input: &BatchForm,
    upload: Upload,
) -> Result<(NewBatch, Option<Recipe>), BatchProblems> {
    let recipe = upload.into_recipe();
    let name = match &recipe {
        Ok(Some(recipe)) if input.name.trim().is_empty() => {
            BatchName::of_recipe(recipe).ok_or(NameError::Blank)
        }
        _ => BatchName::parse(&input.name),
    };
    match (name, Notes::parse(&input.notes), recipe) {
        (Ok(name), Ok(notes), Ok(recipe)) => Ok((NewBatch { name, notes }, recipe)),
        (name, notes, recipe) => Err(BatchProblems {
            name: name.err().map(|err| match err {
                NameError::Blank => {
                    "Give the Batch a name, or attach a Recipe to take its name from.".to_owned()
                }
                NameError::TooLong => name_message(err),
            }),
            notes: notes.err().map(notes_message),
            recipe: recipe.err().map(recipe_message),
        }),
    }
}

fn recipe_message(err: RecipeError) -> String {
    match err {
        RecipeError::TooBig => format!(
            "This file is over {} MB, too big for a BeerXML recipe. Export just the one recipe and try again.",
            RECIPE_MAX_BYTES / (1024 * 1024)
        ),
        RecipeError::NotBeerXml => {
            "This isn't a BeerXML file. Export the recipe as BeerXML and try again.".to_owned()
        }
        RecipeError::NoRecipe => "This BeerXML file has no recipe in it, only ingredients or other records. Export the recipe itself and try again.".to_owned(),
    }
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

/// Which brew date the Brew day form starts at. Missing or unreadable means today.
#[derive(Deserialize)]
struct BrewDayQuery {
    date: Option<String>,
}

/// The Batch `number`'s Brew day form, filled in with what it has and, until it has a brew date,
/// the date asked for or today: in place of its Brew day section for htmx, on a page of its own
/// otherwise. A slot whose day passed asks for its date.
async fn brew_day_form(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(number): Path<i64>,
    Query(query): Query<BrewDayQuery>,
) -> Result<Response, AppError> {
    let number = BatchNumber::from_client(number);
    let start = query
        .date
        .and_then(|date| date.parse::<Date>().ok())
        .unwrap_or_else(|| state.batches.today());
    Ok(match found(&state, htmx, number).await? {
        Ok(batch) => {
            let values = BrewDayValues::of(&batch, start);
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
            recipe: None,
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

/// The Batches tab around `plan`, the form that plans a Brewing slot, and `form`, the one that
/// adds a Batch, with the upcoming slots and the `page_number`th page of the log.
async fn page(
    state: &AppState,
    plan: &Markup,
    form: &Markup,
    page_number: usize,
) -> Result<Markup, AppError> {
    let slots = state.batches.open_slots().await?;
    let log = state.batches.page(page_number).await?;
    Ok(views::batches::page(
        &slot_views::slots_section(&slots, plan),
        form,
        &log,
    ))
}

#[derive(Deserialize)]
struct SlotForm {
    #[serde(default)]
    date: String,
}

/// The date typed into a slot's form, or what to say about it.
fn parse_slot_date(input: &SlotForm) -> Result<Date, String> {
    input
        .date
        .trim()
        .parse()
        .map_err(|_| "Pick a date for the slot.".to_owned())
}

/// What a slot's form says about a date `err` refused.
fn slot_message(err: SlotError) -> Result<String, AppError> {
    match err {
        SlotError::Past => Ok("Pick today or a later date.".to_owned()),
        SlotError::Taken(date) => Ok(format!("There's already a slot on {}.", slot_day(date))),
        SlotError::Database(err) => Err(err.into()),
    }
}

/// Plans a Brewing slot on the date in the form and goes back to the tab, where it is among
/// the upcoming slots. A refused date shows what to fix: alone for htmx, inside the tab
/// otherwise.
async fn plan_slot(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<SlotForm>,
) -> Result<Response, AppError> {
    let message = match parse_slot_date(&input) {
        Ok(date) => match state.batches.plan_slot(member.id, date).await {
            Ok(id) => {
                tracing::info!(
                    member = member.id.get(),
                    slot = id.get(),
                    "a Member planned a Brewing slot"
                );
                return Ok(go_to(htmx, Tab::Batches.href()));
            }
            Err(err) => slot_message(err)?,
        },
        Err(message) => message,
    };
    let form = slot_views::plan_form(&input.date, Some(&message), state.batches.today());
    let body = if htmx {
        form
    } else {
        page(&state, &form, &add_form(), 1).await?
    };
    Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response())
}

/// The slot `id`'s page, where it is moved: its date in the form.
async fn slot_page(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let id = SlotId::from_client(id);
    let Some(slot) = state.batches.slot(id).await? else {
        return Ok(slot_gone(htmx, id));
    };
    let form = slot_views::move_form(id, &slot.date.to_string(), None, state.batches.today());
    let planned = state.batches.planned_batches().await?;
    Ok(fresh(slot_views::slot_page(&slot, &planned, None, &form)).into_response())
}

/// Moves the slot `id` to the date in the form and goes back to the tab. A refused date shows
/// what to fix: alone for htmx, inside the slot's page otherwise.
async fn move_slot(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Form(input): Form<SlotForm>,
) -> Result<Response, AppError> {
    let id = SlotId::from_client(id);
    let message = match parse_slot_date(&input) {
        Ok(date) => match state.batches.move_slot(id, date).await {
            Ok(()) => {
                tracing::info!(
                    member = member.id.get(),
                    slot = id.get(),
                    "a Member moved a Brewing slot"
                );
                return Ok(go_to(htmx, Tab::Batches.href()));
            }
            Err(MoveError::NotFound) => return Ok(slot_gone(htmx, id)),
            Err(MoveError::Date(err)) => slot_message(err)?,
        },
        Err(message) => message,
    };
    // A slot that is gone is reported as gone, not as a form to fix.
    let Some(slot) = state.batches.slot(id).await? else {
        return Ok(slot_gone(htmx, id));
    };
    let form = slot_views::move_form(id, &input.date, Some(&message), state.batches.today());
    let planned = state.batches.planned_batches().await?;
    Ok(fragment_or_page(
        htmx,
        StatusCode::UNPROCESSABLE_ENTITY,
        form,
        |form| slot_views::slot_page(&slot, &planned, None, form),
    ))
}

#[derive(Deserialize)]
struct LinkBatchForm {
    #[serde(default)]
    batch: String,
}

/// Links the Batch picked in the form to the slot `id`, moving it off any slot it was on and
/// taking off any Batch the slot had, and goes back to the tab. A refused pick says why on the
/// slot's page.
async fn link_slot(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
    Form(input): Form<LinkBatchForm>,
) -> Result<Response, AppError> {
    let id = SlotId::from_client(id);
    let message = match input.batch.trim().parse() {
        Ok(number) => match state
            .batches
            .link(id, BatchNumber::from_client(number))
            .await
        {
            Ok(()) => {
                tracing::info!(
                    member = member.id.get(),
                    batch = number,
                    slot = id.get(),
                    "a Member linked a Batch to a Brewing slot"
                );
                return Ok(go_to(htmx, Tab::Batches.href()));
            }
            Err(LinkError::SlotNotFound) => return Ok(slot_gone(htmx, id)),
            // Its Batch is brewed, so the slot is done and no longer shown.
            Err(LinkError::SlotDone) => return Ok(go_to(htmx, Tab::Batches.href())),
            Err(LinkError::BatchNotFound) => {
                "That Batch is gone: someone deleted it. Pick another."
            }
            Err(LinkError::NotPlanned) => "That Batch is brewed already. Pick a Planned Batch.",
            Err(LinkError::Database(err)) => return Err(err.into()),
        },
        Err(_) => "Pick a Batch to brew that day.",
    };
    let Some(slot) = state.batches.slot(id).await? else {
        return Ok(slot_gone(htmx, id));
    };
    let form = slot_views::move_form(id, &slot.date.to_string(), None, state.batches.today());
    let planned = state.batches.planned_batches().await?;
    Ok((
        StatusCode::UNPROCESSABLE_ENTITY,
        fresh(slot_views::slot_page(&slot, &planned, Some(message), &form)),
    )
        .into_response())
}

/// Takes the Batch off the slot `id`, which then has nothing chosen, and goes back to the tab.
async fn unlink_slot(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let id = SlotId::from_client(id);
    match state.batches.unlink_slot(id).await {
        Ok(()) => tracing::info!(
            member = member.id.get(),
            slot = id.get(),
            "a Member took the Batch off a Brewing slot"
        ),
        Err(LinkError::SlotNotFound) => return Ok(slot_gone(htmx, id)),
        // Its Batch is brewed, so the slot is done and no longer shown.
        Err(LinkError::NotPlanned | LinkError::BatchNotFound | LinkError::SlotDone) => {}
        Err(LinkError::Database(err)) => return Err(err.into()),
    }
    Ok(go_to(htmx, Tab::Batches.href()))
}

/// Cancels the slot `id` for good and goes back to the tab. A slot already gone stays gone.
async fn cancel_slot(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let id = SlotId::from_client(id);
    state.batches.cancel_slot(id).await?;
    tracing::info!(
        member = member.id.get(),
        slot = id.get(),
        "a Member cancelled a Brewing slot"
    );
    Ok(go_to(htmx, Tab::Batches.href()))
}

/// What answers a request about the slot `id` once someone has cancelled it: the page saying
/// so. htmx can't swap in a whole page, so it is told to load the slot's page, which says so.
fn slot_gone(htmx: bool, id: SlotId) -> Response {
    if htmx {
        go_to(true, &slot_views::paths::slot(id))
    } else {
        (StatusCode::NOT_FOUND, fresh(slot_views::gone_page())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::{CONTENT_TYPE, COOKIE, LOCATION};
    use axum::http::{Method, Request, Response, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::batches::recipe_fixtures::{
        HOPS_ONLY, LATIN1, SPARSE, TWO_RECIPES, UNETICE,
    };
    use crate::domain::batches::{
        BatchName, NAME_MAX, NOTES_MAX, NewBatch, Notes, PAGE_SIZE, RECIPE_MAX_BYTES,
    };
    use crate::domain::events::Event;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    fn fields<'a>(pairs: &[(&'a str, &str)]) -> Vec<(&'a str, String)> {
        pairs
            .iter()
            .map(|&(name, value)| (name, value.to_owned()))
            .collect()
    }

    const BOUNDARY: &str = "asgard-test-boundary";

    /// Posts `pairs` and the Recipe `file`, as a name and its bytes, as a multipart form to
    /// `uri` from `origin`, as htmx does if `htmx`. Without a file, the Recipe field goes empty,
    /// as a browser sends it when no file is chosen.
    async fn post_multipart(
        app: &TestApp,
        uri: &str,
        cookie: &str,
        pairs: &[(&str, &str)],
        file: Option<(&str, &[u8])>,
        htmx: bool,
        origin: &str,
    ) -> Response<Body> {
        let mut body = Vec::new();
        for (name, value) in pairs {
            body.extend_from_slice(
                format!(
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        }
        let (file_name, bytes) = file.unwrap_or(("", b""));
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"recipe\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        let mut post = request(Method::POST, uri, Some(origin));
        let headers = post.headers_mut();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}")
                .parse()
                .unwrap(),
        );
        if htmx {
            headers.insert(HX_REQUEST, "true".parse().unwrap());
        }
        // In frames of 16 KiB, as it comes over the network, not all at once.
        let frames: Vec<Result<Vec<u8>, std::convert::Infallible>> = body
            .chunks(16 * 1024)
            .map(|frame| Ok(frame.to_vec()))
            .collect();
        *post.body_mut() = Body::from_stream(futures_util::stream::iter(frames));
        app.send(post).await
    }

    /// Posts the add form with `pairs`, as htmx does if `htmx`.
    async fn post_batch(
        app: &TestApp,
        cookie: &str,
        pairs: &[(&str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        post_multipart(app, "/batches", cookie, pairs, None, htmx, PUBLIC_ORIGIN).await
    }

    /// Posts the add form with `pairs` and the Recipe `bytes`, as htmx does.
    async fn post_batch_with_recipe(
        app: &TestApp,
        cookie: &str,
        pairs: &[(&str, &str)],
        bytes: &[u8],
    ) -> Response<Body> {
        post_multipart(
            app,
            "/batches",
            cookie,
            pairs,
            Some(("recipe.xml", bytes)),
            true,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// Uploads `bytes` as the Batch `number`'s Recipe, as htmx does if `htmx`.
    async fn upload(
        app: &TestApp,
        cookie: &str,
        number: i64,
        bytes: &[u8],
        htmx: bool,
    ) -> Response<Body> {
        post_multipart(
            app,
            &format!("/batches/{number}/recipe"),
            cookie,
            &[],
            Some(("recipe.xml", bytes)),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
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
        assert!(
            blank.contains("Give the Batch a name, or attach a Recipe to take its name from."),
            "{blank}"
        );
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
            app.state.batches.add(member, &batch, None).await.unwrap();
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

    /// Expects `response` to be the add form refused, keeping the name and Notes typed, and
    /// saying `message` about the file.
    async fn refused_recipe(response: Response<Body>, message: &str) {
        let status = response.status();
        let html = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
        assert!(html.starts_with("<form "), "a fragment: {html}");
        assert!(html.contains(message), "{message}: {html}");
        assert!(html.contains(r#"value="Kept name""#), "{html}");
        assert!(html.contains("Kept\nnotes</textarea>"), "{html}");
    }

    const NOT_BEERXML: &str = "This isn't a BeerXML file.";
    const NO_RECIPE: &str = "This BeerXML file has no recipe in it";
    const TOO_BIG: &str = "This file is over 1 MB, too big for a BeerXML recipe.";

    #[sqlx::test]
    async fn a_batch_added_with_a_recipe_and_no_name_takes_the_recipes(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;

        let response = post_batch_with_recipe(&app, &josef, &[("name", " ")], UNETICE).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");
        let named = post_batch_with_recipe(&app, &josef, &[("name", "Mine")], SPARSE).await;
        assert_eq!(named.status(), StatusCode::OK);

        assert_eq!(
            listed(&page(&app, &josef, "/batches").await),
            [
                ("#2".to_owned(), "Mine".to_owned()),
                ("#1".to_owned(), "Únětický Vzor 10.7° (G70 30L)".to_owned())
            ]
        );
    }

    #[sqlx::test]
    async fn a_file_of_several_recipes_uses_the_first(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;

        post_batch_with_recipe(&app, &josef, &[], TWO_RECIPES).await;

        let html = page(&app, &josef, "/batches/1").await;
        assert!(
            html.contains(r#"<h1 class="batch-title">First Pale Ale</h1>"#),
            "{html}"
        );
        assert!(html.contains(r#"<p class="recipe-name">First Pale Ale</p>"#));
        assert!(!html.contains("Second Stout"));
    }

    #[sqlx::test]
    async fn the_batch_page_shows_the_recipe_viewer(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        post_batch_with_recipe(&app, &josef, &[("name", "Vzor")], UNETICE).await;

        let html = page(&app, &josef, "/batches/1").await;

        for shown in [
            r#"<p class="recipe-name">Únětický Vzor 10.7° (G70 30L)</p>"#,
            "Czech Premium Pale Lager · Pale Czech Lager · BJCP 2015",
            "<dt>Batch size</dt><dd>30 L</dd>",
            "<dt>Boil time</dt><dd>90 min</dd>",
            "<h3>Style ranges</h3>",
            "<dt>OG</dt><dd>1.044–1.060</dd>",
            r#"<th scope="row">Pilsner (Weyermann)</th><td>5.4 kg</td><td>95.6 %</td>"#,
            r#"<th scope="row">Carapils (Weyermann)</th><td>250 g</td><td>4.4 %</td>"#,
            r#"<th scope="row">Sladek <span class="recipe-aside">Pellet</span></th><td>30 g</td><td>6.5 %</td><td>Boil</td><td>60 min</td>"#,
            r#"NovaLager <span class="recipe-aside">Lallemand · LalBrew NovaLager · Dry</span>"#,
            r#"<th scope="row">Nižší cukrotvorná (Maltose) <span class="recipe-aside">Temperature</span></th><td>63 °C</td><td>40 min</td>"#,
            "Replace Recipe",
            "Remove Recipe",
        ] {
            assert!(html.contains(shown), "{shown}: {html}");
        }
    }

    #[sqlx::test]
    async fn targets_sit_beside_the_measured_og_fg_and_abv(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        post_batch_with_recipe(&app, &josef, &[], UNETICE).await;

        let planned = page(&app, &josef, "/batches/1").await;
        assert!(
            planned.contains(r#"<dt>OG</dt><dd><span class="measured missing">Not measured</span> <span class="target">target 1.043</span></dd>"#),
            "{planned}"
        );

        recorded(
            &app,
            &josef,
            1,
            "brew-day",
            &[("brew_date", "2026-10-03"), ("og", "1.045")],
        )
        .await;
        recorded(
            &app,
            &josef,
            1,
            "bottling",
            &[("bottling_date", "2026-10-17"), ("fg", "1.011")],
        )
        .await;
        let html = page(&app, &josef, "/batches/1").await;
        assert!(
            html.contains(r#"<dt>OG</dt><dd><span class="measured">1.045</span> <span class="target">target 1.043</span></dd>"#),
            "{html}"
        );
        assert!(html.contains(
            r#"<dt>FG</dt><dd><span class="measured">1.011</span> <span class="target">target 1.010</span></dd>"#
        ));
        // The sample has no target ABV.
        assert!(html.contains(r#"<dt>ABV</dt><dd><span class="measured">4.5 %</span></dd>"#));
    }

    #[sqlx::test]
    async fn an_iso_8859_1_recipe_shows_its_accented_names_unchanged(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;

        post_batch_with_recipe(&app, &josef, &[], LATIN1).await;

        let html = page(&app, &josef, "/batches/1").await;
        assert!(
            html.contains(r#"<h1 class="batch-title">Únětický Vzor 10.7°</h1>"#),
            "{html}"
        );
        assert!(html.contains(r#"<p class="recipe-name">Únětický Vzor 10.7°</p>"#));
        assert!(html.contains("Plzeňský slad"));
        let number = 1_i64;
        let stored: Option<Vec<u8>> =
            sqlx::query_scalar!("SELECT recipe FROM batches WHERE number = ?", number)
                .fetch_one(&app.state.db)
                .await
                .unwrap();
        assert_eq!(stored.as_deref(), Some(LATIN1), "kept byte for byte");
    }

    #[sqlx::test]
    async fn a_file_that_isnt_a_recipe_is_refused_keeping_the_rest(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let kept = [("name", "Kept name"), ("notes", "Kept\nnotes")];

        refused_recipe(
            post_batch_with_recipe(&app, &josef, &kept, b"Just some text").await,
            NOT_BEERXML,
        )
        .await;
        refused_recipe(
            post_batch_with_recipe(&app, &josef, &kept, HOPS_ONLY).await,
            NO_RECIPE,
        )
        .await;
        let mut big = UNETICE.to_vec();
        big.resize(RECIPE_MAX_BYTES + 1, b' ');
        refused_recipe(
            post_batch_with_recipe(&app, &josef, &kept, &big).await,
            TOO_BIG,
        )
        .await;

        // Without a name either, both are said.
        let both = refused(&app, &josef, &[("name", "")], true).await;
        assert!(both.contains("Give the Batch a name, or attach a Recipe"));

        let plain = post_multipart(
            &app,
            "/batches",
            &josef,
            &kept,
            Some(("big.xml", &big)),
            false,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(plain.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(plain).await;
        assert!(html.contains("<!DOCTYPE"), "the whole tab");
        assert!(html.contains(TOO_BIG));

        assert!(
            page(&app, &josef, "/batches")
                .await
                .contains("No Batches yet.")
        );
    }

    #[sqlx::test]
    async fn a_file_far_over_the_cap_is_too_big_in_the_form_not_a_bare_413(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        // Far over the request's backstop, not just the file's cap.
        let huge = vec![b'x'; 3 * RECIPE_MAX_BYTES];

        let response = post_batch_with_recipe(&app, &josef, &[("name", "Lost")], &huge).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(response).await;
        assert!(html.starts_with("<form "), "the form: {html}");
        assert!(html.contains(TOO_BIG), "{html}");

        add(&app, &josef, "Pilsner", "").await;
        let response = upload(&app, &josef, 1, &huge, true).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body_text(response).await.contains(TOO_BIG));
        assert_eq!(listed(&page(&app, &josef, "/batches").await).len(), 1);
    }

    #[sqlx::test]
    async fn a_member_attaches_replaces_and_removes_a_recipe(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        add(&app, &josef, "Pilsner", "").await;
        let before = page(&app, &josef, "/batches/1").await;
        assert!(before.contains("No Recipe yet."), "{before}");
        assert!(before.contains("Attach Recipe"));
        assert!(!before.contains("Remove Recipe"));

        let response = upload(&app, &ondra, 1, UNETICE, true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches/1");
        let attached = page(&app, &josef, "/batches/1").await;
        assert!(
            attached.contains("Únětický Vzor 10.7° (G70 30L)"),
            "{attached}"
        );
        assert!(
            attached.contains(r#"<h1 class="batch-title">Pilsner</h1>"#),
            "the name stays"
        );
        assert!(attached.contains("Edited by Ondra"));

        let plain = upload(&app, &josef, 1, SPARSE, false).await;
        assert_eq!(plain.status(), StatusCode::SEE_OTHER);
        assert_eq!(plain.headers()[LOCATION], "/batches/1");
        let replaced = page(&app, &josef, "/batches/1").await;
        assert!(replaced.contains(r#"<p class="recipe-name">Just a name</p>"#));
        assert!(!replaced.contains("Únětický"));

        let removed = post_form(
            &app,
            "/batches/1/recipe/delete",
            &josef,
            &[],
            true,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(removed.status(), StatusCode::OK);
        assert_eq!(removed.headers()[HX_REDIRECT], "/batches/1");
        let after = page(&app, &josef, "/batches/1").await;
        assert!(after.contains("No Recipe yet."), "{after}");
        assert!(!after.contains("Just a name"));
    }

    #[sqlx::test]
    async fn a_refused_upload_says_why_in_place_and_keeps_the_recipe(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;
        upload(&app, &josef, 1, UNETICE, true).await;
        let before = page(&app, &josef, "/batches/1").await;
        let mut big = UNETICE.to_vec();
        big.resize(RECIPE_MAX_BYTES + 1, b' ');

        for (bytes, message) in [
            (b"<html></html>".as_slice(), NOT_BEERXML),
            (HOPS_ONLY, NO_RECIPE),
            (&big, TOO_BIG),
        ] {
            let response = upload(&app, &josef, 1, bytes, true).await;
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let html = body_text(response).await;
            assert!(
                html.starts_with(r#"<div class="recipe-upload" id="recipe-upload">"#),
                "a fragment: {html}"
            );
            assert!(html.contains(message), "{message}: {html}");
            assert!(html.contains("Replace Recipe"), "it still has one");
        }

        let none = post_multipart(
            &app,
            "/batches/1/recipe",
            &josef,
            &[],
            None,
            false,
            PUBLIC_ORIGIN,
        )
        .await;
        assert_eq!(none.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let html = body_text(none).await;
        assert!(html.contains("<h1>Batch #1</h1>"), "a page: {html}");
        assert!(html.contains("Choose a BeerXML file to attach."));

        assert_eq!(page(&app, &josef, "/batches/1").await, before);
    }

    #[sqlx::test]
    async fn the_recipe_of_a_batch_thats_gone_says_so(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Soon gone", "").await;
        delete(&app, &josef, 1, true).await;

        for response in [
            upload(&app, &josef, 1, UNETICE, false).await,
            upload(&app, &josef, 1, HOPS_ONLY, false).await,
            post_form(
                &app,
                "/batches/1/recipe/delete",
                &josef,
                &[],
                false,
                PUBLIC_ORIGIN,
            )
            .await,
        ] {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(body_text(response).await.contains("This Batch is gone"));
        }
        let htmx = upload(&app, &josef, 1, UNETICE, true).await;
        assert_eq!(htmx.headers()[HX_REDIRECT], "/batches/1");
    }

    #[sqlx::test]
    async fn signed_out_recipe_uploads_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Pilsner", "").await;

        for response in [
            upload(&app, "", 1, UNETICE, false).await,
            post_form(
                &app,
                "/batches/1/recipe/delete",
                "",
                &[],
                false,
                PUBLIC_ORIGIN,
            )
            .await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
        assert!(
            page(&app, &josef, "/batches/1")
                .await
                .contains("No Recipe yet.")
        );
    }

    #[sqlx::test]
    async fn recipe_posts_from_another_origin_are_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        add(&app, &josef, "Guarded", "").await;
        upload(&app, &josef, 1, SPARSE, true).await;
        let foreign = "https://evil.example";

        let attach = post_multipart(
            &app,
            "/batches/1/recipe",
            &josef,
            &[],
            Some(("evil.xml", UNETICE)),
            true,
            foreign,
        )
        .await;
        assert_eq!(attach.status(), StatusCode::FORBIDDEN);
        let add_with = post_multipart(
            &app,
            "/batches",
            &josef,
            &[("name", "Hacked")],
            Some(("evil.xml", UNETICE)),
            true,
            foreign,
        )
        .await;
        assert_eq!(add_with.status(), StatusCode::FORBIDDEN);
        let remove = post_form(&app, "/batches/1/recipe/delete", &josef, &[], true, foreign).await;
        assert_eq!(remove.status(), StatusCode::FORBIDDEN);

        let html = page(&app, &josef, "/batches/1").await;
        assert!(
            html.contains(r#"<p class="recipe-name">Just a name</p>"#),
            "{html}"
        );
        assert_eq!(listed(&page(&app, &josef, "/batches").await).len(), 1);
    }

    // Brewing slots. The test app's clock starts on Thursday 1 January 2026 in Prague.

    /// Posts `date` to a slot's form at `uri`, as htmx does if `htmx`.
    async fn post_slot(
        app: &TestApp,
        cookie: &str,
        uri: &str,
        date: &str,
        htmx: bool,
    ) -> Response<Body> {
        post_form(
            app,
            uri,
            cookie,
            &fields(&[("date", date)]),
            htmx,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// Plans a slot on `date` with htmx, expecting it planned.
    async fn plan(app: &TestApp, cookie: &str, date: &str) {
        let response = post_slot(app, cookie, "/batches/slots", date, true).await;
        assert_eq!(response.status(), StatusCode::OK, "{date}");
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");
    }

    /// The dates of the upcoming slots on `html`, in order.
    fn slot_dates(html: &str) -> Vec<String> {
        html.split(r#"<span class="slot-date"><time datetime=""#)
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap_or_default().to_owned())
            .collect()
    }

    /// Posts a slot's form, expecting it refused, and answers with what came back.
    async fn slot_refused(
        app: &TestApp,
        cookie: &str,
        uri: &str,
        date: &str,
        htmx: bool,
    ) -> String {
        let response = post_slot(app, cookie, uri, date, htmx).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        body
    }

    #[sqlx::test]
    async fn signed_out_slot_requests_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        plan(&app, &josef, "2026-01-10").await;

        for response in [
            app.get("/batches/slots/1").await,
            post_slot(&app, "", "/batches/slots", "2026-01-11", false).await,
            post_slot(&app, "", "/batches/slots/1", "2026-01-12", false).await,
            post_slot(&app, "", "/batches/slots/1/cancel", "", false).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
        assert_eq!(
            slot_dates(&page(&app, &josef, "/batches").await),
            ["2026-01-10"]
        );
    }

    #[sqlx::test]
    async fn a_new_tab_has_no_slots_and_the_plan_form(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = page(&app, &cookie, "/batches").await;

        assert!(html.contains("No brew day set aside yet."), "{html}");
        assert!(
            html.contains(r#"action="/batches/slots" hx-post="/batches/slots""#),
            "{html}"
        );
        assert!(html.contains(r#"min="2026-01-01""#));
    }

    #[sqlx::test]
    async fn any_member_plans_slots_shown_soonest_first(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;

        plan(&app, &josef, "2026-01-24").await;
        plan(&app, &ondra, "2026-01-01").await;
        let response = post_slot(&app, &ondra, "/batches/slots", "2026-01-10", false).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/batches");

        let html = page(&app, &josef, "/batches").await;
        assert_eq!(
            slot_dates(&html),
            ["2026-01-01", "2026-01-10", "2026-01-24"]
        );
        assert!(html.contains("Sat 10 Jan"), "{html}");
        assert!(html.contains("Nothing chosen yet"));
    }

    #[sqlx::test]
    async fn a_planned_slots_reminder_is_pending_on_the_reminders_tab(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;

        plan(&app, &ondra, "2026-01-10").await;

        let html = page(&app, &josef, "/reminders").await;
        assert!(
            html.contains("Brew day Sat 10 Jan, and no Batch is picked yet"),
            "{html}"
        );
        assert!(
            html.contains(
                r#"<time datetime="2026-01-05T09:00">Monday 5 January 2026, 09:00</time> · Ondra"#
            ),
            "{html}"
        );
    }

    #[sqlx::test]
    async fn a_taken_past_or_missing_date_is_refused_and_kept(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;

        let taken = slot_refused(&app, &cookie, "/batches/slots", "2026-01-10", true).await;
        assert!(
            taken.starts_with("<form "),
            "a fragment, not the page: {taken}"
        );
        assert!(
            taken.contains("There's already a slot on Sat 10 Jan."),
            "{taken}"
        );
        assert!(taken.contains(r#"value="2026-01-10""#));

        let past = slot_refused(&app, &cookie, "/batches/slots", "2025-12-31", false).await;
        assert!(past.contains("<!DOCTYPE"), "the whole tab");
        assert!(past.contains("Pick today or a later date."), "{past}");
        assert!(past.contains(r#"value="2025-12-31""#));
        assert_eq!(slot_dates(&past), ["2026-01-10"]);

        let missing = slot_refused(&app, &cookie, "/batches/slots", " ", true).await;
        assert!(missing.contains("Pick a date for the slot."), "{missing}");

        assert_eq!(
            slot_dates(&page(&app, &cookie, "/batches").await),
            ["2026-01-10"]
        );
    }

    #[sqlx::test]
    async fn a_passed_slot_with_nothing_brewed_asks_did_you_brew_and_leads_to_the_brew_day(
        db: SqlitePool,
    ) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-02").await;
        plan(&app, &cookie, "2026-01-01").await;
        add(&app, &cookie, "Pils", "").await;

        app.clock.advance(std::time::Duration::from_hours(24));

        let tab = page(&app, &cookie, "/batches").await;
        assert_eq!(
            slot_dates(&tab),
            ["2026-01-01", "2026-01-02"],
            "passed first"
        );
        assert_eq!(tab.matches("Did you brew?").count(), 1, "{tab}");
        assert!(tab.contains(r#"href="/batches/slots/2">Pick the Batch</a>"#));

        let slot = page(&app, &cookie, "/batches/slots/2").await;
        assert!(slot.contains("Did you brew?"), "{slot}");
        assert!(slot.contains(r#"href="/batches/1/brew-day?date=2026-01-01""#));

        let form = page(&app, &cookie, "/batches/1/brew-day?date=2026-01-01").await;
        assert!(form.contains(r#"value="2026-01-01""#), "{form}");
        let today = page(&app, &cookie, "/batches/1/brew-day?date=nonsense").await;
        assert!(today.contains(r#"value="2026-01-02""#), "{today}");

        recorded(&app, &cookie, 1, "brew-day", &[("brew_date", "2026-01-01")]).await;
        assert_eq!(
            slot_dates(&page(&app, &cookie, "/batches").await),
            ["2026-01-02"],
            "the slot is done"
        );
    }

    #[sqlx::test]
    async fn a_passed_slot_with_a_batch_picks_it_straight_to_its_brew_day(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-01").await;
        add(&app, &cookie, "Pils", "").await;
        link_slot(&app, &cookie, 1, "1").await;

        app.clock.advance(std::time::Duration::from_hours(24));

        let tab = page(&app, &cookie, "/batches").await;
        assert!(tab.contains("Did you brew?"), "{tab}");
        assert!(
            tab.contains(r#"href="/batches/1/brew-day?date=2026-01-01">Pick the Batch</a>"#),
            "{tab}"
        );
        assert!(!tab.contains("Unlink"));
    }

    #[sqlx::test]
    async fn a_passed_slot_is_moved_or_cancelled_like_any_other(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-01").await;
        plan(&app, &cookie, "2026-01-02").await;
        app.clock.advance(std::time::Duration::from_hours(48));

        let response = post_slot(&app, &cookie, "/batches/slots/1", "2026-01-10", true).await;
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");
        post_slot(&app, &cookie, "/batches/slots/2/cancel", "", true).await;

        let tab = page(&app, &cookie, "/batches").await;
        assert_eq!(slot_dates(&tab), ["2026-01-10"]);
        assert!(!tab.contains("Did you brew?"), "{tab}");
    }

    #[sqlx::test]
    async fn a_done_slot_cant_be_given_another_batch(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        add(&app, &cookie, "Pils", "").await;
        add(&app, &cookie, "Stout", "").await;
        recorded(&app, &cookie, 1, "brew-day", &[("brew_date", "2026-01-10")]).await;

        posted(
            &app,
            &cookie,
            "/batches/slots/1/batch",
            &[("batch", "2")],
            "/batches",
        )
        .await;
        let html = link_refused(&app, &cookie, "/batches/2/slot", &[("slot", "1")]).await;
        assert!(
            html.contains("That slot's Batch is brewed already."),
            "{html}"
        );

        assert!(
            page(&app, &cookie, "/batches/2")
                .await
                .contains("Waiting for a slot")
        );
        let slot = page(&app, &cookie, "/batches/slots/1").await;
        assert!(slot.contains("#1 Pils</a>"), "{slot}");
    }

    /// Posts `pairs` to `uri` as a plain form, expecting a redirect to `to`.
    async fn posted(app: &TestApp, cookie: &str, uri: &str, pairs: &[(&str, &str)], to: &str) {
        let response = post_form(app, uri, cookie, &fields(pairs), false, PUBLIC_ORIGIN).await;
        let status = response.status();
        let location = response.headers().get(LOCATION).cloned();
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "{}",
            body_text(response).await
        );
        assert_eq!(location.unwrap(), to, "{uri}");
    }

    /// Links the Batch `number` to the slot `id` from the slot's page.
    async fn link_slot(app: &TestApp, cookie: &str, id: i64, number: &str) {
        let uri = format!("/batches/slots/{id}/batch");
        posted(app, cookie, &uri, &[("batch", number)], "/batches").await;
    }

    /// Links the Batch `number` to the slot `id` from the Batch's page.
    async fn link_batch(app: &TestApp, cookie: &str, number: i64, id: &str) {
        let uri = format!("/batches/{number}/slot");
        let to = format!("/batches/{number}");
        posted(app, cookie, &uri, &[("slot", id)], &to).await;
    }

    /// Posts a link form, expecting it refused, and answers with the page that came back.
    async fn link_refused(
        app: &TestApp,
        cookie: &str,
        uri: &str,
        pairs: &[(&str, &str)],
    ) -> String {
        let response = post_form(app, uri, cookie, &fields(pairs), false, PUBLIC_ORIGIN).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        body
    }

    #[sqlx::test]
    async fn any_member_links_a_batch_from_the_slot_and_unlinks_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        plan(&app, &josef, "2026-01-10").await;
        add(&app, &josef, "Pils", "").await;

        let slot = page(&app, &ondra, "/batches/slots/1").await;
        assert!(
            slot.contains(r#"action="/batches/slots/1/batch""#),
            "{slot}"
        );
        assert!(slot.contains(r#"<option value="1">#1 Pils</option>"#));
        link_slot(&app, &ondra, 1, "1").await;

        let tab = page(&app, &josef, "/batches").await;
        assert!(tab.contains(r#"<a href="/batches/1">#1 Pils</a>"#), "{tab}");
        assert!(!tab.contains("Nothing chosen yet"));
        assert!(tab.contains("Brewing on <time datetime=\"2026-01-10\">Sat 10 Jan</time>"));
        let batch = page(&app, &josef, "/batches/1").await;
        assert!(batch.contains("Brewing on"), "{batch}");

        posted(&app, &josef, "/batches/slots/1/unlink", &[], "/batches").await;
        let tab = page(&app, &josef, "/batches").await;
        assert!(tab.contains("Nothing chosen yet"), "{tab}");
        assert!(tab.contains("Waiting for a slot"));
    }

    #[sqlx::test]
    async fn any_member_links_from_the_batch_page_moves_it_and_unlinks_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let simon = app.sign_in_new_member("Šimon").await;
        plan(&app, &josef, "2026-01-10").await;
        plan(&app, &josef, "2026-01-17").await;
        add(&app, &josef, "Pils", "").await;

        let waiting = page(&app, &simon, "/batches/1").await;
        assert!(waiting.contains("Waiting for a slot"), "{waiting}");
        assert!(waiting.contains(r#"<option value="1">Sat 10 Jan</option>"#));
        assert!(waiting.contains(r#"<option value="2">Sat 17 Jan</option>"#));
        link_batch(&app, &simon, 1, "1").await;

        let slotted = page(&app, &simon, "/batches/1").await;
        assert!(
            slotted.contains(r#"Brewing on <time datetime="2026-01-10">Sat 10 Jan</time>"#),
            "{slotted}"
        );
        assert!(
            !slotted.contains(r#"<option value="1">"#),
            "its own slot isn't free"
        );
        link_batch(&app, &simon, 1, "2").await;

        let tab = page(&app, &josef, "/batches").await;
        assert_eq!(tab.matches("Nothing chosen yet").count(), 1, "{tab}");
        assert!(tab.contains("Brewing on <time datetime=\"2026-01-17\">"));

        posted(&app, &simon, "/batches/1/slot/unlink", &[], "/batches/1").await;
        assert!(
            page(&app, &simon, "/batches/1")
                .await
                .contains("Waiting for a slot")
        );
        assert_eq!(
            page(&app, &josef, "/batches")
                .await
                .matches("Nothing chosen yet")
                .count(),
            2
        );
    }

    #[sqlx::test]
    async fn picking_a_batch_for_a_slot_with_one_puts_the_old_one_back_to_waiting(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        add(&app, &cookie, "Pils", "").await;
        add(&app, &cookie, "Stout", "").await;
        link_slot(&app, &cookie, 1, "1").await;

        let slot = page(&app, &cookie, "/batches/slots/1").await;
        assert!(slot.contains("Brew instead"), "{slot}");
        assert!(
            !slot.contains(r#"<option value="1">"#),
            "not the one it has"
        );
        link_slot(&app, &cookie, 1, "2").await;

        assert!(
            page(&app, &cookie, "/batches/1")
                .await
                .contains("Waiting for a slot")
        );
        assert!(
            page(&app, &cookie, "/batches")
                .await
                .contains("#2 Stout</a>")
        );
    }

    #[sqlx::test]
    async fn a_refused_link_says_why_on_the_page_it_came_from(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        plan(&app, &cookie, "2026-01-17").await;
        add(&app, &cookie, "Brewed", "").await;
        add(&app, &cookie, "Gone", "").await;
        add(&app, &cookie, "Pils", "").await;
        recorded(&app, &cookie, 1, "brew-day", &[("brew_date", "2026-01-01")]).await;
        delete(&app, &cookie, 2, true).await;
        post_slot(&app, &cookie, "/batches/slots/2/cancel", "", true).await;

        for (pairs, message) in [
            (&[("batch", "")][..], "Pick a Batch to brew that day."),
            (&[("batch", "1")][..], "That Batch is brewed already."),
            (
                &[("batch", "2")][..],
                "That Batch is gone: someone deleted it.",
            ),
        ] {
            let html = link_refused(&app, &cookie, "/batches/slots/1/batch", pairs).await;
            assert!(html.contains(message), "{message}: {html}");
            assert!(html.contains("Brewing slot on"), "the slot's page");
        }

        for (id, message) in [
            ("", "Pick a slot to brew it on."),
            ("2", "That slot is gone: someone cancelled it."),
        ] {
            let html = link_refused(&app, &cookie, "/batches/3/slot", &[("slot", id)]).await;
            assert!(html.contains(message), "{message}: {html}");
            assert!(html.contains(r#"aria-invalid="true""#), "the Batch's page");
        }

        // A brewed Batch's page shows it's brewed, with no slot to pick.
        posted(
            &app,
            &cookie,
            "/batches/1/slot",
            &[("slot", "1")],
            "/batches/1",
        )
        .await;
        assert!(
            !page(&app, &cookie, "/batches/1")
                .await
                .contains("Brewing slot")
        );
        assert!(
            page(&app, &cookie, "/batches")
                .await
                .contains("Nothing chosen yet")
        );
    }

    #[sqlx::test]
    async fn links_to_a_slot_or_batch_thats_gone_say_so(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        add(&app, &cookie, "Pils", "").await;
        post_slot(&app, &cookie, "/batches/slots/1/cancel", "", true).await;
        delete(&app, &cookie, 1, true).await;

        for uri in ["/batches/slots/1/batch", "/batches/slots/1/unlink"] {
            let response = post_form(
                &app,
                uri,
                &cookie,
                &fields(&[("batch", "1")]),
                false,
                PUBLIC_ORIGIN,
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert!(body_text(response).await.contains("This slot is gone"));
        }
        for uri in ["/batches/1/slot", "/batches/1/slot/unlink"] {
            let response = post_form(
                &app,
                uri,
                &cookie,
                &fields(&[("slot", "1")]),
                false,
                PUBLIC_ORIGIN,
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[sqlx::test]
    async fn brewing_on_a_free_slots_day_links_it_and_the_slot_is_done(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        plan(&app, &cookie, "2026-01-17").await;
        add(&app, &cookie, "Pils", "").await;

        recorded(&app, &cookie, 1, "brew-day", &[("brew_date", "2026-01-10")]).await;

        assert_eq!(
            slot_dates(&page(&app, &cookie, "/batches").await),
            ["2026-01-17"]
        );
        recorded(&app, &cookie, 1, "brew-day", &[("brew_date", "")]).await;
        let tab = page(&app, &cookie, "/batches").await;
        assert_eq!(
            slot_dates(&tab),
            ["2026-01-10", "2026-01-17"],
            "planned again"
        );
        assert!(tab.contains("#1 Pils</a>"), "{tab}");
    }

    #[sqlx::test]
    async fn deleting_a_linked_batch_leaves_its_slot_with_nothing_chosen(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        add(&app, &cookie, "Pils", "").await;
        link_slot(&app, &cookie, 1, "1").await;

        delete(&app, &cookie, 1, true).await;

        let tab = page(&app, &cookie, "/batches").await;
        assert_eq!(slot_dates(&tab), ["2026-01-10"]);
        assert!(tab.contains("Nothing chosen yet"), "{tab}");
    }

    #[sqlx::test]
    async fn signed_out_links_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        add(&app, &cookie, "Pils", "").await;

        for (uri, pairs) in [
            ("/batches/slots/1/batch", &[("batch", "1")][..]),
            ("/batches/slots/1/unlink", &[][..]),
            ("/batches/1/slot", &[("slot", "1")][..]),
            ("/batches/1/slot/unlink", &[][..]),
        ] {
            let response = post_form(&app, uri, "", &fields(pairs), false, PUBLIC_ORIGIN).await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{uri}");
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
        assert!(
            page(&app, &cookie, "/batches")
                .await
                .contains("Nothing chosen yet")
        );
    }

    #[sqlx::test]
    async fn link_posts_from_another_origin_are_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        add(&app, &cookie, "Pils", "").await;

        for uri in ["/batches/slots/1/batch", "/batches/1/slot"] {
            let response = post_form(
                &app,
                uri,
                &cookie,
                &fields(&[("batch", "1"), ("slot", "1")]),
                false,
                "https://evil.example",
            )
            .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        assert!(
            page(&app, &cookie, "/batches")
                .await
                .contains("Nothing chosen yet")
        );

        link_slot(&app, &cookie, 1, "1").await;
        for uri in ["/batches/slots/1/unlink", "/batches/1/slot/unlink"] {
            let response = post_form(&app, uri, &cookie, &[], false, "https://evil.example").await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        assert!(
            page(&app, &cookie, "/batches")
                .await
                .contains("#1 Pils</a>")
        );
    }

    #[sqlx::test]
    async fn any_member_moves_a_slot_another_planned(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let simon = app.sign_in_new_member("Šimon").await;
        plan(&app, &josef, "2026-01-10").await;
        plan(&app, &josef, "2026-01-17").await;

        let form = page(&app, &simon, "/batches/slots/1").await;
        assert!(form.contains("Brewing slot on"), "{form}");
        assert!(form.contains(r#"value="2026-01-10""#));
        assert!(form.contains(r#"min="2026-01-01""#));

        let response = post_slot(&app, &simon, "/batches/slots/1", "2026-01-24", true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");

        assert_eq!(
            slot_dates(&page(&app, &josef, "/batches").await),
            ["2026-01-17", "2026-01-24"]
        );
    }

    #[sqlx::test]
    async fn a_move_to_a_taken_past_or_missing_date_is_refused_and_kept(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        plan(&app, &cookie, "2026-01-17").await;

        let taken = slot_refused(&app, &cookie, "/batches/slots/1", "2026-01-17", true).await;
        assert!(
            taken.starts_with("<form "),
            "a fragment, not the page: {taken}"
        );
        assert!(
            taken.contains("There's already a slot on Sat 17 Jan."),
            "{taken}"
        );
        assert!(taken.contains(r#"value="2026-01-17""#));

        let past = slot_refused(&app, &cookie, "/batches/slots/1", "2025-12-31", false).await;
        assert!(past.contains("<!DOCTYPE"), "the slot's page");
        assert!(past.contains("Brewing slot on"), "{past}");
        assert!(past.contains("Pick today or a later date."));
        assert!(past.contains(r#"value="2025-12-31""#));

        let missing = slot_refused(&app, &cookie, "/batches/slots/1", "", true).await;
        assert!(missing.contains("Pick a date for the slot."), "{missing}");

        assert_eq!(
            slot_dates(&page(&app, &cookie, "/batches").await),
            ["2026-01-10", "2026-01-17"]
        );
    }

    #[sqlx::test]
    async fn only_planning_a_slot_is_announced(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        let mut events = app.state.events.subscribe();

        plan(&app, &ondra, "2026-01-10").await;
        post_slot(&app, &josef, "/batches/slots/1", "2026-01-17", true).await;
        slot_refused(&app, &josef, "/batches/slots", "2026-01-17", true).await;
        post_slot(&app, &josef, "/batches/slots/1/cancel", "", true).await;

        let emitted: Vec<Event> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        let [Event::SlotPlanned(planned)] = emitted.as_slice() else {
            panic!("one slot announced: {emitted:?}");
        };
        assert_eq!(planned.date.to_string(), "2026-01-10");
        assert_eq!(planned.notify.len(), 1, "everyone but Ondra");
        assert_ne!(planned.notify[0], planned.planned_by);
    }

    #[sqlx::test]
    async fn a_cancelled_slot_is_gone_and_frees_its_date(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        plan(&app, &josef, "2026-01-10").await;

        let tab = page(&app, &ondra, "/batches").await;
        assert!(
            tab.contains(
                r#"hx-post="/batches/slots/1/cancel" hx-confirm="Cancel the slot on Sat 10 Jan?"#
            ),
            "{tab}"
        );
        let response = post_slot(&app, &ondra, "/batches/slots/1/cancel", "", true).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[HX_REDIRECT], "/batches");

        let html = page(&app, &josef, "/batches").await;
        assert_eq!(slot_dates(&html), Vec::<String>::new());
        assert!(html.contains("No brew day set aside yet."));
        plan(&app, &josef, "2026-01-10").await;
    }

    #[sqlx::test]
    async fn a_cancelled_slot_cant_be_moved_or_cancelled_again(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        plan(&app, &cookie, "2026-01-10").await;
        post_slot(&app, &cookie, "/batches/slots/1/cancel", "", true).await;

        let response = app.get_with("/batches/slots/1", &cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(body_text(response).await.contains("This slot is gone"));

        let moved = post_slot(&app, &cookie, "/batches/slots/1", "2026-01-17", true).await;
        assert_eq!(moved.headers()[HX_REDIRECT], "/batches/slots/1");
        let refused = post_slot(&app, &cookie, "/batches/slots/1", "", false).await;
        assert_eq!(refused.status(), StatusCode::NOT_FOUND);

        let again = post_slot(&app, &cookie, "/batches/slots/1/cancel", "", false).await;
        assert_eq!(again.status(), StatusCode::SEE_OTHER);
        assert_eq!(again.headers()[LOCATION], "/batches");
        assert_eq!(
            slot_dates(&page(&app, &cookie, "/batches").await),
            Vec::<String>::new()
        );
    }

    #[sqlx::test]
    async fn slot_posts_from_another_origin_are_refused(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        plan(&app, &josef, "2026-01-10").await;

        for uri in [
            "/batches/slots",
            "/batches/slots/1",
            "/batches/slots/1/cancel",
        ] {
            let response = post_form(
                &app,
                uri,
                &josef,
                &fields(&[("date", "2026-01-24")]),
                true,
                "https://evil.example",
            )
            .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        assert_eq!(
            slot_dates(&page(&app, &josef, "/batches").await),
            ["2026-01-10"]
        );
    }
}
