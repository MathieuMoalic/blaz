-- Constant-text defaults are fine:
ALTER TABLE recipes ADD COLUMN source       TEXT NOT NULL DEFAULT '';
ALTER TABLE recipes ADD COLUMN "yield"      TEXT NOT NULL DEFAULT '';
ALTER TABLE recipes ADD COLUMN notes        TEXT NOT NULL DEFAULT '';
ALTER TABLE recipes ADD COLUMN ingredients  TEXT NOT NULL DEFAULT '[]';  -- JSON array as TEXT
ALTER TABLE recipes ADD COLUMN instructions TEXT NOT NULL DEFAULT '[]';  -- JSON array as TEXT

-- Timestamps: add without defaults (expressions not allowed here)
ALTER TABLE recipes ADD COLUMN created_at   TEXT;
ALTER TABLE recipes ADD COLUMN updated_at   TEXT;

-- Backfill existing rows
UPDATE recipes
SET created_at = COALESCE(created_at, CURRENT_TIMESTAMP),
    updated_at = COALESCE(updated_at, CURRENT_TIMESTAMP);

