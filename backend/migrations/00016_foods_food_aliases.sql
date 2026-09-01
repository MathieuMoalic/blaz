-- Canonical Food identity for ingredients.
--
-- foods        stable identity + canonical metadata (display name, default
--              shopping category with provenance)
-- food_aliases normalized alias -> food_id, the fast deterministic lookup
--
-- shopping_items gains:
--   food_id              canonical identity of the item's Food (nullable
--                        during the transition; legacy rows are backfilled)
--   category_override_id one-time per-item category override; the displayed
--                        category is COALESCE(override, foods.category_id)
--
-- The legacy string-based tables (ingredient_aliases,
-- ingredient_normalizations) are left in place: they are seeding inputs for
-- foods/food_aliases and are removed by a later cleanup migration.

DROP VIEW IF EXISTS shopping_items_view;

CREATE TABLE foods (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    canonical_name TEXT NOT NULL,
    normalized_name TEXT NOT NULL UNIQUE,

    category_id INTEGER NULL,

    category_source TEXT NOT NULL DEFAULT 'unknown',
    -- expected values:
    -- unknown
    -- llm
    -- user
    -- migrated
    -- default

    category_confidence REAL NULL,

    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),

    FOREIGN KEY(category_id)
        REFERENCES shopping_categories(id)
);

CREATE TABLE food_aliases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    alias TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,

    food_id INTEGER NOT NULL,

    source TEXT NOT NULL DEFAULT 'automatic',
    -- automatic
    -- llm
    -- user
    -- migrated

    confidence REAL NULL,
    confirmed INTEGER NOT NULL DEFAULT 0,

    created_at INTEGER NOT NULL DEFAULT (unixepoch()),

    FOREIGN KEY(food_id) REFERENCES foods(id),

    UNIQUE(normalized_alias)
);

CREATE INDEX idx_foods_category_id ON foods(category_id);
CREATE INDEX idx_food_aliases_food_id ON food_aliases(food_id);

ALTER TABLE shopping_items ADD COLUMN food_id INTEGER REFERENCES foods(id);
ALTER TABLE shopping_items ADD COLUMN category_override_id INTEGER REFERENCES shopping_categories(id);

CREATE INDEX idx_shopping_items_food_id ON shopping_items(food_id);

-- Recreated view: legacy columns keep their exact semantics; identity
-- columns are appended for the upcoming Food-based flows.
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
  si.name,
  si.quantity,
  si.unit,
  COALESCE(si.category_override_id, f.category_id) AS category_id,
  (si.category_override_id IS NOT NULL) AS category_is_override
FROM shopping_items si
LEFT JOIN foods f ON si.food_id = f.id;
