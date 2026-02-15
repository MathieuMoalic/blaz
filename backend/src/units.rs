use regex::Regex;
use std::sync::LazyLock;

pub static SERVINGS_NUM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\d+(?:[.,]\d+)?)(?:\s*[–-]\s*(\d+(?:[.,]\d+)?))?").unwrap());

pub static BARE_NUM_RANGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ix)^\s*(\d+(?:[.,]\d+)?)(?:\s*[–-]\s*(\d+(?:[.,]\d+)?))?\s*$").unwrap()
});

pub static SERVINGS_HINT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ix)\b(serves?|servings?|portion(?:s)?|people|persons?|pax|makes?)\b").unwrap()
});

pub static NON_SERVING_YIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)\b(
            g|gram(?:s)?|kg|kilogram(?:s)?|
            ml|milliliter(?:s)?|millilitre(?:s)?|
            l|liter(?:s)?|litre(?:s)?|
            oz|ounce(?:s)?|lb|pound(?:s)?|
            tsp|tbsp|cup(?:s)?|
            loaf(?:s)?|slice(?:s)?|piece(?:s)?|pcs
        )\b",
    )
    .unwrap()
});
#[inline]
#[must_use]
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
#[must_use]
pub fn to_canonical_qty_unit(
    unit: Option<&str>,
    qty: Option<f64>,
) -> (Option<&'static str>, Option<f64>) {
    match (unit.map(str::to_ascii_lowercase), qty) {
        (Some(u), Some(q)) if u == "kg" => (Some("g"), Some(q * 1000.0)),
        (Some(u), Some(q)) if u == "l" => (Some("ml"), Some(q * 1000.0)),
        (Some(u), Some(q)) if u == "tbsp" => (Some("ml"), Some(q * 15.0)),
        (Some(u), Some(q)) if u == "tsp" => (Some("ml"), Some(q * 5.0)),
        (Some(u), q) => (canon_unit_str(&u), q),
        (None, q) => (None, q),
    }
}

#[must_use]
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

#[must_use]
pub fn normalize_name(s: &str) -> String {
    norm_whitespace(&s.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_unit_str() {
        assert_eq!(canon_unit_str("g"), Some("g"));
        assert_eq!(canon_unit_str("gram"), Some("g"));
        assert_eq!(canon_unit_str("grams"), Some("g"));
        assert_eq!(canon_unit_str("G"), Some("g"));
        assert_eq!(canon_unit_str("GRAMS"), Some("g"));
        
        assert_eq!(canon_unit_str("kg"), Some("kg"));
        assert_eq!(canon_unit_str("kilogram"), Some("kg"));
        assert_eq!(canon_unit_str("kilograms"), Some("kg"));
        assert_eq!(canon_unit_str("KG"), Some("kg"));
        
        assert_eq!(canon_unit_str("ml"), Some("ml"));
        assert_eq!(canon_unit_str("milliliter"), Some("ml"));
        assert_eq!(canon_unit_str("millilitre"), Some("ml"));
        assert_eq!(canon_unit_str("milliliters"), Some("ml"));
        assert_eq!(canon_unit_str("millilitres"), Some("ml"));
        
        assert_eq!(canon_unit_str("l"), Some("L"));
        assert_eq!(canon_unit_str("liter"), Some("L"));
        assert_eq!(canon_unit_str("litre"), Some("L"));
        assert_eq!(canon_unit_str("liters"), Some("L"));
        assert_eq!(canon_unit_str("litres"), Some("L"));
        assert_eq!(canon_unit_str("L"), Some("L"));
        
        assert_eq!(canon_unit_str("tsp"), Some("tsp"));
        assert_eq!(canon_unit_str("teaspoon"), Some("tsp"));
        assert_eq!(canon_unit_str("teaspoons"), Some("tsp"));
        
        assert_eq!(canon_unit_str("tbsp"), Some("tbsp"));
        assert_eq!(canon_unit_str("tablespoon"), Some("tbsp"));
        assert_eq!(canon_unit_str("tablespoons"), Some("tbsp"));
        
        assert_eq!(canon_unit_str("unknown"), None);
        assert_eq!(canon_unit_str("cup"), None);
        assert_eq!(canon_unit_str(""), None);
    }

    #[test]
    fn test_to_canonical_qty_unit() {
        assert_eq!(to_canonical_qty_unit(Some("kg"), Some(1.0)), (Some("g"), Some(1000.0)));
        assert_eq!(to_canonical_qty_unit(Some("kg"), Some(2.5)), (Some("g"), Some(2500.0)));
        assert_eq!(to_canonical_qty_unit(Some("KG"), Some(1.0)), (Some("g"), Some(1000.0)));
        
        assert_eq!(to_canonical_qty_unit(Some("l"), Some(1.0)), (Some("ml"), Some(1000.0)));
        assert_eq!(to_canonical_qty_unit(Some("L"), Some(1.5)), (Some("ml"), Some(1500.0)));
        
        assert_eq!(to_canonical_qty_unit(Some("tbsp"), Some(2.0)), (Some("ml"), Some(30.0)));
        assert_eq!(to_canonical_qty_unit(Some("TBSP"), Some(3.0)), (Some("ml"), Some(45.0)));
        
        assert_eq!(to_canonical_qty_unit(Some("tsp"), Some(3.0)), (Some("ml"), Some(15.0)));
        assert_eq!(to_canonical_qty_unit(Some("TSP"), Some(2.0)), (Some("ml"), Some(10.0)));
        
        assert_eq!(to_canonical_qty_unit(Some("g"), Some(100.0)), (Some("g"), Some(100.0)));
        assert_eq!(to_canonical_qty_unit(Some("ml"), Some(50.0)), (Some("ml"), Some(50.0)));
        
        assert_eq!(to_canonical_qty_unit(None, Some(5.0)), (None, Some(5.0)));
        assert_eq!(to_canonical_qty_unit(Some("g"), None), (Some("g"), None));
        assert_eq!(to_canonical_qty_unit(None, None), (None, None));
    }

    #[test]
    fn test_norm_whitespace() {
        assert_eq!(norm_whitespace("  hello   world  "), "hello world");
        assert_eq!(norm_whitespace("hello\t\tworld"), "hello world");
        assert_eq!(norm_whitespace("hello\nworld"), "hello world");
        assert_eq!(norm_whitespace("  \t  hello  \n  world  \t  "), "hello world");
        assert_eq!(norm_whitespace("single"), "single");
        assert_eq!(norm_whitespace(""), "");
        assert_eq!(norm_whitespace("   "), "");
    }

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("  Hello   World  "), "hello world");
        assert_eq!(normalize_name("UPPERCASE"), "uppercase");
        assert_eq!(normalize_name("  Mixed\t\tCase  "), "mixed case");
        assert_eq!(normalize_name(""), "");
    }
}
