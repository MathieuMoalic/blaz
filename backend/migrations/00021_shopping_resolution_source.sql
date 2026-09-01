-- Record provenance of legacy shopping rows during backfill.
--
-- `food_id IS NULL AND resolution_source = 'unresolved'` distinguishes
-- "already attempted via the resolver but unresolved" (skipped by default
-- backfill runs) from "never attempted" (processed normally).

ALTER TABLE shopping_items ADD COLUMN resolution_source TEXT;