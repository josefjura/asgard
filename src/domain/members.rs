//! Members of the Crew: their display names, the Admin flag and the date they joined.

use std::fmt;

use jiff::civil::Date;
use sqlx::{SqliteConnection, SqlitePool};

/// The name of the one Crew, shown to people it invites.
pub const CREW_NAME: &str = "Asgard";

/// The longest display name, in characters.
pub const DISPLAY_NAME_MAX: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemberId(i64);

impl MemberId {
    pub(in crate::domain) const fn from_stored(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A trimmed, non-blank display name of at most [`DISPLAY_NAME_MAX`] characters.
///
/// Whether the name is still free in the Crew is a separate check ([`Members::check_available`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayName(String);

impl DisplayName {
    /// Trims `input` and checks it is a usable display name.
    ///
    /// # Errors
    ///
    /// Returns [`NameError::Blank`] or [`NameError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, NameError> {
        let name = input.trim();
        if name.is_empty() {
            Err(NameError::Blank)
        } else if name.chars().count() > DISPLAY_NAME_MAX {
            Err(NameError::TooLong)
        } else {
            Ok(Self(name.to_owned()))
        }
    }

    /// A name read back from the database, which was checked when it was stored.
    pub(in crate::domain) const fn from_stored(name: String) -> Self {
        Self(name)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The form two names share when they differ only in case.
    fn key(&self) -> String {
        self.0.to_lowercase()
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the display name is blank")]
    Blank,
    #[error("the display name is longer than {DISPLAY_NAME_MAX} characters")]
    TooLong,
    #[error("another Member is already called {0}")]
    Taken(DisplayName),
}

#[derive(Debug, thiserror::Error)]
pub enum MemberError {
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: MemberId,
    pub display_name: DisplayName,
    pub is_admin: bool,
    /// From this date on, Expenses are shared with the Member.
    pub joined_on: Date,
}

/// Everything a new Member starts with.
pub struct NewMember<'a> {
    pub display_name: &'a DisplayName,
    pub is_admin: bool,
    pub joined_on: Date,
    /// The WebAuthn user handle their passkeys are registered under.
    pub webauthn_user_id: &'a str,
}

/// Finds and checks Members.
#[derive(Debug, Clone)]
pub struct Members {
    db: SqlitePool,
}

impl Members {
    #[must_use]
    pub const fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Refuses a name another Member already has, ignoring case.
    ///
    /// # Errors
    ///
    /// Returns [`NameError::Taken`], or a database error.
    pub async fn check_available(&self, name: &DisplayName) -> Result<(), MemberError> {
        let key = name.key();
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM members WHERE display_name_key = ?) AS "taken: bool""#,
            key
        )
        .fetch_one(&self.db)
        .await?;
        if taken {
            Err(NameError::Taken(name.clone()).into())
        } else {
            Ok(())
        }
    }

    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn find(&self, id: MemberId) -> Result<Option<Member>, sqlx::Error> {
        let row = sqlx::query!(
            r#"SELECT id, display_name, is_admin AS "is_admin: bool", joined_on
               FROM members WHERE id = ?"#,
            id.0
        )
        .fetch_optional(&self.db)
        .await?;
        row.map(|row| {
            Ok(Member {
                id: MemberId(row.id),
                display_name: DisplayName(row.display_name),
                is_admin: row.is_admin,
                joined_on: parse_date(&row.joined_on)?,
            })
        })
        .transpose()
    }

    /// The Admin's display name, once there is an Admin.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn admin_name(&self) -> Result<Option<DisplayName>, sqlx::Error> {
        let name = sqlx::query_scalar!(
            "SELECT display_name FROM members WHERE is_admin ORDER BY id LIMIT 1"
        )
        .fetch_optional(&self.db)
        .await?;
        Ok(name.map(DisplayName::from_stored))
    }
}

/// Adds a Member as part of a larger transaction.
///
/// # Errors
///
/// Returns [`NameError::Taken`] if another Member got the name first, or a database error.
pub async fn insert(
    conn: &mut SqliteConnection,
    member: NewMember<'_>,
) -> Result<MemberId, MemberError> {
    let key = member.display_name.key();
    let joined_on = member.joined_on.to_string();
    let result = sqlx::query!(
        "INSERT INTO members (display_name, display_name_key, is_admin, joined_on, webauthn_user_id)
         VALUES (?, ?, ?, ?, ?)",
        member.display_name.0,
        key,
        member.is_admin,
        joined_on,
        member.webauthn_user_id,
    )
    .execute(conn)
    .await;
    match result {
        Ok(done) => Ok(MemberId(done.last_insert_rowid())),
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            Err(NameError::Taken(member.display_name.clone()).into())
        }
        Err(err) => Err(err.into()),
    }
}

