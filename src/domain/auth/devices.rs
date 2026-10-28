//! Device links: an existing Member registers a passkey on a new device, as when they lost
//! every device with one on it.

use webauthn_rs::prelude::{
    CreationChallengeResponse, PasskeyRegistration, RegisterPublicKeyCredential,
};

use super::{
    Auth, AuthError, CeremonyId, Joined, LinkToken, PasskeyName, insert_passkey, insert_session,
};
use crate::domain::clock::unix_seconds;
use crate::domain::members::{DisplayName, MemberId};

/// What an unused, unexpired Device link offers: a new passkey for an existing Member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLink {
    pub member: MemberId,
    pub display_name: DisplayName,
}

/// A passkey being registered through a Device link, waiting for the browser's new credential.
pub(super) struct PendingRecovery {
    link_hash: Vec<u8>,
    registration: PasskeyRegistration,
}

impl Auth {
    /// The Member the Device link is for, while it is unused, unexpired and unrevoked.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::LinkUnusable`] otherwise.
    pub async fn device_link(&self, token: &LinkToken) -> Result<DeviceLink, AuthError> {
        let hash = token.hash();
        let now = unix_seconds(self.clock.now());
        // The same conditions as the UPDATE in `finish_recovering`; `query!` needs each as a
        // literal.
        let link = sqlx::query!(
            r#"SELECT members.id AS "member_id!", members.display_name
               FROM links JOIN members ON members.id = links.member_id
               WHERE links.token_hash = ? AND links.kind = 'device'
                 AND links.used_at IS NULL AND links.revoked_at IS NULL AND links.expires_at > ?"#,
            hash,
            now,
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or(AuthError::LinkUnusable)?;
        Ok(DeviceLink {
            member: MemberId::from_stored(link.member_id),
            display_name: DisplayName::from_stored(link.display_name),
        })
    }

    /// Starts registering a new passkey for the Member the Device link is for. A device that
    /// already has one of their passkeys is asked not to make a second.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::LinkUnusable`] if the link no longer works.
    pub async fn start_recovering(
        &self,
        token: &LinkToken,
    ) -> Result<(CeremonyId, CreationChallengeResponse), AuthError> {
        let link = self.device_link(token).await?;
        let (challenge, registration) = self.start_registration(link.member).await?;
        let ceremony = self.recovering.insert(
            self.clock.now(),
            PendingRecovery {
                link_hash: token.hash(),
                registration,
            },
        );
        Ok((ceremony, challenge))
    }

    /// Verifies the new passkey, then in one transaction consumes the link, stores the passkey
    /// under `name` for the link's Member and starts a session. Their other passkeys are left
    /// as they were.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::CeremonyExpired`] for an unknown, expired or mismatched ceremony,
    /// [`AuthError::PasskeyRejected`] if the credential doesn't verify, and
    /// [`AuthError::LinkUnusable`] if the link stopped working meanwhile.
    pub async fn finish_recovering(
        &self,
        token: &LinkToken,
        ceremony: &CeremonyId,
        credential: &RegisterPublicKeyCredential,
        name: &PasskeyName,
    ) -> Result<Joined, AuthError> {
        let now = self.clock.now();
        let recovering = self
            .recovering
            .take(ceremony, now)
            .filter(|recovering| recovering.link_hash == token.hash())
            .ok_or(AuthError::CeremonyExpired)?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &recovering.registration)
            .map_err(AuthError::PasskeyRejected)?;

        let now = unix_seconds(now);
        let mut tx = self.db.begin().await?;
        // Consuming the link is the transaction's first write, so a concurrent use waits for
        // this one to commit and then finds the link used.
        let member = sqlx::query_scalar!(
            r#"UPDATE links SET used_at = ?1
               WHERE token_hash = ?2 AND kind = 'device'
                 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?1
               RETURNING member_id AS "member_id!""#,
            now,
            recovering.link_hash,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AuthError::LinkUnusable)?;
        let member = MemberId::from_stored(member);
        let passkey = insert_passkey(&mut tx, member, &passkey, name, now).await?;
        let session = insert_session(&mut tx, member, passkey, now).await?;
        tx.commit().await?;
        Ok(Joined { member, session })
    }
}
