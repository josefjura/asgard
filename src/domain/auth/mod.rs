//! Invite links, passkeys and sessions: passkey-only, invite-only sign-in (ADR 0005).

mod ceremonies;
mod tokens;

use std::sync::Arc;
use std::time::Duration;

use sqlx::{SqliteConnection, SqlitePool};
use url::Url;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyRegistration, RegisterPublicKeyCredential, Uuid,
    Webauthn, WebauthnBuilder, WebauthnError,
};

pub use ceremonies::{CEREMONY_LIFETIME, CeremonyId};
pub use tokens::{LinkToken, SessionToken};

use ceremonies::Ceremonies;

use super::clock::{Clock, unix_seconds};
use super::members::{
    self, CREW_NAME, DisplayName, Member, MemberError, MemberId, Members, NameError, NewMember,
};

/// How long an Invite or Device link works after it is created.
pub const LINK_LIFETIME: Duration = Duration::from_hours(24);

/// How long a session lasts.
pub const SESSION_LIFETIME: Duration = Duration::from_hours(90 * 24);

/// What a passkey is called until its Member renames it.
const DEFAULT_PASSKEY_NAME: &str = "Passkey";

/// What an unused, unexpired Invite link offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub display_name: DisplayName,
    pub grants_admin: bool,
}

