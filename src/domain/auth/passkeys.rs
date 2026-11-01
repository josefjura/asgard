//! A Member's own passkeys: adding another while signed in, naming, listing and deleting them.

use std::fmt;

use jiff::civil::Date;
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, PasskeyRegistration, RegisterPublicKeyCredential, Uuid,
};

use super::{Auth, AuthError, CeremonyId, insert_passkey, require_discoverable};
use crate::domain::clock::{crew_date_of_unix_seconds, unix_seconds};
use crate::domain::members::MemberId;
use crate::domain::user_agent;

/// The longest passkey name, in characters.
pub const PASSKEY_NAME_MAX: usize = 40;

/// What a passkey is called when its browser can't be told from its user agent.
const FALLBACK_NAME: &str = "Passkey";

/// A stored passkey, which the sessions it starts point back at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasskeyId(i64);

impl PasskeyId {
    pub(in crate::domain) const fn from_stored(id: i64) -> Self {
        Self(id)
    }

    /// An id a client sent back. Nothing about it is trusted until the Member's passkey is found.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A trimmed, non-blank passkey name of at most [`PASSKEY_NAME_MAX`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyName(String);

impl PasskeyName {
    /// Trims `input` and checks it is a usable passkey name.
    ///
    /// # Errors
    ///
    /// Returns [`PasskeyNameError::Blank`] or [`PasskeyNameError::TooLong`].
    pub fn parse(input: &str) -> Result<Self, PasskeyNameError> {
        let name = input.trim();
        if name.is_empty() {
            Err(PasskeyNameError::Blank)
        } else if name.chars().count() > PASSKEY_NAME_MAX {
            Err(PasskeyNameError::TooLong)
        } else {
            Ok(Self(name.to_owned()))
        }
    }

    /// A default name for a passkey made in the browser that sent `user_agent`, such as
    /// "iPhone Safari": the device, the browser, or both when they can be told.
    #[must_use]
    pub fn from_user_agent(user_agent: &str) -> Self {
        Self(user_agent::describe(user_agent).unwrap_or_else(|| FALLBACK_NAME.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PasskeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasskeyNameError {
    #[error("the passkey name is blank")]
    Blank,
    #[error("the passkey name is longer than {PASSKEY_NAME_MAX} characters")]
    TooLong,
}

/// One of a Member's passkeys, as their settings list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeySummary {
    pub id: PasskeyId,
    pub name: PasskeyName,
    /// The date in the Crew's time zone it was registered.
    pub added_on: Date,
}

/// A passkey being added to a signed-in Member, waiting for the browser's new credential.
pub(super) struct PendingAdd {
    member: MemberId,
    registration: PasskeyRegistration,
}

impl Auth {
    /// The Member's passkeys, oldest first.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn passkeys(&self, member: MemberId) -> Result<Vec<PasskeySummary>, sqlx::Error> {
        let member = member.get();
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", name, added_at FROM passkeys
               WHERE member_id = ? ORDER BY added_at, id"#,
            member,
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| PasskeySummary {
                id: PasskeyId(row.id),
                name: PasskeyName(row.name),
                added_on: crew_date_of_unix_seconds(row.added_at),
            })
            .collect())
    }

    /// Renames one of the Member's passkeys.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::NoSuchPasskey`] if the Member has no passkey `id`.
    pub async fn rename_passkey(
        &self,
        member: MemberId,
        id: PasskeyId,
        name: &PasskeyName,
    ) -> Result<(), AuthError> {
        let member = member.get();
        let renamed = sqlx::query!(
            "UPDATE passkeys SET name = ? WHERE id = ? AND member_id = ?",
            name.0,
            id.0,
            member,
        )
        .execute(&self.db)
        .await?;
        if renamed.rows_affected() == 0 {
            return Err(AuthError::NoSuchPasskey);
        }
        Ok(())
    }

