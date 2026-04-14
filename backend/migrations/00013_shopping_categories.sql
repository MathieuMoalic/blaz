-- Shopping categories table for dynamic category management
CREATE TABLE shopping_categories (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL UNIQUE,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
);

-- Seed default categories (matching the original hardcoded enum)
INSERT INTO shopping_categories (name, sort_order) VALUES
  ('Other', 0),
  ('Fruits', 1),
  ('Vegetables', 2),
  ('Bakery', 3),
  ('Vegan', 4),
  ('Drinks', 5),
  ('Alcohol', 6),
  ('Seasoning', 7),
  ('Canned', 8),
  ('Pantry', 9),
  ('Non-Food', 10),
  ('Pharmacy', 11),
  ('Online', 12),
  ('Online Alcohol', 13);

-- Index for efficient ordering queries
CREATE INDEX idx_shopping_categories_sort_order ON shopping_categories(sort_order);
