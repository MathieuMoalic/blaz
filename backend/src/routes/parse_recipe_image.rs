use scraper::{ElementRef, Html, Selector};
use url::Url;

#[derive(Debug, Clone)]
struct ImgCandidate {
    url: String,
    signal: i32,             // source authority score
    declared_w: Option<i32>, // from og:width / srcset
    declared_h: Option<i32>, // from og:height
    dom_bonus: i32,          // near title/article etc.
}

#[must_use]
pub fn extract_main_image_url(html: &str, page_url: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let base_url = page_base_url(&doc, page_url);

    let mut out: Vec<ImgCandidate> = Vec::new();

    // --- JSON-LD Recipe.image ---
    if let Some(list) = json_ld_recipe_images(&doc) {
        for (u, w, h) in list {
            if let Some(abs) = absolutize(&base_url, &u) {
                out.push(ImgCandidate {
                    url: abs,
                    signal: 100,
                    declared_w: w,
                    declared_h: h,
                    dom_bonus: 0,
                });
            }
        }
    }

    // --- Open Graph ---
    for c in og_images(&doc, &base_url) {
        out.push(c);
    }

    // --- Twitter Card ---
    for c in twitter_images(&doc, &base_url) {
        out.push(c);
    }

    // --- link rel=image_src / itemprop=image ---
    for c in misc_meta_images(&doc, &base_url) {
        out.push(c);
    }

    // --- <picture>/<img> (srcset + data-* + src) ---
    for c in dom_img_candidates(&doc, &base_url) {
        out.push(c);
    }

    // Dedup by URL
    dedupe_by(&mut out, |c| c.url.clone());

    // Filter/score
    out.retain(|c| is_plausible_url(&c.url));
    for c in &mut out {
        c.dom_bonus += filename_bonus(&c.url);
        c.dom_bonus += aspect_hint_bonus(c.declared_w, c.declared_h);
    }

    // Pick the best
    out.sort_by_key(|c| -(c.signal + c.dom_bonus + size_hint_score(c.declared_w, c.declared_h)));
    out.first().map(|c| c.url.clone())
}

/* ---------------- helpers ---------------- */

fn page_base_url(doc: &Html, page_url: &str) -> Url {
    let mut base =
        Url::parse(page_url).unwrap_or_else(|_| Url::parse("https://example.com/").unwrap());
    if let Ok(sel) = Selector::parse(r"base[href]")
        && let Some(el) = doc.select(&sel).next()
        && let Some(h) = el.value().attr("href")
        && let Ok(abs) = base.join(h) {
            base = abs;
        }
    base
}

