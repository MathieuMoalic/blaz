use regex::Regex;
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use std::{sync::LazyLock, time::Duration};

#[derive(Debug, Clone)]
pub struct LlmClient {
    pub base: String,
    pub token: String,
    pub model: String,
}

impl LlmClient {
    #[must_use]
    pub const fn new(base: String, token: String, model: String) -> Self {
        Self { base, token, model }
    }

    /// # Errors
    ///
    /// Will return err if the request fails or if the response can't be serialized as json
    pub async fn chat_json(
        &self,
        http: &reqwest::Client,
        system: &str,
        user: &str,
        temperature: f32,
        timeout: Duration,
        max_tokens: Option<u32>,
    ) -> anyhow::Result<JsonValue> {
        #[derive(Serialize)]
        struct Msg<'a> {
            role: &'a str,
            content: &'a str,
        }
        #[derive(Serialize)]
        struct Body<'a> {
            model: &'a str,
            messages: Vec<Msg<'a>>,
            temperature: f32,
            #[serde(skip_serializing_if = "Option::is_none")]
            max_tokens: Option<u32>,
            response_format: JsonValue,
        }

        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));

        let body = Body {
            model: &self.model,
            messages: vec![
                Msg {
                    role: "system",
                    content: system,
                },
                Msg {
                    role: "user",
                    content: user,
                },
            ],
            temperature,
            max_tokens,
            response_format: json!({ "type": "json_object" }),
        };

        let mut req = http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(timeout)
            .json(&body);

        if !self.token.trim().is_empty() {
            req = req.bearer_auth(&self.token);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("LLM HTTP {status}: {text}");
        }

        let envelope: JsonValue = serde_json::from_str(&text)?;
        let content = envelope
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .or_else(|| {
                envelope
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c0| c0.get("text"))
                    .and_then(|v| v.as_str())
            })
            .ok_or_else(|| anyhow::anyhow!("LLM response missing content"))?;

        // 1) direct parse
        if let Ok(js) = serde_json::from_str::<JsonValue>(content) {
            return Ok(js);
        }
        // 2) fenced ```json
        if let Some(js) = extract_fenced_json(content) {
            return Ok(serde_json::from_str(&js)?);
        }
        // 3) balanced object fallback
        if let Some(js) = extract_largest_json_object(content) {
            return Ok(serde_json::from_str(&js)?);
        }

        anyhow::bail!(
            "LLM did not return valid JSON. Preview: {}",
            &content.chars().take(500).collect::<String>()
        )
    }
}

pub struct ImageChatRequest<'a> {
    pub http: &'a reqwest::Client,
    pub system: &'a str,
    pub text_prompt: &'a str,
    pub images: &'a [(String, String)],
    pub temperature: f32,
    pub timeout: Duration,
    pub max_tokens: Option<u32>,
}

impl LlmClient {
    /// Like `chat_json` but attaches base64-encoded images to the user message.
    /// Images must be provided as `(mime_type, base64_data)` pairs, e.g.
    /// `("image/jpeg", "<base64>")`.
    ///
    /// # Errors
    ///
    /// Will return err if the request fails or if the response can't be parsed as JSON.
    pub async fn chat_json_images(
        &self,
        req: ImageChatRequest<'_>,
    ) -> anyhow::Result<JsonValue> {
        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));

        let mut content: Vec<JsonValue> = req.images
            .iter()
            .map(|(mime, b64)| {
                json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{mime};base64,{b64}") }
                })
            })
            .collect();
        content.push(json!({ "type": "text", "text": req.text_prompt }));

        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user",   "content": content }
            ],
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "response_format": { "type": "json_object" }
        });

        let mut http_req = req.http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(req.timeout)
            .json(&body);

        if !self.token.trim().is_empty() {
            http_req = http_req.bearer_auth(&self.token);
        }

        let resp = http_req.send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("LLM HTTP {status}: {text}");
        }

        let envelope: JsonValue = serde_json::from_str(&text)?;
        let content_str = envelope
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("LLM response missing content"))?;

        if let Ok(js) = serde_json::from_str::<JsonValue>(content_str) {
            return Ok(js);
        }
        if let Some(js) = extract_fenced_json(content_str) {
            return Ok(serde_json::from_str(&js)?);
        }
        if let Some(js) = extract_largest_json_object(content_str) {
            return Ok(serde_json::from_str(&js)?);
        }

        anyhow::bail!(
            "Vision LLM did not return valid JSON. Preview: {}",
            &content_str.chars().take(500).collect::<String>()
        )
    }
}
/// Extract JSON object from a ```json ... ``` fenced block.
/// Accepts ```json``` or plain ``` ``` fences (case-insensitive).
pub fn extract_fenced_json(s: &str) -> Option<String> {
    static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| {
        // non-greedy capture of a single JSON object inside a fence
        // supports:
        // ```json { ... } ```
        // ```JSON { ... } ```
        // ``` { ... } ```
        Regex::new(r"(?is)```(?:json)?\s*(\{.*?\})\s*```").unwrap()
    });

    FENCE_RE
        .captures(s)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// Fallback: find the *largest* balanced `{ ... }` JSON-like object in text.
/// This version is string-aware (won't be confused by braces inside quotes).
#[must_use]
pub fn extract_largest_json_object(s: &str) -> Option<String> {
    let mut best: Option<(usize, usize)> = None;

    let mut depth: usize = 0;
    let mut start: Option<usize> = None;

    let mut in_str = false;
    let mut esc = false;

    for (i, ch) in s.char_indices() {
        if in_str {
            match ch {
                '\\' if !esc => {
                    esc = true;
                    continue;
                }
                '"' if !esc => {
                    in_str = false;
                }
                _ => {}
            }
            // reset escape after consuming next char
            if esc && ch != '\\' {
                esc = false;
            } else if esc && ch == '\\' {
                // keep esc true only for this char; next iteration resets unless another backslash
                esc = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_str = true;
                esc = false;
            }
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(st) = start {
                            let cand = (st, i);
                            let cand_len = cand.1.saturating_sub(cand.0);

                            let better = match best {
                                None => true,
                                Some((a, b)) => cand_len > b.saturating_sub(a),
                            };

                            if better {
                                best = Some(cand);
                            }
                        }
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }

    best.map(|(a, b)| s[a..=b].to_string())
}
