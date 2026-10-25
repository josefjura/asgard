# Passkey-only, invite-only auth with admin-generated links

Members sign in only with passkeys (discoverable credentials, no username field). There is no public sign-up and no email: the Admin generates one-time Invite links for new Members and Device links for a Member with no usable passkey, and sends them over chat. This avoids choosing an email provider or any third-party login, and suits a Crew of three that rarely changes. Sessions are server-side rows with an opaque random id in the cookie, because Heimdall's unsigned cookie was a known flaw.

## Consequences

- The WebAuthn relying party is `asgard.joseforge.cz`. Passkeys are bound to it, so changing the domain forces every Member to register new passkeys. It was picked once and must not move.
- Self-service recovery by email magic link may come later, once a provider is chosen.
