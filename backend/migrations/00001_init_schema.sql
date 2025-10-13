PRAGMA foreign_keys = ON;

-- Recipes
CREATE TABLE recipes (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  title              TEXT    NOT NULL,
  source             TEXT    NOT NULL DEFAULT '',
  "yield"            TEXT    NOT NULL DEFAULT '',
  notes              TEXT    NOT NULL DEFAULT '',
  created_at         TEXT    NOT NULL DEFAULT (CURRENT_TIMESTAMP),
  updated_at         TEXT    NOT NULL DEFAULT (CURRENT_TIMESTAMP),

  -- JSON stored as TEXT
  ingredients        TEXT    NOT NULL,  -- JSON array of objects {quantity, unit, name}
  instructions       TEXT    NOT NULL,  -- JSON array of strings

  -- Images
  image_path         TEXT,              -- legacy (keep if you still read it)
  image_path_small   TEXT,
  image_path_full    TEXT
);

-- Meal plan
CREATE TABLE meal_plan (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  day        TEXT    NOT NULL,              -- 'YYYY-MM-DD'
  recipe_id  INTEGER NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
  title      TEXT    NOT NULL,

  UNIQUE(day, recipe_id)
);

-- Shopping items (structured, supports merging)
CREATE TABLE shopping_items (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,

  -- legacy text (optional)
  text      TEXT,

  -- structured
  name      TEXT,        -- display/original name (can be empty)
  unit      TEXT,        -- canonical units: g, kg, ml, L, tsp, tbsp (or NULL)
  quantity  REAL,        -- nullable

  -- canonical merge key: "<unit>|<lower(name)>" or "|<lower(name)>"
  key       TEXT UNIQUE,

  done      INTEGER NOT NULL DEFAULT 0
);

-- Helpful indexes (optional)
CREATE INDEX IF NOT EXISTS idx_recipes_updated_at ON recipes(updated_at);
CREATE INDEX IF NOT EXISTS idx_meal_plan_day ON meal_plan(day);

