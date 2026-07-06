//! The LLM-backed [`Agent`]: renders the seat observation, calls the provider,
//! and parses a strict-JSON action. Malformed/illegal handling and the forced
//! default live in the runner; this type just surfaces a best-effort action or a
//! typed [`AgentError`] so the runner can rethink.

use crate::eval::agent::{Agent, AgentError, Beliefs};
use crate::eval::llm::client::{ChatMessage, LlmClient, LlmError, ModelEndpoint};
use crate::eval::llm::prompt;
use crate::eval::record::Usage;
use crate::game::{Action, Decision, Observation};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

pub struct LlmAgent {
    client: Arc<LlmClient>,
    endpoint: ModelEndpoint,
    temperature: f32,
    max_tokens: u32,
    usage: Mutex<Usage>,
}

impl LlmAgent {
    pub fn new(
        client: Arc<LlmClient>,
        endpoint: ModelEndpoint,
        temperature: f32,
        max_tokens: u32,
    ) -> Self {
        Self {
            client,
            endpoint,
            temperature,
            max_tokens,
            usage: Mutex::new(Usage::default()),
        }
    }

    /// One chat turn: `[system, user]` → assistant text. Accumulates usage even
    /// when the caller later fails to parse the text.
    async fn call(&self, system: String, user: String) -> Result<String, AgentError> {
        let messages = [ChatMessage::system(system), ChatMessage::user(user)];
        match self
            .client
            .chat(
                &self.endpoint,
                &messages,
                self.temperature,
                self.max_tokens,
                true,
            )
            .await
        {
            Ok(completion) => {
                self.usage.lock().unwrap().add(&completion.usage);
                Ok(completion.text)
            }
            Err(LlmError::Api { status, body }) => {
                Err(AgentError::Api(format!("{status}: {body}")))
            }
            Err(e) => Err(AgentError::Api(e.to_string())),
        }
    }
}

#[async_trait]
impl Agent for LlmAgent {
    fn name(&self) -> String {
        format!(
            "{}/{}#{}",
            self.endpoint.provider,
            self.endpoint.model,
            prompt::SCAFFOLD_VERSION
        )
    }

    async fn act(
        &self,
        obs: &Observation,
        decision: &Decision,
        feedback: Option<&str>,
    ) -> Result<Action, AgentError> {
        let user = format!(
            "{}\n\n{}",
            prompt::render_state(obs),
            prompt::decision_instructions(decision, feedback)
        );
        let text = self.call(prompt::system_prompt(), user).await?;
        prompt::parse_action(decision, obs.you, &text)
    }

    async fn discuss(&self, obs: &Observation, round: u8) -> Option<String> {
        // Total round count is not known here; the prompt shows a 1-based index.
        let user = format!(
            "{}\n\n{}",
            prompt::render_state(obs),
            prompt::discussion_prompt(round, round + 1)
        );
        let text = self.call(prompt::system_prompt(), user).await.ok()?;
        prompt::parse_discussion(&text)
    }

    async fn beliefs(&self, obs: &Observation) -> Option<Beliefs> {
        let user = format!(
            "{}\n\n{}",
            prompt::render_state(obs),
            prompt::beliefs_prompt(obs)
        );
        let text = self.call(prompt::system_prompt(), user).await.ok()?;
        prompt::parse_beliefs(&text)
    }

    fn take_usage(&self) -> Usage {
        std::mem::take(&mut self.usage.lock().unwrap())
    }
}
