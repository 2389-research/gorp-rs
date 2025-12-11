// ABOUTME: XDG Base Directory paths for cross-platform config and data storage
// ABOUTME: Provides standardized paths for logs, crypto store, and configuration

use directories::ProjectDirs;
use std::path::PathBuf;

/// Application identifier for XDG directories
const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "2389";
const APPLICATION: &str = "matrix-bridge";

/// Get XDG-compliant directories for the application
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
}

/// Get the data directory path (e.g., ~/.local/share/matrix-bridge/)
/// Falls back to ./data if XDG directories unavailable
pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("./data"))
}

/// Get the log directory path (inside data dir)
/// e.g., ~/.local/share/matrix-bridge/logs/
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Get the crypto store directory path
/// e.g., ~/.local/share/matrix-bridge/crypto_store/
pub fn crypto_store_dir() -> PathBuf {
    data_dir().join("crypto_store")
}

/// Get the config directory path (e.g., ~/.config/matrix-bridge/)
/// Falls back to current directory if XDG directories unavailable
pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Get the default config file path
/// e.g., ~/.config/matrix-bridge/config.toml
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}
