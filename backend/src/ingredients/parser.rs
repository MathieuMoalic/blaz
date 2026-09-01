//! Deterministic ingredient-line parser: quantity/unit syntax only.
//!
//! This is the canonical parser for free-form ingredient lines (standalone
//! shopping entry, manual recipe creation, and import fallback). It never
//! performs semantic resolution: deciding *what food* a phrase means is the
//! resolver's job. Fractions, unicode fractions, ranges, mixed numbers, and
//! unit aliases are always handled deterministically — no LLM is involved.

use std::sync::LazyLock;

use regex::Regex;

use crate::units::canon_unit_str;

/// A deterministically parsed ingredient line.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedIngredient {
    /// The original input line (trimmed), before any normalization.
    pub raw_text: String,
    pub quantity: Option<f64>,
    /// Canonical unit: `g`, `kg`, `ml`, `L`, `tsp`, or `tbsp`.
    pub unit: Option<&'static str>,
    /// Ingredient wording as written, without quantity/unit/prep.
    pub ingredient_phrase: String,
    /// Preparation wording, e.g. `peeled` or `to taste`.
    pub prep: Option<String>,
}

/// Parse a simple fraction like "1/2" or "3/4" into f64.
fn parse_fraction(s: &str) -> Option<f64> {
    let (num, denom) = s.split_once('/')?;
    let numerator = num.trim().parse::<f64>().ok()?;
    let denominator = denom.trim().parse::<f64>().ok()?;
    if denominator == 0.0 {
        return None;
    }
    Some(numerator / denominator)
}

/// Replace common Unicode fraction characters with decimal strings.
/// Adds a leading space so "1½" becomes "1 0.5" for mixed number handling.
fn replace_unicode_fractions(s: &str) -> String {
    s.replace('½', " 0.5")
        .replace('⅓', " 0.333333")
        .replace('⅔', " 0.666667")
        .replace('¼', " 0.25")
        .replace('¾', " 0.75")
        .replace('⅕', " 0.2")
        .replace('⅖', " 0.4")
        .replace('⅗', " 0.6")
        .replace('⅘', " 0.8")
        .replace('⅙', " 0.166667")
        .replace('⅚', " 0.833333")
        .replace('⅛', " 0.125")
        .replace('⅜', " 0.375")
        .replace('⅝', " 0.625")
        .replace('⅞', " 0.875")
}

/// Parse a quantity token: decimals (also comma form), ranges ("2-3",
/// "2–3" → midpoint), and simple fractions ("1/2").
fn parse_qty_token(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }

    // Handle ranges (e.g., "2-3"), but not fraction ranges
    if let Some((a, b)) = t.split_once('-').or_else(|| t.split_once('–'))
        && !a.contains('/')
        && !b.contains('/')
    {
        let x = a.trim().parse::<f64>().ok()?;
        let y = b.trim().parse::<f64>().ok()?;
        return Some(f64::midpoint(x, y));
    }

    // Handle simple fractions (e.g., "1/2", "3/4")
    if let Some(result) = parse_fraction(&t) {
        return Some(result);
    }

    t.parse::<f64>().ok()
}

/// Glued quantity+unit token, e.g. "250g", "1kg", "1.5L", "1/2tsp".
static GLUED_QTY_UNIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(\d+(?:[.,]\d+)?(?:/\d+(?:[.,]\d+)?)?)(kg|g|tbsp|tsp|ml|l)$").unwrap()
});

/// Parse a glued quantity+unit token like "250g" or "1/2tsp".
fn parse_glued_qty_unit(token: &str) -> Option<(f64, &'static str)> {
    let caps = GLUED_QTY_UNIT_RE.captures(token)?;
    let qty = parse_qty_token(&caps.get(1)?.as_str().replace(',', "."))?;
    let unit = canon_unit_str(caps.get(2)?.as_str())?;
    Some((qty, unit))
}

/// Strip an ASCII suffix, case-insensitively.
fn strip_suffix_ignore_ascii_case<'a>(haystack: &'a str, suffix: &str) -> Option<&'a str> {
    let matches = haystack.len() >= suffix.len()
        && haystack
            .chars()
            .rev()
            .zip(suffix.chars().rev())
            .all(|(h, s)| h.eq_ignore_ascii_case(&s));
    matches.then(|| &haystack[..haystack.len() - suffix.len()])
}

