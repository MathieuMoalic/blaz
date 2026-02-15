// Quick test to check actual html function behavior
use std::path::PathBuf;

fn main() {
    let backend_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    println!("Backend path: {:?}", backend_path);
    
    // We'll manually test the functions here
    println!("\nTesting clean_title:");
    
    let test_cases = vec![
        "Best Pasta Recipe",
        "Easy Quick Simple Cake", 
        "Ultimate Vegan Burger Recipe",
        "Gluten-Free Pizza",
        "Pasta | Site Name",
        "Pasta • Recipe",
        "Pasta — Site",
        "Pasta: The Best",
        "pasta recipe",
        "  whitespace  ",
        "Pasta &amp; Sauce",
        "Multiple  Spaces",
        "Simple Title",
        "Pasta Carbonara",
    ];
    
    for input in test_cases {
        let result = clean_title_test(input);
        println!("clean_title({:?}) = {:?}", input, result);
    }
    
    println!("\nTesting html_to_plain_text:");
    let html_cases = vec![
        "<p>Hello <b>world</b></p>",
        "<div><script>alert('test')</script>Content</div>",
        "<style>body{color:red}</style><p>Text</p>",
        "<p>Line 1</p><p>Line 2</p>",
    ];
    
    for input in html_cases {
        let result = html_to_plain_text_test(input);
        println!("html_to_plain_text({:?}) = {:?}", input, result);
    }
}

// Copied from html.rs for testing
use regex::Regex;
use std::sync::LazyLock;

fn decode_entities_basic(s: &str) -> String {
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

fn clean_title_test(input: &str) -> String {
    static ADJ_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)^(best|easy|quick|simple|ultimate|perfect|authentic|classic|vegan|keto|paleo|gluten[- ]free)\s+",
        )
        .unwrap()
    });
    static RECIPE_TAIL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\s+recipes?$").unwrap());
    let mut s = decode_entities_basic(input).trim().to_string();

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

    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(first) = s.get(..1) {
        s = format!("{}{}", first.to_uppercase(), s.get(1..).unwrap_or(""));
    }

    s
}

fn html_to_plain_text_test(html: &str) -> String {
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
