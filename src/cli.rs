//! What `asgard` was asked to do on the command line: serve, or a one-off subcommand.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use url::Url;

use crate::domain::auth::{Auth, AuthError, LinkKind};
use crate::domain::clock::Clock;
use crate::domain::heimdall::{self, ImportError, ImportOptions, Imported, Rename};
use crate::domain::members::DisplayName;
use crate::web::links;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    /// Prints the first Admin's Invite link.
    BootstrapAdmin {
        display_name: String,
    },
    /// Imports Heimdall's database into a fresh Asgard, once, on the cutover day.
    ImportHeimdall {
        heimdall: PathBuf,
        options: ImportOptions,
    },
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "usage: asgard [bootstrap-admin <display name>]
       asgard import-heimdall <heimdall.db> --admin <display name> [--rename <Heimdall name>=<display name>]... [--dry-run]"
)]
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
            [command, rest @ ..] if command == "import-heimdall" => parse_import(rest),
            _ => Err(UsageError),
        }
    }
}

/// Reads `import-heimdall`'s arguments: Heimdall's database, a required `--admin`, any number
/// of `--rename`s and an optional `--dry-run`, in any order.
fn parse_import(args: &[String]) -> Result<Command, UsageError> {
    let mut heimdall = None;
    let mut admin = None;
    let mut renames = Vec::new();
    let mut dry_run = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--admin" => {
                let name = args.next().ok_or(UsageError)?;
                if admin.replace(name.clone()).is_some() {
                    return Err(UsageError);
                }
            }
            "--rename" => {
                let (from, to) = args
                    .next()
                    .and_then(|rename| rename.split_once('='))
                    .ok_or(UsageError)?;
                renames.push(Rename {
                    from: from.to_owned(),
                    to: to.to_owned(),
                });
            }
            "--dry-run" => dry_run = true,
            flag if flag.starts_with("--") => return Err(UsageError),
            path => {
                if heimdall.replace(PathBuf::from(path)).is_some() {
                    return Err(UsageError);
                }
            }
        }
    }
    Ok(Command::ImportHeimdall {
        heimdall: heimdall.ok_or(UsageError)?,
        options: ImportOptions {
            admin: admin.ok_or(UsageError)?,
            renames,
            dry_run,
        },
    })
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

/// What `import-heimdall` did: the import, and a Device link for each Member once it is
/// committed.
#[derive(Debug)]
pub struct ImportRun {
    pub imported: Imported,
    /// Empty unless the import was committed.
    pub device_links: Vec<(DisplayName, Url)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportFailure {
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error("the import is committed, but a Device link couldn't be made: {0}")]
    DeviceLink(#[from] AuthError),
}

/// Imports the Heimdall database at `heimdall` into `db`, then makes Device links.
///
/// Once the import is committed, the Admin gets a Device link for every imported Member, so the
/// whole Crew can register passkeys. A dry run or a failed verification makes none.
///
/// # Errors
///
/// Returns [`ImportFailure::Import`] if the import stopped before writing anything, or
/// [`ImportFailure::DeviceLink`] if it was committed but a link couldn't be made.
pub async fn import_heimdall(
    db: &SqlitePool,
    clock: &Clock,
    auth: &Auth,
    public_url: &Url,
    heimdall: &Path,
    options: &ImportOptions,
) -> Result<ImportRun, ImportFailure> {
    let imported = heimdall::import(db, clock, heimdall, options).await?;
    let mut device_links = Vec::new();
    if imported.committed {
        for member in &imported.members {
            let (token, name) = auth.create_device_link(member.id, imported.admin).await?;
            let mut link = public_url.clone();
            link.set_path(&links::path(LinkKind::Device, &token));
            device_links.push((name, link));
        }
    }
    Ok(ImportRun {
        imported,
        device_links,
    })
}

#[cfg(test)]
mod import_tests;

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
    fn reads_the_import_subcommand_with_its_options_in_any_order() {
        assert_eq!(
            parse(&[
                "import-heimdall",
                "--rename",
                "josef_j=Josef",
                "/data/heimdall.db",
                "--dry-run",
                "--admin",
                "Josef",
                "--rename",
                "ondrej_k=Ondra = Ondřej",
            ]),
            Ok(Command::ImportHeimdall {
                heimdall: PathBuf::from("/data/heimdall.db"),
                options: ImportOptions {
                    admin: "Josef".to_owned(),
                    renames: vec![
                        Rename {
                            from: "josef_j".to_owned(),
                            to: "Josef".to_owned()
                        },
                        Rename {
                            from: "ondrej_k".to_owned(),
                            to: "Ondra = Ondřej".to_owned()
                        },
                    ],
                    dry_run: true,
                },
            })
        );
        assert_eq!(
            parse(&["import-heimdall", "heimdall.db", "--admin", "Josef"]),
            Ok(Command::ImportHeimdall {
                heimdall: PathBuf::from("heimdall.db"),
                options: ImportOptions {
                    admin: "Josef".to_owned(),
                    renames: Vec::new(),
                    dry_run: false,
                },
            })
        );
    }

    #[test]
    fn refuses_anything_else() {
        for args in [
            &["bootstrap-admin"][..],
            &["bootstrap-admin", "Josef", "Jura"],
            &["serve"],
            &["import-heimdall", "--admin", "Josef"],
            &["import-heimdall", "heimdall.db"],
            &["import-heimdall", "heimdall.db", "--admin"],
            &["import-heimdall", "a.db", "b.db", "--admin", "Josef"],
            &[
                "import-heimdall",
                "heimdall.db",
                "--admin",
                "Josef",
                "--admin",
                "Ondra",
            ],
            &[
                "import-heimdall",
                "heimdall.db",
                "--admin",
                "Josef",
                "--rename",
                "Ondra",
            ],
            &[
                "import-heimdall",
                "heimdall.db",
                "--admin",
                "Josef",
                "--force",
            ],
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
