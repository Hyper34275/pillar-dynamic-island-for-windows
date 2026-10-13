//! The three wire formats, as pure functions: the URL, the headers, the JSON body, and the answer text out
//! of the response. No I/O here (see `http`), so every format is unit-tested.

use super::config::{AiConfig, Provider, DEFAULT_AZURE_API_VERSION};
use serde_json::{json, Value};

/// Anthropic Messages API version header.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// One turn of the conversation sent to the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub user: bool,
    pub text: String,
}

/// The parts of an http(s) URL WinHTTP needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// Path and query, starting with '/'.
    pub path: String,
}

pub fn parse_url(url: &str) -> Result<Url, String> {
    let url = url.trim();
    let (secure, rest) = if let Some(r) = strip_prefix_ci(url, "https://") {
        (true, r)
    } else if let Some(r) = strip_prefix_ci(url, "http://") {
        (false, r)
    } else {
        return Err("the endpoint must start with http:// or https://".into());
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    if authority.contains('@') {
        return Err("put the key in apiKey, not in the endpoint".into());
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let end = v6.find(']').ok_or("invalid endpoint host")?;
        let port = v6[end + 1..].strip_prefix(':');
        (format!("[{}]", &v6[..end]), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    if host.is_empty() || host.contains([' ', '\\']) {
        return Err("invalid endpoint host".into());
    }
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or("invalid endpoint port")?,
        None if secure => 443,
        None => 80,
    };
    if path.contains([' ', '\r', '\n']) {
        return Err("the endpoint must not contain spaces".into());
    }
    Ok(Url { secure, host, port, path })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)).then(|| &s[prefix.len()..])
}

/// The endpoint without a trailing '/' (query kept).
fn base(endpoint: &str) -> (&str, &str) {
    let e = endpoint.trim();
    let (path, query) = match e.find('?') {
        Some(i) => (&e[..i], &e[i..]),
        None => (e, ""),
    };
    (path.trim_end_matches('/'), query)
}

fn path_part(url_path: &str) -> &str {
    // what follows "scheme://host[:port]"
    let after = url_path.find("://").map(|i| &url_path[i + 3..]).unwrap_or(url_path);
    after.find('/').map(|i| &after[i..]).unwrap_or("")
}

/// The full request URL for `provider`, from the endpoint as the user wrote it (a base like
/// `http://host:8000/v1`, or the complete URL).
pub fn request_url(c: &AiConfig, provider: Provider) -> String {
    let (path, query) = base(&c.endpoint);
    let lower = path.to_ascii_lowercase();
    match provider {
        Provider::OpenAi => {
            if lower.ends_with("/chat/completions") {
                format!("{path}{query}")
            } else if path_part(path).is_empty() {
                format!("{path}/v1/chat/completions{query}")
            } else {
                format!("{path}/chat/completions{query}")
            }
        }
        Provider::Azure => {
            let url = if lower.ends_with("/chat/completions") {
                path.to_string()
            } else if lower.contains("/openai/deployments/") {
                format!("{path}/chat/completions")
            } else if lower.ends_with("/openai") {
                format!("{path}/deployments/{}/chat/completions", c.model.trim())
            } else {
                format!("{path}/openai/deployments/{}/chat/completions", c.model.trim())
            };
            if query.to_ascii_lowercase().contains("api-version=") {
                format!("{url}{query}")
            } else {
                let version = if c.api_version.trim().is_empty() { DEFAULT_AZURE_API_VERSION } else { c.api_version.trim() };
                let sep = if query.is_empty() { "?" } else { "&" };
                format!("{url}{query}{sep}api-version={version}")
            }
        }
        Provider::Anthropic => {
            if lower.ends_with("/messages") {
                format!("{path}{query}")
            } else if lower.ends_with("/v1") {
                format!("{path}/messages{query}")
            } else {
                format!("{path}/v1/messages{query}")
            }
        }
    }
}

