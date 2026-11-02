-- Who bought an Item and when (Unix seconds, UTC). Both are empty while the Item is open, and
-- both are set once it's bought.

ALTER TABLE items ADD COLUMN bought_by INTEGER REFERENCES members (id);
ALTER TABLE items ADD COLUMN bought_at INTEGER CHECK ((bought_at IS NULL) = (bought_by IS NULL));
