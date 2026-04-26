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

    /// Default LLM model (used when client doesn't specify one)
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

    /// System prompt for recipe extraction (stage 1: raw text to strings)
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_EXTRACT", default_value = DEFAULT_SYSTEM_PROMPT_EXTRACT)]
    pub system_prompt_extract: String,

    /// System prompt for ingredient structuring (stage 2: strings to components)
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_STRUCTURE", default_value = DEFAULT_SYSTEM_PROMPT_STRUCTURE)]
    pub system_prompt_structure: String,

    /// System prompt for metric conversion (stage 3: imperial to metric)
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_CONVERT", default_value = DEFAULT_SYSTEM_PROMPT_CONVERT)]
    pub system_prompt_convert: String,

    /// System prompt for macro estimation
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_MACROS", default_value = DEFAULT_SYSTEM_PROMPT_MACROS)]
    pub system_prompt_macros: String,

    /// System prompt for ingredient normalization
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_NORMALIZE", default_value = DEFAULT_SYSTEM_PROMPT_NORMALIZE)]
    pub system_prompt_normalize: String,

    /// System prompt for prep reminder detection
    #[arg(long, env = "BLAZ_SYSTEM_PROMPT_PREP_REMINDERS", default_value = DEFAULT_SYSTEM_PROMPT_PREP_REMINDERS)]
    pub system_prompt_prep_reminders: String,

    /// ntfy URL to send error notifications to (e.g. `<https://ntfy.sh/my-topic>`)
    #[arg(long, env = "BLAZ_NTFY_URL")]
    pub ntfy_url: Option<String>,
}

const DEFAULT_SYSTEM_PROMPT_IMPORT: &str = r###"You are a precise recipe data extractor and normalizer.

