-- Add soft delete support for recipes
ALTER TABLE recipes ADD COLUMN deleted_at TEXT DEFAULT NULL;

-- Index for efficient filtering of non-deleted recipes
CREATE INDEX idx_recipes_deleted_at ON recipes(deleted_at);