/// Split preparation wording off an ingredient phrase.
///
/// The first comma separates preparation wording ("large potatoes, peeled"
/// → "large potatoes" + "peeled"); a trailing "to taste" without a comma is
/// also preparation ("salt to taste" → "salt" + "to taste"). Malformed
/// fragments (empty head or tail) keep the original wording.
fn split_prep(phrase: &str) -> (String, Option<String>) {
    let mut prep: Option<String> = None;
    let mut working = phrase.trim().to_string();
    if working.is_empty() {
        return (working, None);
    }

    if let Some((head, tail)) = working.split_once(',') {
        let head = head.trim();
        let tail = tail.trim();
        if !head.is_empty() && !tail.is_empty() {
            return (head.to_string(), Some(tail.to_string()));
        }
    }

    if let Some(stripped) = strip_suffix_ignore_ascii_case(&working, " to taste") {
        prep = Some("to taste".to_string());
        working = stripped.trim_end().to_string();
    }

    (working, prep)
}

fn log_parsed(original: &str, parsed: &ParsedIngredient, reason: &str) {
    tracing::info!(
        raw = %original,
        qty = ?parsed.quantity,
        unit = ?parsed.unit,
        phrase = %parsed.ingredient_phrase,
        prep = ?parsed.prep,
        "parsed ingredient line ({reason})"
    );
}

/// Tolerant fallback: the whole wording becomes the phrase.
fn plain_item(original: &str, tokens: &[&str], reason: &str) -> ParsedIngredient {
    let (phrase, prep) = split_prep(&tokens.join(" "));
    let parsed = ParsedIngredient {
        raw_text: original.to_string(),
        quantity: None,
        unit: None,
        ingredient_phrase: phrase,
        prep,
    };
    log_parsed(original, &parsed, reason);
    parsed
}

/// Glued leading quantity+unit: "250g flour", "1kg potatoes", "½tsp cumin".
fn parse_glued(original: &str, tokens: &[&str]) -> Option<ParsedIngredient> {
    let (qty, unit) = parse_glued_qty_unit(tokens[0])?;
    let mut idx = 1;
    if tokens.get(idx).copied() == Some("of") {
        idx += 1;
    }
    if idx >= tokens.len() {
        return Some(plain_item(original, tokens, "missing name after glued qty+unit"));
    }
    let (phrase, prep) = split_prep(&tokens[idx..].join(" "));
    let parsed = ParsedIngredient {
        raw_text: original.to_string(),
        quantity: Some(qty),
        unit: Some(unit),
        ingredient_phrase: phrase,
        prep,
    };
    log_parsed(original, &parsed, "glued qty+unit");
    Some(parsed)
}

