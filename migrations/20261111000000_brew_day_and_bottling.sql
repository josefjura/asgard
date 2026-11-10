-- A Batch's Brew day (brew date, OG, Volume into the fermenter) and Bottling (bottling date, FG).
-- Every value is optional and can be cleared again. The dates are Europe/Prague calendar dates
-- (`YYYY-MM-DD`), like `expenses.expense_date`. The Stage, ABV and attenuation follow from these
-- values and are never stored.
--
-- The ranges match the domain's `Gravity` and `Volume`. The rules across values (FG below OG,
-- bottling on or after brewing, no bottling without brewing) are the domain's to check.

-- Gravities are integer thousandths: 1.052 is 1052.
ALTER TABLE batches ADD COLUMN brew_date TEXT;
ALTER TABLE batches ADD COLUMN og INTEGER CHECK (og BETWEEN 1000 AND 1200);
-- Integer decilitres: 23.5 L is 235.
ALTER TABLE batches ADD COLUMN volume INTEGER CHECK (volume BETWEEN 1 AND 10000);
ALTER TABLE batches ADD COLUMN bottling_date TEXT;
ALTER TABLE batches ADD COLUMN fg INTEGER CHECK (fg BETWEEN 990 AND 1100);
