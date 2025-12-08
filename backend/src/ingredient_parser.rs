use std::sync::LazyLock;

use crate::models::Ingredient;
use crate::units;
use regex::Regex;

// Regex for lines like:
// - "120 g flour"
// - "2-3 tbsp sugar"
// - "1.5 L water"
// - "2 carrots, diced"
// Case-insensitive, supports plural/synonym units, optional "of".
static ING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?xi)
        ^\s*
        (?P<a1>\d+(?:[.,]\d+)?)                  # first number
        (?:\s*[–-]\s*(?P<a2>\d+(?:[.,]\d+)?))?   # optional range second number
        (?:\s*(?P<u>
            kg|g|ml|l|tsp|tbsp|
            grams?|kilograms?|
            millilit(?:er|re)s?|
            lit(?:er|re)s?|
            teaspoons?|tablespoons?
        ))?                                       # optional unit
        (?:\s+of\b)?                              # optional "of"
        \s*(?P<rest>.+?)\s*$                      # the name / remainder
    "#,
    )
    .unwrap()
});

/// Convert a single free-text ingredient line into a structured `Ingredient`.
pub fn parse_ingredient_line(s: &str) -> Ingredient {
    let s = s.trim();
    if s.is_empty() {
        return Ingredient {
            quantity: None,
            unit: None,
            name: String::new(),
        };
    }

    if let Some(caps) = ING_RE.captures(s) {
        // numbers: support , or . as decimal separator
        let parse_num = |m: Option<regex::Match>| -> Option<f64> {
            m.map(|x| x.as_str().replace(',', "."))
                .and_then(|t| t.parse::<f64>().ok())
        };
        let a1 = parse_num(caps.name("a1"));
        let a2 = parse_num(caps.name("a2"));
        let mut quantity = a1;
        if let (Some(x), Some(y)) = (a1, a2) {
            quantity = Some((x + y) / 2.0);
        }

        let unit_raw = caps.name("u").map(|m| m.as_str());
        let unit = unit_raw
            .and_then(units::canon_unit_str)
            .map(std::string::ToString::to_string);

        // Clean up the remainder as the ingredient name
        let name = units::norm_whitespace(caps.name("rest").map_or("", |m| m.as_str()));

        return Ingredient {
            quantity,
            unit,
            name,
        };
    }

    // fallback: treat whole line as the name
    Ingredient {
        quantity: None,
        unit: None,
        name: s.to_string(),
    }
}