/// Request headers (name, value). The key is only added when there is one.
pub fn headers(c: &AiConfig, provider: Provider, key: &str) -> Vec<(String, String)> {
    let mut h = vec![("Content-Type".to_string(), "application/json; charset=utf-8".to_string()), ("Accept".into(), "application/json".into())];
    if !key.is_empty() {
        match provider {
            Provider::OpenAi => h.push(("Authorization".into(), format!("Bearer {key}"))),
            Provider::Azure => h.push(("api-key".into(), key.to_string())),
            Provider::Anthropic => h.push(("x-api-key".into(), key.to_string())),
        }
    }
    if provider == Provider::Anthropic {
        h.push(("anthropic-version".into(), ANTHROPIC_VERSION.into()));
    }
    for (name, value) in &c.headers {
        // a custom header replaces a built-in one of the same name
        h.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        h.push((name.clone(), value.clone()));
    }
    h
}

/// The JSON body: the system prompt, the earlier turns, then the question.
pub fn body(c: &AiConfig, provider: Provider, system: &str, turns: &[Turn]) -> Value {
    let messages: Vec<Value> = turns.iter().map(|t| json!({ "role": if t.user { "user" } else { "assistant" }, "content": t.text })).collect();
    match provider {
        Provider::OpenAi | Provider::Azure => {
            let mut all = vec![json!({ "role": "system", "content": system })];
            all.extend(messages);
            let mut b = json!({ "messages": all, "stream": false });
            if !c.model.trim().is_empty() {
                b["model"] = c.model.trim().into();
            }
            if c.max_tokens() > 0 {
                b["max_tokens"] = c.max_tokens().into();
            }
            if let Some(t) = c.temperature {
                b["temperature"] = t.into();
            }
            b
        }
        Provider::Anthropic => {
            let max = if c.max_tokens() > 0 { c.max_tokens() } else { 1024 };
            let mut b = json!({ "model": c.model.trim(), "max_tokens": max, "system": system, "messages": messages });
            if let Some(t) = c.temperature {
                b["temperature"] = t.into();
            }
            b
        }
    }
}

/// The answer text of a successful response, or an `AI-00x` error.
pub fn parse_answer(provider: Provider, body: &[u8]) -> Result<String, String> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "AI-006: the server's answer is not JSON".to_string())?;
    let text = match provider {
        Provider::OpenAi | Provider::Azure => {
            let choice = v.get("choices").and_then(Value::as_array).and_then(|c| c.first()).ok_or("AI-006: the answer has no choices")?;
            if choice.get("finish_reason").and_then(Value::as_str) == Some("content_filter") {
                return Err("AI-007: the server's content filter blocked the answer".into());
            }
            let content = choice.get("message").and_then(|m| m.get("content")).or_else(|| choice.get("text"));
            match content {
                Some(Value::String(s)) => s.clone(),
                // content as parts: [{ "type": "text", "text": "…" }]
                Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""),
                _ => String::new(),
            }
        }
        Provider::Anthropic => {
            if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
                return Err("AI-007: the model declined to answer".into());
            }
            let blocks = v.get("content").and_then(Value::as_array).ok_or("AI-006: the answer has no content")?;
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        }
    };
    let text = strip_thinking(&text);
    if text.trim().is_empty() {
        return Err("AI-007: the model returned an empty answer".into());
    }
    Ok(text.trim().to_string())
}

/// Reasoning models served through OpenAI-compatible servers (DeepSeek-R1, Qwen3 …) may put their
/// reasoning in `<think>…</think>` before the answer: only the answer is shown.
fn strip_thinking(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find("<think>") {
        match out[start..].find("</think>") {
            Some(end) => out.replace_range(start..start + end + "</think>".len(), ""),
            None => {
                out.truncate(start);
                break;
            }
        }
    }
    out
}

