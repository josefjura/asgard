//! What `asgard` was asked to do on the command line: serve, or a one-off subcommand.

use url::Url;

use crate::domain::auth::{Auth, AuthError, LinkKind};
use crate::domain::members::DisplayName;
use crate::web::links;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    /// Prints the first Admin's Invite link.
    BootstrapAdmin {
        display_name: String,
    },
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("usage: asgard [bootstrap-admin <display name>]")]
pub struct UsageError;

impl Command {
    /// Reads the arguments that follow the program name.
    ///
    /// # Errors
    ///
    /// Returns [`UsageError`] for anything but no arguments or a known subcommand.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, UsageError> {
        let args: Vec<String> = args.into_iter().collect();
        match args.as_slice() {
            [] => Ok(Self::Serve),
            [command, display_name] if command == "bootstrap-admin" => Ok(Self::BootstrapAdmin {
                display_name: display_name.clone(),
            }),
            _ => Err(UsageError),
        }
    }
}

/// Creates the first Admin's Invite link and returns its full URL.
///
/// # Errors
///
/// Fails if the name isn't a valid display name or is taken, or if the Crew already has an
/// Admin.
pub async fn bootstrap_admin(
    auth: &Auth,
    public_url: &Url,
    display_name: &str,
) -> Result<Url, AuthError> {
    let name = DisplayName::parse(display_name)?;
    let token = auth.create_admin_invite(&name).await?;
    let mut link = public_url.clone();
    link.set_path(&links::path(LinkKind::Invite, &token));
    Ok(link)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use sqlx::SqlitePool;

    use super::*;
    use crate::domain::members::NameError;
    use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, body_text};

    fn parse(args: &[&str]) -> Result<Command, UsageError> {
        Command::parse(args.iter().map(|&arg| arg.to_owned()))
    }

    #[test]
    fn no_arguments_means_serve() {
        assert_eq!(parse(&[]), Ok(Command::Serve));
    }

    #[test]
    fn reads_the_bootstrap_subcommand() {
        assert_eq!(
            parse(&["bootstrap-admin", "Josef"]),
            Ok(Command::BootstrapAdmin {
                display_name: "Josef".to_owned()
            })
        );
    }

    #[test]
    fn refuses_anything_else() {
        for args in [
            &["bootstrap-admin"][..],
            &["bootstrap-admin", "Josef", "Jura"],
            &["serve"],
        ] {
            assert_eq!(parse(args), Err(UsageError), "{args:?}");
        }
    }

    #[sqlx::test]
    async fn prints_a_working_admin_invite_link(db: SqlitePool) {
        let app = TestApp::new(db);
        let public_url = Url::parse(PUBLIC_ORIGIN).unwrap();

        let link = bootstrap_admin(&app.state.auth, &public_url, " Josef ")
            .await
            .unwrap();

        assert_eq!(link.origin(), public_url.origin());
        let response = app.get(link.path()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_text(response).await.contains("<strong>Josef</strong>"));
    }

    #[sqlx::test]
    async fn refuses_an_invalid_display_name(db: SqlitePool) {
        let app = TestApp::new(db);
        let public_url = Url::parse(PUBLIC_ORIGIN).unwrap();

        let result = bootstrap_admin(&app.state.auth, &public_url, "  ").await;

        assert!(matches!(result, Err(AuthError::Name(NameError::Blank))));
    }
}
