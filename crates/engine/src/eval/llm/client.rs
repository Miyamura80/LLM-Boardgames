//! A minimal provider-agnostic LLM client: **prefix routing** over an
//! OpenAI-compatible `chat/completions` surface.
//!
//! A model spec is `"<provider>/<model>"` (e.g. `"gemini/gemini-3-flash-preview"`).
//! The provider prefix selects a base URL and an API key; the request body is
//! the OpenAI chat schema, which Gemini, Groq, OpenAI, Anthropic, and Perplexity
//! all accept via their compat endpoints. Transient failures are retried with
//! exponential backoff; 4xx (except 429) fail fast.

use crate::eval::record::Usage;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// API keys per provider (filled from `app_config` by the CLI so the engine
/// stays free of the config crate).
#[derive(Debug, Clone, Default)]
pub struct ProviderKeys {
    pub openai: Option<String>,
    pub anthropic: Option<String>,
    pub gemini: Option<String>,
    pub groq: Option<String>,
    pub perplexity: Option<String>,
}

/// A resolved endpoint: where to POST and with which key/model.
#[derive(Debug, Clone)]
pub struct ModelEndpoint {
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("unknown or unkeyed provider: {0}")]
    Config(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("could not parse completion: {0}")]
    Parse(String),
}

/// Map a `provider/model` spec to a base URL + key.
pub fn route(spec: &str, keys: &ProviderKeys) -> Result<ModelEndpoint, LlmError> {
    let (provider, model) = spec
        .split_once('/')
        .ok_or_else(|| LlmError::Config(format!("missing provider prefix in '{spec}'")))?;

    let (base_url, key) = match provider {
        "openai" => ("https://api.openai.com/v1", keys.openai.clone()),
        "gemini" | "google" => (
            "https://generativelanguage.googleapis.com/v1beta/openai",
            keys.gemini.clone(),
        ),
        "groq" => ("https://api.groq.com/openai/v1", keys.groq.clone()),
        "anthropic" | "claude" => ("https://api.anthropic.com/v1", keys.anthropic.clone()),
        "perplexity" => ("https://api.perplexity.ai", keys.perplexity.clone()),
        other => return Err(LlmError::Config(format!("unknown provider '{other}'"))),
    };

    let api_key =
        key.ok_or_else(|| LlmError::Config(format!("no API key for provider '{provider}'")))?;
    Ok(ModelEndpoint {
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        api_key,
        model: model.to_string(),
    })
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub min_wait_ms: u64,
    pub max_wait_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            min_wait_ms: 500,
            max_wait_ms: 5000,
        }
    }
}

/// One chat message.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
}

/// A parsed completion.
#[derive(Debug, Clone)]
pub struct ChatCompletion {
    pub text: String,
    pub usage: Usage,
}

pub struct LlmClient {
    http: reqwest::Client,
    retry: RetryConfig,
}

impl LlmClient {
    pub fn new(retry: RetryConfig) -> Result<Self, LlmError> {
        Ok(Self {
            http: build_http()?,
            retry,
        })
    }

    /// A single chat completion. `json_mode` requests a JSON object response;
    /// `reasoning_effort` (e.g. "low") caps thinking on reasoning models so a
    /// short structured reply fits the token budget.
    pub async fn chat(
        &self,
        endpoint: &ModelEndpoint,
        messages: &[ChatMessage],
        temperature: f32,
        max_tokens: u32,
        json_mode: bool,
        reasoning_effort: Option<&str>,
    ) -> Result<ChatCompletion, LlmError> {
        let url = format!("{}/chat/completions", endpoint.base_url);
        let mut body = serde_json::json!({
            "model": endpoint.model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
        });
        if json_mode {
            body["response_format"] = serde_json::json!({ "type": "json_object" });
        }
        if let Some(effort) = reasoning_effort {
            body["reasoning_effort"] = serde_json::json!(effort);
        }

        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self
                .http
                .post(&url)
                .bearer_auth(&endpoint.api_key)
                .json(&body)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let raw = resp
                            .text()
                            .await
                            .map_err(|e| LlmError::Http(e.to_string()))?;
                        return parse_completion(&raw);
                    }
                    let code = status.as_u16();
                    let retriable = code == 429 || (500..600).contains(&code);
                    let text = resp.text().await.unwrap_or_default();
                    if retriable && attempt < self.retry.max_attempts {
                        self.backoff(attempt).await;
                        continue;
                    }
                    return Err(LlmError::Api {
                        status: code,
                        body: truncate(&text, 400),
                    });
                }
                Err(e) => {
                    if attempt < self.retry.max_attempts {
                        self.backoff(attempt).await;
                        continue;
                    }
                    return Err(LlmError::Http(e.to_string()));
                }
            }
        }
    }

    async fn backoff(&self, attempt: u32) {
        let wait = (self.retry.min_wait_ms * 2u64.pow(attempt.saturating_sub(1)))
            .min(self.retry.max_wait_ms);
        tokio::time::sleep(Duration::from_millis(wait)).await;
    }
}

/// Build the HTTP client, trusting the agent proxy's CA bundle when present so
/// outbound HTTPS through the sandbox proxy verifies correctly.
fn build_http() -> Result<reqwest::Client, LlmError> {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(120));
    let ca_path =
        std::env::var("SSL_CERT_FILE").unwrap_or_else(|_| "/root/.ccr/ca-bundle.crt".into());
    if let Ok(pem) = std::fs::read(&ca_path) {
        if let Ok(certs) = reqwest::Certificate::from_pem_bundle(&pem) {
            for cert in certs {
                builder = builder.add_root_certificate(cert);
            }
        }
    }
    builder.build().map_err(|e| LlmError::Http(e.to_string()))
}

// ---- OpenAI response parsing ---------------------------------------------

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<String>,
}
#[derive(Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

fn parse_completion(raw: &str) -> Result<ChatCompletion, LlmError> {
    let parsed: OpenAiResponse = serde_json::from_str(raw)
        .map_err(|e| LlmError::Parse(format!("{e}: {}", truncate(raw, 300))))?;
    let text = parsed
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .ok_or_else(|| LlmError::Parse("no message content".into()))?;
    let usage = parsed
        .usage
        .map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            calls: 1,
        })
        .unwrap_or(Usage {
            calls: 1,
            ..Default::default()
        });
    Ok(ChatCompletion { text, usage })
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_known_providers() {
        let keys = ProviderKeys {
            gemini: Some("k".into()),
            ..Default::default()
        };
        let ep = route("gemini/gemini-3-flash-preview", &keys).unwrap();
        assert_eq!(ep.provider, "gemini");
        assert!(ep.base_url.contains("generativelanguage"));
        assert_eq!(ep.model, "gemini-3-flash-preview");
    }

    #[test]
    fn missing_prefix_is_config_error() {
        let keys = ProviderKeys::default();
        assert!(route("gpt-4o", &keys).is_err());
    }

    #[test]
    fn missing_key_is_config_error() {
        let keys = ProviderKeys::default();
        assert!(matches!(
            route("openai/gpt-4o", &keys),
            Err(LlmError::Config(_))
        ));
    }

    #[test]
    fn parses_openai_shaped_response() {
        let raw = r#"{"choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#;
        let c = parse_completion(raw).unwrap();
        assert_eq!(c.text, "hello");
        assert_eq!(c.usage.prompt_tokens, 3);
        assert_eq!(c.usage.calls, 1);
    }
}
