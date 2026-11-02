-- The Shopping list: things the Crew needs to buy. Instants are Unix seconds (UTC); they are
-- shown in Europe/Prague.

CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    note TEXT CHECK (length(note) BETWEEN 1 AND 500),
    urgency TEXT NOT NULL CHECK (urgency IN ('asap', 'next-brewday', 'nice-to-have')),
    added_by INTEGER NOT NULL REFERENCES members (id),
    added_at INTEGER NOT NULL
);