/// The server's own error message from an error response (`{"error":{"message":…}}`, `{"message":…}`,
/// `{"detail":…}`), clipped. Empty when there is none.
pub fn error_message(body: &[u8]) -> String {
    let msg = serde_json::from_slice::<Value>(body).ok().and_then(|v| {
        let e = v.get("error").unwrap_or(&v);
        e.get("message")
            .or_else(|| v.get("message"))
            .or_else(|| v.get("detail"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| e.as_str().map(str::to_string))
    });
    let msg = msg.unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string());
    msg.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// The error for a non-2xx status.
pub fn status_error(status: u32, body: &[u8]) -> String {
    let msg = error_message(body);
    let tail = if msg.is_empty() { String::new() } else { format!(": {msg}") };
    match status {
        401 | 403 => format!("AI-004: the server refused the key (HTTP {status}){tail}"),
        404 => format!("AI-005: the address or model was not found (HTTP 404){tail}"),
        429 => format!("AI-005: the server is busy or over quota (HTTP 429){tail}"),
        _ => format!("AI-005: the server answered HTTP {status}{tail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: &str, endpoint: &str) -> AiConfig {
        AiConfig { provider: provider.into(), endpoint: endpoint.into(), model: "m1".into(), ..AiConfig::default() }
    }

    #[test]
    fn openai_urls_from_a_base_or_the_full_address() {
        let u = |e: &str| request_url(&cfg("openai", e), Provider::OpenAi);
        assert_eq!(u("http://ai:8000/v1"), "http://ai:8000/v1/chat/completions");
        assert_eq!(u("http://ai:8000/v1/"), "http://ai:8000/v1/chat/completions");
        assert_eq!(u("http://ai:8000"), "http://ai:8000/v1/chat/completions");
        assert_eq!(u("https://gw.corp/llm/v1/chat/completions"), "https://gw.corp/llm/v1/chat/completions");
        assert_eq!(u("http://localhost:11434/v1"), "http://localhost:11434/v1/chat/completions");
    }

    #[test]
    fn azure_urls_get_the_deployment_and_the_api_version() {
        let mut c = cfg("azure", "https://x.openai.azure.com/");
        c.model = "gpt4o".into();
        assert_eq!(request_url(&c, Provider::Azure), "https://x.openai.azure.com/openai/deployments/gpt4o/chat/completions?api-version=2024-10-21");
        c.api_version = "2025-01-01-preview".into();
        c.endpoint = "https://x.openai.azure.com/openai/deployments/d1".into();
        assert_eq!(request_url(&c, Provider::Azure), "https://x.openai.azure.com/openai/deployments/d1/chat/completions?api-version=2025-01-01-preview");
        c.endpoint = "https://x.openai.azure.com/openai/deployments/d1/chat/completions?api-version=2024-06-01".into();
        assert_eq!(request_url(&c, Provider::Azure), c.endpoint);
    }

    #[test]
    fn anthropic_urls() {
        let u = |e: &str| request_url(&cfg("anthropic", e), Provider::Anthropic);
        assert_eq!(u("https://api.anthropic.com"), "https://api.anthropic.com/v1/messages");
        assert_eq!(u("https://api.anthropic.com/v1"), "https://api.anthropic.com/v1/messages");
        assert_eq!(u("https://gw.corp/anthropic/v1/messages"), "https://gw.corp/anthropic/v1/messages");
    }

    #[test]
    fn urls_are_split_for_winhttp() {
        assert_eq!(parse_url("http://ai:8000/v1/chat/completions").unwrap(), Url { secure: false, host: "ai".into(), port: 8000, path: "/v1/chat/completions".into() });
        assert_eq!(parse_url("HTTPS://Host.corp").unwrap(), Url { secure: true, host: "Host.corp".into(), port: 443, path: "/".into() });
        assert_eq!(parse_url("https://h?x=1").unwrap().path, "/?x=1");
        assert_eq!(parse_url("http://[::1]:9/a").unwrap(), Url { secure: false, host: "[::1]".into(), port: 9, path: "/a".into() });
        for bad in ["ftp://x", "http://", "http://u:p@h/", "http://h:0/", "http://h:99999/", "http://h/a b", "ai:8000"] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn headers_per_provider_and_custom_ones_win() {
        let mut c = cfg("openai", "http://a");
        assert!(headers(&c, Provider::OpenAi, "").iter().all(|(n, _)| n != "Authorization"), "no key, no Authorization");
        assert!(headers(&c, Provider::OpenAi, "k").contains(&("Authorization".into(), "Bearer k".into())));
        assert!(headers(&c, Provider::Azure, "k").contains(&("api-key".into(), "k".into())));
        let a = headers(&c, Provider::Anthropic, "k");
        assert!(a.contains(&("x-api-key".into(), "k".into())) && a.contains(&("anthropic-version".into(), ANTHROPIC_VERSION.into())));
        c.headers.insert("authorization".into(), "Token abc".into());
        let h = headers(&c, Provider::OpenAi, "k");
        assert_eq!(h.iter().filter(|(n, _)| n.eq_ignore_ascii_case("authorization")).count(), 1);
        assert!(h.contains(&("authorization".into(), "Token abc".into())));
    }

    #[test]
    fn bodies() {
        let mut c = cfg("openai", "http://a");
        c.temperature = Some(0.2);
        let turns = [Turn { user: true, text: "שלום".into() }];
        let b = body(&c, Provider::OpenAi, "sys", &turns);
        assert_eq!(b["model"], "m1");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "שלום");
        assert_eq!(b["temperature"], 0.2);
        assert_eq!(b["max_tokens"], 1024);
        let a = body(&c, Provider::Anthropic, "sys", &turns);
        assert_eq!(a["system"], "sys");
        assert_eq!(a["messages"].as_array().unwrap().len(), 1);
        assert_eq!(a["messages"][0]["role"], "user");
        c.max_tokens = 0;
        c.temperature = None;
        let b = body(&c, Provider::OpenAi, "sys", &turns);
        assert!(b.get("max_tokens").is_none() && b.get("temperature").is_none());
        assert_eq!(body(&c, Provider::Anthropic, "s", &turns)["max_tokens"], 1024, "anthropic requires max_tokens");
    }

    #[test]
    fn answers_are_read_from_each_format() {
        let oa = r#"{"choices":[{"message":{"role":"assistant","content":"  שלום!  "},"finish_reason":"stop"}]}"#.as_bytes();
        assert_eq!(parse_answer(Provider::OpenAi, oa).unwrap(), "שלום!");
        let parts = br#"{"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]}"#;
        assert_eq!(parse_answer(Provider::Azure, parts).unwrap(), "ab");
        let think = r#"{"choices":[{"message":{"content":"<think>hmm</think>\nתשובה"}}]}"#.as_bytes();
        assert_eq!(parse_answer(Provider::OpenAi, think).unwrap(), "תשובה");
        let an = br#"{"id":"msg_1","type":"message","content":[{"type":"thinking","thinking":""},{"type":"text","text":"Hi"},{"type":"text","text":" there"}],"stop_reason":"end_turn"}"#;
        assert_eq!(parse_answer(Provider::Anthropic, an).unwrap(), "Hi there");
        let refusal = br#"{"content":[],"stop_reason":"refusal"}"#;
        assert!(parse_answer(Provider::Anthropic, refusal).unwrap_err().starts_with("AI-007"));
        let filtered = br#"{"choices":[{"message":{"content":null},"finish_reason":"content_filter"}]}"#;
        assert!(parse_answer(Provider::Azure, filtered).unwrap_err().starts_with("AI-007"));
        assert!(parse_answer(Provider::OpenAi, b"<html>").unwrap_err().starts_with("AI-006"));
        assert!(parse_answer(Provider::OpenAi, br#"{"choices":[]}"#).unwrap_err().starts_with("AI-006"));
        assert!(parse_answer(Provider::OpenAi, br#"{"choices":[{"message":{"content":""}}]}"#).unwrap_err().starts_with("AI-007"));
    }

    #[test]
    fn server_errors_keep_the_servers_reason() {
        let e = status_error(401, br#"{"error":{"type":"authentication_error","message":"invalid x-api-key"}}"#);
        assert_eq!(e, "AI-004: the server refused the key (HTTP 401): invalid x-api-key");
        assert!(status_error(404, br#"{"detail":"Not Found"}"#).ends_with("Not Found"));
        assert_eq!(status_error(500, b""), "AI-005: the server answered HTTP 500");
        assert!(status_error(502, b"<html>bad gateway</html>").contains("bad gateway"));
    }
}
