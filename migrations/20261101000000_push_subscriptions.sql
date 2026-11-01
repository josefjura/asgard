-- Each device's Web Push subscription, tied to the session that made it: ending the session
-- (logging out, or deleting the passkey that started it) removes the subscription with it.

CREATE TABLE push_subscriptions (
    id INTEGER PRIMARY KEY,
    -- One per session: subscribing again from the same device replaces it.
    session_id INTEGER NOT NULL UNIQUE REFERENCES sessions (id) ON DELETE CASCADE,
    member_id INTEGER NOT NULL REFERENCES members (id),
    -- The push service's URL for this browser, which the browser keeps across sessions.
    endpoint TEXT NOT NULL UNIQUE,
    -- The browser's P-256 public key and auth secret, base64url, for encrypting each push.
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    -- The device and browser, as told from the user agent, such as "iPhone Safari".
    device_label TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
