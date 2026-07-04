//! `system_info` – return OS, architecture, hostname, and headless status.

use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct SystemInfo;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SystemInfoInput {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SystemInfoOutput {
    pub os: String,
    pub arch: String,
    pub hostname: String,
    pub headless: bool,
}

#[async_trait]
impl Command for SystemInfo {
    type Input = SystemInfoInput;
    type Output = SystemInfoOutput;

    fn name(&self) -> &'static str {
        "system_info"
    }

    fn description(&self) -> &'static str {
        "Report OS, architecture, hostname, and headless status."
    }

    async fn run(
        &self,
        _input: SystemInfoInput,
        _cx: &Ctx<'_>,
    ) -> Result<SystemInfoOutput, CommandError> {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string());

        Ok(SystemInfoOutput {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            hostname,
            headless: crate::types::detect_headless(),
        })
    }
}

register_command!(SystemInfo);
