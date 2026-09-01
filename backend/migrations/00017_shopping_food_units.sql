-- Canonical shopping storage units + Food-based merge keys.
--
-- Mass is stored in g and volume in ml so that "500 g" and "1 kg" of the
-- same food merge into a single row. The view formats >= 1000 g/ml back to
-- kg/L for display.
--
-- The view also gains:
--   * display name from foods.canonical_name (fallback: legacy si.name)
--   * display category = COALESCE(override name, food's category name,
--     legacy si.category text)

DROP VIEW IF EXISTS shopping_items_view;

UPDATE shopping_items
   SET quantity = CASE WHEN quantity IS NULL THEN NULL ELSE quantity * 1000.0 END,
       unit = 'g'
 WHERE unit = 'kg';

UPDATE shopping_items
   SET quantity = CASE WHEN quantity IS NULL THEN NULL ELSE quantity * 1000.0 END,
       unit = 'ml'
 WHERE unit = 'L';

CREATE VIEW shopping_items_view AS
SELECT
  si.id,
  CASE
    WHEN si.quantity IS NOT NULL AND si.unit IS NOT NULL AND si.unit <> '' THEN
      CASE
        WHEN si.unit = 'g' AND si.quantity >= 1000 THEN
          TRIM(printf('%g', si.quantity / 1000.0)) || ' kg ' || COALESCE(f.canonical_name, si.name)
        WHEN si.unit = 'ml' AND si.quantity >= 1000 THEN
          TRIM(printf('%g', si.quantity / 1000.0)) || ' L ' || COALESCE(f.canonical_name, si.name)
        ELSE
          TRIM(printf('%g', si.quantity)) || ' ' || si.unit || ' ' || COALESCE(f.canonical_name, si.name)
      END
    WHEN si.quantity IS NOT NULL THEN
      TRIM(printf('%g', si.quantity)) || ' ' || COALESCE(f.canonical_name, si.name)
    ELSE
      COALESCE(f.canonical_name, si.name)
  END AS text,
  si.done,
  COALESCE(co.name, cf.name, si.category) AS category,
  si.notes,
  si.recipe_ids,
  (
    SELECT GROUP_CONCAT(
      r.title ||
      CASE
        WHEN mp.day IS NOT NULL THEN ' (' || mp.day || ')'
        ELSE ''
      END,
      ', '
    )
    FROM recipes r
    JOIN json_each(si.recipe_ids) je ON r.id = je.value
    LEFT JOIN (
      SELECT recipe_id, MIN(day) as day
      FROM meal_plan
      WHERE date(day) >= date('now')
      GROUP BY recipe_id
    ) mp ON r.id = mp.recipe_id
  ) AS recipe_titles,
  si.food_id,
  COALESCE(f.canonical_name, si.name) AS name,
  si.quantity,
  si.unit,
  COALESCE(si.category_override_id, f.category_id) AS category_id,
  (si.category_override_id IS NOT NULL) AS category_is_override
FROM shopping_items si
LEFT JOIN foods f ON si.food_id = f.id
LEFT JOIN shopping_categories co ON si.category_override_id = co.id
LEFT JOIN shopping_categories cf ON f.category_id = cf.id;
