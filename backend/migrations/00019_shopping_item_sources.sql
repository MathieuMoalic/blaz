-- Recipe → shopping contribution tracking.
--
-- Every add records a contribution row, so quantities can later be traced
-- and subtracted per recipe ("Remove Shepherd's Pie ingredients").
--
-- Legacy rows are backfilled (approximation: one row per recipe_id recorded
-- on the item, with the item's total quantity).

CREATE TABLE shopping_item_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    shopping_item_id INTEGER NOT NULL,

    source_type TEXT NOT NULL,
    -- recipe
    -- manual
    -- meal_plan
    -- adjustment

    recipe_id INTEGER NULL,
    recipe_ingredient_id TEXT NULL,

    quantity REAL NULL,
    unit TEXT NULL,

    created_at INTEGER NOT NULL DEFAULT (unixepoch()),

    FOREIGN KEY(shopping_item_id)
        REFERENCES shopping_items(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_shopping_item_sources_item
    ON shopping_item_sources(shopping_item_id);

CREATE INDEX idx_shopping_item_sources_recipe
    ON shopping_item_sources(recipe_id);

INSERT INTO shopping_item_sources (shopping_item_id, source_type, recipe_id, quantity, unit)
SELECT si.id, 'recipe', je.value, si.quantity, si.unit
  FROM shopping_items si, json_each(si.recipe_ids) je;