/// A new Member, signed in on the device they joined from.
#[derive(Debug)]
pub struct Joined {
    pub member: MemberId,
    pub session: SessionToken,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Unknown, used, expired or revoked: the person needs a new link either way.
    #[error("This link has expired or was already used.")]
    LinkUnusable,
    #[error("The Crew already has an Admin.")]
    AdminExists,
    #[error(transparent)]
    Name(#[from] NameError),
    #[error("That took too long. Please try again.")]
    CeremonyExpired,
    #[error("Your passkey couldn't be verified. Please try again.")]
    PasskeyRejected(#[source] WebauthnError),
    #[error("WebAuthn failed")]
    Webauthn(#[source] WebauthnError),
    #[error("cannot store the passkey")]
    Credential(#[from] serde_json::Error),
    #[error("database error")]
    Database(#[from] sqlx::Error),
}

impl From<MemberError> for AuthError {
    fn from(err: MemberError) -> Self {
        match err {
            MemberError::Name(err) => Self::Name(err),
            MemberError::Database(err) => Self::Database(err),
        }
    }
}

/// A registration waiting for the browser's new credential.
struct Joining {
    link_hash: Vec<u8>,
    webauthn_user_id: Uuid,
    registration: PasskeyRegistration,
}

/// Creates links, runs passkey ceremonies and finds the Member behind a session.
#[derive(Clone)]
pub struct Auth {
    db: SqlitePool,
    clock: Clock,
    members: Members,
    webauthn: Arc<Webauthn>,
    joining: Arc<Ceremonies<Joining>>,
}

impl Auth {
    /// The relying party is `public_url`'s host, and passkeys only work at its origin.
    ///
    /// # Errors
    ///
    /// Fails if `public_url` can't be a WebAuthn relying party.
    pub fn new(
        db: SqlitePool,
        clock: Clock,
        members: Members,
        public_url: &Url,
    ) -> Result<Self, WebauthnError> {
        let rp_id = public_url.host_str().unwrap_or_default();
        let webauthn = WebauthnBuilder::new(rp_id, public_url)?
            .rp_name(CREW_NAME)
            .build()?;
        Ok(Self {
            members,
            db,
            clock,
            webauthn: Arc::new(webauthn),
            joining: Arc::new(Ceremonies::new()),
        })
    }

    /// The first Admin's Invite link, for a fresh database.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::AdminExists`] once there is an Admin, or [`AuthError::Name`] if the
    /// name is taken.
    pub async fn create_admin_invite(&self, name: &DisplayName) -> Result<LinkToken, AuthError> {
        if self.members.admin_name().await?.is_some() {
            return Err(AuthError::AdminExists);
        }
        self.members.check_available(name).await?;
        self.insert_invite(name, true, None).await
    }

    async fn insert_invite(
        &self,
        name: &DisplayName,
        grants_admin: bool,
        created_by: Option<MemberId>,
    ) -> Result<LinkToken, AuthError> {
        let token = LinkToken::generate();
        let hash = token.hash();
        let display_name = name.as_str();
        let created_by = created_by.map(MemberId::get);
        let now = self.clock.now();
        let created_at = unix_seconds(now);
        let expires_at = unix_seconds(now + LINK_LIFETIME);
        sqlx::query!(
            "INSERT INTO links (token_hash, kind, display_name, grants_admin, created_by, created_at, expires_at)
             VALUES (?, 'invite', ?, ?, ?, ?, ?)",
            hash,
            display_name,
            grants_admin,
            created_by,
            created_at,
            expires_at,
        )
        .execute(&self.db)
        .await?;
        Ok(token)
    }

    /// What the Invite link offers, while it is unused, unexpired and unrevoked. An Admin link
    /// also stops working once the Crew has an Admin, so there is only ever one.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::LinkUnusable`] otherwise.
    pub async fn invite(&self, token: &LinkToken) -> Result<Invite, AuthError> {
        let hash = token.hash();
        let now = unix_seconds(self.clock.now());
        let link = sqlx::query!(
            r#"SELECT display_name AS "display_name!", grants_admin AS "grants_admin: bool"
               FROM links
               WHERE token_hash = ? AND kind = 'invite'
                 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?
                 AND NOT (grants_admin AND EXISTS (SELECT 1 FROM members WHERE is_admin))"#,
            hash,
            now,
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(AuthError::LinkUnusable)?;
        Ok(Invite {
            display_name: DisplayName::from_stored(link.display_name),
            grants_admin: link.grants_admin,
        })
    }

    /// Starts registering the invited person's first passkey.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::LinkUnusable`] if the link no longer works.
    pub async fn start_joining(
        &self,
        token: &LinkToken,
    ) -> Result<(CeremonyId, CreationChallengeResponse), AuthError> {
        let invite = self.invite(token).await?;
        let webauthn_user_id = Uuid::new_v4();
        let name = invite.display_name.as_str();
        let (challenge, registration) = self
            .webauthn
            .start_passkey_registration(webauthn_user_id, name, name, None)
            .map_err(AuthError::Webauthn)?;
        let ceremony = self.joining.insert(
            self.clock.now(),
            Joining {
                link_hash: token.hash(),
                webauthn_user_id,
                registration,
            },
        );
        Ok((ceremony, challenge))
    }

    /// Verifies the new passkey, then in one transaction consumes the link, creates the Member
    /// (joined today), stores the passkey and starts a session.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::CeremonyExpired`] for an unknown, expired or mismatched ceremony,
    /// [`AuthError::PasskeyRejected`] if the credential doesn't verify, and
    /// [`AuthError::LinkUnusable`] if the link stopped working meanwhile.
    pub async fn finish_joining(
        &self,
        token: &LinkToken,
        ceremony: &CeremonyId,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<Joined, AuthError> {
        let now = self.clock.now();
        let joining = self
            .joining
            .take(ceremony, now)
            .filter(|joining| joining.link_hash == token.hash())
            .ok_or(AuthError::CeremonyExpired)?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &joining.registration)
            .map_err(AuthError::PasskeyRejected)?;

        let now = unix_seconds(now);
        let mut tx = self.db.begin().await?;
        // Consuming the link is the transaction's first write, so a concurrent use waits for
        // this one to commit and then finds the link used.
        let link = sqlx::query!(
            r#"UPDATE links SET used_at = ?1
               WHERE token_hash = ?2 AND kind = 'invite'
                 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?1
                 AND NOT (grants_admin AND EXISTS (SELECT 1 FROM members WHERE is_admin))
               RETURNING display_name AS "display_name!", grants_admin AS "grants_admin: bool""#,
            now,
            joining.link_hash,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AuthError::LinkUnusable)?;
        let member = members::insert(
            &mut tx,
            NewMember {
                display_name: &DisplayName::from_stored(link.display_name),
                is_admin: link.grants_admin,
                joined_on: self.clock.today(),
                webauthn_user_id: &joining.webauthn_user_id.to_string(),
            },
        )
        .await?;
        let passkey = insert_passkey(&mut tx, member, &passkey, now).await?;
        let session = insert_session(&mut tx, member, passkey, now).await?;
        tx.commit().await?;
        Ok(Joined { member, session })
    }

    /// The Member signed in with `token`, while the session lasts.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn session_member(
        &self,
        token: &SessionToken,
    ) -> Result<Option<Member>, sqlx::Error> {
        let hash = token.hash();
        let now = unix_seconds(self.clock.now());
        let member = sqlx::query_scalar!(
            "SELECT member_id FROM sessions WHERE token_hash = ? AND expires_at > ?",
            hash,
            now,
        )
        .fetch_optional(&self.db)
        .await?;
        match member {
            Some(id) => self.members.find(MemberId::from_stored(id)).await,
            None => Ok(None),
        }
    }
}

async fn insert_passkey(
    conn: &mut SqliteConnection,
    member: MemberId,
    passkey: &Passkey,
    now: i64,
) -> Result<i64, AuthError> {
    let member = member.get();
    let credential_id = passkey.cred_id().as_ref();
    let credential = serde_json::to_string(passkey)?;
    let id = sqlx::query!(
        "INSERT INTO passkeys (member_id, credential_id, credential, name, added_at)
         VALUES (?, ?, ?, ?, ?)",
        member,
        credential_id,
        credential,
        DEFAULT_PASSKEY_NAME,
        now,
    )
    .execute(conn)
    .await?
    .last_insert_rowid();
    Ok(id)
}

async fn insert_session(
    conn: &mut SqliteConnection,
    member: MemberId,
    passkey: i64,
    now: i64,
) -> Result<SessionToken, AuthError> {
    let token = SessionToken::generate();
    let hash = token.hash();
    let member = member.get();
    let expires_at = now.saturating_add_unsigned(SESSION_LIFETIME.as_secs());
    sqlx::query!(
        "INSERT INTO sessions (token_hash, member_id, passkey_id, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
        hash,
        member,
        passkey,
        now,
        expires_at,
    )
    .execute(conn)
    .await?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::domain::clock::ManualClock;

    fn auth(db: SqlitePool) -> Auth {
        let clock = ManualClock::starting_at(std::time::SystemTime::UNIX_EPOCH);
        let members = Members::new(db.clone());
        Auth::new(
            db,
            clock.clock(),
            members,
            &Url::parse("https://asgard.test").unwrap(),
        )
        .unwrap()
    }

    fn name(input: &str) -> DisplayName {
        DisplayName::parse(input).unwrap()
    }

    async fn add_member(db: &SqlitePool, display_name: &str, is_admin: bool) {
        let mut conn = db.acquire().await.unwrap();
        members::insert(
            &mut conn,
            NewMember {
                display_name: &name(display_name),
                is_admin,
                joined_on: date(2026, 1, 1),
                webauthn_user_id: display_name,
            },
        )
        .await
        .unwrap();
    }

    #[sqlx::test]
    async fn an_admin_invite_offers_the_admin_role_under_its_name(db: SqlitePool) {
        let auth = auth(db);

        let token = auth.create_admin_invite(&name("Josef")).await.unwrap();

        assert_eq!(
            auth.invite(&token).await.unwrap(),
            Invite {
                display_name: name("Josef"),
                grants_admin: true,
            }
        );
    }

    #[sqlx::test]
    async fn there_is_no_admin_invite_once_there_is_an_admin(db: SqlitePool) {
        add_member(&db, "Josef", true).await;

        let result = auth(db).create_admin_invite(&name("Ondra")).await;

        assert!(matches!(result, Err(AuthError::AdminExists)), "{result:?}");
    }

    #[sqlx::test]
    async fn an_admin_invite_stops_working_once_there_is_an_admin(db: SqlitePool) {
        let auth = auth(db.clone());
        let token = auth.create_admin_invite(&name("Josef")).await.unwrap();

        add_member(&db, "Ondra", true).await;

        let result = auth.invite(&token).await;
        assert!(matches!(result, Err(AuthError::LinkUnusable)), "{result:?}");
    }

    #[sqlx::test]
    async fn an_admin_invite_needs_a_free_name(db: SqlitePool) {
        add_member(&db, "Josef", false).await;

        let result = auth(db).create_admin_invite(&name("josef")).await;

        assert!(
            matches!(result, Err(AuthError::Name(NameError::Taken(_)))),
            "{result:?}"
        );
    }

    #[sqlx::test]
    async fn an_unknown_token_is_unusable(db: SqlitePool) {
        let result = auth(db)
            .invite(&LinkToken::from_client("made-up".to_owned()))
            .await;

        assert!(matches!(result, Err(AuthError::LinkUnusable)), "{result:?}");
    }

    #[sqlx::test]
    async fn an_unknown_session_has_no_member(db: SqlitePool) {
        let member = auth(db)
            .session_member(&SessionToken::generate())
            .await
            .unwrap();

        assert_eq!(member, None);
    }
}
