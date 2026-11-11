-- Brewing slots: dates the Crew has set aside for brewing. The date is a Europe/Prague calendar
-- date (`YYYY-MM-DD`), like `expenses.expense_date`; instants are Unix seconds (UTC). A slot's
-- state (upcoming, "Did you brew?", done) follows from its date and its Batch and is never
-- stored.

CREATE TABLE brewing_slots (
    id INTEGER PRIMARY KEY,
    -- One slot per date.
    date TEXT NOT NULL UNIQUE,
    -- The Planned Batch brewed that day, if one is chosen: at most one Batch per slot and one
    -- slot per Batch. Deleting the Batch leaves the slot without one.
    batch_number INTEGER UNIQUE REFERENCES batches (number) ON DELETE SET NULL,
    -- The pending Reminder for the evening before, if there is one. A Member deleting it leaves
    -- the slot without one.
    reminder_id INTEGER REFERENCES reminders (id) ON DELETE SET NULL,
    created_by INTEGER NOT NULL REFERENCES members (id),
    created_at INTEGER NOT NULL
);
