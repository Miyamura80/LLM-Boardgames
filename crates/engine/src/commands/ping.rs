//! `ping` – liveness check. Proves the wiring works end to end.

use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct Ping;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PingInput {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PingOutput {
    pub pong: bool,
}

#[async_trait]
impl Command for Ping {
    type Input = PingInput;
    type Output = PingOutput;

    fn name(&self) -> &'static str {
        "ping"
    }

    fn description(&self) -> &'static str {
        "Liveness check; returns { pong: true }."
    }

    async fn run(&self, _input: PingInput, _cx: &Ctx<'_>) -> Result<PingOutput, CommandError> {
        Ok(PingOutput { pong: true })
    }
}

register_command!(Ping);
