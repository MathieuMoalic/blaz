-- Cache for ingredient name normalizations to avoid repeated LLM calls
CREATE TABLE IF NOT EXISTS ingredient_normalizations (
    raw_name TEXT PRIMARY KEY,
    normalized_name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_normalizations_created 
    ON ingredient_normalizations(created_at);
