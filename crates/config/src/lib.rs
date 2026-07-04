//! Application configuration, shared across every transport (CLI, HTTP API,
//! and future MCP).
//!
//! [`AppConfig`] is the full config including secret credentials;
//! [`FrontendConfig`] is the sanitized projection safe to expose over HTTP. The
//! sanitizer is a **security boundary** — no secret field may ever cross it
//! (enforced by `#[serde(skip_serializing)]` and covered by tests here). Config
//! loads from `global_config.yaml` (next to this crate, or `APP_CONFIG_PATH`),
//! layered with optional `production_config.yaml` / `.global_config.yaml`, then
//! overridden by `APP__`-prefixed env vars.

use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppConfig {
    pub model_name: String,
    pub dot_global_config_health_check: bool,
    #[serde(default = "default_dev_env")]
    pub dev_env: String,

    pub example_parent: ExampleParent,
    pub default_llm: DefaultLlm,
    pub llm_config: LlmConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub features: HashMap<String, bool>,

    // Secret credentials — never serialized (`skip_serializing` = the security
    // boundary; see the sanitization test). Read via the accessors below.
    #[serde(skip_serializing)]
    pub openai_api_key: Option<String>,
    #[serde(skip_serializing)]
    pub anthropic_api_key: Option<String>,
    #[serde(skip_serializing)]
    pub groq_api_key: Option<String>,
    #[serde(skip_serializing)]
    pub perplexity_api_key: Option<String>,
    #[serde(skip_serializing)]
    pub gemini_api_key: Option<String>,
}

impl AppConfig {
    pub fn openai_api_key(&self) -> Option<&str> {
        self.openai_api_key.as_deref()
    }
    pub fn anthropic_api_key(&self) -> Option<&str> {
        self.anthropic_api_key.as_deref()
    }
    pub fn groq_api_key(&self) -> Option<&str> {
        self.groq_api_key.as_deref()
    }
    pub fn perplexity_api_key(&self) -> Option<&str> {
        self.perplexity_api_key.as_deref()
    }
    pub fn gemini_api_key(&self) -> Option<&str> {
        self.gemini_api_key.as_deref()
    }
}

/// A sanitized version of the configuration intended for exposure to the frontend.
/// This strictly excludes sensitive information like API keys.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FrontendConfig {
    pub model_name: String,
    pub dot_global_config_health_check: bool,
    pub dev_env: String,
    pub example_parent: ExampleParent,
    pub default_llm: DefaultLlm,
    pub llm_config: LlmConfig,
    pub features: HashMap<String, bool>,
}

impl From<&AppConfig> for FrontendConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            model_name: config.model_name.clone(),
            dot_global_config_health_check: config.dot_global_config_health_check,
            dev_env: config.dev_env.clone(),
            example_parent: config.example_parent.clone(),
            default_llm: config.default_llm.clone(),
            llm_config: config.llm_config.clone(),
            features: config.features.clone(),
        }
    }
}

