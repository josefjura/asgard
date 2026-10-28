//! The Admin's Invite and Device links: creating them, listing the open ones and revoking them.

use jiff::civil::DateTime;

use super::{Auth, AuthError, LINK_LIFETIME, LinkToken};
use crate::domain::clock::{crew_time_of_unix_seconds, unix_seconds};
use crate::domain::members::{DisplayName, MemberId};

/// A stored link, which the admin page revokes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkId(i64);

impl LinkId {
    /// An id a client sent back. Nothing about it is trusted until an open link is found.
    #[must_use]
    pub const fn from_client(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Whom a link is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// A new person, who joins the Crew through it.
    Invite,
    /// An existing Member, who registers a passkey on a new device through it.
    Device,
}

impl LinkKind {
    fn from_stored(kind: &str) -> Result<Self, sqlx::Error> {
        match kind {
            "invite" => Ok(Self::Invite),
            "device" => Ok(Self::Device),
            other => Err(sqlx::Error::Decode(
                format!("unknown link kind {other:?}").into(),
            )),
        }
    }
}

/// A link that is unused, unexpired and unrevoked, as the admin page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLink {
    pub id: LinkId,
    pub kind: LinkKind,
    /// The name the new Member will join under, or the Member a Device link is for.
    pub display_name: DisplayName,
    /// When it stops working, on the clock in the Crew's time zone.
    pub expires: DateTime,
}

impl Auth {
    /// An Invite link the Admin `created_by` made for a new, non-Admin Member called `name`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Name`] if a Member already has the name, ignoring case.
    pub async fn create_invite(
        &self,
        name: &DisplayName,
        created_by: MemberId,
    ) -> Result<LinkToken, AuthError> {
        self.members.check_available(name).await?;
        self.insert_invite(name, false, Some(created_by)).await
    }

    /// A Device link the Admin `created_by` made, which registers a passkey for `member` on
    /// whichever device opens it.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::NoSuchMember`] if there is no Member `member`.
    pub async fn create_device_link(
        &self,
        member: MemberId,
        created_by: MemberId,
    ) -> Result<LinkToken, AuthError> {
        if self.members.find(member).await?.is_none() {
            return Err(AuthError::NoSuchMember);
        }
        let token = LinkToken::generate();
        let hash = token.hash();
        let member = member.get();
        let created_by = created_by.get();
        let now = self.clock.now();
        let created_at = unix_seconds(now);
        let expires_at = unix_seconds(now + LINK_LIFETIME);
        sqlx::query!(
            "INSERT INTO links (token_hash, kind, member_id, created_by, created_at, expires_at)
             VALUES (?, 'device', ?, ?, ?, ?)",
            hash,
            member,
            created_by,
            created_at,
            expires_at,
        )
        .execute(&self.db)
        .await?;
        Ok(token)
    }

