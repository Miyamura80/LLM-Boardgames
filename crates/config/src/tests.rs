//! Unit tests for config loading, env-override precedence, type coercion,
//! and the `FrontendConfig` sanitization security boundary. Split out of
//! `lib.rs` to keep that file under the file-length cap; `use super::*` still
//! reaches the crate-private loader/reset helpers.

use super::*;
use serial_test::serial;
use std::env;
struct EnvGuard(&'static str);
impl EnvGuard {
    fn new(key: &'static str, val: &str) -> Self {
        reset_config();
        env::set_var(key, val);
        Self(key)
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        env::remove_var(self.0);
        reset_config();
    }
}

#[test]
#[serial]
fn test_load_config() {
    // Ensure the config loads without error
    let config = load_config();
    assert!(config.is_ok(), "Failed to load config: {:?}", config.err());

    let config = config.unwrap();
    // Verify some default values from global_config.yaml
    assert_eq!(
        config.default_llm.default_model,
        "gemini/gemini-3-flash-preview"
    );
    assert_eq!(config.llm_config.retry.max_attempts, 3);
}

#[test]
#[serial]
fn test_env_var_override_precedence() {
    // YAML value is "gemini/gemini-3-flash-preview"
    let _guard = EnvGuard::new("APP__MODEL_NAME", "override-model");

    let config = load_config().expect("Should load config");
    assert_eq!(config.model_name, "override-model");
}

#[test]
#[serial]
fn test_type_coercion_boolean() {
    {
        let _guard = EnvGuard::new("APP__LLM_CONFIG__CACHE_ENABLED", "true");
        let config = load_config().expect("Should load config");
        assert!(config.llm_config.cache_enabled);
    }

    {
        let _guard = EnvGuard::new("APP__LLM_CONFIG__CACHE_ENABLED", "false");
        let config = load_config().expect("Should load config");
        assert!(!config.llm_config.cache_enabled);
    }

    // Test boolean coercion from '1' and '0' (porting from Python tests)
    {
        let _guard = EnvGuard::new("APP__LOGGING__FORMAT__LOCATION__ENABLED", "1");
        let config = load_config().expect("Should load config");
        assert!(config.logging.format.location.enabled);
    }

    {
        let _guard = EnvGuard::new("APP__LOGGING__FORMAT__LOCATION__ENABLED", "0");
        let config = load_config().expect("Should load config");
        assert!(!config.logging.format.location.enabled);
    }
}

#[test]
#[serial]
fn test_type_coercion_numeric() {
    let _guard1 = EnvGuard::new("APP__DEFAULT_LLM__DEFAULT_TEMPERATURE", "0.95");
    let _guard2 = EnvGuard::new("APP__LLM_CONFIG__RETRY__MAX_ATTEMPTS", "10");

    let config = load_config().expect("Should load config");
    assert_eq!(config.default_llm.default_temperature, 0.95);
    assert_eq!(config.llm_config.retry.max_attempts, 10);

    // Test float-to-int coercion if applicable (usually handled by serde)
    // Here we test string-to-numeric coercion specifically.
    let _guard3 = EnvGuard::new("APP__LLM_CONFIG__RETRY__MAX_ATTEMPTS", "5");
    let config = load_config().expect("Should load config");
    assert_eq!(config.llm_config.retry.max_attempts, 5);
}

#[test]
#[serial]
fn test_dev_env_override() {
    // Field name is lowercase `dev_env`
    let _guard = EnvGuard::new("APP__DEV_ENV", "production");
    let config = load_config().expect("Should load config");
    assert_eq!(config.dev_env, "production");
}

#[test]
#[serial]
fn test_api_key_loading() {
    // Test that API keys are loaded from environment variables (APP__ prefix)
    let _guard1 = EnvGuard::new("APP__OPENAI_API_KEY", "test-openai-key");
    let _guard2 = EnvGuard::new("APP__ANTHROPIC_API_KEY", "test-anthropic-key");

    let config = load_config().expect("Should load config");
    assert_eq!(config.openai_api_key(), Some("test-openai-key"));
    assert_eq!(config.anthropic_api_key(), Some("test-anthropic-key"));
}

#[test]
#[serial]
fn test_frontend_config_sanitization() {
    let config = AppConfig {
        model_name: "gpt-4".to_string(),
        dot_global_config_health_check: true,
        dev_env: "dev".to_string(),
        example_parent: ExampleParent {
            example_child: "val".to_string(),
        },
        default_llm: DefaultLlm {
            default_model: "gpt-4".to_string(),
            fallback_model: None,
            default_temperature: 0.7,
            default_max_tokens: 100,
        },
        llm_config: LlmConfig {
            cache_enabled: true,
            retry: RetryConfig {
                max_attempts: 1,
                min_wait_seconds: 1,
                max_wait_seconds: 1,
            },
        },
        logging: LoggingConfig {
            verbose: true,
            format: LoggingFormatConfig {
                show_time: true,
                show_session_id: true,
                location: LoggingLocationConfig {
                    enabled: true,
                    show_file: true,
                    show_function: true,
                    show_line: true,
                    show_for_info: true,
                    show_for_debug: true,
                    show_for_warning: true,
                    show_for_error: true,
                },
            },
            levels: LoggingLevelsConfig {
                debug: true,
                info: true,
                warning: true,
                error: true,
                critical: true,
            },
            redaction: RedactionConfig::default(),
        },
        server: ServerConfig::default(),
        features: HashMap::new(),
        openai_api_key: Some("secret-key".to_string()),
        anthropic_api_key: None,
        groq_api_key: None,
        perplexity_api_key: None,
        gemini_api_key: None,
    };

    // The sanitized projection must not leak the secret value or its key.
    let frontend_config = FrontendConfig::from(&config);
    let json = serde_json::to_string(&frontend_config).unwrap();

    assert!(!json.contains("secret-key"));
    assert!(!json.contains("openai_api_key"));

    // Security boundary: even serializing the *full* AppConfig (e.g. by
    // accident in a log line or debug endpoint) must never emit a secret.
    let mut config = config;
    config.openai_api_key = Some("secret-openai".into());
    config.anthropic_api_key = Some("secret-anthropic".into());
    config.groq_api_key = Some("secret-groq".into());
    config.perplexity_api_key = Some("secret-perplexity".into());
    config.gemini_api_key = Some("secret-gemini".into());

    let full_json = serde_json::to_string(&config).unwrap();
    for leaked in [
        "secret-openai",
        "secret-anthropic",
        "secret-groq",
        "secret-perplexity",
        "secret-gemini",
        "api_key",
    ] {
        assert!(
            !full_json.contains(leaked),
            "AppConfig serialization leaked `{leaked}`: {full_json}"
        );
    }
}

#[test]
#[serial]
fn test_logging_verbose_default_is_false() {
    let config = load_config().expect("Should load config");
    assert!(
        !config.logging.verbose,
        "Logging verbose should be false by default"
    );
}
