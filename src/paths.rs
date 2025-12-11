// ABOUTME: Standardized paths for config and data storage
// ABOUTME: Uses ~/.config/gorp for config and ~/.local/share/gorp for data

use directories::BaseDirs;
use std::path::PathBuf;

/// Get the home directory
fn home_dir() -> PathBuf {
    BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Get the config directory path: ~/.config/gorp/
pub fn config_dir() -> PathBuf {
    home_dir().join(".config").join("gorp")
}

/// Get the default config file path: ~/.config/gorp/config.toml
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Get the data directory path: ~/.local/share/gorp/
/// Falls back to ./data if home directory unavailable
pub fn data_dir() -> PathBuf {
    let home = home_dir();
    if home == PathBuf::from(".") {
        PathBuf::from("./data")
    } else {
        home.join(".local").join("share").join("gorp")
    }
}

/// Get the log directory path: ~/.local/share/gorp/logs/
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Get the crypto store directory path: ~/.local/share/gorp/crypto_store/
pub fn crypto_store_dir() -> PathBuf {
    data_dir().join("crypto_store")
}