fn default_dev_env() -> String {
    "dev".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExampleParent {
    pub example_child: String,
}

/// HTTP server settings for `appctl serve` (override via `APP__SERVER__*`).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// HTTP request timeout in seconds (`None` → 30s).
    #[serde(default)]
    pub request_timeout_secs: Option<u64>,
    /// Allowed CORS origins; empty = permissive dev default, else locks the API.
    #[serde(default)]
    pub cors_allow_origins: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            request_timeout_secs: None,
            cors_allow_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DefaultLlm {
    pub default_model: String,
    pub fallback_model: Option<String>,
    pub default_temperature: f32,
    pub default_max_tokens: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LlmConfig {
    pub cache_enabled: bool,
    pub retry: RetryConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RetryConfig {
    pub max_attempts: i32,
    pub min_wait_seconds: i32,
    pub max_wait_seconds: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingConfig {
    pub verbose: bool,
    pub format: LoggingFormatConfig,
    pub levels: LoggingLevelsConfig,
    #[serde(default)]
    pub redaction: RedactionConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingFormatConfig {
    pub show_time: bool,
    pub show_session_id: bool,
    pub location: LoggingLocationConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingLocationConfig {
    pub enabled: bool,
    pub show_file: bool,
    pub show_function: bool,
    pub show_line: bool,
    pub show_for_info: bool,
    pub show_for_debug: bool,
    pub show_for_warning: bool,
    pub show_for_error: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingLevelsConfig {
    pub debug: bool,
    pub info: bool,
    pub warning: bool,
    pub error: bool,
    pub critical: bool,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct RedactionConfig {
    #[serde(default = "true_default")]
    pub enabled: bool,
    #[serde(default = "true_default")]
    pub use_default_pii: bool,
    #[serde(default)]
    pub patterns: Vec<RedactionPattern>,
}

fn true_default() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RedactionPattern {
    pub name: String,
    pub regex: String,
    pub placeholder: String,
}

#[derive(Clone, Copy)]
struct ConfigStorage {
    app: &'static AppConfig,
    frontend: &'static FrontendConfig,
}

static CONFIG_STORAGE: RwLock<Option<ConfigStorage>> = RwLock::new(None);

fn try_get_config_storage() -> Result<ConfigStorage, ConfigError> {
    if let Some(storage) = *CONFIG_STORAGE.read().unwrap() {
        return Ok(storage);
    }

    let mut write = CONFIG_STORAGE.write().unwrap();
    if let Some(storage) = *write {
        return Ok(storage);
    }

    let app_config = load_config()?;
    let frontend_config = FrontendConfig::from(&app_config);

    let app_cfg = Box::leak(Box::new(app_config));
    let frontend_cfg = Box::leak(Box::new(frontend_config));

    let storage = ConfigStorage {
        app: app_cfg,
        frontend: frontend_cfg,
    };
    *write = Some(storage);
    Ok(storage)
}

fn get_config_storage() -> ConfigStorage {
    try_get_config_storage().unwrap_or_else(|e| {
        panic!(
            "Failed to load configuration: {e}\n\
             Ensure `global_config.yaml` exists next to the `app-config` crate, \
             or set `APP_CONFIG_PATH` to point at your config file."
        )
    })
}

/// Fallible config accessor for callers that want to handle a missing/malformed
/// config file gracefully (e.g. print a friendly error and exit) instead of
/// panicking like [`get_config`]. Initialization is shared: the first successful
/// call caches the config for both accessors.
pub fn try_get_config() -> Result<&'static AppConfig, ConfigError> {
    Ok(try_get_config_storage()?.app)
}

pub fn get_config() -> &'static AppConfig {
    get_config_storage().app
}

pub fn get_frontend_config() -> &'static FrontendConfig {
    get_config_storage().frontend
}

#[cfg(test)]
pub fn reset_config() {
    let mut write = CONFIG_STORAGE.write().unwrap();
    *write = None;
}

/// Directory config files resolve against: this crate's dir
/// (`CARGO_MANIFEST_DIR`) for `cargo run`/`test`; a deployed binary should set
/// `APP_CONFIG_PATH` instead (sibling overrides resolve next to that file).
fn config_base_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_config() -> Result<AppConfig, ConfigError> {
    // `APP_CONFIG_PATH` (full path to the primary YAML) overrides the default
    // location; production/local overrides are resolved next to it.
    let config_path = std::env::var("APP_CONFIG_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config_base_dir().join("global_config.yaml"));

    let config_dir = config_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let prod_config_path = config_dir.join("production_config.yaml");
    let local_config_path = config_dir.join(".global_config.yaml");

    let builder = Config::builder()
        // Load default config (mandatory)
        .add_source(File::from(config_path).required(true))
        // Load production config if in prod
        .add_source(File::from(prod_config_path).required(false))
        // Load local override
        .add_source(File::from(local_config_path).required(false))
        // Load environment variables
        // Map nested env vars like APP__LOGGING__VERBOSE=true
        .add_source(Environment::with_prefix("APP").separator("__"));

    builder.build()?.try_deserialize()
}

#[cfg(test)]
mod tests;
