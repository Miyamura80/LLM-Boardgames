//! Platform-specific implementations of OS capability traits.
//!
//! - [`StdFilesystem`]: real std::fs operations
//! - [`ReqwestNetwork`]: real HTTP via reqwest

use crate::traits::*;
use std::path::{Path, PathBuf};

// ===========================================================================
// Filesystem – wraps std::fs
// ===========================================================================

pub struct StdFilesystem;

impl FilesystemOps for StdFilesystem {
    fn read_file(&self, path: &Path) -> CapResult<Vec<u8>> {
        std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CapError::Io(e),
            std::io::ErrorKind::PermissionDenied => {
                CapError::PermissionDenied(format!("cannot read {}: {}", path.display(), e))
            }
            _ => CapError::Io(e),
        })
    }

    fn write_file(&self, path: &Path, data: &[u8]) -> CapResult<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, data).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => {
                CapError::PermissionDenied(format!("cannot write {}: {}", path.display(), e))
            }
            _ => CapError::Io(e),
        })
    }

    fn remove_file(&self, path: &Path) -> CapResult<()> {
        std::fs::remove_file(path).map_err(CapError::Io)
    }

    fn create_dir_all(&self, path: &Path) -> CapResult<()> {
        std::fs::create_dir_all(path).map_err(CapError::Io)
    }

    fn remove_dir_all(&self, path: &Path) -> CapResult<()> {
        std::fs::remove_dir_all(path).map_err(CapError::Io)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn temp_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }

    fn list_dir(&self, path: &Path) -> CapResult<Vec<DirEntry>> {
        let read_dir = std::fs::read_dir(path)?;
        let mut entries = Vec::new();
        for entry in read_dir {
            let entry = entry?;
            // Skip entries whose metadata is transiently unavailable
            let Ok(meta) = entry.metadata() else { continue };
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size_bytes: meta.len(),
            });
        }
        Ok(entries)
    }
}

// ===========================================================================
// Network – wraps reqwest
// ===========================================================================

pub struct ReqwestNetwork;

#[async_trait::async_trait]
impl NetworkOps for ReqwestNetwork {
    async fn dns_resolve(&self, host: &str) -> CapResult<Vec<String>> {
        use tokio::net::lookup_host;
        let addrs: Vec<String> = lookup_host(format!("{}:443", host))
            .await
            .map_err(|e| CapError::Network(format!("DNS resolution failed for {}: {}", host, e)))?
            .map(|a| a.ip().to_string())
            .collect();
        if addrs.is_empty() {
            return Err(CapError::Network(format!(
                "DNS resolution returned no addresses for {}",
                host
            )));
        }
        Ok(addrs)
    }

    async fn https_get(&self, url: &str, timeout_ms: u64) -> CapResult<(u16, String)> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| CapError::Network(format!("failed to build HTTP client: {}", e)))?;

        let resp = client.get(url).send().await.map_err(|e| {
            if e.is_timeout() {
                CapError::Timeout
            } else {
                CapError::Network(format!("HTTPS GET {}: {}", url, e))
            }
        })?;

        let status = resp.status().as_u16();
        // Read at most 4 KiB for the snippet
        let body = resp
            .text()
            .await
            .map_err(|e| CapError::Network(format!("reading body: {}", e)))?;
        let snippet: String = body.chars().take(4096).collect();
        Ok((status, snippet))
    }
}
