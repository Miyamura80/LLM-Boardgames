use std::path::{Path, PathBuf};

/// Result type for trait operations that may be unsupported.
pub type CapResult<T> = Result<T, CapError>;

#[derive(Debug, thiserror::Error)]
pub enum CapError {
    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("dependency missing: {0}")]
    DependencyMissing(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("network error: {0}")]
    Network(String),

    #[error("timeout")]
    Timeout,

    #[error("{0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// Filesystem operations
// ---------------------------------------------------------------------------

/// Metadata for a single directory entry.
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

pub trait FilesystemOps: Send + Sync {
    fn read_file(&self, path: &Path) -> CapResult<Vec<u8>>;
    fn write_file(&self, path: &Path, data: &[u8]) -> CapResult<()>;
    fn remove_file(&self, path: &Path) -> CapResult<()>;
    fn create_dir_all(&self, path: &Path) -> CapResult<()>;
    fn remove_dir_all(&self, path: &Path) -> CapResult<()>;
    fn exists(&self, path: &Path) -> bool;
    fn temp_dir(&self) -> PathBuf;
    fn list_dir(&self, path: &Path) -> CapResult<Vec<DirEntry>>;
}

// ---------------------------------------------------------------------------
// Network operations
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait NetworkOps: Send + Sync {
    /// Resolve a hostname to at least one IP address.
    async fn dns_resolve(&self, host: &str) -> CapResult<Vec<String>>;

    /// Perform an HTTPS GET and return (status_code, body_snippet).
    async fn https_get(&self, url: &str, timeout_ms: u64) -> CapResult<(u16, String)>;
}
