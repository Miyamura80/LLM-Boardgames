//! `provider/model` prefix routing: maps a litellm-style model string onto an
//! OpenAI-compatible base URL and the matching API key.

use serde::{Deserialize, Serialize};

/// API keys per provider, resolved by the command layer (from `app_config` /
/// environment) and passed down so the engine core stays config-agnostic.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderKeys {
    pub openai: Option<String>,
    pub anthropic: Option<String>,
    pub groq: Option<String>,
    pub gemini: Option<String>,
    pub openrouter: Option<String>,
}

impl ProviderKeys {
    /// Pull keys from the app config, falling back to conventional env vars.
    pub fn from_app_config(cfg: &app_config::AppConfig) -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Self {
            openai: cfg
                .openai_api_key()
                .map(String::from)
                .or_else(|| env("OPENAI_API_KEY")),
            anthropic: cfg
                .anthropic_api_key()
                .map(String::from)
                .or_else(|| env("ANTHROPIC_API_KEY")),
            groq: cfg
                .groq_api_key()
                .map(String::from)
                .or_else(|| env("GROQ_API_KEY")),
            gemini: cfg
                .gemini_api_key()
                .map(String::from)
                .or_else(|| env("GEMINI_API_KEY")),
            openrouter: env("OPENROUTER_API_KEY"),
        }
    }
}

/// A model string resolved to a concrete endpoint.
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub provider: &'static str,
    /// Model id as the provider expects it (prefix stripped).
    pub model: String,
    /// OpenAI-compatible chat-completions base, e.g. `…/v1`.
    pub base_url: String,
    pub api_key: String,
}

/// Resolve `provider/model` (e.g. `gemini/gemini-3-flash-preview`). A bare
/// model name with no `/` defaults to the `openai` provider.
pub fn resolve_provider(model: &str, keys: &ProviderKeys) -> Result<ResolvedProvider, String> {
    let (prefix, name) = model.split_once('/').unwrap_or(("openai", model));
    if name.trim().is_empty() {
        return Err(format!("model string '{model}' has an empty model name"));
    }
    let (provider, base_url, key): (&'static str, &str, &Option<String>) = match prefix {
        "openai" => ("openai", "https://api.openai.com/v1", &keys.openai),
        "anthropic" => ("anthropic", "https://api.anthropic.com/v1", &keys.anthropic),
        "groq" => ("groq", "https://api.groq.com/openai/v1", &keys.groq),
        "gemini" => (
            "gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            &keys.gemini,
        ),
        "openrouter" => (
            "openrouter",
            "https://openrouter.ai/api/v1",
            &keys.openrouter,
        ),
        other => return Err(format!("unknown LLM provider prefix: {other}")),
    };
    let api_key = key
        .clone()
        .ok_or_else(|| format!("no API key configured for provider '{provider}'"))?;
    Ok(ResolvedProvider {
        provider,
        model: name.to_string(),
        base_url: base_url.to_string(),
        api_key,
    })
}
