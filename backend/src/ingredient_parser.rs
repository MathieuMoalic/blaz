use regex::Regex;

use crate::models::Ingredient;

// leading: number (optional range) + unit (optional), then name
// examples: "120 g flour", "2-3 tbsp sugar", "1.5 L water", "2 carrots, diced"
static ING_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
    Regex::new(
        r#"^\s*
        (?P<a1>\d+(?:[.,]\d+)?)
        (?:\s*[–-]\s*(?P<a2>\d+(?:[.,]\d+)?))?
        (?:\s*(?P<u>kg|g|ml|l|tsp|tbsp))?
        \s*(?P<rest>.+?)\s*$"#,
    )
    .unwrap()
});

pub fn parse_ingredient_line(s: &str) -> Ingredient {
    let s = s
        .trim()
        .trim_start_matches(['•', '-', '–', '—', '·', '*', '+', ' '])
        .trim();
    if let Some(c) = ING_RE.captures(s) {
        let mut unit = c.name("u").map(|m| m.as_str().to_string());
        if unit.as_deref() == Some("l") {
            unit = Some("L".into());
        }

        // pick midpoint for ranges
        let a1 = c
            .name("a1")
            .and_then(|m| m.as_str().replace(',', ".").parse::<f64>().ok());
        let a2 = c
            .name("a2")
            .and_then(|m| m.as_str().replace(',', ".").parse::<f64>().ok());
        let quantity = match (a1, a2) {
            (Some(x), Some(y)) => Some((x + y) / 2.0),
            (Some(x), None) => Some(x),
            _ => None,
        };

        let mut name = c
            .name("rest")
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        // move preps after name if user wrote "diced carrots"
        name = name.trim().trim_matches(',').to_string();
        return Ingredient {
            quantity,
            unit,
            name,
        };
    }
    // fallback: whole line is the name
    Ingredient {
        quantity: None,
        unit: None,
        name: s.to_string(),
    }
}
