-- Payments: money one Member (the Sender) sent straight to another (the Recipient) to settle
-- up. Amounts are whole crowns; instants are Unix seconds (UTC). A Payment is dated when it's
-- logged.

CREATE TABLE payments (
    id INTEGER PRIMARY KEY,
    sender_id INTEGER NOT NULL REFERENCES members (id),
    recipient_id INTEGER NOT NULL REFERENCES members (id),
    amount INTEGER NOT NULL CHECK (amount BETWEEN 1 AND 1000000),
    -- NULL when the Payment needs no description.
    description TEXT,
    -- The Logger; NULL for entries imported from Heimdall.
    logged_by INTEGER REFERENCES members (id),
    logged_at INTEGER NOT NULL,
    edited_by INTEGER REFERENCES members (id),
    edited_at INTEGER,
    -- A deleted Payment is kept, but left out of every list and Balance.
    deleted_by INTEGER REFERENCES members (id),
    deleted_at INTEGER,
    CHECK (sender_id <> recipient_id)
);
