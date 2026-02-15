use regex::Regex;
use std::sync::LazyLock;

/// Extract <title>...</title> from raw HTML and decode basic entities.
pub fn extract_title(html: &str) -> Option<String> {
    static TITLE_RE: std::sync::LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

    let raw = TITLE_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())?;

    Some(decode_entities_basic(&raw))
}

#[must_use]
/// Fallback guess if we couldn't get a good page/LLM title.
pub fn fallback_title_from_url(url: &str) -> Option<String> {
    reqwest::Url::parse(url).map_or(None, |u| {
        let host = u.host_str().unwrap_or_default().to_string();
        let p = u.path().trim_matches('/');
        if p.is_empty() {
            Some(host)
        } else {
            Some(format!("{host} — {p}"))
        }
    })
}

/// Convert HTML to readable plain text.
pub fn html_to_plain_text(html: &str) -> String {
    static SCRIPT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
    static STYLE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
    static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").unwrap());
    static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\r\f]+").unwrap());
    static NL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

    let mut s = SCRIPT_RE.replace_all(html, " ").into_owned();
    s = STYLE_RE.replace_all(&s, " ").into_owned();
    s = TAG_RE.replace_all(&s, "\n").into_owned();
    s = decode_entities_basic(&s);
    s = WS_RE.replace_all(&s, " ").into_owned();
    s = s.replace("\r\n", "\n").replace('\r', "\n");
    s = NL_RE.replace_all(&s, "\n\n").into_owned();
    s.trim().to_string()
}

#[must_use]
/// Minimal HTML entity decoding for titles/content extraction.
pub fn decode_entities_basic(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&#x27;", "'")
        .replace("&#8211;", "–")
        .replace("&#8212;", "—")
        .replace("&#8226;", "•")
        .replace("&nbsp;", " ")
}

/// Normalize noisy page titles.
pub fn clean_title(input: &str) -> String {
    // Strip adjectives / diet tags
    static ADJ_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)^(best|easy|quick|simple|ultimate|perfect|authentic|classic|vegan|keto|paleo|gluten[- ]free)\s+",
        )
        .unwrap()
    });
    static RECIPE_TAIL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\s+recipes?$").unwrap());
    let mut s = decode_entities_basic(input).trim().to_string();

    // Cut at common separators (keep the left part)
    let seps = ['•', '|', '—', '–', ':'];
    if let Some(idx) = s.find(|c| seps.contains(&c)) {
        s = s[..idx].trim().to_string();
    }

    loop {
        let new = ADJ_RE.replace(&s, "").trim().to_string();
        if new == s {
            break;
        }
        s = new;
    }

    s = RECIPE_TAIL_RE.replace(&s, "").trim().to_string();

    // Normalize whitespace & capitalize first letter
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(first) = s.get(..1) {
        s = format!("{}{}", first.to_uppercase(), s.get(1..).unwrap_or(""));
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        assert_eq!(
            extract_title("<html><head><title>Test Title</title></head></html>"),
            Some("Test Title".to_string())
        );
        assert_eq!(
            extract_title("<title>  Whitespace  </title>"),
            Some("Whitespace".to_string())
        );
        assert_eq!(
            extract_title("<TITLE>UPPERCASE</TITLE>"),
            Some("UPPERCASE".to_string())
        );
        assert_eq!(
            extract_title("<title>Entities &amp; &lt;&gt;</title>"),
            Some("Entities & <>".to_string())
        );
        assert_eq!(extract_title("<html>no title</html>"), None);
        assert_eq!(extract_title(""), None);
    }

    #[test]
    fn test_fallback_title_from_url() {
        assert_eq!(
            fallback_title_from_url("https://example.com/recipe/pasta"),
            Some("example.com — recipe/pasta".to_string())
        );
        assert_eq!(
            fallback_title_from_url("https://example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            fallback_title_from_url("https://example.com/"),
            Some("example.com".to_string())
        );
        assert_eq!(fallback_title_from_url("not a url"), None);
    }

    #[test]
    fn test_decode_entities_basic() {
        assert_eq!(decode_entities_basic("&amp;"), "&");
        assert_eq!(decode_entities_basic("&lt;&gt;"), "<>");
        assert_eq!(decode_entities_basic("&quot;test&quot;"), "\"test\"");
        assert_eq!(decode_entities_basic("&#39;"), "'");
        assert_eq!(decode_entities_basic("&#039;"), "'");
        assert_eq!(decode_entities_basic("&#x27;"), "'");
        assert_eq!(decode_entities_basic("&#8211;"), "–");
        assert_eq!(decode_entities_basic("&#8212;"), "—");
        assert_eq!(decode_entities_basic("&#8226;"), "•");
        assert_eq!(decode_entities_basic("&nbsp;"), " ");
        assert_eq!(decode_entities_basic("no entities"), "no entities");
    }

    #[test]
    fn test_html_to_plain_text() {
        assert_eq!(
            html_to_plain_text("<p>Hello <b>world</b></p>"),
            "Hello \nworld"
        );
        assert_eq!(
            html_to_plain_text("<div><script>alert('test')</script>Content</div>"),
            "Content"
        );
        assert_eq!(
            html_to_plain_text("<style>body{color:red}</style><p>Text</p>"),
            "Text"
        );
        assert_eq!(
            html_to_plain_text("<p>Line 1</p><p>Line 2</p>"),
            "Line 1\n\nLine 2"
        );
        assert_eq!(html_to_plain_text(""), "");
    }

    #[test]
    fn test_clean_title() {
        assert_eq!(clean_title("Best Pasta Recipe"), "Pasta");
        assert_eq!(clean_title("Easy Quick Simple Cake"), "Cake");
        assert_eq!(clean_title("Ultimate Vegan Burger Recipe"), "Burger");
        assert_eq!(clean_title("Gluten-Free Pizza"), "Pizza");
        assert_eq!(clean_title("Pasta | Site Name"), "Pasta");
        assert_eq!(clean_title("Pasta • Recipe"), "Pasta");
        assert_eq!(clean_title("Pasta — Site"), "Pasta");
        assert_eq!(clean_title("Pasta: The Best"), "Pasta");
        assert_eq!(clean_title("pasta recipe"), "Pasta");
        assert_eq!(clean_title("  whitespace  "), "Whitespace");
        assert_eq!(clean_title("Pasta & Sauce"), "Pasta & Sauce");
        assert_eq!(clean_title("Multiple  Spaces"), "Multiple Spaces");
    }

    #[test]
    fn test_clean_title_capitalization() {
        assert_eq!(clean_title("pasta"), "Pasta");
        assert_eq!(clean_title("PASTA"), "PASTA");
        assert_eq!(clean_title("pASTA"), "PASTA");
    }

    #[test]
    fn test_clean_title_no_changes_needed() {
        assert_eq!(clean_title("Pasta Carbonara"), "Pasta Carbonara");
        assert_eq!(clean_title("Chicken Soup"), "Chicken Soup");
    }
}