INPUT: plain text from a recipe page (any language).
OUTPUT: STRICT JSON with exactly these keys:
{
  "title": string,
  "ingredients": [
    {"section": string}              ← section header (use when recipe has named groups)
    |
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
- If the recipe ingredients have named groups (e.g. "For the sauce", "Topping", "Dough"),
  insert a {"section": "Name"} object BEFORE the ingredients of that group.
  * Use a short, clean English name for each section (e.g. "Sauce", "Topping", "Dough").
  * If all ingredients belong to one unnamed group, do NOT add any section headers.
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
  * If the recipe instructions have named sections (e.g. "Make the sauce", "To serve"),
    insert a string "## Section Name" BEFORE the steps of that section.
    Example: ["## Pulled Jackfruit", "Shred the jackfruit.", "## Tzatziki", "Mix yogurt."]
  * Only add section headers when the recipe text clearly names the groups.
- Remove all mentions of "Vegan" inside the title.

FORMAT EXAMPLE (with sections):
{
  "title": "BBQ Pulled Jackfruit",
  "ingredients": [
    {"section": "Pulled Jackfruit"},
    {"quantity":560,"unit":"g","name":"jackfruit","prep":"drained and rinsed"},
    {"quantity":1,"unit":"tbsp","name":"olive oil","prep":null},
    {"section": "Tzatziki"},
    {"quantity":240,"unit":"ml","name":"non-dairy yogurt","prep":null},
    {"quantity":0.5,"unit":null,"name":"cucumber","prep":"grated"}
  ],
  "instructions": [
    "## Pulled Jackfruit",
    "Shred the jackfruit and toss with spices.",
    "Cook until caramelised.",
    "## Tzatziki",
    "Grate cucumber and squeeze out excess liquid.",
    "Mix with yogurt, garlic, and lemon juice."
  ]
}

SELF-CHECK:
Before answering, verify no banned units appear in "unit".
Verify "name" does not contain comma-prep fragments.
Answer only with the final JSON."###;

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

const DEFAULT_SYSTEM_PROMPT_EXTRACT: &str = r###"You are a recipe text extractor.

INPUT: Plain text from a recipe page (any language).
OUTPUT: STRICT JSON with exactly these keys:
{
  "title": string,
  "ingredients": [string],
  "instructions": [string]
}

CRITICAL: Your job is to EXTRACT, not parse or simplify. Extract EVERY SINGLE ingredient from the recipe, preserving ALL details.

RULES FOR INGREDIENTS:
- Extract ingredients as an array of strings, one per line
- Extract EVERY ingredient - don't skip any, even if there are many
- PRESERVE EXACT TEXT including:
  * ALL quantities (2, 1/2, 2-3, etc.)
  * ALL units (cups, oz, g, tablespoons, cloves, cans, etc.)
  * ALL parenthetical details (450g, 15 oz, drained, etc.)
  * ALL descriptions (fresh, kosher, non-dairy, etc.)
  * ALL prep notes (chopped, diced, grated, etc.)
- If the recipe has named sections (e.g., "For the sauce", "Citrus Vinaigrette", "Topping"):
  * Insert a string starting with "## " followed by the section name
  * Example: "## Sauce", "## Citrus Vinaigrette", "## Topping"
  * Make sure to include ALL ingredients from ALL sections
- Translate to English if needed
- Do NOT simplify, summarize, or remove any details
- Do NOT skip ingredients - if there are 20 ingredients, return all 20

RULES FOR INSTRUCTIONS:
- Extract instructions as an array of strings, one step per line
- If the recipe has named sections:
  * Insert a string starting with "## " followed by the section name
- Preserve all details and timing information
- Translate to English if needed

RULES FOR TITLE:
- Extract a clean, concise title
- Remove "Vegan" if present
- Translate to English if needed

BAD EXAMPLES (what NOT to do):
❌ "2 cups (400g) chickpeas, drained" → "Chickpeas" (lost quantities!)
❌ "1 tablespoon olive oil" → "Olive oil" (lost quantity!)
❌ "6 garlic cloves, minced" → "Fresh garlic" (lost quantity and prep!)

GOOD EXAMPLES (correct extraction):
✅ "2 cups (400g) chickpeas, drained" → "2 cups (400g) chickpeas, drained"
✅ "1 tablespoon olive oil" → "1 tablespoon olive oil"
✅ "6 garlic cloves, minced" → "6 garlic cloves, minced"
✅ "1 1/2 inch piece ginger, peeled" → "1 1/2 inch piece ginger, peeled"
✅ "2-3 Thai chili peppers, sliced" → "2-3 Thai chili peppers, sliced"

FORMAT EXAMPLE:
{
  "title": "Roasted Beet and Fennel Salad",
  "ingredients": [
    "3 medium red or golden beets (12 to 15 ounces, 340-430g)",
    "2 medium fennel bulbs (reserve fronds for salad)",
    "Extra virgin olive oil (for roasting)",
    "Kosher salt",
    "1 small lemon, zested and juiced",
    "1/3 cup (38g) raw walnuts, finely chopped",
    "½ teaspoon Aleppo pepper",
    "1 cup (160g) cooked chickpeas",
    "½ cup (6g) flat-leaf Italian parsley, chopped",
    "¼ cup (4g) fresh dill, finely chopped",
    "1 medium orange",
    "1 large ripe avocado",
    "## Citrus Vinaigrette",
    "3 tablespoons extra virgin olive oil",
    "½ teaspoon lemon zest",
    "2 tablespoons fresh lemon juice",
    "1 teaspoon Dijon mustard",
    "¼ teaspoon sea salt",
    "1 teaspoon maple syrup"
  ],
  "instructions": [
    "## Roast Vegetables",
    "Preheat oven to 425°F (230°C).",
    "Roast beets for 45-90 minutes until fork-tender.",
    "## Make Vinaigrette",
    "Combine all vinaigrette ingredients in a jar and shake."
  ]
}

Answer only with the final JSON."###;

const DEFAULT_SYSTEM_PROMPT_STRUCTURE: &str = r###"You are an ingredient parser.

INPUT: An array of ingredient strings (already translated to English).
OUTPUT: STRICT JSON array of structured ingredients.

Each ingredient can be either:
1. A section header: {"section": "Name"}
2. A structured ingredient: {"quantity": null|number, "unit": null|string, "name": string, "prep": null|string}

CRITICAL PARSING RULES:
- For section headers (lines starting with "##"), output: {"section": "Name"}
  Example: "## Sauce" → {"section": "Sauce"}

- For regular ingredients, YOU MUST extract all 4 fields:
  1. QUANTITY (number or null):
     - **PRIORITY**: If weight/volume is in parentheses, ALWAYS use that: 
       * "1/3 cup (38g)" → quantity: 38
       * "2 cans (400g)" → quantity: 400
       * "1 cup (160g)" → quantity: 160
     - Otherwise, look for numbers at the start: "2 carrots" → quantity: 2
     - Convert fractions: 1/2 → 0.5, 1/4 → 0.25, 3/4 → 0.75, 1/3 → 0.33
     - For ranges, use mean: "2-3 cups" → 2.5
     - If no quantity found, set null
  
  2. UNIT (string or null):
     - **PRIORITY**: If weight/volume is in parentheses, ALWAYS use that unit:
       * "1/3 cup (38g)" → unit: "g"
       * "2 cans (400g)" → unit: "g"  
       * "1 cup (160g)" → unit: "g"
       * "2 cups (480ml)" → unit: "ml"
     - Otherwise, look for unit words: "2 cups flour" → unit: "cups"
     - Common units: cup, cups, tbsp, tablespoon, tsp, teaspoon, oz, ounce, lb, pound, g, gram, kg, ml, L
     - Keep units AS-IS from input (don't convert yet)
     - If no unit found, set null
  
  3. NAME (string):
     - The main ingredient, WITHOUT quantity, unit, or prep words
     - Example: "2 cups flour, sifted" → name: "flour"
     - Example: "1 can (15oz) chickpeas" → name: "chickpeas"
  
  4. PREP (string or null):
     - Preparation instructions ONLY
     - Common prep words: sliced, diced, minced, chopped, grated, shredded, softened, melted, peeled, crushed, drained, rinsed
     - Example: "2 carrots, diced" → prep: "diced"
     - If no prep found, set null

IMPORTANT EXAMPLES:
Input: "2 cups all-purpose flour"
Output: {"quantity": 2, "unit": "cups", "name": "all-purpose flour", "prep": null}

Input: "1 can (15 oz) chickpeas, drained"
Output: {"quantity": 15, "unit": "oz", "name": "chickpeas", "prep": "drained"}

Input: "1/2 teaspoon salt"
Output: {"quantity": 0.5, "unit": "teaspoon", "name": "salt", "prep": null}

Input: "2-3 cloves garlic, minced"
Output: {"quantity": 2.5, "unit": "cloves", "name": "garlic", "prep": "minced"}

Input: "Fresh basil for garnish"
Output: {"quantity": null, "unit": null, "name": "fresh basil", "prep": "for garnish"}

FORMAT EXAMPLE:
INPUT:
[
  "## Pulled Jackfruit",
  "2 cans (560g) young jackfruit",
  "1 tablespoon olive oil",
  "1/2 teaspoon smoked paprika",
  "## Tzatziki",
  "1 cup non-dairy yogurt",
  "1/2 cucumber, grated",
  "2 cloves garlic, minced"
]

OUTPUT:
[
  {"section": "Pulled Jackfruit"},
  {"quantity": 560, "unit": "g", "name": "young jackfruit", "prep": null},
  {"quantity": 1, "unit": "tablespoon", "name": "olive oil", "prep": null},
  {"quantity": 0.5, "unit": "teaspoon", "name": "smoked paprika", "prep": null},
  {"section": "Tzatziki"},
  {"quantity": 1, "unit": "cup", "name": "non-dairy yogurt", "prep": null},
  {"quantity": 0.5, "unit": null, "name": "cucumber", "prep": "grated"},
  {"quantity": 2, "unit": "cloves", "name": "garlic", "prep": "minced"}
]

Answer only with the JSON array."###;

const DEFAULT_SYSTEM_PROMPT_CONVERT: &str = r#"You are a unit converter for recipes.

INPUT: JSON array of structured ingredients (with sections and regular ingredients).
OUTPUT: Same JSON array with ALL imperial units converted to metric.

CONVERSION RULES:
- ALLOWED metric units ONLY: g, kg, ml, L, tsp, tbsp
- BANNED units (must be converted): cup, cups, oz, ounce, ounces, fl oz, fluid ounce, pound, lb, lbs, pint, quart, gallon
- Keep tsp and tbsp as-is (these are okay)
- For section headers ({"section": "..."}), pass them through unchanged

CONVERSIONS:
Solid ingredients (flour, sugar, butter, etc.):
- 1 oz → 28 g
- 1 lb → 454 g
- 1 cup flour → 120 g
- 1 cup sugar → 200 g
- 1 cup butter → 227 g
- For generic solids: 1 cup → 150 g (reasonable default)

Liquid ingredients (water, milk, oil, etc.):
- 1 fl oz → 30 ml
- 1 cup → 240 ml
- 1 pint → 475 ml
- 1 quart → 950 ml
- 1 gallon → 3785 ml (or 3.8 L)

RULES:
- Convert quantities accordingly
- Round sensibly (no 227.5g → just 230g or 225g)
- Use g for small amounts, kg for > 1000g
- Use ml for small amounts, L for > 1000ml
- If an ingredient already has metric units, leave it unchanged
- Preserve the "prep" field exactly as-is
- Do NOT add commentary or extra fields

FORMAT EXAMPLE:
INPUT:
[
  {"section": "Pulled Jackfruit"},
  {"quantity": 2, "unit": "cups", "name": "jackfruit", "prep": null},
  {"quantity": 1, "unit": "tablespoon", "name": "olive oil", "prep": null},
  {"section": "Tzatziki"},
  {"quantity": 1, "unit": "cup", "name": "non-dairy yogurt", "prep": null},
  {"quantity": 0.5, "unit": null, "name": "cucumber", "prep": "grated"}
]

OUTPUT:
[
  {"section": "Pulled Jackfruit"},
  {"quantity": 300, "unit": "g", "name": "jackfruit", "prep": null},
  {"quantity": 1, "unit": "tbsp", "name": "olive oil", "prep": null},
  {"section": "Tzatziki"},
  {"quantity": 240, "unit": "ml", "name": "non-dairy yogurt", "prep": null},
  {"quantity": 0.5, "unit": null, "name": "cucumber", "prep": "grated"}
]

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
