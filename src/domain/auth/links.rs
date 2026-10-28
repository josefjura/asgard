//! The Admin's Invite links: creating them, listing the open ones and revoking them.

use jiff::civil::DateTime;

use super::{Auth, AuthError, LinkToken};
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

/// An Invite link that is unused, unexpired and unrevoked, as the admin page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLink {
    pub id: LinkId,
    /// The name the new Member will join under.
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

    /// The Invite links that still work, oldest first. The bootstrap Admin link is left out:
    /// it stopped working when the Admin joined.
    ///
    /// # Errors
    ///
    /// Fails if the database does.
    pub async fn open_links(&self) -> Result<Vec<OpenLink>, sqlx::Error> {
        let now = unix_seconds(self.clock.now());
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", display_name AS "display_name!", expires_at
               FROM links
               WHERE kind = 'invite' AND NOT grants_admin
                 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?
               ORDER BY created_at, id"#,
            now,
        )
        .fetch_all(&self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| OpenLink {
                id: LinkId(row.id),
                display_name: DisplayName::from_stored(row.display_name),
                expires: crew_time_of_unix_seconds(row.expires_at),
            })
            .collect())
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
               WHERE id = ?2 AND kind = 'invite'
                 AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?1
               RETURNING display_name AS "display_name!""#,
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
    use crate::domain::auth::LINK_LIFETIME;
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
