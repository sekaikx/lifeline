//! HTTP for the two API shapes: OpenAI-compatible chat completions and
//! Anthropic messages.

use crate::settings::{Provider, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    /// A PNG image, base64 without a data: prefix
    Image {
        png_base64: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// `"user"` or `"assistant"`
    pub role: String,
    pub content: Vec<Part>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message {
            role: "user".into(),
            content: vec![Part::Text { text: text.into() }],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub text: String,
    pub provider: String,
    pub model: String,
    pub local: bool,
    pub ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    /// Known to cost nothing (OpenRouter reports prices)
    pub free: Option<bool>,
    /// Known to accept images
    pub vision: Option<bool>,
}

fn client(local: bool) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("Lifeline/", env!("CARGO_PKG_VERSION")))
        // a local model on a laptop CPU can take minutes for a long answer
        .timeout(Duration::from_secs(if local { 600 } else { 180 }))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}

fn authorize(
    req: reqwest::blocking::RequestBuilder,
    p: &Provider,
) -> reqwest::blocking::RequestBuilder {
    let key = p.api_key.as_deref().filter(|k| !k.is_empty());
    match p.kind {
        ProviderKind::OpenAi => match key {
            Some(k) => req.bearer_auth(k),
            None => req,
        },
        ProviderKind::Anthropic => {
            let req = req.header("anthropic-version", "2023-06-01");
            match key {
                Some(k) => req.header("x-api-key", k),
                None => req,
            }
        }
    }
}

fn send(req: reqwest::blocking::RequestBuilder, p: &Provider) -> Result<Value, String> {
    let resp = req.send().map_err(|e| {
        if e.is_connect() && p.local {
            format!("cannot reach {} at {}: is it running?", p.name, p.base_url)
        } else if e.is_timeout() {
            format!("{} did not answer in time", p.name)
        } else {
            format!("{}: {e}", p.name)
        }
    })?;
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        // providers put the reason in error.message
        let reason = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v["error"]["message"]
                    .as_str()
                    .or(v["error"].as_str())
                    .or(v["message"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        let hint = match status.as_u16() {
            401 | 403 => " (check the API key)",
            404 => " (check the address and the model name)",
            429 => " (rate limit reached; wait or choose another model)",
            _ => "",
        };
        return Err(format!("{} returned {status}: {reason}{hint}", p.name));
    }
    serde_json::from_str(&text).map_err(|e| format!("{}: invalid response: {e}", p.name))
}

fn openai_content(parts: &[Part]) -> Value {
    if let [Part::Text { text }] = parts {
        return json!(text);
    }
    Value::Array(
        parts
            .iter()
            .map(|p| match p {
                Part::Text { text } => json!({"type": "text", "text": text}),
                Part::Image { png_base64 } => json!({
                    "type": "image_url",
                    "image_url": {"url": format!("data:image/png;base64,{png_base64}")}
                }),
            })
            .collect(),
    )
}

fn anthropic_content(parts: &[Part]) -> Value {
    Value::Array(
        parts
            .iter()
            .map(|p| match p {
                Part::Text { text } => json!({"type": "text", "text": text}),
                Part::Image { png_base64 } => json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": png_base64}
                }),
            })
            .collect(),
    )
}

/// Strip the reasoning some open models print before their answer.
fn strip_thinking(text: &str) -> String {
    let mut out = text.to_string();
    while let (Some(a), Some(b)) = (out.find("<think>"), out.find("</think>")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + "</think>".len(), "");
    }
    out.trim().to_string()
}

/// One completion from `model` at provider `p`.
pub fn complete(
    p: &Provider,
    model: &str,
    system: &str,
    messages: &[Message],
    max_tokens: u32,
    temperature: f32,
) -> Result<Completion, String> {
    let start = Instant::now();
    let http = client(p.local)?;
    let text = match p.kind {
        ProviderKind::OpenAi => {
            let mut msgs = vec![json!({"role": "system", "content": system})];
            msgs.extend(
                messages
                    .iter()
                    .map(|m| json!({"role": m.role, "content": openai_content(&m.content)})),
            );
            let body = json!({
                "model": model,
                "messages": msgs,
                "max_tokens": max_tokens,
                "temperature": temperature,
                "stream": false,
            });
            let v = send(
                authorize(http.post(format!("{}/chat/completions", p.base_url)), p).json(&body),
                p,
            )?;
            let msg = &v["choices"][0]["message"]["content"];
            match msg {
                Value::String(s) => s.clone(),
                // some servers answer with content parts
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|x| x["text"].as_str())
                    .collect::<Vec<_>>()
                    .join(""),
                _ => return Err(format!("{} sent no answer", p.name)),
            }
        }
        ProviderKind::Anthropic => {
            let body = json!({
                "model": model,
                "system": system,
                "max_tokens": max_tokens,
                "temperature": temperature,
                "messages": messages
                    .iter()
                    .map(|m| json!({"role": m.role, "content": anthropic_content(&m.content)}))
                    .collect::<Vec<_>>(),
            });
            let v = send(
                authorize(http.post(format!("{}/messages", p.base_url)), p).json(&body),
                p,
            )?;
            v["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|x| x["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .ok_or_else(|| format!("{} sent no answer", p.name))?
        }
    };
    let text = strip_thinking(&text);
    if text.is_empty() {
        return Err(format!("{model} returned an empty answer"));
    }
    Ok(Completion {
        text,
        provider: p.name.clone(),
        model: model.to_string(),
        local: p.local,
        ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

/// The models a provider serves, sorted by name.
pub fn list_models(p: &Provider) -> Result<Vec<ModelInfo>, String> {
    let http = client(p.local)?;
    let v = send(authorize(http.get(format!("{}/models", p.base_url)), p), p)?;
    let mut out: Vec<ModelInfo> = v["data"]
        .as_array()
        .or(v["models"].as_array())
        .ok_or_else(|| format!("{} did not list its models", p.name))?
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str().or(m["name"].as_str())?;
            let id = id.trim_start_matches("models/").to_string();
            let price = |k: &str| m["pricing"][k].as_str().and_then(|s| s.parse::<f64>().ok());
            let free = match (price("prompt"), price("completion")) {
                (Some(a), Some(b)) => Some(a == 0.0 && b == 0.0),
                _ => p.local.then_some(true),
            };
            let vision = m["architecture"]["input_modalities"]
                .as_array()
                .map(|a| a.iter().any(|x| x == "image"));
            Some(ModelInfo { id, free, vision })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_is_removed() {
        assert_eq!(
            strip_thinking("<think>hmm\nok</think>\n{\"a\":1}"),
            "{\"a\":1}"
        );
        assert_eq!(strip_thinking("plain"), "plain");
        assert_eq!(
            strip_thinking("</think> odd <think>"),
            "</think> odd <think>"
        );
    }

    #[test]
    fn content_shapes() {
        assert_eq!(
            openai_content(&[Part::Text { text: "hi".into() }]),
            json!("hi")
        );
        let both = [
            Part::Text { text: "see".into() },
            Part::Image {
                png_base64: "AAA".into(),
            },
        ];
        let o = openai_content(&both);
        assert_eq!(o[1]["image_url"]["url"], "data:image/png;base64,AAA");
        let a = anthropic_content(&both);
        assert_eq!(a[1]["source"]["data"], "AAA");
    }
}
