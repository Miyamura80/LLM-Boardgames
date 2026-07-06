//! Provider-agnostic LLM chat client.
//!
//! One OpenAI-compatible chat-completions client covers every configured
//! provider; the `provider/model` prefix in a model string routes to a base URL
//! and API key (litellm-style naming, e.g. `gemini/gemini-3-flash-preview`).
//! Anthropic, Gemini, Groq and OpenRouter all expose OpenAI-compatible
//! endpoints, so the wire format is shared.

mod client;
mod providers;

pub use client::{ChatClient, ChatMessage, ChatOutcome, LlmError, TokenUsage};
pub use providers::{resolve_provider, ProviderKeys, ResolvedProvider};

/// Retry policy for transient API failures (HTTP 429/5xx, network errors).
/// Distinct from the illegal-move rethink loop, which lives in the game loop.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub min_wait_ms: u64,
    pub max_wait_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            min_wait_ms: 1_000,
            max_wait_ms: 5_000,
        }
    }
}

impl RetryPolicy {
    /// Build from the repo's `llm_config.retry` section.
    pub fn from_app_config(cfg: &app_config::AppConfig) -> Self {
        Self {
            max_attempts: cfg.llm_config.retry.max_attempts.max(1) as u32,
            min_wait_ms: cfg.llm_config.retry.min_wait_seconds.max(0) as u64 * 1_000,
            max_wait_ms: cfg.llm_config.retry.max_wait_seconds.max(1) as u64 * 1_000,
        }
    }
}
