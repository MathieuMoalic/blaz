PRAGMA foreign_keys = ON;

CREATE TABLE recipes (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  title              TEXT    NOT NULL,
  source             TEXT    NOT NULL DEFAULT '',
  "yield"            TEXT    NOT NULL DEFAULT '',
  notes              TEXT    NOT NULL DEFAULT '',
  created_at         TEXT    NOT NULL DEFAULT (CURRENT_TIMESTAMP),
  updated_at         TEXT    NOT NULL DEFAULT (CURRENT_TIMESTAMP),

  ingredients        TEXT    NOT NULL,  -- JSON array of objects {quantity, unit, name}
  instructions       TEXT    NOT NULL,  -- JSON array of strings

  image_path_small   TEXT,
  image_path_full    TEXT,

  macros             TEXT
);

CREATE TABLE meal_plan (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  day        TEXT    NOT NULL,              -- 'YYYY-MM-DD'
  recipe_id  INTEGER NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
  title      TEXT    NOT NULL,

  UNIQUE(day, recipe_id)
);

CREATE TABLE shopping_items (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,

  name      TEXT,        -- display/original name (can be empty)
  unit      TEXT,        -- canonical units: g, kg, ml, L, tsp, tbsp (or NULL)
  quantity  REAL,        -- nullable

  -- canonical merge key: "<unit>|<lower(name)>" or "|<lower(name)>"
  key       TEXT UNIQUE,

  done      BOOLEAN NOT NULL DEFAULT 0,    -- 0/1
  category  TEXT                           
);

CREATE TABLE users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
);

CREATE TABLE IF NOT EXISTS settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  jwt_secret  TEXT NOT NULL DEFAULT (lower(hex(randomblob(32)))),
  llm_api_key TEXT,
  llm_model   TEXT NOT NULL DEFAULT 'deepseek/deepseek-chat-v3.1',
  llm_api_url TEXT NOT NULL DEFAULT 'https://openrouter.ai/api/v1',
   system_prompt_import  TEXT NOT NULL DEFAULT 'You are a precise recipe data extractor and normalizer.

INPUT: plain text from a recipe page (any language).
OUTPUT: STRICT JSON with exactly these keys:
{
  "title": string,
  "ingredients": [
    {
      "quantity": null | number,
      "unit": null | "g" | "kg" | "ml" | "L" | "tsp" | "tbsp",
      "name": string,
      "prep": null | string
    }
  ],
  "instructions": [string]
}

TASK:
- Translate to English.
- Extract a clean, concise title.
- Convert ALL imperial units to metric in the INGREDIENTS.
  * Allowed units: g, kg, ml, L, tsp, tbsp.
  * Never use: cup, cups, oz, ounce, ounces, fl oz, pound, lb.
  * Keep tsp and tbsp abbreviations as written (do not spell out).
- For solid items, convert oz→g (1 oz ≈ 28 g).
  For liquids, convert fl oz→ml (1 fl oz ≈ 30 ml).
  For cups→ml (1 cup ≈ 240 ml).
- If an ingredient has preparation words (e.g., sliced, diced, minced, grated, softened),
  place them ONLY in the "prep" field.
  Example:
    {"quantity":2,"unit":null,"name":"carrots","prep":"diced"}
- The "name" field must NOT contain prep words.
- If data is missing, return an empty array for that key.
- Do NOT include commentary or extra keys.
- When a quantity is a range, replace the range with the mean value.
- Round quantities sensibly.
- Use 0.5/0.25/0.75 style; never 1/2, 1/4, etc.
- If no numeric quantity, set "quantity": null and "unit": null.
- "instructions": array of steps (strings). No commentary.
- Remove all mentions of "Vegan" inside the title.

FORMAT EXAMPLE:
{
  "title": "Carrot Soup",
  "ingredients": [
    {"quantity":2,"unit":null,"name":"carrots","prep":"diced"},
    {"quantity":150,"unit":"g","name":"flour","prep":null},
    {"quantity":2,"unit":null,"name":"cloves garlic","prep":"minced"}
  ],
  "instructions": [
    "Cook the garlic.",
    "Fold in flour."
  ]
}

SELF-CHECK:
Before answering, verify no banned units appear in "unit".
Verify "name" does not contain comma-prep fragments.
Answer only with the final JSON.
',
  system_prompt_macros  TEXT NOT NULL DEFAULT 'You are a precise nutrition estimator.

Return STRICT JSON with the following keys, all numeric grams with up to 1 decimal:
{
  "protein_g": number,
  "fat_g": number,     // saturated + unsaturated combined
  "carbs_g": number    // carbohydrates EXCLUDING fiber
}

Rules:
- Use common nutrition databases and reasonable approximations.
- Always include ALL three keys.
- Carbs exclude fiber (i.e., net carbs).
- If servings are provided, compute PER SERVING. Otherwise, compute for the ENTIRE RECIPE.
- Never add extra fields or commentary.');

-- Create default settings
INSERT OR IGNORE INTO settings (id) VALUES (1);
-- =====================================================================
-- Indexes
-- =====================================================================

CREATE INDEX IF NOT EXISTS idx_recipes_updated_at ON recipes(updated_at);
CREATE INDEX IF NOT EXISTS idx_meal_plan_day ON meal_plan(day);
CREATE INDEX IF NOT EXISTS users_email_idx ON users(email);
CREATE INDEX IF NOT EXISTS shopping_items_category_idx ON shopping_items(category);

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


