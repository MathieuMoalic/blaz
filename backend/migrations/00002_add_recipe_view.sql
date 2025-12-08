CREATE VIEW IF NOT EXISTS shopping_items_view AS
SELECT
  id,
  CASE
    WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
      THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
    WHEN quantity IS NOT NULL
      THEN TRIM(printf('%g', quantity)) || ' ' || name
    ELSE name
  END AS text,
  done,
  category
FROM shopping_items;

