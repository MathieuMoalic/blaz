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

    /// LLM API URL
    #[arg(long, env = "BLAZ_LLM_API_URL", default_value = "https://openrouter.ai/api/v1")]
    pub llm_api_url: String,

    /// System prompt for recipe import
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_IMPORT", default_value = DEFAULT_SYSTEM_PROMPT_IMPORT)]
    pub system_prompt_import: String,

    /// System prompt for macro estimation
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_MACROS", default_value = DEFAULT_SYSTEM_PROMPT_MACROS)]
    pub system_prompt_macros: String,
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

const DEFAULT_SYSTEM_PROMPT_MACROS: &str = r#"You are a precise nutrition estimator.

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
- Never add extra fields or commentary."#;

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
