-- recipes
CREATE TABLE IF NOT EXISTS recipes (
  id    INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL
);

-- meal plan: day -> recipe
CREATE TABLE IF NOT EXISTS meal_plan (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  day        TEXT NOT NULL,           -- YYYY-MM-DD
  recipe_id  INTEGER NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
  CONSTRAINT uniq_day_recipe UNIQUE (day, recipe_id)
);

-- shopping list
CREATE TABLE IF NOT EXISTS shopping_items (
  id    INTEGER PRIMARY KEY AUTOINCREMENT,
  text  TEXT NOT NULL,
  done  INTEGER NOT NULL DEFAULT 0     -- 0/1
);

