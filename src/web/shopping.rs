//! The Shopping tab: adding an Item to the Crew's Shopping list, the open Items grouped by
//! Urgency, and marking them bought and back.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::htmx::Htmx;
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::shopping::{
    ItemId, ItemName, ItemNote, NAME_MAX, NOTE_MAX, NameError, NewItem, Urgency,
};
use crate::views;
use crate::views::nav::Tab;
use crate::views::shopping::{Fold, ItemProblems, ItemValues};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Shopping.href(), get(show).post(add))
        .route("/shopping/{id}/bought", post(mark_bought))
        .route("/shopping/{id}/undo", post(undo_bought))
}

/// The Shopping tab with an empty form.
async fn show(
    State(state): State<AppState>,
    Member(_): Member,
) -> Result<impl IntoResponse, AppError> {
    Ok(fresh(page(&state, &empty_form()).await?))
}

#[derive(Deserialize)]
struct ItemForm {
    name: String,
    #[serde(default)]
    note: String,
    /// Missing when the browser sent no choice, which means the default.
    urgency: Option<String>,
}

impl ItemForm {
    /// The Urgency chosen, the default if none was, or `None` for one there isn't.
    fn urgency(&self) -> Option<Urgency> {
        self.urgency
            .as_deref()
            .map_or_else(|| Some(Urgency::default()), Urgency::from_key)
    }

    fn values(&self) -> ItemValues<'_> {
        ItemValues {
            name: &self.name,
            note: &self.note,
            urgency: self.urgency().unwrap_or_default(),
        }
    }
}

/// Adds the Item. htmx gets the form emptied for the next one and the open list with the Item in
/// its group; a plain form post goes back to the tab. A refused form shows what to fix: alone
/// for htmx, inside the tab otherwise.
async fn add(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<ItemForm>,
) -> Result<Response, AppError> {
    let item = match parse(&input) {
        Ok(item) => item,
        Err(problems) => {
            let form = views::shopping::form(input.values(), &problems);
            let body = if htmx {
                form
            } else {
                page(&state, &form).await?
            };
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response());
        }
    };
    state.shopping.add(member.id, &item).await?;
    tracing::info!(member = member.id.get(), "a Member added an Item");
    if !htmx {
        return Ok(Redirect::to(Tab::Shopping.href()).into_response());
    }
    let open = state.shopping.open().await?;
    Ok(fresh(views::shopping::added(&empty_form(), &open)).into_response())
}

/// Marks the open Item `id` bought by the Member. htmx gets the open list without it and Recently
/// bought with it; a plain form post goes back to the tab.
async fn mark_bought(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    state
        .shopping
        .mark_bought(member.id, ItemId::from_client(id))
        .await?;
    tracing::info!(member = member.id.get(), "a Member marked an Item bought");
    changed_lists(&state, htmx, Fold::Folded).await
}

/// Takes back marking the Item `id` bought. htmx gets the open list with it back in its group and
/// Recently bought, still unfolded, without it; a plain form post goes back to the tab.
async fn undo_bought(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    state.shopping.undo_bought(ItemId::from_client(id)).await?;
    tracing::info!(
        member = member.id.get(),
        "a Member took back buying an Item"
    );
    changed_lists(&state, htmx, Fold::Unfolded).await
}

/// What answers a change to an Item: for htmx, the open list and Recently bought, folded as
/// `fold`; otherwise the way back to the tab.
async fn changed_lists(state: &AppState, htmx: bool, fold: Fold) -> Result<Response, AppError> {
    if !htmx {
        return Ok(Redirect::to(Tab::Shopping.href()).into_response());
    }
    let open = state.shopping.open().await?;
    let bought = state.shopping.recently_bought().await?;
    Ok(fresh(views::shopping::changed_lists(&open, &bought, fold)).into_response())
}

