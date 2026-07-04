//! `list_dir` – list entries in a directory.

use crate::commands::{Command, CommandError, Expose};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct ListDir;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDirInput {
    /// Directory path to list.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListDirOutput {
    pub entries: Vec<DirEntry>,
}

#[async_trait]
impl Command for ListDir {
    type Input = ListDirInput;
    type Output = ListDirOutput;

    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List the entries of a directory."
    }

    /// Lists a caller-supplied path with no sandbox — CLI-only so it is not
    /// reachable as an unauthenticated directory-enumeration over the HTTP API.
    fn expose(&self) -> Expose {
        Expose::cli_only()
    }

    async fn run(&self, input: ListDirInput, cx: &Ctx<'_>) -> Result<ListDirOutput, CommandError> {
        let path = std::path::Path::new(&input.path);
        let entries = cx
            .fs()
            .list_dir(path)?
            .into_iter()
            .map(|e| DirEntry {
                name: e.name,
                is_dir: e.is_dir,
                size_bytes: e.size_bytes,
            })
            .collect();
        Ok(ListDirOutput { entries })
    }
}

register_command!(ListDir);
