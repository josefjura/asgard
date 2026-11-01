-- Reminders: a text the whole Crew is notified about at a given time. Instants are Unix seconds
-- (UTC); they are entered and shown in Europe/Prague.

CREATE TABLE reminders (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 500),
    due_at INTEGER NOT NULL,
    created_by INTEGER NOT NULL REFERENCES members (id),
    created_at INTEGER NOT NULL,
    -- NULL while the Reminder is pending; when it went out once it is sent.
    sent_at INTEGER
);

-- Pending lists and the ticker both look for the soonest unsent Reminders.
CREATE INDEX reminders_pending ON reminders (due_at) WHERE sent_at IS NULL;
