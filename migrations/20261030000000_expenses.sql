-- Expenses: money a Member spent on behalf of the Crew, shared by everyone in it on the
-- Expense date. Amounts are whole crowns; the Expense date is a Europe/Prague calendar date;
-- instants are Unix seconds (UTC).

CREATE TABLE expenses (
    id INTEGER PRIMARY KEY,
    payer_id INTEGER NOT NULL REFERENCES members (id),
    amount INTEGER NOT NULL CHECK (amount BETWEEN 1 AND 1000000),
    description TEXT NOT NULL,
    expense_date TEXT NOT NULL,
    -- The Logger; NULL for entries imported from Heimdall.
    logged_by INTEGER REFERENCES members (id),
    logged_at INTEGER NOT NULL,
    edited_by INTEGER REFERENCES members (id),
    edited_at INTEGER,
    -- A deleted Expense is kept, but left out of every list and Balance.
    deleted_by INTEGER REFERENCES members (id),
    deleted_at INTEGER
);
