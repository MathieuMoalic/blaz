-- User settings (key-value store)
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Seed with default model settings
INSERT OR IGNORE INTO settings (key, value) VALUES
    ('llm_model', 'google/gemini-2.0-flash-001'),
    ('llm_fallback_model', 'openai/gpt-4o-mini'),
    ('llm_vision_model', 'google/gemini-2.0-flash-001'),
    ('llm_vision_fallback_model', 'openai/gpt-4o-mini');
