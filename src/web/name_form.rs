//! The display-name form the welcome page and settings share.

use axum::http::StatusCode;
use axum::response::Response;
use maud::Markup;
use serde::Deserialize;

use super::error::name_message;
use super::htmx::fragment_or_page;
use super::{AppError, AppState};
use crate::domain::members::{DisplayName, Member, MemberError};
use crate::views::display_name::{self, Outcome};

#[derive(Deserialize)]
pub struct NameForm {
    display_name: String,
}

/// Renames `member` to what they typed and answers with the form posting to `action`: saved,
/// or refused with a 422 and the reason. htmx gets the form alone; a plain form post gets it
/// inside the whole page `page` renders.
///
/// # Errors
///
/// Fails if the database does.
pub async fn submit(
    state: &AppState,
    member: &Member,
    input: &NameForm,
    action: &str,
    htmx: bool,
    page: impl FnOnce(&Markup) -> Markup,
) -> Result<Response, AppError> {
    let (status, form) = answer(state, member, input, action).await?;
    Ok(fragment_or_page(htmx, status, form, page))
}

async fn answer(
    state: &AppState,
    member: &Member,
    input: &NameForm,
    action: &str,
) -> Result<(StatusCode, Markup), AppError> {
    let renamed = match DisplayName::parse(&input.display_name) {
        Ok(name) => state.members.rename(member.id, &name).await.map(|()| name),
        Err(err) => Err(err.into()),
    };
    match renamed {
        Ok(name) => Ok((
            StatusCode::OK,
            display_name::form(action, name.as_str(), Some(Outcome::Saved)),
        )),
        Err(MemberError::Name(err)) => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            display_name::form(
                action,
                &input.display_name,
                Some(Outcome::Refused(&name_message(&err))),
            ),
        )),
        Err(MemberError::Database(err)) => Err(err.into()),
    }
}
