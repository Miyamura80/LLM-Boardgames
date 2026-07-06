//! The LLM integration: provider-routed OpenAI-compatible client, the prompt
//! scaffold, and the LLM-backed agent. Kept behind the [`crate::eval::Agent`]
//! trait so the rest of the eval layer is provider-agnostic.

pub mod agent;
pub mod client;
pub mod prompt;

pub use agent::LlmAgent;
pub use client::{route, LlmClient, LlmError, ModelEndpoint, ProviderKeys, RetryConfig};
