//! The Quotes tab: a random Quote, adding a Quote to the Crew's collection, and reading it a
//! page at a time.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Form, Router};
use maud::Markup;
use serde::Deserialize;

use super::access::Member;
use super::htmx::Htmx;
use super::pages::fresh;
use super::{AppError, AppState};
use crate::domain::quotes::{
    AUTHOR_MAX, Author, AuthorError, NewQuote, QuoteText, TEXT_MAX, TextError,
};
use crate::views;
use crate::views::nav::Tab;
use crate::views::quotes::paths;
use crate::views::quotes::{QuoteProblems, QuoteValues};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(Tab::Quotes.href(), get(show).post(add))
        .route(paths::NEW, get(new_form))
        .route(paths::RANDOM, get(random))
}

/// Which page of the collection to show. Missing means the first.
#[derive(Deserialize)]
struct PageQuery {
    page: Option<usize>,
}

/// The Quotes tab: a random Quote, "+ Quote" and one page of the collection. A page past the last shows the last.
async fn show(
    State(state): State<AppState>,
    Member(_): Member,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, AppError> {
    Ok(fresh(
        page(
            &state,
            &views::quotes::add_button(),
            query.page.unwrap_or(1),
        )
        .await?,
    ))
}

#[derive(Deserialize)]
struct QuoteForm {
    #[serde(default)]
    text: String,
    #[serde(default)]
    author: String,
}

impl QuoteForm {
    fn values(&self) -> QuoteValues<'_> {
        QuoteValues {
            text: &self.text,
            author: &self.author,
        }
    }
}

/// The add form, swapped in place of "+ Quote". Only htmx asks for it; a browser is sent back
/// to the tab.
async fn new_form(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
) -> Result<Response, AppError> {
    if !htmx {
        return Ok(Redirect::to(Tab::Quotes.href()).into_response());
    }
    Ok(fresh(empty_form(&state).await?).into_response())
}

/// Adds the Quote. htmx gets "+ Quote" back and the first page of the collection, with the new
/// Quote at the top. A plain form post goes back to the tab. A refused form shows what to fix:
/// alone for htmx, inside the tab otherwise.
async fn add(
    State(state): State<AppState>,
    Member(member): Member,
    Htmx(htmx): Htmx,
    Form(input): Form<QuoteForm>,
) -> Result<Response, AppError> {
    let quote = match parse(&input) {
        Ok(quote) => quote,
        Err(problems) => {
            let authors = state.quotes.authors().await?;
            let form = views::quotes::form(input.values(), &problems, &authors);
            let body = if htmx {
                form
            } else {
                page(&state, &form, 1).await?
            };
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, fresh(body)).into_response());
        }
    };
    state.quotes.add(member.id, &quote).await?;
    tracing::info!(member = member.id.get(), "a Member added a Quote");
    if !htmx {
        return Ok(Redirect::to(Tab::Quotes.href()).into_response());
    }
    let list = state.quotes.page(1).await?;
    Ok(fresh(views::quotes::added(&list)).into_response())
}

/// A fresh random-Quote card, for "Another one". Only htmx asks for it; a browser is sent to the
/// tab, which picks one too.
async fn random(
    State(state): State<AppState>,
    Member(_): Member,
    Htmx(htmx): Htmx,
) -> Result<Response, AppError> {
    if !htmx {
        return Ok(Redirect::to(Tab::Quotes.href()).into_response());
    }
    let quote = state.quotes.random().await?;
    Ok(fresh(views::quotes::random_card(quote.as_ref(), false)).into_response())
}

/// The Quote as typed, or what is wrong with each field.
fn parse(input: &QuoteForm) -> Result<NewQuote, QuoteProblems> {
    match (QuoteText::parse(&input.text), Author::parse(&input.author)) {
        (Ok(text), Ok(author)) => Ok(NewQuote { text, author }),
        (text, author) => Err(QuoteProblems {
            text: text.err().map(text_message),
            author: author.err().map(author_message),
        }),
    }
}

/// The Quotes tab around `adder`, showing a random Quote and the collection's `page`.
async fn page(state: &AppState, adder: &Markup, page_number: usize) -> Result<Markup, AppError> {
    let random = state.quotes.random().await?;
    let list = state.quotes.page(page_number).await?;
    Ok(views::quotes::page(random.as_ref(), adder, &list))
}

async fn empty_form(state: &AppState) -> Result<Markup, AppError> {
    let authors = state.quotes.authors().await?;
    Ok(views::quotes::form(
        QuoteValues::default(),
        &QuoteProblems::default(),
        &authors,
    ))
}