    /// Deletes one of the Member's passkeys, and with it every session it started, so a lost
    /// device is signed out. Returns the deleted passkey's name.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::NoSuchPasskey`] if the Member has no passkey `id`, and
    /// [`AuthError::LastPasskey`] if it is their only one.
    pub async fn delete_passkey(
        &self,
        member: MemberId,
        id: PasskeyId,
    ) -> Result<PasskeyName, AuthError> {
        let member = member.get();
        // One statement, so two deletes at once can't remove a Member's last two passkeys.
        // The sessions it started go with it (ON DELETE CASCADE).
        let deleted = sqlx::query_scalar!(
            "DELETE FROM passkeys
             WHERE id = ?1 AND member_id = ?2
               AND (SELECT COUNT(*) FROM passkeys WHERE member_id = ?2) > 1
             RETURNING name",
            id.0,
            member,
        )
        .fetch_optional(&self.db)
        .await?;
        if let Some(name) = deleted {
            return Ok(PasskeyName(name));
        }
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM passkeys WHERE id = ? AND member_id = ?) AS "exists: bool""#,
            id.0,
            member,
        )
        .fetch_one(&self.db)
        .await?;
        Err(if exists {
            AuthError::LastPasskey
        } else {
            AuthError::NoSuchPasskey
        })
    }

    /// Starts registering another passkey for a signed-in Member. A device that already has
    /// one of their passkeys is asked not to make a second.
    ///
    /// # Errors
    ///
    /// Fails if the database or WebAuthn does.
    pub async fn start_adding_passkey(
        &self,
        member: MemberId,
    ) -> Result<(CeremonyId, CreationChallengeResponse), AuthError> {
        let (challenge, registration) = self.start_registration(member).await?;
        let ceremony = self.adding.insert(
            self.clock.now(),
            PendingAdd {
                member,
                registration,
            },
        );
        Ok((ceremony, challenge))
    }

    /// The challenge for another passkey for `member`, under their user handle, excluding the
    /// passkeys they already have.
    pub(super) async fn start_registration(
        &self,
        member: MemberId,
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration), AuthError> {
        let member_id = member.get();
        let row = sqlx::query!(
            "SELECT display_name, webauthn_user_id FROM members WHERE id = ?",
            member_id,
        )
        .fetch_one(&self.db)
        .await?;
        let webauthn_user_id = Uuid::parse_str(&row.webauthn_user_id)
            .map_err(|err| sqlx::Error::Decode(err.into()))?;
        let existing: Vec<CredentialID> = sqlx::query_scalar!(
            "SELECT credential_id FROM passkeys WHERE member_id = ?",
            member_id,
        )
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(CredentialID::from)
        .collect();
        let name = row.display_name.as_str();
        let (mut challenge, registration) = self
            .webauthn
            .start_passkey_registration(webauthn_user_id, name, name, Some(existing))
            .map_err(AuthError::Webauthn)?;
        require_discoverable(&mut challenge);
        Ok((challenge, registration))
    }

    /// Verifies the new passkey and stores it under `name` for the Member who started the
    /// ceremony.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::CeremonyExpired`] for an unknown or expired ceremony, or one another
    /// Member started, and [`AuthError::PasskeyRejected`] if the credential doesn't verify.
    pub async fn finish_adding_passkey(
        &self,
        member: MemberId,
        ceremony: &CeremonyId,
        credential: &RegisterPublicKeyCredential,
        name: &PasskeyName,
    ) -> Result<PasskeyId, AuthError> {
        let now = self.clock.now();
        let adding = self
            .adding
            .take(ceremony, now)
            .filter(|adding| adding.member == member)
            .ok_or(AuthError::CeremonyExpired)?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &adding.registration)
            .map_err(AuthError::PasskeyRejected)?;
        let mut conn = self.db.acquire().await?;
        insert_passkey(&mut conn, member, &passkey, name, unix_seconds(now)).await
    }
}

/// Test data for the passkey tests that don't run a WebAuthn ceremony.
#[cfg(test)]
pub mod fixtures {
    use sqlx::SqlitePool;

    use super::PasskeyId;
    use crate::domain::members::MemberId;

