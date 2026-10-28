//! Invite and Device links, passkeys and sessions: passkey-only, invite-only sign-in (ADR 0005).

mod ceremonies;
mod devices;
mod links;
mod passkeys;
mod tokens;

use std::sync::Arc;
use std::time::Duration;

use sqlx::{SqliteConnection, SqlitePool};
use url::Url;
use webauthn_rs::prelude::{
    CreationChallengeResponse, DiscoverableAuthentication, DiscoverableKey, Passkey,
    PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse, Uuid, Webauthn, WebauthnBuilder, WebauthnError,
};
use webauthn_rs_proto::ResidentKeyRequirement;

pub use ceremonies::{CEREMONY_LIFETIME, CeremonyId};
pub use devices::DeviceLink;
pub use links::{LinkId, LinkKind, OpenLink};
pub use passkeys::{PASSKEY_NAME_MAX, PasskeyId, PasskeyName, PasskeyNameError, PasskeySummary};
pub use tokens::{LinkToken, SessionToken};

use ceremonies::Ceremonies;
use devices::PendingDeviceSetup;
use passkeys::PendingAdd;

use super::clock::{Clock, unix_seconds};
use super::members::{
    self, CREW_NAME, DisplayName, Member, MemberError, MemberId, Members, NameError, NewMember,
};

/// How many hours an Invite or Device link works after it is created.
pub const LINK_LIFETIME_HOURS: u64 = 24;

/// How long an Invite or Device link works after it is created.
pub const LINK_LIFETIME: Duration = Duration::from_hours(LINK_LIFETIME_HOURS);

/// How long a session lasts without being used.
pub const SESSION_LIFETIME: Duration = Duration::from_hours(90 * 24);

/// How often using a session moves its expiry forward, so a request isn't a write every time.
/// A session therefore ends between 89 and 90 days after it was last used.
pub const SESSION_RENEWAL_INTERVAL: Duration = Duration::from_hours(24);

/// What an unused, unexpired Invite link offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub display_name: DisplayName,
    pub grants_admin: bool,
}

/// The Member behind a live session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub member: Member,
    /// Whether this use moved the session's expiry forward, so the browser's cookie should too.
    pub renewed: bool,
}

/// A Member signed in through a link, on the device it was opened on: a new Member for an
/// Invite link, an existing one for a Device link.
#[derive(Debug)]
pub struct Joined {
    pub member: MemberId,
    pub session: SessionToken,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Unknown, used, expired or revoked: the person needs a new link either way.
    #[error("the link is unknown, used, expired or revoked")]
    LinkUnusable,
    #[error("the Crew already has an Admin")]
    AdminExists,
    #[error(transparent)]
    Name(#[from] NameError),
    /// A Device link was asked for a Member who doesn't exist.
    #[error("no such Member")]
    NoSuchMember,
    #[error("the ceremony is unknown or expired")]
    CeremonyExpired,
    /// No Member has this passkey: it was never registered here, or has been deleted.
    #[error("the passkey is unknown")]
    UnknownPasskey,
    /// The Member has no such passkey: it is another Member's, or already deleted.
    #[error("the Member has no such passkey")]
    NoSuchPasskey,
    /// Deleting it would leave the Member no way to sign in.
    #[error("a Member's last passkey can't be deleted")]
    LastPasskey,
    #[error("the passkey didn't verify")]
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
struct PendingJoin {
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
    joining: Arc<Ceremonies<PendingJoin>>,
    signing_in: Arc<Ceremonies<DiscoverableAuthentication>>,
    adding: Arc<Ceremonies<PendingAdd>>,
    setting_up: Arc<Ceremonies<PendingDeviceSetup>>,
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
            signing_in: Arc::new(Ceremonies::new()),
            adding: Arc::new(Ceremonies::new()),
            setting_up: Arc::new(Ceremonies::new()),
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
        // The same conditions as the UPDATE in `finish_joining`; `query!` needs each as a literal.
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
        let (mut challenge, registration) = self
            .webauthn
            .start_passkey_registration(webauthn_user_id, name, name, None)
            .map_err(AuthError::Webauthn)?;
        require_discoverable(&mut challenge);
        let ceremony = self.joining.insert(
            self.clock.now(),
            PendingJoin {
                link_hash: token.hash(),
                webauthn_user_id,
                registration,
            },
        );
        Ok((ceremony, challenge))
    }

    /// Verifies the new passkey, then in one transaction consumes the link, creates the Member
    /// (joined today), stores the passkey under `name` and starts a session.
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
        name: &PasskeyName,
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
        let passkey = insert_passkey(&mut tx, member, &passkey, name, now).await?;
        let session = insert_session(&mut tx, member, passkey, now).await?;
        tx.commit().await?;
        Ok(Joined { member, session })
    }

    /// Starts signing in with whichever of their passkeys the Member's device offers.
    ///
    /// # Errors
    ///
    /// Fails only if WebAuthn can't make a challenge.
    pub fn start_signing_in(&self) -> Result<(CeremonyId, RequestChallengeResponse), AuthError> {
        let (challenge, authentication) = self
            .webauthn
            .start_discoverable_authentication()
            .map_err(AuthError::Webauthn)?;
        let ceremony = self.signing_in.insert(self.clock.now(), authentication);
        Ok((ceremony, challenge))
    }