fn text_message(err: TextError) -> String {
    match err {
        TextError::Blank => "Write the Quote.".to_owned(),
        TextError::TooLong => format!("Keep the Quote to {TEXT_MAX} characters or fewer."),
    }
}

fn author_message(err: AuthorError) -> String {
    match err {
        AuthorError::Blank => "Say who said it.".to_owned(),
        AuthorError::TooLong => format!("Keep the Author to {AUTHOR_MAX} characters or fewer."),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::header::{COOKIE, LOCATION};
    use axum::http::{Method, Request, Response, StatusCode};
    use sqlx::SqlitePool;

    use crate::domain::quotes::{
        AUTHOR_MAX, Author, NewQuote, PAGE_SIZE, QuoteText, TEXT_MAX, fold,
    };
    use crate::views::quotes::paths;
    use crate::web::htmx::{HX_REDIRECT, HX_REQUEST};
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text, post_form, request};

    /// Posts the Quote form with `fields`, as htmx does if `htmx`.
    async fn post_quote(
        app: &TestApp,
        cookie: &str,
        fields: &[(&str, &str)],
        htmx: bool,
    ) -> Response<Body> {
        let fields: Vec<(&str, String)> = fields
            .iter()
            .map(|&(name, value)| (name, value.to_owned()))
            .collect();
        post_form(app, "/quotes", cookie, &fields, htmx, PUBLIC_ORIGIN).await
    }

    /// Adds a Quote with htmx, expecting it added, and answers with what came back.
    async fn add(app: &TestApp, cookie: &str, text: &str, author: &str) -> String {
        let response = post_quote(app, cookie, &[("text", text), ("author", author)], true).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// Posts the Quote form, expecting it refused, and answers with what came back.
    async fn refused(app: &TestApp, cookie: &str, fields: &[(&str, &str)], htmx: bool) -> String {
        let response = post_quote(app, cookie, fields, htmx).await;
        let status = response.status();
        let body = body_text(response).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        body
    }

    async fn tab(app: &TestApp, cookie: &str) -> String {
        let response = app.get_with("/quotes", cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    async fn tab_at(app: &TestApp, cookie: &str, page: usize) -> String {
        let response = app.get_with(&format!("/quotes?page={page}"), cookie).await;
        assert_eq!(response.status(), StatusCode::OK);
        body_text(response).await
    }

    fn new_quote(text: &str) -> NewQuote {
        NewQuote {
            text: QuoteText::parse(text).unwrap(),
            author: Author::parse("Pepa").unwrap(),
        }
    }

    fn quote_texts(html: &str) -> Vec<&str> {
        html.split("<p class=\"quote-text\">")
            .skip(1)
            .map(|rest| rest.split("</p>").next().unwrap_or_default())
            .collect()
    }

    /// A GET htmx makes, from a browser that holds `cookie`.
    fn htmx_get(uri: &str, cookie: &str) -> Request<Body> {
        let mut get = request(Method::GET, uri, None);
        let headers = get.headers_mut();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(HX_REQUEST, "true".parse().unwrap());
        get
    }

    #[sqlx::test]
    async fn signed_out_quotes_redirect_to_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);

        for response in [
            app.get("/quotes").await,
            app.get(paths::NEW).await,
            app.get(paths::RANDOM).await,
            post_quote(&app, "", &[("text", "Hello"), ("author", "Pepa")], false).await,
        ] {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[LOCATION], "/sign-in");
        }
    }

    #[sqlx::test]
    async fn signed_out_htmx_is_told_to_load_sign_in(db: SqlitePool) {
        let app = TestApp::new(db);
        let mut post = request(Method::POST, "/quotes", Some(PUBLIC_ORIGIN));
        post.headers_mut()
            .insert(HX_REQUEST, "true".parse().unwrap());

        let response = app.send(post).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[HX_REDIRECT], "/sign-in");
    }

    #[sqlx::test]
    async fn a_new_tab_offers_to_add_a_quote_and_has_none(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = tab(&app, &cookie).await;

        assert!(html.contains(r#"hx-get="/quotes/new""#), "{html}");
        assert!(html.contains(">+ Quote</a>"));
        assert!(html.contains("No Quotes yet."));
        assert!(!html.contains("will show up here"), "not the placeholder");
    }

    #[sqlx::test]
    async fn adding_a_quote_shows_it_at_the_top_with_its_author_and_who_added_it(db: SqlitePool) {
        let app = TestApp::new(db);
        let josef = app.sign_in_new_admin("Josef").await;
        let ondra = app.sign_in_new_member("Ondra").await;

        add(&app, &josef, "Older words", "Pepa").await;
        app.clock.advance(std::time::Duration::from_secs(60));
        let fragment = add(&app, &ondra, "  To pivo je spíš polévka. ", " Pepa ").await;

        assert!(!fragment.contains("<!DOCTYPE"), "a fragment, not the page");
        assert!(fragment.contains(">+ Quote</a>"), "{fragment}");
        assert!(
            fragment.contains(r#"id="quote-list" hx-swap-oob="true""#),
            "{fragment}"
        );
        let texts = quote_texts(&fragment);
        assert_eq!(texts.first().copied(), Some("To pivo je spíš polévka."));
        assert_eq!(texts.get(1).copied(), Some("Older words"));
        assert!(fragment.contains(
            r#"Added by Ondra · <time datetime="2026-01-01T01:01">Thursday 1 January 2026, 01:01</time>"#
        ), "{fragment}");
        assert!(fragment.contains("Added by Josef"));
    }

    #[sqlx::test]
    async fn the_add_form_is_the_fragment_htmx_asks_for(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let plain = app.get_with(paths::NEW, &cookie).await;
        assert_eq!(plain.status(), StatusCode::SEE_OTHER);
        assert_eq!(plain.headers()[LOCATION], "/quotes");

        let response = app.send(htmx_get(paths::NEW, &cookie)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;
        assert!(html.contains(r#"<form class="entry-form""#), "{html}");
        assert!(!html.contains("<!DOCTYPE"), "{html}");
    }

    #[sqlx::test]
    async fn a_plain_post_comes_back_to_the_tab_with_the_quote_on_top(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = post_quote(
            &app,
            &cookie,
            &[("text", "Hello there"), ("author", "Pepa")],
            false,
        )
        .await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/quotes");
        let html = tab(&app, &cookie).await;
        assert_eq!(quote_texts(&html), ["Hello there"]);
        assert!(html.contains("Added by Josef"));
        assert!(html.contains("Thursday 1 January 2026, 01:00"));
    }

    #[sqlx::test]
    async fn line_breaks_in_a_quote_are_kept(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let fragment = add(&app, &cookie, "line one\nline two", "Pepa").await;

        assert!(fragment.contains("line one\nline two"), "{fragment}");
    }

    #[sqlx::test]
    async fn a_missing_or_overlong_quote_or_author_is_refused_and_kept(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let too_long_text = format!("{}!", "š".repeat(TEXT_MAX));
        let too_long_author = "ř".repeat(AUTHOR_MAX + 1);

        let blank = refused(&app, &cookie, &[("text", " \n "), ("author", " ")], true).await;
        assert!(blank.contains("Write the Quote."), "{blank}");
        assert!(blank.contains("Say who said it."), "{blank}");
        assert!(blank.contains(" \n "), "{blank}");
        assert!(!blank.contains("<!DOCTYPE"));

        let long_text = refused(
            &app,
            &cookie,
            &[("text", &too_long_text), ("author", "Pepa")],
            true,
        )
        .await;
        assert!(
            long_text.contains(&format!(
                "Keep the Quote to {TEXT_MAX} characters or fewer."
            )),
            "{long_text}"
        );
        assert!(long_text.contains(&too_long_text));

        let long_author = refused(
            &app,
            &cookie,
            &[
                ("text", "first line\nsecond line"),
                ("author", &too_long_author),
            ],
            false,
        )
        .await;
        assert!(long_author.contains("<!DOCTYPE"), "the whole tab");
        assert!(long_author.contains(&format!(
            "Keep the Author to {AUTHOR_MAX} characters or fewer."
        )));
        assert!(
            long_author.contains("first line\nsecond line"),
            "{long_author}"
        );
        assert!(long_author.contains(&too_long_author));
        assert!(tab(&app, &cookie).await.contains("No Quotes yet."));
    }

    #[sqlx::test]
    async fn the_author_field_suggests_each_distinct_author_once(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        for author in ["Pepa", "Ondra", "Pepa", "pepa"] {
            add(&app, &cookie, "Something memorable", author).await;
        }

        let response = app.send(htmx_get(paths::NEW, &cookie)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let html = body_text(response).await;

        assert!(!html.contains("<!DOCTYPE"), "just the form");
        assert_eq!(html.matches("value=\"Pepa\"").count(), 1, "{html}");
        assert_eq!(html.matches("value=\"pepa\"").count(), 1, "{html}");
        assert_eq!(html.matches("value=\"Ondra\"").count(), 1, "{html}");
        let ondra = html.find("value=\"Ondra\"").unwrap();
        let pepa = html.find("value=\"Pepa\"").unwrap();
        let lower = html.find("value=\"pepa\"").unwrap();
        assert!(ondra < pepa && pepa < lower, "{html}");
    }

    #[sqlx::test]
    async fn the_list_is_newest_first_and_a_page_past_the_last_is_the_last(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let member = app.state.members.all().await.unwrap()[0].id;
        for n in 1..=PAGE_SIZE + 1 {
            app.state
                .quotes
                .add(member, &new_quote(&format!("Quote {n:02}")))
                .await
                .unwrap();
            app.clock.advance(std::time::Duration::from_secs(1));
        }

        let first = tab(&app, &cookie).await;
        let texts = quote_texts(&first);
        assert_eq!(texts.len(), PAGE_SIZE);
        assert_eq!(texts[0], "Quote 26");
        assert!(first.contains(r#"href="/quotes?page=2""#), "{first}");
        assert!(first.contains("Page 1 of 2"));

        let last = tab_at(&app, &cookie, 2).await;
        assert_eq!(quote_texts(&last), ["Quote 01"]);
        assert!(last.contains("Page 2 of 2"));
        assert!(last.contains(r#"rel="prev" href="/quotes""#), "{last}");

        let past = tab_at(&app, &cookie, 99).await;
        assert_eq!(quote_texts(&past), ["Quote 01"]);
        assert!(past.contains("Page 2 of 2"));
    }

    /// The text of the random-Quote card in `html`, if it shows one.
    fn random_text(html: &str) -> Option<&str> {
        let rest = html
            .split("<blockquote class=\"random-quote-text\">")
            .nth(1)?;
        rest.split("</blockquote>").next()
    }

    #[sqlx::test]
    async fn the_tab_shows_a_random_quote_above_the_adder(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        add(
            &app,
            &cookie,
            "line one
line two",
            "Pepa",
        )
        .await;

        let html = tab(&app, &cookie).await;

        assert_eq!(
            random_text(&html),
            Some(
                "line one
line two"
            ),
            "{html}"
        );
        let card = html.find(r#"id="random-quote""#).unwrap();
        let adder = html.find(r#"id="quote-adder""#).unwrap();
        assert!(card < adder, "{html}");
        assert!(html.contains(">Another one</a>"));
    }

    #[sqlx::test]
    async fn without_quotes_the_card_is_empty_and_offers_no_other(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let html = tab(&app, &cookie).await;

        assert!(html.contains("No Quotes to pick from yet."), "{html}");
        assert_eq!(random_text(&html), None);
        assert!(!html.contains("Another one"));
    }

    #[sqlx::test]
    async fn another_one_is_just_a_card_of_a_stored_quote(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let stored = ["First", "Second", "Third"];
        for text in stored {
            add(&app, &cookie, text, "Pepa").await;
        }

        for _ in 0..10 {
            let response = app.send(htmx_get(paths::RANDOM, &cookie)).await;
            assert_eq!(response.status(), StatusCode::OK);
            let html = body_text(response).await;
            assert!(!html.contains("<!DOCTYPE"), "a fragment, not the page");
            assert!(html.starts_with(r#"<section class="card random-quote" id="random-quote""#));
            assert!(!html.contains("quote-list"), "{html}");
            let text = random_text(&html).unwrap();
            assert!(stored.contains(&text), "{html}");
        }
    }

    #[sqlx::test]
    async fn another_one_without_htmx_reloads_the_tab(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let response = app.get_with(paths::RANDOM, &cookie).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[LOCATION], "/quotes");
    }

    #[sqlx::test]
    async fn adding_the_first_quote_puts_it_on_the_card(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;

        let first = add(&app, &cookie, "The very first", "Pepa").await;
        assert_eq!(random_text(&first), Some("The very first"), "{first}");

        let second = add(&app, &cookie, "Another", "Pepa").await;
        assert_eq!(random_text(&second), None, "{second}");
    }

    #[sqlx::test]
    async fn a_quote_without_added_by_renders_without_that_line(db: SqlitePool) {
        let app = TestApp::new(db);
        let cookie = app.sign_in_new_admin("Josef").await;
        let text = "Imported from the old book";
        let author = "A friend";
        let text_search = fold(text);
        let author_search = fold(author);
        let added_at = 1_767_225_600_i64;
        sqlx::query!(
            "INSERT INTO quotes (text, author, added_at, text_search, author_search)
             VALUES (?, ?, ?, ?, ?)",
            text,
            author,
            added_at,
            text_search,
            author_search,
        )
        .execute(&app.state.db)
        .await
        .unwrap();

        let html = tab(&app, &cookie).await;

        assert!(html.contains(text), "{html}");
        assert!(html.contains(author));
        assert!(!html.contains("Added by"), "{html}");
    }
}