fn absolutize(base: &Url, raw: &str) -> Option<String> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    if raw.starts_with("//") {
        return Some(format!("{}:{}", base.scheme(), raw));
    }
    base.join(raw).ok().map(|u| u.to_string())
}
type Image = Option<Vec<(String, Option<i32>, Option<i32>)>>;
fn json_ld_recipe_images(doc: &Html) -> Image {
    use serde_json::Value;
    let sel = Selector::parse(r#"script[type="application/ld+json"]"#).ok()?;
    let mut out = vec![];
    for node in doc.select(&sel) {
        let raw = node.text().collect::<String>();
        if let Ok(val) = serde_json::from_str::<Value>(&raw)
            && let Some(imgs) = find_recipe_images_in_ld(&val) {
                out.extend(imgs);
            }
    }
    if out.is_empty() { None } else { Some(out) }
}
fn find_recipe_images_in_ld(v: &serde_json::Value) -> Image {
    use serde_json::{Map, Value};

    fn grab(o: &Map<String, Value>) -> Image {
        fn to_i32(v: Option<i64>) -> Option<i32> {
            v.and_then(|x| i32::try_from(x).ok())
        }

        let t = o.get("@type");
        let is_recipe = match t {
            Some(Value::String(s)) => s.eq_ignore_ascii_case("Recipe"),
            Some(Value::Array(a)) => a
                .iter()
                .any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case("Recipe"))),
            _ => false,
        };
        if !is_recipe {
            return None;
        }

        let mut out = vec![];
        match o.get("image") {
            Some(Value::String(s)) => out.push((s.clone(), None, None)),
            Some(Value::Array(arr)) => {
                for it in arr {
                    match it {
                        Value::String(s) => out.push((s.clone(), None, None)),
                        Value::Object(io) => {
                            let url = io
                                .get("url")
                                .or_else(|| io.get("contentUrl"))
                                .and_then(|x| x.as_str())
                                .map(std::string::ToString::to_string);

                            if let Some(u) = url {
                                let w = to_i32(io.get("width").and_then(Value::as_i64));
                                let h = to_i32(io.get("height").and_then(Value::as_i64));
                                out.push((u, w, h));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some(Value::Object(io)) => {
                let url = io
                    .get("url")
                    .or_else(|| io.get("contentUrl"))
                    .and_then(|x| x.as_str())
                    .map(std::string::ToString::to_string);

                if let Some(u) = url {
                    let w = to_i32(io.get("width").and_then(Value::as_i64));
                    let h = to_i32(io.get("height").and_then(Value::as_i64));
                    out.push((u, w, h));
                }
            }
            _ => {}
        }

        Some(out)
    }

    match v {
        Value::Object(o) => {
            if let Some(g) = o.get("@graph")
                && let Some(list) = find_recipe_images_in_ld(g) {
                    return Some(list);
                }
            grab(o)
        }
        Value::Array(a) => {
            for n in a {
                if let Some(list) = find_recipe_images_in_ld(n) {
                    return Some(list);
                }
            }
            None
        }
        _ => None,
    }
}

fn og_images(doc: &Html, base: &Url) -> Vec<ImgCandidate> {
    let mut out = vec![];
    let sel = Selector::parse(r#"meta[property^="og:image"]"#).unwrap();
    let mut url: Option<String> = None;
    let mut w: Option<i32> = None;
    let mut h: Option<i32> = None;
    for el in doc.select(&sel) {
        let prop = el.value().attr("property").unwrap_or("");
        let content = el.value().attr("content").unwrap_or("");
        match prop {
            "og:image" | "og:image:url" | "og:image:secure_url" => {
                if let Some(abs) = absolutize(base, content) {
                    url = Some(abs);
                }
            }
            "og:image:width" => {
                w = content.parse().ok();
            }
            "og:image:height" => {
                h = content.parse().ok();
            }
            _ => {}
        }
        if prop == "og:image" && url.is_some() {
            // push current block; multiple og:image blocks may exist
            out.push(ImgCandidate {
                url: url.clone().unwrap(),
                signal: 90,
                declared_w: w,
                declared_h: h,
                dom_bonus: 0,
            });
            // reset dims; next block may declare its own w/h
            w = None;
            h = None;
        }
    }
    // fallthrough if only :secure_url present
    if let Some(u) = url {
        out.push(ImgCandidate {
            url: u,
            signal: 90,
            declared_w: w,
            declared_h: h,
            dom_bonus: 0,
        });
    }
    out
}

fn twitter_images(doc: &Html, base: &Url) -> Vec<ImgCandidate> {
    let mut out = vec![];
    let sel = Selector::parse(r#"meta[name^="twitter:image"]"#).unwrap();
    for el in doc.select(&sel) {
        if let Some(c) = el.value().attr("content")
            && let Some(abs) = absolutize(base, c) {
                out.push(ImgCandidate {
                    url: abs,
                    signal: 80,
                    declared_w: None,
                    declared_h: None,
                    dom_bonus: 0,
                });
            }
    }
    out
}

fn misc_meta_images(doc: &Html, base: &Url) -> Vec<ImgCandidate> {
    let mut out = vec![];
    if let Ok(sel) = Selector::parse(r#"link[rel="image_src"]"#) {
        for el in doc.select(&sel) {
            if let Some(h) = el.value().attr("href")
                && let Some(abs) = absolutize(base, h) {
                    out.push(ImgCandidate {
                        url: abs,
                        signal: 70,
                        declared_w: None,
                        declared_h: None,
                        dom_bonus: 0,
                    });
                }
        }
    }
    if let Ok(sel) = Selector::parse(r#"meta[itemprop="image"]"#) {
        for el in doc.select(&sel) {
            if let Some(c) = el.value().attr("content")
                && let Some(abs) = absolutize(base, c) {
                    out.push(ImgCandidate {
                        url: abs,
                        signal: 70,
                        declared_w: None,
                        declared_h: None,
                        dom_bonus: 0,
                    });
                }
        }
    }
    out
}

fn dom_img_candidates(doc: &Html, base: &Url) -> Vec<ImgCandidate> {
    let mut out = vec![];
    let img_sel = Selector::parse("img, picture source").unwrap();

    // title proximity bonus
    let title_text = extract_title_like(doc);
    for el in doc.select(&img_sel) {
        // srcset before src (often higher res)
        let srcset = attr_chain(&el, &["srcset", "data-srcset", "data-lazy-srcset"]);
        if let Some(ss) = srcset {
            for (u, w) in parse_srcset(ss) {
                if let Some(abs) = absolutize(base, &u) {
                    out.push(ImgCandidate {
                        url: abs,
                        signal: 60,
                        declared_w: w,
                        declared_h: None,
                        dom_bonus: if near_title(&el, &title_text) { 10 } else { 0 },
                    });
                }
            }
        }
        // plain src / data-src
        if let Some(s) = attr_chain(&el, &["src", "data-src", "data-original", "data-lazy"])
            && let Some(abs) = absolutize(base, s) {
                out.push(ImgCandidate {
                    url: abs,
                    signal: 55,
                    declared_w: None,
                    declared_h: None,
                    dom_bonus: if near_title(&el, &title_text) { 10 } else { 0 },
                });
            }
        // inline background-image
        if let Some(style) = el.value().attr("style")
            && let Some(bg) = extract_bg_url(style)
            && let Some(abs) = absolutize(base, &bg) {
                out.push(ImgCandidate {
                    url: abs,
                    signal: 50,
                    declared_w: None,
                    declared_h: None,
                    dom_bonus: 0,
                });
            }
    }
    out
}

fn attr_chain<'a>(el: &'a ElementRef<'a>, names: &[&str]) -> Option<&'a str> {
    for n in names {
        if let Some(v) = el.value().attr(n)
            && !v.trim().is_empty() {
                return Some(v);
            }
    }
    None
}

fn extract_bg_url(style: &str) -> Option<String> {
    // crude: background-image:url("...") or url('...') or url(...)
    let s = style;
    let start = s.find("background-image")?;
    let rest = &s[start..];
    let u = rest.find("url(")?;
    let mut sub = &rest[u + 4..];
    if let Some(end) = sub.find(')') {
        sub = &sub[..end];
        return Some(
            sub.trim_matches(|c| c == ' ' || c == '"' || c == '\'')
                .to_string(),
        );
    }
    None
}

fn parse_srcset(s: &str) -> Vec<(String, Option<i32>)> {
    // "img-800.jpg 800w, img-2x.jpg 2x"
    s.split(',')
        .filter_map(|part| {
            let p = part.trim();
            if p.is_empty() {
                return None;
            }
            let mut it = p.split_whitespace();
            let url = it.next()?.to_string();
            let desc = it.next().unwrap_or("");
            let w = desc.strip_suffix('w').and_then(|s| s.parse::<i32>().ok());

            Some((url, w))
        })
        .collect()
}

fn extract_title_like(doc: &Html) -> String {
    // og:title > <title> > first h1
    if let Ok(sel) = Selector::parse(r#"meta[property="og:title"]"#)
        && let Some(el) = doc.select(&sel).next()
        && let Some(c) = el.value().attr("content") {
            return c.trim().to_string();
        }
    if let Ok(sel) = Selector::parse("title")
        && let Some(el) = doc.select(&sel).next() {
            let t = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return t;
            }
        }
    if let Ok(sel) = Selector::parse("h1")
        && let Some(el) = doc.select(&sel).next() {
            let t = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return t;
            }
        }
    String::new()
}

fn near_title(el: &ElementRef<'_>, title: &str) -> bool {
    // very light heuristic: bonus if the element shares an ancestor section/article with a node containing the title text
    if title.is_empty() {
        return false;
    }
    let mut up = *el;
    for _ in 0..6 {
        if let Some(parent) = up.parent().and_then(ElementRef::wrap) {
            if node_contains_text(&parent, title) {
                return true;
            }
            up = parent;
        } else {
            break;
        }
    }
    false
}

fn node_contains_text(root: &ElementRef<'_>, needle: &str) -> bool {
    root.text().any(|t| t.contains(needle))
}

fn filename_bonus(u: &str) -> i32 {
    let l = u.to_ascii_lowercase();
    if l.contains("logo") || l.contains("sprite") || l.contains("icon") || l.contains("badge") {
        return -30;
    }
    if l.contains("hero") || l.contains("main") || l.contains("recipe") {
        return 10;
    }
    0
}

fn aspect_hint_bonus(w: Option<i32>, h: Option<i32>) -> i32 {
    match (w, h) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            let r = f64::from(w) / f64::from(h);
            if (0.8..=2.2).contains(&r) { 5 } else { -5 }
        }
        _ => 0,
    }
}

fn size_hint_score(w: Option<i32>, h: Option<i32>) -> i32 {
    match (w, h) {
        (Some(w), Some(h)) => ((w * h) / 10000).clamp(0, 200), // cap contribution
        (Some(w), None) => (w / 100).clamp(0, 100),
        _ => 0,
    }
}

fn is_plausible_url(u: &str) -> bool {
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return false;
    }

    // Prefer proper URL parsing so query strings don't confuse extension checks.
    if let Ok(parsed) = Url::parse(u) {
        let path = parsed.path().to_ascii_lowercase();
        if std::path::Path::new(&path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            return false;
        }
        return true;
    }

    // Fallback: strip query/fragment manually.
    let no_q = u.split('?').next().unwrap_or(u);
    let no_frag = no_q.split('#').next().unwrap_or(no_q);

    !no_frag.to_ascii_lowercase().ends_with(".svg")
}

fn dedupe_by<T, F, K: std::cmp::Eq + std::hash::Hash>(v: &mut Vec<T>, mut key: F)
where
    F: FnMut(&T) -> K,
{
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(key(x)));
}