fn parse_date(value: &str) -> Result<Date, sqlx::Error> {
    value
        .parse()
        .map_err(|err: jiff::Error| sqlx::Error::Decode(err.into()))
}

/// Test data shared by the domain's tests.
#[cfg(test)]
pub(crate) mod fixtures {
    use jiff::civil::date;
    use sqlx::SqlitePool;

    use super::{DisplayName, MemberId, NewMember, insert};

    pub fn name(input: &str) -> DisplayName {
        DisplayName::parse(input).unwrap()
    }

    /// Adds a Member who joined on 2026-03-14.
    pub async fn add_member(db: &SqlitePool, display_name: &str, is_admin: bool) -> MemberId {
        let mut conn = db.acquire().await.unwrap();
        insert(
            &mut conn,
            NewMember {
                display_name: &name(display_name),
                is_admin,
                joined_on: date(2026, 3, 14),
                webauthn_user_id: &format!("handle-{display_name}"),
            },
        )
        .await
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::fixtures::{add_member, name};
    use super::*;

    #[test]
    fn trims_a_display_name() {
        assert_eq!(name("  Josef \t").as_str(), "Josef");
    }

    #[test]
    fn refuses_a_blank_display_name() {
        for input in ["", "   ", "\t\n"] {
            assert_eq!(
                DisplayName::parse(input),
                Err(NameError::Blank),
                "{input:?}"
            );
        }
    }

    #[test]
    fn limits_a_display_name_by_characters_not_bytes() {
        let longest = "Ř".repeat(DISPLAY_NAME_MAX);

        assert_eq!(name(&longest).as_str(), longest);
        assert_eq!(
            DisplayName::parse(&format!("{longest}x")),
            Err(NameError::TooLong)
        );
    }

    #[test]
    fn the_length_limit_applies_after_trimming() {
        let padded = format!("  {}  ", "a".repeat(DISPLAY_NAME_MAX));

        assert!(DisplayName::parse(&padded).is_ok());
    }

    #[sqlx::test]
    async fn a_free_name_is_available(db: SqlitePool) {
        add_member(&db, "Josef", true).await;

        Members::new(db)
            .check_available(&name("Ondra"))
            .await
            .unwrap();
    }

    #[sqlx::test]
    async fn refuses_a_name_taken_ignoring_case(db: SqlitePool) {
        add_member(&db, "Šimon", false).await;
        let members = Members::new(db);

        for input in ["Šimon", "šimon", "ŠIMON"] {
            let result = members.check_available(&name(input)).await;

            assert!(
                matches!(result, Err(MemberError::Name(NameError::Taken(ref taken))) if taken.as_str() == input),
                "{input}: {result:?}"
            );
        }
    }

    #[sqlx::test]
    async fn the_database_refuses_a_duplicate_that_slipped_past_the_check(db: SqlitePool) {
        add_member(&db, "Josef", false).await;
        let mut conn = db.acquire().await.unwrap();

        let result = insert(
            &mut conn,
            NewMember {
                display_name: &name("JOSEF"),
                is_admin: false,
                joined_on: date(2026, 3, 14),
                webauthn_user_id: "another-handle",
            },
        )
        .await;

        assert!(matches!(
            result,
            Err(MemberError::Name(NameError::Taken(_)))
        ));
    }

    #[sqlx::test]
    async fn finds_a_member_as_they_joined(db: SqlitePool) {
        let id = add_member(&db, "Josef", true).await;

        let member = Members::new(db).find(id).await.unwrap().unwrap();

        assert_eq!(
            member,
            Member {
                id,
                display_name: name("Josef"),
                is_admin: true,
                joined_on: date(2026, 3, 14),
            }
        );
    }

    #[sqlx::test]
    async fn names_the_admin_once_there_is_one(db: SqlitePool) {
        let members = Members::new(db.clone());
        assert_eq!(members.admin_name().await.unwrap(), None);

        add_member(&db, "Ondra", false).await;
        add_member(&db, "Josef", true).await;

        assert_eq!(members.admin_name().await.unwrap(), Some(name("Josef")));
    }
}
