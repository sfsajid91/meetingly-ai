//! Direct OpenAI-compatible HTTP/SSE client for chat, vision, model listing, and provider verification.

use crate::settings::ProviderRole;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Valid 1x1 black JPEG used to check whether a brain model accepts `image_url` inputs.
const TINY_JPEG_BASE64: &str = "/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: LlmContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmContent {
    Text(String),
    TextAndImage { text: String, base64_jpeg: String },
}

impl LlmMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: LlmRole::System, content: LlmContent::Text(text.into()) }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self { role: LlmRole::User, content: LlmContent::Text(text.into()) }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: LlmRole::Assistant, content: LlmContent::Text(text.into()) }
    }

    pub fn user_image(text: impl Into<String>, base64_jpeg: impl Into<String>) -> Self {
        Self {
            role: LlmRole::User,
            content: LlmContent::TextAndImage {
                text: text.into(),
                base64_jpeg: base64_jpeg.into(),
            },
        }
    }

    fn to_json(&self) -> Value {
        let content = match &self.content {
            LlmContent::Text(text) => Value::String(text.clone()),
            LlmContent::TextAndImage { text, base64_jpeg } => json!([
                { "type": "text", "text": text },
                { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{base64_jpeg}") } }
            ]),
        };
        json!({ "role": self.role, "content": content })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResult {
    pub text: String,
    pub tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub ok: bool,
    pub role: ProviderRole,
    pub streaming: Option<bool>,
    pub vision: Option<bool>,
    pub detail: String,
}

pub fn normalize_base_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed.strip_suffix("/chat/completions").unwrap_or(trimmed).trim_end_matches('/').to_string()
}

pub fn host_of(url: &str) -> String {
    Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url.to_string())
}

pub fn reasoning_effort_for(model: &str) -> Option<&'static str> {
    let name = model.rsplit('/').next().unwrap_or(model).to_ascii_lowercase();
    let is_o_series = name.starts_with('o') && name.as_bytes().get(1).is_some_and(u8::is_ascii_digit);
    if name.starts_with("gpt-5") || is_o_series { Some("low") } else { None }
}

pub fn describe_error(status: Option<u16>, raw: &str, endpoint: &str, model: &str) -> String {
    let host = host_of(endpoint);
    let extracted = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("error")?.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    let detail: String = extracted.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    match status {
        Some(401 | 403) => format!("{host} rejected the API key. Check the key and try again."),
        Some(404) => format!("{host} answered 404: check the API address and the model name ({model})."),
        Some(429) => format!("{host} says you are over your rate limit or out of credits."),
        Some(code) => format!("{host} answered with an error ({code}): {detail}"),
        None => format!("Cannot reach {host}: {detail}"),
    }
}

