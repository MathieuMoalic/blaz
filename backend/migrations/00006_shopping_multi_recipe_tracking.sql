-- Migrate from single recipe_id to recipe_ids JSON array for tracking multiple recipes

-- First, DROP the view that depends on shopping_items
DROP VIEW IF EXISTS shopping_items_view;

-- Migrate existing recipe_id values to recipe_ids JSON array
ALTER TABLE shopping_items ADD COLUMN recipe_ids TEXT;

UPDATE shopping_items 
SET recipe_ids = CASE 
  WHEN recipe_id IS NOT NULL THEN json_array(recipe_id)
  ELSE '[]'
END;

-- Set default for new rows
UPDATE shopping_items SET recipe_ids = '[]' WHERE recipe_ids IS NULL;

-- Recreate the table without recipe_id column
CREATE TABLE shopping_items_new (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  name      TEXT,
  unit      TEXT,
  quantity  REAL,
  key       TEXT UNIQUE,
  done      BOOLEAN NOT NULL DEFAULT 0,
  category  TEXT,
  recipe_ids TEXT NOT NULL DEFAULT '[]'
);

INSERT INTO shopping_items_new (id, name, unit, quantity, key, done, category, recipe_ids)
SELECT id, name, unit, quantity, key, done, category, recipe_ids
FROM shopping_items;

DROP TABLE shopping_items;
ALTER TABLE shopping_items_new RENAME TO shopping_items;

-- NOW recreate view with recipe_ids and aggregated recipe_titles
CREATE VIEW shopping_items_view AS
SELECT
  si.id,
  CASE
    WHEN si.quantity IS NOT NULL AND si.unit IS NOT NULL AND si.unit <> ''
      THEN TRIM(printf('%g', si.quantity)) || ' ' || si.unit || ' ' || si.name
    WHEN si.quantity IS NOT NULL
      THEN TRIM(printf('%g', si.quantity)) || ' ' || si.name
    ELSE si.name
  END AS text,
  si.done,
  si.category,
  si.recipe_ids,
  (
    SELECT GROUP_CONCAT(r.title, ', ')
    FROM recipes r, json_each(si.recipe_ids)
    WHERE r.id = json_each.value
  ) AS recipe_titles
FROM shopping_items si;
