use clap::{ArgAction, Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser, Debug)]
#[command(name = "blaz", version, about = "HTTP API server for Blaz")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub config: Config,
}

#[derive(Subcommand, Debug, Clone, Copy)]
pub enum Commands {
    /// Generate an Argon2 password hash for authentication
    HashPassword,
}

/// Blaz server configuration
#[derive(Parser, Debug, Clone)]
pub struct Config {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short = 'v', action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Decrease verbosity (-q, -qq, -qqq)
    #[arg(short = 'q', action = ArgAction::Count, global = true)]
    pub quiet: u8,

    /// Address to bind the HTTP server to
    #[arg(long, env = "BLAZ_BIND_ADDR", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,

    /// Directory to store media files
    #[arg(long, env = "BLAZ_MEDIA_DIR", default_value = "media")]
    pub media_dir: PathBuf,

    /// Database path
    #[arg(long, env = "BLAZ_DATABASE_PATH", default_value = "blaz.sqlite")]
    pub database_path: String,

    /// Optional log file path (logs are written to stdout + this file)
    #[arg(long, env = "BLAZ_LOG_FILE", default_value = "blaz.logs")]
    pub log_file: PathBuf,

    /// CORS allowed origin (e.g., <https://blaz.yourdomain.com>)
    /// If not set, allows all origins (⚠️ insecure for production!)
    #[arg(long, env = "BLAZ_CORS_ORIGIN")]
    pub cors_origin: Option<String>,

    /// JWT secret for authentication (if not set, generates a random one)
    #[arg(long, env = "BLAZ_JWT_SECRET")]
    pub jwt_secret: Option<String>,

    /// Argon2 password hash for authentication (required for production)
    /// Generate with: blaz hash-password
    #[arg(long, env = "BLAZ_PASSWORD_HASH")]
    pub password_hash: Option<String>,

    /// LLM API key (optional, for recipe parsing and macro estimation)
    #[arg(long, env = "BLAZ_LLM_API_KEY")]
    pub llm_api_key: Option<String>,

    /// LLM model to use
    #[arg(long, env = "BLAZ_LLM_MODEL", default_value = "deepseek/deepseek-chat")]
    pub llm_model: String,

    /// LLM vision model to use for image-based recipe import
    #[arg(long, env = "BLAZ_LLM_VISION_MODEL", default_value = "google/gemini-2.0-flash-001")]
    pub llm_vision_model: String,

    /// LLM API URL
    #[arg(long, env = "BLAZ_LLM_API_URL", default_value = "https://openrouter.ai/api/v1")]
    pub llm_api_url: String,

    /// System prompt for recipe import
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_IMPORT", default_value = DEFAULT_SYSTEM_PROMPT_IMPORT)]
    pub system_prompt_import: String,

    /// System prompt for macro estimation
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_MACROS", default_value = DEFAULT_SYSTEM_PROMPT_MACROS)]
    pub system_prompt_macros: String,

    /// System prompt for ingredient normalization
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_NORMALIZE", default_value = DEFAULT_SYSTEM_PROMPT_NORMALIZE)]
    pub system_prompt_normalize: String,

    /// System prompt for prep reminder detection
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_PREP_REMINDERS", default_value = DEFAULT_SYSTEM_PROMPT_PREP_REMINDERS)]
    pub system_prompt_prep_reminders: String,
}

const DEFAULT_SYSTEM_PROMPT_IMPORT: &str = r#"You are a precise recipe data extractor and normalizer.

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
Answer only with the final JSON."#;

const DEFAULT_SYSTEM_PROMPT_NORMALIZE: &str = r#"You are an ingredient name normalizer for a shopping list.

Your task: Convert ingredient descriptions to their base form for merging duplicate items.

