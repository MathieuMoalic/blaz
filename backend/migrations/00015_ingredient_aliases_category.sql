-- Add category column to ingredient_aliases for shopping categorization
-- confirmed_category=1 means the user has manually set this category and it should not be auto-changed
ALTER TABLE ingredient_aliases ADD COLUMN category TEXT DEFAULT 'Other';
ALTER TABLE ingredient_aliases ADD COLUMN confirmed_category INTEGER DEFAULT 0;