/// The Item as typed, or what is wrong with each field.
fn parse(input: &ItemForm) -> Result<NewItem, ItemProblems> {
    match (
        ItemName::parse(&input.name),
        ItemNote::parse(&input.note),
        input.urgency(),
    ) {
        (Ok(name), Ok(note), Some(urgency)) => Ok(NewItem {
            name,
            note,
            urgency,
        }),
        (name, note, urgency) => Err(ItemProblems {
            name: name.err().map(name_message),
            note: note.err().map(|_| note_message()),
            urgency: urgency
                .is_none()
                .then(|| "Pick how soon it's needed.".to_owned()),
        }),
    }
}

fn empty_form() -> Markup {
    views::shopping::form(ItemValues::default(), &ItemProblems::default())
}

/// The Shopping tab around `form`, with every open Item and those bought lately.
async fn page(state: &AppState, form: &Markup) -> Result<Markup, AppError> {
    let open = state.shopping.open().await?;
    let bought = state.shopping.recently_bought().await?;
    Ok(views::shopping::page(form, &open, &bought))
}

fn name_message(err: NameError) -> String {
    match err {
        NameError::Blank => "Say what's needed.".to_owned(),
        NameError::TooLong => format!("Keep the name to {NAME_MAX} characters or fewer."),
    }
}

