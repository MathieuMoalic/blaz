-- Cleanup: final migration of the legacy string-based alias system.
--
-- Seeds `foods` / `food_aliases` from `ingredient_aliases`, then drops the
-- legacy tables. Idempotent: on databases already migrated by the earlier
-- startup seeder the rows already exist, and ON CONFLICT keeps the existing
-- user/confirmed mappings and never rewrites them.
--
-- Normalization matches `crate::units::normalize_name`: lowercase, every run
-- of whitespace collapsed to a single space, trimmed.

CREATE TEMP TABLE legacy_food_source AS
SELECT
  raw_name,
  canonical_name,
  confirmed,
  category,
  confirmed_category,
  lower(trim(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(canonical_name, char(9), ' '), char(10), ' '), char(13), ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '))) AS canonical_norm,
  lower(trim(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(raw_name, char(9), ' '), char(10), ' '), char(13), ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '), '  ', ' '))) AS raw_norm
FROM ingredient_aliases;

-- Foods: one per distinct canonical name (conflicts merge into the winner).
INSERT INTO foods (canonical_name, normalized_name, category_id, category_source)
SELECT
  TRIM(a.canonical_name),
  a.canonical_norm,
  (SELECT c.id FROM shopping_categories c
    WHERE c.name = COALESCE(
      (SELECT a2.category FROM legacy_food_source a2
        WHERE a2.canonical_name = a.canonical_name
              AND a2.confirmed_category = 1
              AND a2.category IS NOT NULL AND TRIM(a2.category) <> ''
        LIMIT 1),
      (SELECT a2.category FROM legacy_food_source a2
        WHERE a2.canonical_name = a.canonical_name
              AND a2.category IS NOT NULL AND TRIM(a2.category) <> ''
        GROUP BY a2.category ORDER BY COUNT(*) DESC, a2.category ASC LIMIT 1)
    )),
  CASE WHEN EXISTS (
    SELECT 1 FROM legacy_food_source a2
      WHERE a2.canonical_name = a.canonical_name AND a2.confirmed_category = 1
  ) THEN 'user' ELSE 'migrated' END
FROM (SELECT canonical_name, MIN(canonical_norm) AS canonical_norm
      FROM legacy_food_source GROUP BY canonical_name) a
WHERE 1 = 1  -- keeps 'ON' unambiguous as the UPSERT clause (SQLite parser quirk)
ON CONFLICT(normalized_name) DO NOTHING;

-- Aliases: one per legacy raw name, pointing at its Food.
INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed)
SELECT a.raw_name, a.raw_norm, f.id, 'migrated', a.confirmed
FROM legacy_food_source a
JOIN foods f ON f.normalized_name = a.canonical_norm
WHERE a.raw_norm <> ''
ON CONFLICT(normalized_alias) DO UPDATE SET
  alias = CASE WHEN (excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user')
               THEN excluded.alias ELSE food_aliases.alias END,
  food_id = CASE WHEN (excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user')
               THEN excluded.food_id ELSE food_aliases.food_id END,
  source = CASE WHEN (excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user')
               THEN excluded.source ELSE food_aliases.source END,
  confirmed = CASE WHEN (excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user')
               THEN excluded.confirmed ELSE food_aliases.confirmed END,
  confidence = CASE WHEN (excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user')
               THEN excluded.confidence ELSE food_aliases.confidence END;

-- The legacy tables are now a pure migration input: drop them.
DROP TABLE ingredient_aliases;
DROP TABLE ingredient_normalizations;