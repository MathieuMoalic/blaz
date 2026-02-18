-- Add meal plan date information to shopping items view
-- This shows when each recipe is planned so users can prioritize shopping

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
  ) AS recipe_titles
FROM shopping_items si;
