use once_cell::sync::Lazy;
use regex::Regex;

pub static DECIMAL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\s*(\d+(?:[.,]\d+)?)(?:\s*[–-]\s*(\d+(?:[.,]\d+)?))?\s*$"#).unwrap()
});

#[inline]
pub fn canon_unit_str(u: &str) -> Option<&'static str> {
    match u.to_ascii_lowercase().as_str() {
        "g" | "gram" | "grams" => Some("g"),
        "kg" | "kilogram" | "kilograms" => Some("kg"),
        "ml" | "milliliter" | "millilitre" | "milliliters" | "millilitres" => Some("ml"),
        "l" | "liter" | "litre" | "liters" | "litres" => Some("L"),
        "tsp" | "teaspoon" | "teaspoons" => Some("tsp"),
        "tbsp" | "tablespoon" | "tablespoons" => Some("tbsp"),
        _ => None,
    }
}

// kg->g, L->ml, tbsp->ml(15), tsp->ml(5); pass-through otherwise
pub fn to_canonical_qty_unit(
    unit: Option<&str>,
    qty: Option<f64>,
) -> (Option<&'static str>, Option<f64>) {
    match (unit.map(|u| u.to_ascii_lowercase()), qty) {
        (Some(u), Some(q)) if u == "kg" => (Some("g"), Some(q * 1000.0)),
        (Some(u), Some(q)) if u == "l" => (Some("ml"), Some(q * 1000.0)),
        (Some(u), Some(q)) if u == "tbsp" => (Some("ml"), Some(q * 15.0)),
        (Some(u), Some(q)) if u == "tsp" => (Some("ml"), Some(q * 5.0)),
        (Some(u), q) => (canon_unit_str(&u), q),
        (None, q) => (None, q),
    }
}

pub fn norm_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for ch in s.trim().chars() {
        if ch.is_whitespace() {
            if !ws {
                out.push(' ');
                ws = true;
            }
        } else {
            ws = false;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

pub fn normalize_name(s: &str) -> String {
    norm_whitespace(&s.to_lowercase())
}