Rules:
1. Remove quantities: "3 apples" → "apple"
2. Singular form: "potatoes" → "potato", "tomatoes" → "tomato"
3. Remove size/quality adjectives: "large", "small", "medium", "fresh", "ripe", etc.
4. Remove container words: "cloves", "bunch", "head", "stalk", "sprig"
5. Remove prep instructions: "diced", "chopped", "sliced", etc.
6. Keep compound names intact: "sweet potato" stays "sweet potato"
7. Lowercase everything
8. Trim whitespace

INPUT: Either a single ingredient OR a JSON array of ingredients.
OUTPUT: If single string input → return ONLY the normalized name.
        If JSON array input → return JSON array of normalized names in same order.

Examples (single):
- "3 Cloves garlic" → "garlic"
- "5 Potatoes" → "potato"
- "1 Medium sweet potato" → "sweet potato"
- "4 Small sweet potatoes, scrubbed" → "sweet potato"
- "2 large red onions, diced" → "red onion"
- "1 bunch fresh parsley" → "parsley"

Examples (batch):
- ["3 Cloves garlic", "5 Potatoes", "1 bunch fresh parsley"] → ["garlic", "potato", "parsley"]
"#;

const DEFAULT_SYSTEM_PROMPT_MACROS: &str = r#"You are a precise nutrition estimator.

Return STRICT JSON with per-ingredient macros estimates:
{
  "ingredients": [
    {
      "name": "ingredient name",
      "protein_g": number,
      "fat_g": number,
      "carbs_g": number,
      "skip": boolean  // true if ingredient is negligible (< 5 calories)
    }
  ]
}

Rules:
- Estimate macros for EACH ingredient separately based on the quantity given.
- Use common nutrition databases and reasonable approximations.
- fat_g includes saturated + unsaturated combined.
- carbs_g excludes fiber (i.e., net carbs).
- Set "skip": true for ingredients with negligible calories (< 5 kcal):
  * Water, broth (unless cream-based)
  * Salt, pepper, spices in small amounts (< 1 tsp)
  * Herbs, garlic, onion in small amounts (< 1 clove/piece)
  * Lemon juice, vinegar, soy sauce in small amounts
  * Baking powder, baking soda, yeast
- Set "skip": false for all other ingredients.
- If servings are provided, compute PER SERVING. Otherwise, compute for the ENTIRE RECIPE.
- Always include all ingredients in the array, even if skipped.
- Never add extra fields or commentary."#;

const DEFAULT_SYSTEM_PROMPT_PREP_REMINDERS: &str = r#"You are a recipe prep planner.

Given a list of recipe instructions, identify any steps that must be done significantly in advance (at least 2 hours before cooking).

Common examples: soaking beans/legumes overnight, marinating meat, making dough that needs to rise, chilling, freezing, fermenting, brining, or any step that explicitly says "overnight", "the day before", "X hours ahead", etc.

Return STRICT JSON — an array of objects. If no advance prep is needed, return an empty array.

OUTPUT FORMAT:
[
  {"step": "short description of what to do", "hours_before": N}
]

Rules:
- "step" must be a short, actionable phrase (max ~10 words), e.g. "Soak beans overnight" or "Marinate chicken for 4 hours"
- "hours_before" is an integer: the minimum number of hours before the meal this step should be started
- Only include steps requiring AT LEAST 2 hours lead time
- Do not include regular cooking steps
- Do not include commentary or extra fields
- Return [] if nothing qualifies

Answer only with the JSON array."#;

impl Config {
    #[must_use]
    pub fn verbosity_delta(&self) -> i16 {
        i16::from(self.verbose) - i16::from(self.quiet)
    }
    #[must_use]
    pub fn log_filter(&self) -> &'static str {
        match self.verbosity_delta() {
            d if d <= -2 => "error",
            -1 => "warn",
            0 => "info,blaz=info,axum=info,tower_http=info",
            1 => "debug,blaz=debug,axum=info,tower_http=info,sqlx=warn",
            2 => "trace,blaz=trace,axum=debug,tower_http=trace,sqlx=info,hyper=info",
            _ => "trace,blaz=trace,axum=trace,tower_http=trace,sqlx=debug,hyper=debug",
        }
    }
}
