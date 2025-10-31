CREATE TABLE IF NOT EXISTS settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  llm_api_key TEXT,
  llm_model   TEXT NOT NULL DEFAULT 'meta-llama/Llama-3.1-8B-Instruct',
  llm_api_url TEXT NOT NULL DEFAULT 'https://router.huggingface.co/v1',
  allow_registration INTEGER NOT NULL DEFAULT 1,
  system_prompt_import  TEXT NOT NULL DEFAULT '',
  system_prompt_macros  TEXT NOT NULL DEFAULT ''
);
INSERT OR IGNORE INTO settings (id) VALUES (1);
