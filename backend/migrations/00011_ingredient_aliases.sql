-- User-managed ingredient alias table.
-- Maps raw ingredient names (as they arrive from recipes) to canonical shopping item names.
-- confirmed=0: auto-generated on first encounter, may need review.
-- confirmed=1: explicitly set by the user, never auto-changed.
CREATE TABLE ingredient_aliases (
    raw_name       TEXT    PRIMARY KEY,
    canonical_name TEXT    NOT NULL,
    confirmed      INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Seed from existing LLM normalization cache (all unconfirmed).
INSERT OR IGNORE INTO ingredient_aliases (raw_name, canonical_name, confirmed)
SELECT raw_name, normalized_name, 0
FROM ingredient_normalizations;