#[derive(Clone)]
pub struct LlmClient {
    http: Client,
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client builds");
        Self { http }
    }

    pub async fn list_models(&self, endpoint: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
        let base = normalize_base_url(endpoint);
        let url = format!("{base}/models");
        let mut req = self.http.get(&url).timeout(CONNECT_TIMEOUT);
        if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
            req = req.bearer_auth(key.trim());
        }
        let res = req.send().await.map_err(|e| describe_error(None, &e.to_string(), &base, ""))?;
        let status = res.status().as_u16();
        let body = res.text().await.unwrap_or_default();
        if status >= 400 {
            return Err(describe_error(Some(status), &body, &base, ""));
        }
        let parsed: Value = serde_json::from_str(&body).map_err(|e| format!("Invalid models response: {e}"))?;
        let mut models: Vec<String> = parsed
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        models.sort();
        models.dedup();
        Ok(models)
    }

    pub async fn chat_once(
        &self,
        endpoint: &str,
        api_key: Option<&str>,
        model: &str,
        messages: &[LlmMessage],
        max_tokens: Option<u32>,
    ) -> Result<LlmResult, String> {
        let base = normalize_base_url(endpoint);
        let body = build_chat_body(model, messages, false, max_tokens, false);
        let res = self.post_chat(&base, api_key, &body, REQUEST_TIMEOUT).await.map_err(|e| describe_error(None, &e, &base, model))?;
        let status = res.status().as_u16();
        let raw = res.text().await.unwrap_or_default();
        if status >= 400 {
            return Err(describe_error(Some(status), &raw, &base, model));
        }
        let parsed: Value = serde_json::from_str(&raw).map_err(|e| format!("Invalid response from {}: {e}", host_of(&base)))?;
        let text = parsed
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tokens = parsed.pointer("/usage/total_tokens").and_then(Value::as_u64).unwrap_or(0);
        Ok(LlmResult { text, tokens })
    }

    pub async fn chat_stream<F>(
        &self,
        endpoint: &str,
        api_key: Option<&str>,
        model: &str,
        messages: &[LlmMessage],
        mut on_delta: F,
    ) -> Result<LlmResult, String>
    where
        F: FnMut(&str) + Send,
    {
        let base = normalize_base_url(endpoint);
        let res = self.post_stream_chat(&base, api_key, model, messages, None, REQUEST_TIMEOUT).await?;

        let mut stream = res.bytes_stream();
        let mut buffer = Vec::<u8>::new();
        let mut full = String::new();
        let mut tokens = 0u64;

        while let Some(next) = stream.next().await {
            let chunk = next.map_err(|e| describe_error(None, &e.to_string(), &base, model))?;
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                if process_sse_line(&line, &mut full, &mut tokens, &mut on_delta)? {
                    return Ok(LlmResult { text: full, tokens });
                }
            }
        }
        if !buffer.is_empty() {
            let line = String::from_utf8_lossy(&buffer);
            let _ = process_sse_line(&line, &mut full, &mut tokens, &mut on_delta)?;
        }
        Ok(LlmResult { text: full, tokens })
    }

    pub async fn vision(
        &self,
        endpoint: &str,
        api_key: Option<&str>,
        model: &str,
        base64_jpeg: &str,
        prompt: &str,
        system: Option<&str>,
    ) -> Result<LlmResult, String> {
        let mut messages = Vec::new();
        if let Some(sys) = system.filter(|s| !s.trim().is_empty()) {
            messages.push(LlmMessage::system(sys));
        }
        messages.push(LlmMessage::user_image(prompt, base64_jpeg));
        self.chat_once(endpoint, api_key, model, &messages, None).await
    }

    pub async fn test_provider(&self, endpoint: &str, api_key: Option<&str>, model: &str) -> ProviderTestResult {
        let base = normalize_base_url(endpoint);
        let res = match self
            .post_stream_chat(&base, api_key, model, &[LlmMessage::user("Reply with OK.")], Some(16), TEST_TIMEOUT)
            .await
        {
            Ok(res) => res,
            Err(detail) => {
                return ProviderTestResult {
                    ok: false,
                    role: ProviderRole::Brain,
                    streaming: Some(false),
                    vision: None,
                    detail,
                };
            }
        };

        let mut stream = res.bytes_stream();
        let mut buf = Vec::new();
        let mut full = String::new();
        let mut tokens = 0u64;
        let mut err_msg = None;
        let mut saw_sse_data = false;
        'sse: while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                        let line = String::from_utf8_lossy(&line_bytes);
                        if line.trim_start().starts_with("data:") {
                            saw_sse_data = true;
                        }
                        match process_sse_line(&line, &mut full, &mut tokens, &mut |_| {}) {
                            Ok(true) => break 'sse,
                            Ok(false) => {}
                            Err(e) => {
                                err_msg = Some(e);
                                break 'sse;
                            }
                        }
                    }
                }
                Err(e) => {
                    err_msg = Some(describe_error(None, &e.to_string(), &base, model));
                    break;
                }
            }
        }
        if err_msg.is_none() && !buf.is_empty() {
            let line = String::from_utf8_lossy(&buf);
            if line.trim_start().starts_with("data:") {
                saw_sse_data = true;
            }
            if let Err(e) = process_sse_line(&line, &mut full, &mut tokens, &mut |_| {}) {
                err_msg = Some(e);
            }
        }
        if err_msg.is_none() && !saw_sse_data {
            err_msg = Some(format!("Provider at {} returned no SSE data chunks.", host_of(&base)));
        }
        if let Some(detail) = err_msg {
            return ProviderTestResult {
                ok: false,
                role: ProviderRole::Brain,
                streaming: Some(false),
                vision: None,
                detail,
            };
        }

        let vision_body = build_chat_body(model, &[LlmMessage::user_image("Reply OK.", TINY_JPEG_BASE64)], false, Some(16), false);
        let vision_ok = match self.post_chat(&base, api_key, &vision_body, TEST_TIMEOUT).await {
            Ok(res) => res.status().is_success(),
            Err(_) => false,
        };

        let detail = if vision_ok {
            "Streaming and image analysis are supported.".into()
        } else {
            "Streaming works. Image analysis is not supported by this model or endpoint.".into()
        };
        ProviderTestResult {
            ok: true,
            role: ProviderRole::Brain,
            streaming: Some(true),
            vision: Some(vision_ok),
            detail,
        }
    }

    async fn post_stream_chat(
        &self,
        base: &str,
        api_key: Option<&str>,
        model: &str,
        messages: &[LlmMessage],
        max_tokens: Option<u32>,
        timeout: Duration,
    ) -> Result<reqwest::Response, String> {
        let mut include_stream_options = true;
        loop {
            let body = build_chat_body(model, messages, true, max_tokens, include_stream_options);
            let res = self
                .post_chat(base, api_key, &body, timeout)
                .await
                .map_err(|e| describe_error(None, &e, base, model))?;
            let status = res.status().as_u16();
            if status < 400 {
                return Ok(res);
            }
            let raw = res.text().await.unwrap_or_default();
            if include_stream_options && status == 400 && raw.to_lowercase().contains("stream_options") {
                include_stream_options = false;
                continue;
            }
            return Err(describe_error(Some(status), &raw, base, model));
        }
    }

    async fn post_chat(&self, base: &str, api_key: Option<&str>, body: &Value, timeout: Duration) -> Result<reqwest::Response, String> {
        let url = format!("{base}/chat/completions");
        let mut req = self.http.post(&url).timeout(timeout).json(body);
        if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
            req = req.bearer_auth(key.trim());
        } else {
            req = req.bearer_auth("none");
        }
        req.send().await.map_err(|e| e.to_string())
    }
}

