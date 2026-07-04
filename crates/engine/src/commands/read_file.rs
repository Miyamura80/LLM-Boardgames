//! `read_file` – read a file and return its contents as a UTF-8 string.

use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct ReadFile;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileInput {
    /// Absolute path to the file to read.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadFileOutput {
    pub content: String,
    pub size_bytes: usize,
}

#[async_trait]
impl Command for ReadFile {
    type Input = ReadFileInput;
    type Output = ReadFileOutput;

    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a file and return its UTF-8 contents."
    }

    /// Reads a caller-supplied path with no sandbox — CLI-only so it is not
    /// reachable as an unauthenticated arbitrary-file-read over the HTTP API.
    fn expose(&self) -> Expose {
        Expose::cli_only()
    }

    async fn run(
        &self,
        input: ReadFileInput,
        cx: &Ctx<'_>,
    ) -> Result<ReadFileOutput, CommandError> {
        let path = std::path::Path::new(&input.path);
        let data = cx.fs().read_file(path)?;
        let size_bytes = data.len();
        // The output contract is UTF-8; surface a clear error on binary input
        // rather than silently returning lossily-replaced content.
        let content = String::from_utf8(data)
            .map_err(|_| CommandError::InvalidInput("file is not valid UTF-8".to_string()))?;
        Ok(ReadFileOutput {
            content,
            size_bytes,
        })
    }
}

register_command!(ReadFile);