    /// Stores a passkey row for `member` without a usable credential, added at `added_at`.
    pub async fn add_passkey(
        db: &SqlitePool,
        member: MemberId,
        name: &str,
        added_at: i64,
    ) -> PasskeyId {
        let member = member.get();
        let credential_id = format!("credential-{member}-{name}").into_bytes();
        let id = sqlx::query!(
            "INSERT INTO passkeys (member_id, credential_id, credential, name, added_at)
             VALUES (?, ?, '{}', ?, ?)",
            member,
            credential_id,
            name,
            added_at,
        )
        .execute(db)
        .await
        .unwrap()
        .last_insert_rowid();
        PasskeyId(id)
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use sqlx::SqlitePool;

    use super::fixtures::add_passkey;
    use super::*;
    use crate::domain::auth::tests::auth;
    use crate::domain::members::fixtures::add_member;

    /// 2026-03-14T12:00:00Z.
    const MARCH_14: i64 = 1_773_489_600;

    fn named(input: &str) -> PasskeyName {
        PasskeyName::parse(input).unwrap()
    }

    #[test]
    fn trims_a_passkey_name() {
        assert_eq!(named("  Old iPhone \t").as_str(), "Old iPhone");
    }

    #[test]
    fn refuses_a_blank_passkey_name() {
        assert_eq!(PasskeyName::parse(" "), Err(PasskeyNameError::Blank));
    }

    #[test]
    fn refuses_a_passkey_name_that_is_too_long() {
        let longest = "ř".repeat(PASSKEY_NAME_MAX);

        assert_eq!(named(&longest).as_str(), longest);
        assert_eq!(
            PasskeyName::parse(&format!("{longest}x")),
            Err(PasskeyNameError::TooLong)
        );
    }

    #[test]
    fn names_a_passkey_after_the_device_and_browser() {
        for (user_agent, expected) in [
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1",
                "iPhone Safari",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/138.0.7204.156 Mobile/15E148 Safari/604.1",
                "iPhone Chrome",
            ),
            (
                "Mozilla/5.0 (iPad; CPU OS 17_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/140.0 Mobile/15E148 Safari/605.1.15",
                "iPad Firefox",
            ),
            (
                "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Mobile Safari/537.36",
                "Android Chrome",
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/28.0 Chrome/130.0.0.0 Mobile Safari/537.36",
                "Android Samsung Internet",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15",
                "Mac Safari",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36 Edg/139.0.0.0",
                "Windows Edge",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:141.0) Gecko/20100101 Firefox/141.0",
                "Windows Firefox",
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36 OPR/121.0.0.0",
                "Linux Opera",
            ),
            (
                "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36",
                "Chromebook Chrome",
            ),
        ] {
            assert_eq!(
                PasskeyName::from_user_agent(user_agent).as_str(),
                expected,
                "{user_agent}"
            );
        }
    }

    #[test]
    fn a_passkey_from_an_unknown_browser_gets_a_plain_name() {
        assert_eq!(
            PasskeyName::from_user_agent("curl/8.9.1").as_str(),
            "Passkey"
        );
        assert_eq!(PasskeyName::from_user_agent("").as_str(), "Passkey");
    }

    #[sqlx::test]
    async fn lists_a_members_own_passkeys_oldest_first_with_their_prague_dates(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        // 23:30 UTC on 14 March is already 15 March in Prague.
        let pixel = add_passkey(&db, josef, "Pixel", MARCH_14 + 11 * 3600 + 1800).await;
        let iphone = add_passkey(&db, josef, "iPhone", MARCH_14).await;
        add_passkey(&db, ondra, "Ondra's phone", MARCH_14).await;

        let passkeys = auth(db).passkeys(josef).await.unwrap();

        assert_eq!(
            passkeys,
            [
                PasskeySummary {
                    id: iphone,
                    name: named("iPhone"),
                    added_on: date(2026, 3, 14),
                },
                PasskeySummary {
                    id: pixel,
                    name: named("Pixel"),
                    added_on: date(2026, 3, 15),
                },
            ]
        );
    }

    #[sqlx::test]
    async fn renames_a_members_passkey(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let iphone = add_passkey(&db, josef, "iPhone", MARCH_14).await;
        let auth = auth(db);

        auth.rename_passkey(josef, iphone, &named("Old iPhone"))
            .await
            .unwrap();

        assert_eq!(
            auth.passkeys(josef).await.unwrap()[0].name,
            named("Old iPhone")
        );
    }

    #[sqlx::test]
    async fn cannot_rename_another_members_passkey(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let ondras = add_passkey(&db, ondra, "Phone", MARCH_14).await;
        let auth = auth(db);

        let result = auth.rename_passkey(josef, ondras, &named("Mine")).await;

        assert!(
            matches!(result, Err(AuthError::NoSuchPasskey)),
            "{result:?}"
        );
        assert_eq!(auth.passkeys(ondra).await.unwrap()[0].name, named("Phone"));
    }

    #[sqlx::test]
    async fn deletes_one_of_several_passkeys(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let iphone = add_passkey(&db, josef, "iPhone", MARCH_14).await;
        let pixel = add_passkey(&db, josef, "Pixel", MARCH_14).await;
        let auth = auth(db);

        let deleted = auth.delete_passkey(josef, iphone).await.unwrap();

        assert_eq!(deleted, named("iPhone"));
        let left: Vec<_> = auth
            .passkeys(josef)
            .await
            .unwrap()
            .into_iter()
            .map(|passkey| passkey.id)
            .collect();
        assert_eq!(left, [pixel]);
    }

    #[sqlx::test]
    async fn refuses_to_delete_the_last_passkey(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        let iphone = add_passkey(&db, josef, "iPhone", MARCH_14).await;
        let auth = auth(db);

        let result = auth.delete_passkey(josef, iphone).await;

        assert!(matches!(result, Err(AuthError::LastPasskey)), "{result:?}");
        assert_eq!(auth.passkeys(josef).await.unwrap().len(), 1);
    }

    #[sqlx::test]
    async fn cannot_delete_another_members_passkey(db: SqlitePool) {
        let josef = add_member(&db, "Josef", true).await;
        add_passkey(&db, josef, "iPhone", MARCH_14).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let ondras = add_passkey(&db, ondra, "Phone", MARCH_14).await;
        add_passkey(&db, ondra, "Laptop", MARCH_14).await;
        let auth = auth(db);

        let result = auth.delete_passkey(josef, ondras).await;

        assert!(
            matches!(result, Err(AuthError::NoSuchPasskey)),
            "{result:?}"
        );
        assert_eq!(auth.passkeys(ondra).await.unwrap().len(), 2);
    }
}
