//! Where things live. Everything is XDG-correct and overridable by env var,
//! which is what makes the daemon testable without touching a real home dir.

use std::path::PathBuf;

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn home() -> PathBuf {
    env_dir("HOME").unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_dir() -> PathBuf {
    env_dir("XDG_CONFIG_HOME")
        .unwrap_or_else(|| home().join(".config"))
        .join("brimlim")
}

pub fn state_dir() -> PathBuf {
    env_dir("XDG_STATE_HOME")
        .unwrap_or_else(|| home().join(".local").join("state"))
        .join("brimlim")
}

/// `~/.claude`, or `$CLAUDE_CONFIG_DIR` when the user moved it.
pub fn claude_dir() -> PathBuf {
    env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home().join(".claude"))
}

/// `~/.codex`, or `$CODEX_HOME` when the user moved it.
pub fn codex_dir() -> PathBuf {
    env_dir("CODEX_HOME").unwrap_or_else(|| home().join(".codex"))
}