    /// Verifies the passkey against the one stored for its Member, then starts a session.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::CeremonyExpired`] for an unknown or expired ceremony,
    /// [`AuthError::UnknownPasskey`] if no Member has the passkey, and
    /// [`AuthError::PasskeyRejected`] if it doesn't verify.
    pub async fn finish_signing_in(
        &self,
        ceremony: &CeremonyId,
        credential: &PublicKeyCredential,
    ) -> Result<SessionToken, AuthError> {
        let now = self.clock.now();
        let authentication = self
            .signing_in
            .take(ceremony, now)
            .ok_or(AuthError::CeremonyExpired)?;
        let (webauthn_user_id, credential_id) = self
            .webauthn
            .identify_discoverable_authentication(credential)
            .map_err(|_| AuthError::UnknownPasskey)?;
        let webauthn_user_id = webauthn_user_id.to_string();
        let stored = sqlx::query!(
            r#"SELECT passkeys.id AS "id!", passkeys.member_id, passkeys.credential
             FROM passkeys JOIN members ON members.id = passkeys.member_id
             WHERE passkeys.credential_id = ? AND members.webauthn_user_id = ?"#,
            credential_id,
            webauthn_user_id,
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(AuthError::UnknownPasskey)?;
        let mut passkey: Passkey = serde_json::from_str(&stored.credential)?;
        let result = self
            .webauthn
            .finish_discoverable_authentication(
                credential,
                authentication,
                &[DiscoverableKey::from(&passkey)],
            )
            .map_err(AuthError::PasskeyRejected)?;
        // Keeps the signature counter current, so a cloned authenticator can be told apart.
        passkey.update_credential(&result);
        let updated = serde_json::to_string(&passkey)?;

        let now = unix_seconds(now);
        let mut tx = self.db.begin().await?;
        sqlx::query!(
            "UPDATE passkeys SET credential = ?, last_used_at = ? WHERE id = ?",
            updated,
            now,
            stored.id,
        )
        .execute(&mut *tx)
        .await?;
        let member = MemberId::from_stored(stored.member_id);
        let session =
            insert_session(&mut tx, member, PasskeyId::from_stored(stored.id), now).await?;
        tx.commit().await?;
        Ok(session)
    }

    /// The Member signed in with `token`, while the session lasts. Using a session moves its
    /// expiry forward, at most once per [`SESSION_RENEWAL_INTERVAL`].
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn session(&self, token: &SessionToken) -> Result<Option<Session>, sqlx::Error> {
        let hash = token.hash();
        let now = unix_seconds(self.clock.now());
        let Some(session) = sqlx::query!(
            "SELECT id, member_id, expires_at FROM sessions WHERE token_hash = ? AND expires_at > ?",
            hash,
            now,
        )
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };
        let expires_at = now.saturating_add_unsigned(SESSION_LIFETIME.as_secs());
        // The expiry was last moved this long ago.
        let since_renewal = expires_at.saturating_sub(session.expires_at);
        let renewed = since_renewal >= renewal_interval_secs();
        if renewed {
            sqlx::query!(
                "UPDATE sessions SET expires_at = ? WHERE id = ?",
                expires_at,
                session.id,
            )
            .execute(&self.db)
            .await?;
        }
        let member = self
            .members
            .find(MemberId::from_stored(session.member_id))
            .await?;
        Ok(member.map(|member| Session { member, renewed }))
    }

    /// Ends the session, so its cookie no longer signs anyone in. An unknown token is ignored.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn end_session(&self, token: &SessionToken) -> Result<(), sqlx::Error> {
        let hash = token.hash();
        sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", hash)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// An Invite link for a Member without the Admin role that no Member created, for tests
    /// that need a second Member without going through the admin page.
    #[cfg(test)]
    pub(crate) async fn create_test_invite(&self, name: &DisplayName) -> LinkToken {
        self.insert_invite(name, false, None).await.unwrap()
    }
}

fn renewal_interval_secs() -> i64 {
    i64::try_from(SESSION_RENEWAL_INTERVAL.as_secs()).unwrap_or(i64::MAX)
}

/// Signing in has no username field, so every passkey must be discoverable.
const fn require_discoverable(challenge: &mut CreationChallengeResponse) {
    if let Some(selection) = challenge.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Required);
        selection.require_resident_key = true;
    }
}

async fn insert_passkey(
    conn: &mut SqliteConnection,
    member: MemberId,
    passkey: &Passkey,
    name: &PasskeyName,
    now: i64,
) -> Result<PasskeyId, AuthError> {
    let member = member.get();
    let credential_id = passkey.cred_id().as_ref();
    let credential = serde_json::to_string(passkey)?;
    let name = name.as_str();
    let id = sqlx::query!(
        "INSERT INTO passkeys (member_id, credential_id, credential, name, added_at)
         VALUES (?, ?, ?, ?, ?)",
        member,
        credential_id,
        credential,
        name,
        now,
    )
    .execute(conn)
    .await?
    .last_insert_rowid();
    Ok(PasskeyId::from_stored(id))
}

async fn insert_session(
    conn: &mut SqliteConnection,
    member: MemberId,
    passkey: PasskeyId,
    now: i64,
) -> Result<SessionToken, AuthError> {
    let token = SessionToken::generate();
    let hash = token.hash();
    let member = member.get();
    let passkey = passkey.get();
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
    use super::*;
    use crate::domain::clock::ManualClock;
    use crate::domain::members::fixtures::{add_member, name};

    pub(super) fn auth(db: SqlitePool) -> Auth {
        auth_with_clock(db).0
    }

    /// The service, and the clock it reads, which starts at the Unix epoch.
    pub(super) fn auth_with_clock(db: SqlitePool) -> (Auth, ManualClock) {
        let clock = ManualClock::starting_at(std::time::SystemTime::UNIX_EPOCH);
        let members = Members::new(db.clone());
        let auth = Auth::new(
            db,
            clock.clock(),
            members,
            &Url::parse("https://asgard.test").unwrap(),
        )
        .unwrap();
        (auth, clock)
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
        let session = auth(db).session(&SessionToken::generate()).await.unwrap();

        assert_eq!(session, None);
    }
}
