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
    /// First-party providers (OpenAI / Anthropic / Gemini) route natively;
    /// every other family goes through OpenRouter (`OPENROUTER_API_KEY`).
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
        // Everything that isn't a first-party provider above routes through
        // OpenRouter (one key reaches Mistral / DeepSeek / xAI / GLM / MiniMax /
        // etc.), so those seats are written `openrouter/<org>/<model>`.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn all_keys() -> ProviderKeys {
        ProviderKeys {
            openai: Some("k".into()),
            anthropic: Some("k".into()),
            groq: Some("k".into()),
            gemini: Some("k".into()),
            openrouter: Some("k".into()),
        }
    }

    #[test]
    fn routes_every_known_prefix_and_strips_it() {
        let keys = all_keys();
        let cases = [
            (
                "openai/gpt-4.1-nano",
                "https://api.openai.com/v1",
                "gpt-4.1-nano",
            ),
            (
                "anthropic/claude-haiku-4-5",
                "https://api.anthropic.com/v1",
                "claude-haiku-4-5",
            ),
            // Non-first-party families route through OpenRouter (org/model kept).
            (
                "openrouter/minimax/minimax-m3",
                "https://openrouter.ai/api/v1",
                "minimax/minimax-m3",
            ),
            (
                "openrouter/x-ai/grok-4.3",
                "https://openrouter.ai/api/v1",
                "x-ai/grok-4.3",
            ),
        ];
        for (input, base, model) in cases {
            let r = resolve_provider(input, &keys).unwrap();
            assert_eq!(r.base_url, base, "base url for {input}");
            assert_eq!(r.model, model, "stripped model for {input}");
        }
    }

    #[test]
    fn bare_model_defaults_to_openai() {
        let r = resolve_provider("gpt-4.1-mini", &all_keys()).unwrap();
        assert_eq!(r.provider, "openai");
        assert_eq!(r.model, "gpt-4.1-mini");
    }

    #[test]
    fn missing_key_is_an_error_not_a_panic() {
        let keys = ProviderKeys::default(); // no keys configured
        let err = resolve_provider("openrouter/minimax/minimax-m3", &keys).unwrap_err();
        assert!(
            err.contains("openrouter"),
            "error names the provider: {err}"
        );
    }

    #[test]
    fn unknown_prefix_rejected() {
        // A one-segment model with an unknown-looking prefix is treated as a
        // bare openai model; a genuine unknown provider prefix must error.
        let err = resolve_provider("cohere/command-r", &all_keys()).unwrap_err();
        assert!(err.contains("unknown LLM provider prefix"), "{err}");
    }
}
