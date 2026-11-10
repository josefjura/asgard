-- A Batch's Recipe: the BeerXML file a Member uploaded, kept byte for byte as it came. It is
-- never transcoded: the reader works out the encoding from the bytes themselves, so an
-- ISO-8859-1 file stored as UTF-8 would garble its names. It is read again on every view rather
-- than parsed into columns, so the Recipe viewer can improve without a migration.
--
-- The cap matches the domain's `RECIPE_MAX_BYTES`, 1 MB.
ALTER TABLE batches ADD COLUMN recipe BLOB CHECK (recipe IS NULL OR length(recipe) <= 1048576);