/// Parse an ingredient line that may look like:
/// - "120 g flour", "120g flour", "1kg potatoes" (glued unit)
/// - "2-3 apples", "1/2 cup flour", "1 1/2 cups flour" (range/fraction/mixed)
/// - "½ cup flour", "1½ cups flour" (unicode fractions)
/// - "1 tsp of cumin" ("of" separator)
/// - "3 large potatoes, peeled" (trailing preparation wording)
/// - "salt to taste", "a pinch of salt", "salt"
///
/// The parser is intentionally tolerant:
/// - If it doesn't start with a number, qty/unit are None and the remaining
///   wording is the phrase.
/// - If it starts with a number but no phrase follows, the whole line
///   becomes the phrase.
#[must_use]
pub fn parse_ingredient_line(raw: &str) -> Option<ParsedIngredient> {
    let original = raw.trim();
    if original.is_empty() {
        return None;
    }

    let replaced = replace_unicode_fractions(original);
    let mut tokens: Vec<&str> = replaced.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    // Drop a leading indefinite article: "a pinch of salt" → "pinch of salt".
    if tokens.len() > 1
        && matches!(tokens[0].to_ascii_lowercase().as_str(), "a" | "an")
    {
        tokens.remove(0);
    }

    if let Some(parsed) = parse_glued(original, &tokens) {
        return Some(parsed);
    }

    // Try parse leading qty (handles decimals, ranges, and fractions).
    let first_qty = parse_qty_token(tokens[0]);
    if first_qty.is_none() {
        return Some(plain_item(original, &tokens, "no leading quantity"));
    }

    let mut qty = first_qty;
    let mut idx = 1usize;
    let mut unit: Option<&'static str> = None;

    // Mixed number: "1 1/2", "1 0.5" (unicode), possibly with a glued unit
    // ("1½kg" → "1 0.5kg").
    if let Some(second) = tokens.get(1) {
        if let Some(frac) = parse_fraction(second) {
            qty = Some(qty.unwrap_or(0.0) + frac);
            idx = 2;
        } else if let Some((decimal, glued_unit)) = parse_glued_qty_unit(second)
            && decimal > 0.0
            && decimal < 1.0
        {
            qty = Some(qty.unwrap_or(0.0) + decimal);
            unit = Some(glued_unit);
            idx = 2;
        } else if let Ok(decimal) = second.parse::<f64>()
            && decimal > 0.0
            && decimal < 1.0
        {
            qty = Some(qty.unwrap_or(0.0) + decimal);
            idx = 2;
        }
    }

    // Optional unit
    if unit.is_none()
        && let Some(t) = tokens.get(idx)
        && let Some(un) = canon_unit_str(t)
    {
        unit = Some(un);
        idx += 1;
    }

    // Optional "of"
    if tokens.get(idx).copied() == Some("of") {
        idx += 1;
    }

    // Remaining tokens are the phrase
    if idx >= tokens.len() {
        // Mirror old fallback: ignore parsed qty/unit if the name is missing.
        return Some(plain_item(original, &tokens, "missing name after qty"));
    }

    let (phrase, prep) = split_prep(&tokens[idx..].join(" "));
    let parsed = ParsedIngredient {
        raw_text: original.to_string(),
        quantity: qty,
        unit,
        ingredient_phrase: phrase,
        prep,
    };
    log_parsed(original, &parsed, "ok");
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ParsedIngredient {
        parse_ingredient_line(s).expect("line parses")
    }

    /* ---------- moved from shopping.rs (unchanged behavior) ---------- */

    #[test]
    fn test_parse_qty_token() {
        assert_eq!(parse_qty_token("2"), Some(2.0));
        assert_eq!(parse_qty_token("10"), Some(10.0));
        assert_eq!(parse_qty_token("1.5"), Some(1.5));
        assert_eq!(parse_qty_token("1,5"), Some(1.5));

        assert_eq!(parse_qty_token("2-3"), Some(2.5));
        assert_eq!(parse_qty_token("2–3"), Some(2.5));
        assert_eq!(parse_qty_token("1.5-2.5"), Some(2.0));
        assert_eq!(parse_qty_token("10–20"), Some(15.0));

        assert_eq!(parse_qty_token(""), None);
        assert_eq!(parse_qty_token("  "), None);
        assert_eq!(parse_qty_token("abc"), None);
    }

    #[test]
    fn test_parse_fraction() {
        assert_eq!(parse_fraction("1/2"), Some(0.5));
        assert_eq!(parse_fraction("1/4"), Some(0.25));
        assert_eq!(parse_fraction("3/4"), Some(0.75));
        assert_eq!(parse_fraction("2/3"), Some(2.0 / 3.0));
        assert_eq!(parse_fraction("1/0"), None);
        assert_eq!(parse_fraction("abc"), None);
        assert_eq!(parse_fraction("1"), None);
    }

    #[test]
    fn test_parse_qty_token_fractions() {
        assert_eq!(parse_qty_token("1/2"), Some(0.5));
        assert_eq!(parse_qty_token("1/4"), Some(0.25));
        assert_eq!(parse_qty_token("3/4"), Some(0.75));
    }

    #[test]
    fn test_parse_item_line_simple() {
        let p = parse("milk");
        assert_eq!(p.quantity, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "milk");
        assert_eq!(p.prep, None);
    }

    #[test]
    fn test_parse_item_line_with_qty() {
        let p = parse("2 apples");
        assert_eq!(p.quantity, Some(2.0));
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "apples");
    }

    #[test]
    fn test_parse_item_line_with_qty_and_unit() {
        let p = parse("120 g flour");
        assert_eq!(p.quantity, Some(120.0));
        assert_eq!(p.unit, Some("g"));
        assert_eq!(p.ingredient_phrase, "flour");
    }

    #[test]
    fn test_parse_item_line_range() {
        let p = parse("2-3 kg potatoes");
        assert_eq!(p.quantity, Some(2.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "potatoes");
    }

    #[test]
    fn test_parse_item_line_with_of() {
        let p = parse("2 kg of rice");
        assert_eq!(p.quantity, Some(2.0));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "rice");
    }

    #[test]
    fn test_parse_item_line_decimal() {
        let p = parse("1.5 L water");
        assert_eq!(p.quantity, Some(1.5));
        assert_eq!(p.unit, Some("L"));
        assert_eq!(p.ingredient_phrase, "water");
    }

    #[test]
    fn test_parse_item_line_comma_decimal() {
        let p = parse("1,5 kg sugar");
        assert_eq!(p.quantity, Some(1.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "sugar");
    }

    #[test]
    fn test_parse_item_line_case_insensitive() {
        let p = parse("200 ML Milk");
        assert_eq!(p.quantity, Some(200.0));
        assert_eq!(p.unit, Some("ml"));
        assert_eq!(p.ingredient_phrase, "Milk");
    }

    #[test]
    fn test_parse_item_line_missing_name_fallback() {
        let p = parse("2 kg");
        assert_eq!(p.quantity, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "2 kg");
    }

    #[test]
    fn test_parse_item_line_empty() {
        assert!(parse_ingredient_line("").is_none());
        assert!(parse_ingredient_line("   ").is_none());
    }

    #[test]
    fn test_parse_item_line_unknown_unit() {
        let p = parse("2 cups flour");
        assert_eq!(p.quantity, Some(2.0));
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "cups flour");
    }

    #[test]
    fn test_parse_item_line_whitespace_normalization() {
        let p = parse("  2   kg    of   flour  ");
        assert_eq!(p.quantity, Some(2.0));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "flour");
    }

    #[test]
    fn test_parse_item_line_simple_fraction() {
        let p = parse("1/2 kg flour");
        assert_eq!(p.quantity, Some(0.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "flour");
    }

    #[test]
    fn test_parse_item_line_mixed_number() {
        let p = parse("1 1/2 kg flour");
        assert_eq!(p.quantity, Some(1.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "flour");
    }

    #[test]
    fn test_parse_item_line_unicode_half() {
        let p = parse("½ kg flour");
        assert_eq!(p.quantity, Some(0.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "flour");
    }

    #[test]
    fn test_parse_item_line_unicode_mixed() {
        let p = parse("1½ kg flour");
        assert_eq!(p.quantity, Some(1.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "flour");
    }

    #[test]
    fn test_parse_item_line_unicode_three_quarters() {
        let p = parse("¾ kg butter");
        assert_eq!(p.quantity, Some(0.75));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "butter");
    }

    /* ---------- §33 matrix: fractions, ranges, glued units ---------- */

    #[test]
    fn test_fraction_teaspoon_variants() {
        for line in ["1/2 teaspoon cumin", "½ teaspoon cumin", "0.5 teaspoon cumin"] {
            let p = parse(line);
            assert_eq!(p.quantity, Some(0.5), "line: {line}");
            assert_eq!(p.unit, Some("tsp"), "line: {line}");
            assert_eq!(p.ingredient_phrase, "cumin", "line: {line}");
        }

        for line in ["1 1/2 teaspoons cumin", "1½ tsp cumin"] {
            let p = parse(line);
            assert_eq!(p.quantity, Some(1.5), "line: {line}");
            assert_eq!(p.unit, Some("tsp"), "line: {line}");
            assert_eq!(p.ingredient_phrase, "cumin", "line: {line}");
        }
    }

    #[test]
    fn test_range_teaspoons() {
        let p = parse("2-3 teaspoons cumin");
        assert_eq!(p.quantity, Some(2.5));
        assert_eq!(p.unit, Some("tsp"));
        assert_eq!(p.ingredient_phrase, "cumin");
    }

    #[test]
    fn test_glued_unit_forms() {
        let p = parse("250g flour");
        assert_eq!(p.quantity, Some(250.0));
        assert_eq!(p.unit, Some("g"));
        assert_eq!(p.ingredient_phrase, "flour");

        let p = parse("250 G FLOUR");
        assert_eq!(p.quantity, Some(250.0));
        assert_eq!(p.unit, Some("g"));
        assert_eq!(p.ingredient_phrase, "FLOUR");

        let p = parse("1kg potatoes");
        assert_eq!(p.quantity, Some(1.0));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "potatoes");

        let p = parse("1,5kg potatoes");
        assert_eq!(p.quantity, Some(1.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "potatoes");

        let p = parse("250g of flour");
        assert_eq!(p.quantity, Some(250.0));
        assert_eq!(p.unit, Some("g"));
        assert_eq!(p.ingredient_phrase, "flour");

        // Glued fallback when no name follows.
        let p = parse("250g");
        assert_eq!(p.quantity, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "250g");
    }

    #[test]
    fn test_glued_fraction_units() {
        let p = parse("1/2tsp cumin");
        assert_eq!(p.quantity, Some(0.5));
        assert_eq!(p.unit, Some("tsp"));
        assert_eq!(p.ingredient_phrase, "cumin");

        let p = parse("½tsp cumin");
        assert_eq!(p.quantity, Some(0.5));
        assert_eq!(p.unit, Some("tsp"));
        assert_eq!(p.ingredient_phrase, "cumin");

        // "1½kg" becomes "1 0.5kg" after unicode fraction replacement.
        let p = parse("1½kg potatoes");
        assert_eq!(p.quantity, Some(1.5));
        assert_eq!(p.unit, Some("kg"));
        assert_eq!(p.ingredient_phrase, "potatoes");
    }

    #[test]
    fn test_tablespoons_olive_oil() {
        let p = parse("2 tablespoons olive oil");
        assert_eq!(p.quantity, Some(2.0));
        assert_eq!(p.unit, Some("tbsp"));
        assert_eq!(p.ingredient_phrase, "olive oil");
    }

    /* ---------- prep / article handling ---------- */

    #[test]
    fn test_plain_names() {
        let p = parse("salt");
        assert_eq!(p.quantity, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "salt");
        assert_eq!(p.prep, None);
    }

    #[test]
    fn test_to_taste_variants() {
        let p = parse("salt to taste");
        assert_eq!(p.quantity, None);
        assert_eq!(p.ingredient_phrase, "salt");
        assert_eq!(p.prep.as_deref(), Some("to taste"));

        let p = parse("Salt, to taste");
        assert_eq!(p.ingredient_phrase, "Salt");
        assert_eq!(p.prep.as_deref(), Some("to taste"));

        let p = parse("1/2 tsp salt, to taste");
        assert_eq!(p.quantity, Some(0.5));
        assert_eq!(p.unit, Some("tsp"));
        assert_eq!(p.ingredient_phrase, "salt");
        assert_eq!(p.prep.as_deref(), Some("to taste"));
    }

    #[test]
    fn test_leading_indefinite_article() {
        let p = parse("a pinch of salt");
        assert_eq!(p.quantity, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "pinch of salt");

        let p = parse("A pinch of salt");
        assert_eq!(p.ingredient_phrase, "pinch of salt");

        let p = parse("an apple");
        assert_eq!(p.ingredient_phrase, "apple");

        // A single "a" stays as-is.
        let p = parse("a");
        assert_eq!(p.ingredient_phrase, "a");
    }

    #[test]
    fn test_comma_prep_extraction() {
        let p = parse("3 large potatoes, peeled");
        assert_eq!(p.quantity, Some(3.0));
        assert_eq!(p.unit, None);
        assert_eq!(p.ingredient_phrase, "large potatoes");
        assert_eq!(p.prep.as_deref(), Some("peeled"));

        let p = parse("2 potatoes, peeled and cubed");
        assert_eq!(p.ingredient_phrase, "potatoes");
        assert_eq!(p.prep.as_deref(), Some("peeled and cubed"));

        // Prep extraction also applies to lines without a quantity.
        let p = parse("milk, warm");
        assert_eq!(p.quantity, None);
        assert_eq!(p.ingredient_phrase, "milk");
        assert_eq!(p.prep.as_deref(), Some("warm"));
    }

    /* ---------- raw text preservation ---------- */

    #[test]
    fn test_raw_text_preserved() {
        let p = parse("  1/2 tsp  cumin  ");
        assert_eq!(p.raw_text, "1/2 tsp  cumin");
        assert_eq!(p.ingredient_phrase, "cumin");

        let p = parse("3 large potatoes, peeled");
        assert_eq!(p.raw_text, "3 large potatoes, peeled");
    }

    #[test]
    fn test_split_prep_edge_cases() {
        assert_eq!(
            split_prep("large potatoes, peeled"),
            ("large potatoes".to_string(), Some("peeled".to_string()))
        );
        assert_eq!(split_prep("salt"), ("salt".to_string(), None));
        assert_eq!(split_prep(""), (String::new(), None));
        // Empty head keeps the original wording rather than inventing a prep.
        assert_eq!(
            split_prep(", peeled"),
            (", peeled".to_string(), None)
        );
    }
}