fn build_chat_body(model: &str, messages: &[LlmMessage], stream: bool, max_tokens: Option<u32>, include_stream_options: bool) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("model".into(), Value::String(model.to_string()));
    obj.insert("messages".into(), Value::Array(messages.iter().map(LlmMessage::to_json).collect()));
    if let Some(effort) = reasoning_effort_for(model) {
        obj.insert("reasoning_effort".into(), Value::String(effort.into()));
    }
    if stream {
        obj.insert("stream".into(), Value::Bool(true));
        if include_stream_options {
            obj.insert("stream_options".into(), json!({ "include_usage": true }));
        }
    }
    if let Some(limit) = max_tokens {
        obj.insert("max_tokens".into(), json!(limit));
    }
    Value::Object(obj)
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    usage: Option<StreamUsage>,
    error: Option<Value>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamUsage {
    total_tokens: Option<u64>,
}

fn process_sse_line<F>(line: &str, full: &mut String, tokens: &mut u64, on_delta: &mut F) -> Result<bool, String>
where
    F: FnMut(&str),
{
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Ok(false);
    }
    let Some(data) = trimmed.strip_prefix("data:").map(str::trim) else {
        return Ok(false);
    };
    if data == "[DONE]" {
        return Ok(true);
    }
    let chunk: StreamChunk = serde_json::from_str(data).map_err(|e| format!("Malformed stream chunk: {e}"))?;
    if let Some(err) = chunk.error {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or("Provider stream error");
        return Err(msg.to_string());
    }
    if let Some(u) = chunk.usage.and_then(|u| u.total_tokens) {
        *tokens = u;
    }
    if let Some(delta) = chunk.choices.into_iter().next().and_then(|c| c.delta).and_then(|d| d.content)
        && !delta.is_empty()
    {
        full.push_str(&delta);
        on_delta(&delta);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn normalizes_base_urls_and_reasoning_effort() {
        assert_eq!(normalize_base_url(" https://api.openai.com/v1/ "), "https://api.openai.com/v1");
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1/chat/completions"),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(reasoning_effort_for("gpt-5.5"), Some("low"));
        assert_eq!(reasoning_effort_for("openai/o4-mini"), Some("low"));
        assert_eq!(reasoning_effort_for("gemini-2.5-flash"), None);
    }

    #[test]
    fn describes_errors_without_leaking_keys() {
        let msg = describe_error(Some(401), "bad key", "https://api.openai.com/v1", "gpt-4o");
        assert!(msg.contains("api.openai.com") && msg.contains("API key"));
        let not_found = describe_error(Some(404), "", "https://example.com/v1", "missing-model");
        assert!(not_found.contains("missing-model"));
    }

    async fn spawn_mock_http<F>(handler: F) -> String
    where
        F: Fn(String, String) -> (u16, &'static str, Vec<Vec<u8>>) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handler = std::sync::Arc::new(handler);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let handler = handler.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let header_end;
                    loop {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = pos + 4;
                            break;
                        }
                    }
                    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let content_len = headers
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok()).flatten()
                        })
                        .unwrap_or(0);
                    while buf.len() < header_end + content_len {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let body = String::from_utf8_lossy(&buf[header_end..header_end + content_len]).to_string();
                    let (status, ctype, chunks) = handler(headers, body);
                    let total: usize = chunks.iter().map(Vec::len).sum();
                    let head = if ctype == "text/event-stream" {
                        format!("HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nConnection: close\r\n\r\n")
                    } else {
                        format!("HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n")
                    };
                    let _ = sock.write_all(head.as_bytes()).await;
                    for chunk in chunks {
                        let _ = sock.write_all(&chunk).await;
                        let _ = sock.flush().await;
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                });
            }
        });
        format!("http://{addr}/v1")
    }

    #[tokio::test]
    async fn lists_models_and_streams_sse_across_chunks() {
        let base = spawn_mock_http(|headers, body| {
            let line = headers.lines().next().unwrap_or("");
            if line.starts_with("GET /v1/models") {
                return (200, "application/json", vec![br#"{"data":[{"id":"z"},{"id":"a"}]}"#.to_vec()]);
            }
            let parsed: Value = serde_json::from_str(&body).unwrap();
            if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
                let first = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n".to_vec();
                let split_a = b"data: {\"choices\":[{\"delta\":{\"con".to_vec();
                let split_b = b"tent\":\"lo\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"total_tokens\":9}}\n\ndata: [DONE]\n\n".to_vec();
                return (200, "text/event-stream", vec![first, split_a, split_b]);
            }
            (200, "application/json", vec![br#"{"choices":[{"message":{"content":"OK"}}],"usage":{"total_tokens":4}}"#.to_vec()])
        })
        .await;

        let client = LlmClient::new();
        let models = client.list_models(&base, Some("sk-test")).await.unwrap();
        assert_eq!(models, vec!["a", "z"]);

        let mut deltas = Vec::new();
        let streamed = client
            .chat_stream(&base, Some("sk-test"), "gpt-5.5", &[LlmMessage::user("hi")], |d| deltas.push(d.to_string()))
            .await
            .unwrap();
        assert_eq!(deltas, vec!["Hel", "lo"]);
        assert_eq!(streamed, LlmResult { text: "Hello".into(), tokens: 9 });
    }

    #[tokio::test]
    async fn test_provider_reports_streaming_ok_and_vision_unsupported() {
        let base = spawn_mock_http(|_headers, body| {
            let parsed: Value = serde_json::from_str(&body).unwrap();
            let has_image = parsed["messages"][0]["content"].is_array();
            if has_image {
                return (400, "application/json", vec![br#"{"error":{"message":"image_url unsupported"}}"#.to_vec()]);
            }
            (
                200,
                "text/event-stream",
                vec![b"data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\ndata: [DONE]\n\n".to_vec()],
            )
        })
        .await;

        let res = LlmClient::new().test_provider(&base, Some("sk"), "text-only").await;
        assert!(res.ok);
        assert_eq!(res.streaming, Some(true));
        assert_eq!(res.vision, Some(false));
        assert!(res.detail.contains("not supported"));
    }

    #[tokio::test]
    async fn retries_without_stream_options_on_400_and_reports_errors() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_srv = calls.clone();
        let base = spawn_mock_http(move |_headers, body| {
            let parsed: Value = serde_json::from_str(&body).unwrap();
            let model = parsed["model"].as_str().unwrap_or("");
            match model {
                "no-stream-opts" => {
                    calls_srv.fetch_add(1, Ordering::SeqCst);
                    if parsed.get("stream_options").is_some() {
                        return (400, "application/json", vec![br#"{"error":{"message":"unknown parameter stream_options"}}"#.to_vec()]);
                    }
                    (200, "text/event-stream", vec![b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n".to_vec()])
                }
                "empty-sse" => (200, "text/event-stream", vec![b": keepalive\n\n".to_vec()]),
                "bad-chunk" => (200, "text/event-stream", vec![b"data: {not-json}\n\n".to_vec()]),
                "unauth" => (401, "application/json", vec![br#"{"error":{"message":"invalid key"}}"#.to_vec()]),
                _ => (500, "application/json", vec![br#"{"error":{"message":"boom"}}"#.to_vec()]),
            }
        })
        .await;

        let client = LlmClient::new();
        let res = client
            .chat_stream(&base, None, "no-stream-opts", &[LlmMessage::user("hi")], |_| {})
            .await
            .unwrap();
        assert_eq!(res.text, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let empty = client.test_provider(&base, None, "empty-sse").await;
        assert!(!empty.ok);
        assert!(empty.detail.contains("no SSE"));

        let bad_sse = client
            .chat_stream(&base, None, "bad-chunk", &[LlmMessage::user("hi")], |_| {})
            .await
            .unwrap_err();
        assert!(bad_sse.contains("Malformed"));

        let unauth = client
            .chat_stream(&base, Some("sk-secret"), "unauth", &[LlmMessage::user("hi")], |_| {})
            .await
            .unwrap_err();
        assert!(unauth.contains("API key") && !unauth.contains("sk-secret"));

        let err500 = client
            .chat_stream(&base, None, "fail", &[LlmMessage::user("hi")], |_| {})
            .await
            .unwrap_err();
        assert!(err500.contains("boom"));
    }
}
