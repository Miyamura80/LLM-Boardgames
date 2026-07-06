//! OpenAI-compatible chat-completions client with transient-failure retry and
//! token/cost accounting.

use super::providers::{resolve_provider, ProviderKeys};
use super::RetryPolicy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("configuration: {0}")]
    Config(String),
    #[error("network: {0}")]
    Network(String),
    #[error("api status {status}: {body}")]
    Api { status: u16, body: String },
    #[error("empty completion")]
    EmptyCompletion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// One completed chat call.
#[derive(Debug, Clone)]
pub struct ChatOutcome {
    pub content: String,
    pub usage: TokenUsage,
}

/// Chat client bound to one `provider/model` string. Cheap to clone the
/// underlying reqwest client; accumulates total usage across calls.
pub struct ChatClient {
    /// Built on first use: constructing a reqwest client initializes TLS,
    /// which must not be a prerequisite for merely instantiating agents
    /// (offline construction, e.g. in unit tests, stays panic-free).
    http: std::sync::OnceLock<reqwest::Client>,
    model_string: String,
    keys: ProviderKeys,
    retry: RetryPolicy,
    total_prompt: AtomicU64,
    total_completion: AtomicU64,
}

impl ChatClient {
    pub fn new(model_string: impl Into<String>, keys: ProviderKeys, retry: RetryPolicy) -> Self {
        Self {
            http: std::sync::OnceLock::new(),
            model_string: model_string.into(),
            keys,
            retry,
            total_prompt: AtomicU64::new(0),
            total_completion: AtomicU64::new(0),
        }
    }

    fn http(&self) -> &reqwest::Client {
        self.http.get_or_init(reqwest::Client::new)
    }

    pub fn model_string(&self) -> &str {
        &self.model_string
    }

    /// Total usage across every call made through this client.
    pub fn total_usage(&self) -> TokenUsage {
        TokenUsage {
            prompt_tokens: self.total_prompt.load(Ordering::Relaxed),
            completion_tokens: self.total_completion.load(Ordering::Relaxed),
        }
    }

    /// One chat completion with transient-failure retry (429/5xx/network).
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatOutcome, LlmError> {
        let resolved =
            resolve_provider(&self.model_string, &self.keys).map_err(LlmError::Config)?;
        let url = format!("{}/chat/completions", resolved.base_url);
        let body = json!({
            "model": resolved.model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
        });

        let mut wait = self.retry.min_wait_ms;
        let mut last_err = LlmError::EmptyCompletion;
        for attempt in 0..self.retry.max_attempts {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(wait)).await;
                wait = (wait * 2).min(self.retry.max_wait_ms);
            }
            match self.try_chat(&url, &resolved.api_key, &body).await {
                Ok(outcome) => {
                    self.total_prompt
                        .fetch_add(outcome.usage.prompt_tokens, Ordering::Relaxed);
                    self.total_completion
                        .fetch_add(outcome.usage.completion_tokens, Ordering::Relaxed);
                    return Ok(outcome);
                }
                Err(e) => {
                    let retryable = matches!(
                        &e,
                        LlmError::Network(_)
                            | LlmError::Api {
                                status: 429 | 500..=599,
                                ..
                            }
                    );
                    if !retryable {
                        return Err(e);
                    }
                    tracing::warn!(attempt, error = %e, "transient LLM failure; retrying");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    async fn try_chat(
        &self,
        url: &str,
        api_key: &str,
        body: &serde_json::Value,
    ) -> Result<ChatOutcome, LlmError> {
        let resp = self
            .http()
            .post(url)
            .bearer_auth(api_key)
            .json(body)
            .timeout(Duration::from_secs(180))
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(LlmError::Api {
                status,
                body: text.chars().take(500).collect(),
            });
        }

        #[derive(Deserialize)]
        struct Resp {
            choices: Vec<Choice>,
            usage: Option<Usage>,
        }
        #[derive(Deserialize)]
        struct Choice {
            message: Msg,
        }
        #[derive(Deserialize)]
        struct Msg {
            content: Option<String>,
        }
        #[derive(Deserialize)]
        struct Usage {
            prompt_tokens: Option<u64>,
            completion_tokens: Option<u64>,
            /// Includes hidden reasoning tokens on thinking models — the
            /// number that actually drives cost.
            total_tokens: Option<u64>,
        }

        let parsed: Resp = serde_json::from_str(&text).map_err(|e| LlmError::Api {
            status,
            body: format!(
                "unparseable response ({e}): {}",
                &text.chars().take(300).collect::<String>()
            ),
        })?;
        let content = parsed
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .filter(|c| !c.trim().is_empty())
            .ok_or(LlmError::EmptyCompletion)?;
        let usage = parsed
            .usage
            .map(|u| {
                let prompt = u.prompt_tokens.unwrap_or(0);
                // Prefer total − prompt: on reasoning models the visible
                // completion_tokens excludes billed thinking tokens.
                let completion = u
                    .total_tokens
                    .map(|t| t.saturating_sub(prompt))
                    .filter(|&c| c > 0)
                    .or(u.completion_tokens)
                    .unwrap_or(0);
                TokenUsage {
                    prompt_tokens: prompt,
                    completion_tokens: completion,
                }
            })
            .unwrap_or_default();
        Ok(ChatOutcome { content, usage })
    }
}
