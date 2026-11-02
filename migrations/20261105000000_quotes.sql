-- The Crew's Quotes. Instants are Unix seconds (UTC); they are shown in Europe/Prague.
-- added_by is NULL for a Quote imported from Heimdall, which has no Member to name.
-- text_search and author_search are the folded search form of the text and the Author:
-- lowercase, with diacritics removed, computed in Rust on every write. Nothing reads them
-- until search lands; they live here so that doesn't need a backfill outside SQL.

CREATE TABLE quotes (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 2000),
    author TEXT NOT NULL CHECK (length(author) BETWEEN 1 AND 100),
    added_by INTEGER REFERENCES members (id),
    added_at INTEGER NOT NULL,
    text_search TEXT NOT NULL,
    author_search TEXT NOT NULL
);
