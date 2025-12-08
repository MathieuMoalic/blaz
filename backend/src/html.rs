use once_cell::sync::Lazy;
use regex::Regex;

/// Extract <title>...</title> from raw HTML and decode basic entities.
pub fn extract_title(html: &str) -> Option<String> {
    static TITLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

    let raw = TITLE_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())?;

    Some(decode_entities_basic(&raw))
}

/// Fallback guess if we couldn't get a good page/LLM title.
pub fn fallback_title_from_url(url: &str) -> Option<String> {
    if let Ok(u) = reqwest::Url::parse(url) {
        let host = u.host_str().unwrap_or_default().to_string();
        let p = u.path().trim_matches('/');
        if p.is_empty() {
            Some(host)
        } else {
            Some(format!("{host} — {p}"))
        }
    } else {
        None
    }
}

/// Convert HTML to readable plain text.
pub fn html_to_plain_text(html: &str) -> String {
    static SCRIPT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
    static STYLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
    static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<[^>]+>").unwrap());
    static WS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t\r\f]+").unwrap());
    static NL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());

    let mut s = SCRIPT_RE.replace_all(html, " ").into_owned();
    s = STYLE_RE.replace_all(&s, " ").into_owned();
    s = TAG_RE.replace_all(&s, "\n").into_owned();
    s = decode_entities_basic(&s);
    s = WS_RE.replace_all(&s, " ").into_owned();
    s = s.replace("\r\n", "\n").replace('\r', "\n");
    s = NL_RE.replace_all(&s, "\n\n").into_owned();
    s.trim().to_string()
}

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
    let mut s = decode_entities_basic(input).trim().to_string();

    // Cut at common separators (keep the left part)
    let seps = ['•', '|', '—', '–', ':'];
    if let Some(idx) = s.find(|c| seps.contains(&c)) {
        s = s[..idx].trim().to_string();
    }

    // Strip adjectives / diet tags
    static ADJ_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?i)^(best|easy|quick|simple|ultimate|perfect|authentic|classic|vegan|keto|paleo|gluten[- ]free)\s+",
        )
        .unwrap()
    });

    loop {
        let new = ADJ_RE.replace(&s, "").trim().to_string();
        if new == s {
            break;
        }
        s = new;
    }

    static RECIPE_TAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\s+recipes?$").unwrap());
    s = RECIPE_TAIL_RE.replace(&s, "").trim().to_string();

    // Normalize whitespace & capitalize first letter
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(first) = s.get(..1) {
        s = format!("{}{}", first.to_uppercase(), s.get(1..).unwrap_or(""));
    }

    s
}
