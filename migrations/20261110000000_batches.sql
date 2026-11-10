-- The Crew's Batches: one brew each, with a name and free-text Notes. Instants are Unix seconds
-- (UTC); they are shown in Europe/Prague. Brew day, Bottling and the Recipe add their columns
-- in their own migrations.

CREATE TABLE batches (
    -- The Batch number. AUTOINCREMENT, not a plain INTEGER PRIMARY KEY: SQLite then never hands
    -- out a number again once its Batch is deleted, so a number always means the same brew.
    number INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    -- Empty when there are no Notes; line breaks are kept.
    notes TEXT NOT NULL CHECK (length(notes) <= 5000),
    added_by INTEGER NOT NULL REFERENCES members (id),
    added_at INTEGER NOT NULL,
    edited_by INTEGER REFERENCES members (id),
    edited_at INTEGER
);