fn note_message() -> String {
    format!("Keep the note to {NOTE_MAX} characters or fewer.")
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::LOCATION;
    use axum::http::{Method, Response, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::events::Event;
    use crate::domain::shopping::{NAME_MAX, NOTE_MAX};
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    /// Posts the Item form with `fields`, as htmx does if `htmx`.
    async fn post_item(
        app: &TestApp,
        cookie: &str,
        fields: &[(&'static str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        let fields: Vec<(&str, String)> = fields
            .iter()
            .map(|&(name, value)| (name, value.to_owned()))
            .collect();
        post_form(app, "/shopping", cookie, &fields, htmx, PUBLIC_ORIGIN).await
    }

    /// Adds an Item with htmx, expecting it added.
    async fn add(app: &TestApp, cookie: &str, fields: &[(&'static str, &str)]) -> String {
        let response = post_item(app, cookie, fields, true).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Posts the Item form, expecting it refused, and answers with what came back.
    async fn refused(
        app: &TestApp,
        cookie: &str,
        fields: &[(&'static str, &str)],
        htmx: bool,
    ) -> String {
        let response = post_item(app, cookie, fields, htmx).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        body
    }

    async fn tab(app: &TestApp, cookie: &str) -> String {
        let response = app.get_with("/shopping", cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    /// The name of the Item a tag in `html` split at each `<` starts, if it starts one.
    fn item_name(tag: &str) -> Option<&str> {
        let (_, name) = tag
            .strip_prefix("span class=\"item-name\" id=\"")?
            .split_once('>')?;
        Some(name)
    }

    /// The Urgency heading each open Item named in `html` sits under, in the order they're
    /// listed.
    fn grouped(html: &str) -> Vec<(String, String)> {
        let mut group = String::new();
        let mut listed = Vec::new();
        for tag in html.split('<') {
            if let Some(heading) = tag.strip_prefix("h2 id=\"urgency-") {
                group = heading.split('>').nth(1).unwrap_or_default().to_owned();
            } else if tag.starts_with("details class=\"section recently-bought\"") {
                break;
            } else if let Some(name) = item_name(tag) {
                listed.push((group.clone(), name.to_owned()));
            }
        }
        listed
    }

    #[sqlx::test]
    async fn signed_out_the_shopping_tab_redirects_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/shopping").await,
            post_item(&app, "", &[("name", "Saaz")], false).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn signed_out_htmx_is_told_to_load_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut post = request(Method::POST, "/shopping", Some(PUBLIC_ORIGIN));
        post.headers_mut()
            .insert(HX_REQUEST, "true".parse().unwrap());

        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[HX_REDIRECT], "/sign-in");
    }

    #[sqlx::test]
    async fn a_new_tab_has_the_form_and_nothing_to_buy(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = tab(&app, &cookie).await;

        assert!(html.contains(r#"<form class="entry-form" method="post" action="/shopping""#));
        assert!(html.contains("Nothing to buy."));
        assert!(!html.contains("will show up here"), "not the placeholder");
    }

    #[sqlx::test]
    async fn an_item_with_no_urgency_chosen_is_needed_by_the_next_brewday(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;

        add(&app, &ondra, &[("name", "  Pilsner malt "), ("note", "")]).await;

        // The whole Crew sees it, with who added it and when, in Prague time.
        let html = tab(&app, &josef).await;
        assert_eq!(
            grouped(&html),
            [("Next brewday".to_owned(), "Pilsner malt".to_owned())]
        );
        assert!(html.contains(
            r#"<time datetime="2026-01-01T01:00">Thursday 1 January 2026, 01:00</time> · Ondra"#
        ));
        assert!(!html.contains("Nothing to buy."));
    }

    #[sqlx::test]
    async fn the_list_is_grouped_by_urgency_oldest_first(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        for (name, urgency) in [
            ("Bottle caps", "nice-to-have"),
            ("Saaz", "next-brewday"),
            ("Star San", "asap"),
            ("Pilsner malt", "next-brewday"),
            ("CO₂", "asap"),
        ] {
            add(&app, &cookie, &[("name", name), ("urgency", urgency)]).await;
            app.clock.advance(std::time::Duration::from_mins(1));
        }

        let listed = grouped(&tab(&app, &cookie).await);

        let listed: Vec<(&str, &str)> = listed
            .iter()
            .map(|(group, name)| (group.as_str(), name.as_str()))
            .collect();
        assert_eq!(
            listed,
            [
                ("ASAP", "Star San"),
                ("ASAP", "CO₂"),
                ("Next brewday", "Saaz"),
                ("Next brewday", "Pilsner malt"),
                ("Nice to have", "Bottle caps"),
            ]
        );
    }

    #[sqlx::test]
    async fn htmx_gets_an_empty_form_and_the_list_swapped_out_of_band(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = add(
            &app,
            &cookie,
            &[
                ("name", "Star San"),
                ("urgency", "asap"),
                ("note", "the big one"),
            ],
        )
        .await;

        assert!(html.starts_with("<form "), "the form first: {html}");
        assert!(html.contains(r#"name="name" value="" "#), "emptied: {html}");
        assert!(html.contains(r#"value="next-brewday" checked>"#));
        assert!(html.contains(r#"<div id="open-items" hx-swap-oob="true">"#));
        assert_eq!(grouped(&html), [("ASAP".to_owned(), "Star San".to_owned())]);
        assert!(html.contains(r#"<p class="item-note">the big one</p>"#));
        assert!(!html.contains("<h1>"), "a fragment, not the page");
    }

    #[sqlx::test]
    async fn a_plain_post_goes_back_to_the_tab(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = post_item(&app, &cookie, &[("name", "Saaz")], false).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/shopping");
        assert!(
            tab(&app, &cookie)
                .await
                .contains(r#"<span class="item-name" id="item-1-name">Saaz</span>"#)
        );
    }

    #[sqlx::test]
    async fn a_link_in_a_note_opens_in_a_new_tab(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        add(
            &app,
            &cookie,
            &[
                ("name", "Malt"),
                ("note", "<i>25 kg</i> https://shop.example.cz/malt"),
            ],
        )
        .await;

        assert!(tab(&app, &cookie).await.contains(
            r#"<p class="item-note">&lt;i&gt;25 kg&lt;/i&gt; <a href="https://shop.example.cz/malt" target="_blank" rel="noopener noreferrer">"#
        ));
    }

    #[sqlx::test]
    async fn refuses_a_missing_name_with_htmx_keeping_what_was_typed(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = refused(
            &app,
            &cookie,
            &[("name", "  "), ("urgency", "asap"), ("note", "two packs")],
            true,
        )
        .await;

        assert!(html.starts_with("<form"), "a fragment: {html}");
        assert!(html.contains("Say what's needed."));
        assert!(html.contains(r#"value="asap" checked>"#));
        assert!(html.contains(">two packs</textarea>"));
        assert!(tab(&app, &cookie).await.contains("Nothing to buy."));
    }

    #[sqlx::test]
    async fn refuses_a_name_or_note_too_long_without_htmx_inside_the_tab(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let long_name = "a".repeat(NAME_MAX + 1);
        let long_note = "b".repeat(NOTE_MAX + 1);

        let html = refused(
            &app,
            &cookie,
            &[("name", &long_name), ("note", &long_note)],
            false,
        )
        .await;

        assert!(html.contains("<h1>Shopping</h1>"), "the whole tab: {html}");
        assert!(html.contains("Keep the name to 200 characters or fewer."));
        assert!(html.contains("Keep the note to 500 characters or fewer."));
        assert!(html.contains(&format!(r#"value="{long_name}""#)));
        assert!(html.contains("Nothing to buy."));
    }

    #[sqlx::test]
    async fn refuses_an_urgency_there_isnt(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = refused(
            &app,
            &cookie,
            &[("name", "Saaz"), ("urgency", "yesterday")],
            true,
        )
        .await;

        assert!(html.contains("Pick how soon it's needed."));
        assert!(html.contains(r#"value="Saaz""#));
        assert!(tab(&app, &cookie).await.contains("Nothing to buy."));
    }

    #[sqlx::test]
    async fn refuses_a_post_from_another_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = post_form(
            &app,
            "/shopping",
            &cookie,
            &[("name", "Saaz".to_owned())],
            false,
            "https://evil.test",
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(tab(&app, &cookie).await.contains("Nothing to buy."));
    }

    /// Marks the Item `id` bought, or takes that back if `action` is `undo`, as htmx does.
    async fn press(app: &TestApp, cookie: &str, id: i64, action: &str) -> Response<Body> {
        post_form(
            app,
            &format!("/shopping/{id}/{action}"),
            cookie,
            &[],
            true,
            PUBLIC_ORIGIN,
        )
        .await
    }

    /// Presses a button on the Item `id`, expecting it done, and answers with what came back.
    async fn pressed(app: &TestApp, cookie: &str, id: i64, action: &str) -> String {
        let response = press(app, cookie, id, action).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// The names in Recently bought, in the order they're listed.
    fn recently_bought(html: &str) -> Vec<String> {
        let Some((_, section)) = html.split_once(r#"id="recently-bought""#) else {
            return Vec::new();
        };
        section
            .split('<')
            .filter_map(item_name)
            .map(str::to_owned)
            .collect()
    }

    #[sqlx::test]
    async fn a_member_marks_an_item_bought_and_it_moves_to_recently_bought(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;
        add(&app, &josef, &[("name", "Saaz")]).await;
        add(&app, &josef, &[("name", "Star San"), ("urgency", "asap")]).await;
        app.clock.advance(std::time::Duration::from_hours(3));

        let html = pressed(&app, &ondra, 1, "bought").await;

        assert!(html.starts_with(r#"<div id="open-items">"#), "{html}");
        assert!(!html.contains("<h1>"), "a fragment, not the page");
        assert!(
            html.contains(r#"id="recently-bought" hx-swap-oob="true">"#),
            "folded: {html}"
        );
        let (open, _) = html.split_once(r#"id="recently-bought""#).unwrap();
        assert_eq!(grouped(open), [("ASAP".to_owned(), "Star San".to_owned())]);
        assert_eq!(recently_bought(&html), ["Saaz"]);
        assert!(html.contains(
            r#"Bought <time datetime="2026-01-01T04:00">Thursday 1 January 2026, 04:00</time> · Ondra"#
        ));
        // The whole Crew sees it too.
        let tab = tab(&app, &josef).await;
        assert_eq!(recently_bought(&tab), ["Saaz"]);
        assert!(tab.contains(r#"<details class="section recently-bought" id="recently-bought">"#));
    }

    #[sqlx::test]
    async fn undo_puts_an_item_back_in_its_group(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let simon = app.sign_in_new_member("Šimon").await;
        add(
            &app,
            &josef,
            &[("name", "Caps"), ("urgency", "nice-to-have")],
        )
        .await;
        add(&app, &josef, &[("name", "Saaz")]).await;
        pressed(&app, &josef, 1, "bought").await;

        let html = pressed(&app, &simon, 1, "undo").await;

        assert!(html.starts_with(r#"<div id="open-items">"#), "{html}");
        assert!(
            html.contains(r#"id="recently-bought" open hx-swap-oob="true">"#),
            "unfolded: {html}"
        );
        assert!(html.contains("Nothing bought in the last 7 days."));
        assert_eq!(
            grouped(&tab(&app, &josef).await),
            [
                ("Next brewday".to_owned(), "Saaz".to_owned()),
                ("Nice to have".to_owned(), "Caps".to_owned()),
            ]
        );
    }

    #[sqlx::test]
    async fn recently_bought_is_newest_first_and_reaches_back_seven_days(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        for name in ["Oldest", "Older", "Newest"] {
            add(&app, &cookie, &[("name", name)]).await;
        }
        for id in 1..=3 {
            pressed(&app, &cookie, id, "bought").await;
            app.clock.advance(std::time::Duration::from_hours(24));
        }
        assert_eq!(
            recently_bought(&tab(&app, &cookie).await),
            ["Newest", "Older", "Oldest"]
        );

        // "Oldest" was bought 7 days and a minute ago.
        app.clock
            .advance(std::time::Duration::from_hours(4 * 24) + std::time::Duration::from_mins(1));

        let html = tab(&app, &cookie).await;
        assert_eq!(recently_bought(&html), ["Newest", "Older"]);
        assert!(html.contains("Nothing to buy."), "hidden, not open again");
    }

    #[sqlx::test]
    async fn a_plain_post_goes_back_to_the_tab_after_buying_or_undoing(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        add(&app, &cookie, &[("name", "Saaz")]).await;

        for action in ["bought", "undo"] {
            let response = app
                .post_with(&format!("/shopping/1/{action}"), &cookie)
                .await;

            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/shopping");
        }
        assert_eq!(grouped(&tab(&app, &cookie).await).len(), 1);
    }

    #[sqlx::test]
    async fn an_item_that_isnt_there_to_buy_or_undo_is_not_found(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        add(&app, &cookie, &[("name", "Saaz")]).await;
        add(&app, &cookie, &[("name", "Caps")]).await;
        pressed(&app, &cookie, 2, "bought").await;

        for (id, action) in [(999, "bought"), (2, "bought"), (999, "undo"), (1, "undo")] {
            let response = press(&app, &cookie, id, action).await;

            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id} {action}");
            assert!(body_text(response).await.contains("Reload the page"));
        }
    }

    #[sqlx::test]
    async fn buying_and_undoing_need_a_member_and_our_origin(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        add(&app, &cookie, &[("name", "Saaz")]).await;

        for action in ["bought", "undo"] {
            let path = format!("/shopping/1/{action}");
            let response = app.post_with(&path, "").await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");

            let response = post_form(&app, &path, &cookie, &[], true, "https://evil.test").await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        assert_eq!(
            recently_bought(&tab(&app, &cookie).await),
            Vec::<String>::new()
        );
    }

    #[sqlx::test]
    async fn buying_and_undoing_announce_nothing(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        app.sign_in_new_member("Ondra").await;
        add(&app, &josef, &[("name", "Star San"), ("urgency", "asap")]).await;
        let mut events = app.state.events.subscribe();

        pressed(&app, &josef, 1, "bought").await;
        pressed(&app, &josef, 1, "undo").await;

        assert!(events.try_recv().is_err());
    }

    #[sqlx::test]
    async fn only_an_asap_item_is_announced(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        app.sign_in_new_member("Ondra").await;
        let mut events = app.state.events.subscribe();

        add(&app, &josef, &[("name", "Saaz")]).await;
        add(
            &app,
            &josef,
            &[("name", "Caps"), ("urgency", "nice-to-have")],
        )
        .await;
        add(&app, &josef, &[("name", "Star San"), ("urgency", "asap")]).await;

        let emitted: Vec<Event> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        let [Event::AsapItemAdded(added)] = emitted.as_slice() else {
            panic!("one ASAP Item announced: {emitted:?}");
        };
        assert_eq!(added.name.as_str(), "Star San");
        assert_eq!(added.notify.len(), 1, "everyone but Josef");
        assert_ne!(added.notify[0], added.added_by);
    }
}
