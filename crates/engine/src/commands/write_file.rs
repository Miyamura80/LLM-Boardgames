//! `write_file` – write string content to a file.

use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct WriteFile;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileInput {
    /// Absolute path to write to (parent directories are created).
    pub path: String,
    /// UTF-8 content to write.
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WriteFileOutput {
    pub bytes_written: usize,
}

#[async_trait]
impl Command for WriteFile {
    type Input = WriteFileInput;
    type Output = WriteFileOutput;

    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write UTF-8 content to a file, creating parent directories."
    }

    /// Writes to a caller-supplied path with no sandbox — CLI-only so it is not
    /// reachable as an unauthenticated arbitrary-file-write over the HTTP API.
    fn expose(&self) -> Expose {
        Expose::cli_only()
    }

    async fn run(
        &self,
        input: WriteFileInput,
        cx: &Ctx<'_>,
    ) -> Result<WriteFileOutput, CommandError> {
        let path = std::path::Path::new(&input.path);
        // This command creates parent dirs and writes arbitrary content, so
        // reject relative/traversal paths rather than writing somewhere unintended.
        if !path.is_absolute() {
            return Err(CommandError::InvalidInput(format!(
                "path must be absolute: {}",
                input.path
            )));
        }
        let data = input.content.as_bytes();
        cx.fs().write_file(path, data)?;
        Ok(WriteFileOutput {
            bytes_written: data.len(),
        })
    }
}

register_command!(WriteFile);
