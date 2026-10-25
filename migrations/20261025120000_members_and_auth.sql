-- Members, the links that bring them in, their passkeys and sessions (ADR 0005).
-- Instants are Unix seconds (UTC); a Member's joined date is a Europe/Prague calendar date.

CREATE TABLE members (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    -- The display name folded to lowercase, so uniqueness ignores case beyond ASCII too.
    display_name_key TEXT NOT NULL UNIQUE,
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    joined_on TEXT NOT NULL,
    -- The WebAuthn user handle, which a discoverable passkey hands back on sign-in.
    webauthn_user_id TEXT NOT NULL UNIQUE
);

CREATE TABLE links (
    id INTEGER PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('invite', 'device')),
    -- An Invite link names the Member to be; a Device link names an existing Member.
    display_name TEXT,
    member_id INTEGER REFERENCES members (id),
    grants_admin BOOLEAN NOT NULL DEFAULT FALSE,
    -- NULL for the bootstrap link, which no Member created.
    created_by INTEGER REFERENCES members (id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    revoked_at INTEGER,
    CHECK ((kind = 'invite') = (display_name IS NOT NULL AND member_id IS NULL))
);

CREATE TABLE passkeys (
    id INTEGER PRIMARY KEY,
    member_id INTEGER NOT NULL REFERENCES members (id),
    credential_id BLOB NOT NULL UNIQUE,
    -- webauthn-rs's serialized Passkey: public key, counter and flags.
    credential TEXT NOT NULL,
    name TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    -- SHA-256 of the id in the cookie, so a leaked database can't be replayed as cookies.
    token_hash BLOB NOT NULL UNIQUE,
    member_id INTEGER NOT NULL REFERENCES members (id),
    passkey_id INTEGER NOT NULL REFERENCES passkeys (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