    /// The Invite and Device links that still work, oldest first. The bootstrap Admin link is
    /// left out: it stopped working when the Admin joined.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn open_links(&self) -> Result<Vec<OpenLink>, sqlx::Error> {
        let now = unix_seconds(self.clock.now());
        let rows = sqlx::query!(
            r#"SELECT links.id AS "id!", links.kind,
                      COALESCE(links.display_name, members.display_name) AS "display_name!: String",
                      links.expires_at
               FROM links LEFT JOIN members ON members.id = links.member_id
               WHERE NOT links.grants_admin
                 AND links.used_at IS NULL AND links.revoked_at IS NULL AND links.expires_at > ?
               ORDER BY links.created_at, links.id"#,
            now,
        )
        .fetch_all(&self.db)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(OpenLink {
                    id: LinkId(row.id),
                    kind: LinkKind::from_stored(&row.kind)?,
                    display_name: DisplayName::from_stored(row.display_name),
                    expires: crew_time_of_unix_seconds(row.expires_at),
                })
            })
            .collect()
    }

    /// Stops an open link working, and returns the name it was for. A link that is no longer
    /// open is left alone and gives `None`.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn revoke_link(&self, id: LinkId) -> Result<Option<DisplayName>, sqlx::Error> {
        let now = unix_seconds(self.clock.now());
        let name = sqlx::query_scalar!(
            r#"UPDATE links SET revoked_at = ?1
               WHERE id = ?2 AND NOT grants_admin
                 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?1
               RETURNING COALESCE(
                   display_name,
                   (SELECT members.display_name FROM members WHERE members.id = links.member_id)
               ) AS "display_name!: String""#,
            now,
            id.0,
        )
        .fetch_optional(&self.db)
        .await?;
        Ok(name.map(DisplayName::from_stored))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::SqlitePool;

    use super::*;
    use crate::domain::auth::tests::{auth, auth_with_clock};
    use crate::domain::members::NameError;
    use crate::domain::members::fixtures::{add_member, name};

    fn names(links: &[OpenLink]) -> Vec<&str> {
        links
            .iter()
            .map(|link| link.display_name.as_str())
            .collect()
    }

    #[sqlx::test]
    async fn an_invite_offers_a_non_admin_membership_under_its_name(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let auth = auth(db);

        let token = auth.create_invite(&name("Ondra"), admin).await.unwrap();

        let invite = auth.invite(&token).await.unwrap();
        assert_eq!(invite.display_name, name("Ondra"));
        assert!(!invite.grants_admin);
    }

    #[sqlx::test]
    async fn an_invite_needs_a_name_no_member_has_in_any_case(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        add_member(&db, "Šimon", false).await;
        let auth = auth(db);

        for taken in ["JOSEF", "šimon"] {
            let result = auth.create_invite(&name(taken), admin).await;

            assert!(
                matches!(result, Err(AuthError::Name(NameError::Taken(_)))),
                "{taken}: {result:?}"
            );
        }
        assert_eq!(auth.open_links().await.unwrap(), []);
    }

    #[sqlx::test]
    async fn lists_open_links_oldest_first_with_their_expiry(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let auth = auth(db);

        auth.create_invite(&name("Ondra"), admin).await.unwrap();
        auth.create_invite(&name("Karel"), admin).await.unwrap();

        let links = auth.open_links().await.unwrap();
        assert_eq!(names(&links), ["Ondra", "Karel"]);
        // The test clock starts at the epoch, 01:00 in Prague.
        assert_eq!(
            links[0].expires,
            jiff::civil::date(1970, 1, 2).at(1, 0, 0, 0)
        );
    }

    #[sqlx::test]
    async fn leaves_out_expired_revoked_and_bootstrap_links(db: SqlitePool) {
        let (auth, clock) = auth_with_clock(db.clone());
        auth.create_admin_invite(&name("Josef")).await.unwrap();
        let admin = add_member(&db, "Josef", true).await;
        auth.create_invite(&name("Expired"), admin).await.unwrap();
        clock.advance(LINK_LIFETIME.checked_sub(Duration::from_secs(1)).unwrap());
        auth.create_invite(&name("Ondra"), admin).await.unwrap();
        auth.create_invite(&name("Revoked"), admin).await.unwrap();
        clock.advance(Duration::from_secs(1));

        let revoked = auth.open_links().await.unwrap()[1].id;
        auth.revoke_link(revoked).await.unwrap();

        assert_eq!(names(&auth.open_links().await.unwrap()), ["Ondra"]);
    }

    #[sqlx::test]
    async fn a_revoked_link_stops_working(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let auth = auth(db);
        let token = auth.create_invite(&name("Ondra"), admin).await.unwrap();
        let id = auth.open_links().await.unwrap()[0].id;

        let revoked = auth.revoke_link(id).await.unwrap();

        assert_eq!(revoked, Some(name("Ondra")));
        let result = auth.invite(&token).await;
        assert!(matches!(result, Err(AuthError::LinkUnusable)), "{result:?}");
    }

    #[sqlx::test]
    async fn a_device_link_offers_a_new_passkey_to_its_member(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let auth = auth(db);

        let token = auth.create_device_link(ondra, admin).await.unwrap();

        let link = auth.device_link(&token).await.unwrap();
        assert_eq!(link.member, ondra);
        assert_eq!(link.display_name, name("Ondra"));
    }

    #[sqlx::test]
    async fn a_device_link_needs_a_member(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let auth = auth(db);

        let result = auth
            .create_device_link(MemberId::from_client(999), admin)
            .await;

        assert!(matches!(result, Err(AuthError::NoSuchMember)), "{result:?}");
        assert_eq!(auth.open_links().await.unwrap(), []);
    }

    #[sqlx::test]
    async fn invite_and_device_links_cannot_stand_in_for_each_other(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let auth = auth(db);
        let invite = auth.create_invite(&name("Karel"), admin).await.unwrap();
        let device = auth.create_device_link(ondra, admin).await.unwrap();

        let as_device = auth.device_link(&invite).await;
        let as_invite = auth.invite(&device).await;

        assert!(
            matches!(as_device, Err(AuthError::LinkUnusable)),
            "{as_device:?}"
        );
        assert!(
            matches!(as_invite, Err(AuthError::LinkUnusable)),
            "{as_invite:?}"
        );
    }

    #[sqlx::test]
    async fn lists_device_links_under_their_members_name(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let auth = auth(db);
        auth.create_invite(&name("Karel"), admin).await.unwrap();
        auth.create_device_link(ondra, admin).await.unwrap();

        let links = auth.open_links().await.unwrap();

        assert_eq!(names(&links), ["Karel", "Ondra"]);
        assert_eq!(links[0].kind, LinkKind::Invite);
        assert_eq!(links[1].kind, LinkKind::Device);
    }

    #[sqlx::test]
    async fn a_revoked_device_link_stops_working(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let auth = auth(db);
        let token = auth.create_device_link(ondra, admin).await.unwrap();
        let id = auth.open_links().await.unwrap()[0].id;

        let revoked = auth.revoke_link(id).await.unwrap();

        assert_eq!(revoked, Some(name("Ondra")));
        let result = auth.device_link(&token).await;
        assert!(matches!(result, Err(AuthError::LinkUnusable)), "{result:?}");
        assert_eq!(auth.open_links().await.unwrap(), []);
    }

    #[sqlx::test]
    async fn a_device_link_expires_like_an_invite(db: SqlitePool) {
        let (auth, clock) = auth_with_clock(db.clone());
        let admin = add_member(&db, "Josef", true).await;
        let ondra = add_member(&db, "Ondra", false).await;
        let token = auth.create_device_link(ondra, admin).await.unwrap();

        clock.advance(LINK_LIFETIME);

        let result = auth.device_link(&token).await;
        assert!(matches!(result, Err(AuthError::LinkUnusable)), "{result:?}");
        assert_eq!(auth.open_links().await.unwrap(), []);
    }

    #[sqlx::test]
    async fn the_bootstrap_link_cannot_be_revoked_as_it_is_never_listed(db: SqlitePool) {
        let auth = auth(db.clone());
        let token = auth.create_admin_invite(&name("Josef")).await.unwrap();
        let id: i64 = sqlx::query_scalar("SELECT id FROM links WHERE grants_admin")
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(
            auth.revoke_link(LinkId::from_client(id)).await.unwrap(),
            None
        );
        assert!(auth.invite(&token).await.is_ok());
    }

    #[sqlx::test]
    async fn revoking_a_link_that_is_no_longer_open_does_nothing(db: SqlitePool) {
        let admin = add_member(&db, "Josef", true).await;
        let auth = auth(db);
        auth.create_invite(&name("Ondra"), admin).await.unwrap();
        let id = auth.open_links().await.unwrap()[0].id;
        auth.revoke_link(id).await.unwrap();

        assert_eq!(auth.revoke_link(id).await.unwrap(), None);
        assert_eq!(
            auth.revoke_link(LinkId::from_client(999)).await.unwrap(),
            None
        );
    }
}
