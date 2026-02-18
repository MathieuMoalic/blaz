-- Add recipe_id to shopping_items to track which recipe an ingredient came from
ALTER TABLE shopping_items ADD COLUMN recipe_id INTEGER REFERENCES recipes(id) ON DELETE SET NULL;

-- Recreate view to include recipe title
DROP VIEW IF EXISTS shopping_items_view;

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
  si.recipe_id,
  r.title AS recipe_title
FROM shopping_items si
LEFT JOIN recipes r ON si.recipe_id = r.id;
