use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the Calibre OPDS server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to Calibre library directory
    pub library_path: PathBuf,

    /// Server host address
    #[serde(default = "default_host")]
    pub host: String,

    /// Server port
    #[serde(default = "default_port")]
    pub port: u16,

    /// Base URL for the server (used in OPDS links)
    pub base_url: Option<String>,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

impl Config {
    /// Get the database path from library path
    pub fn db_path(&self) -> PathBuf {
        self.library_path.join("metadata.db")
    }

    /// Get the base URL or construct from host:port
    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", self.host, self.port))
    }

    /// Get the URL prefix path (e.g. "/cops" from "https://host:9080/cops")
    pub fn url_prefix(&self) -> String {
        if let Some(ref url) = self.base_url
            && let Some(pos) = url.find("://")
        {
            let after_scheme = &url[pos + 3..];
            if let Some(slash_pos) = after_scheme.find('/') {
                return after_scheme[slash_pos..].trim_end_matches('/').to_string();
            }
        }
        String::new()
    }
}
