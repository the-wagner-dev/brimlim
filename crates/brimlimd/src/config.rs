//! `$XDG_CONFIG_HOME/brimlim/config.toml`. Absent is a valid configuration.

use std::time::Duration;

use serde::Deserialize;

use crate::util::paths;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// How often the daemon re-reads everything cheap (sessions, local logs).
    pub tick_secs: u64,
    /// Floor on how often a provider is allowed to hit a network API. Usage
    /// windows are hours and days long, so there is nothing to gain from
    /// asking more often than this — and a rate-limited provider is worse
    /// than a slightly older number.
    pub api_interval_secs: u64,
    /// A reading older than this is still shown, but flagged `stale`: the
    /// number is real, it is just not current.
    pub stale_after_minutes: i64,
    /// A remembered reading older than this stops being shown at all — a
    /// week-old percentage is closer to fiction than to data.
    pub max_reading_age_hours: i64,
    pub providers: Providers,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Providers {
    pub claude: bool,
    pub codex: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tick_secs: 3,
            api_interval_secs: 300,
            stale_after_minutes: 20,
            max_reading_age_hours: 12,
            providers: Providers::default(),
        }
    }
}

impl Default for Providers {
    fn default() -> Self {
        Self {
            claude: true,
            codex: true,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = paths::config_dir().join("config.toml");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&raw) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "ignoring unreadable config");
                Self::default()
            }
        }
    }

    pub fn tick(&self) -> Duration {
        Duration::from_secs(self.tick_secs.max(1))
    }

    pub fn api_interval(&self) -> Duration {
        Duration::from_secs(self.api_interval_secs.max(5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_defaults_for_the_rest() {
        let config: Config =
            toml::from_str("tick_secs = 10\n[providers]\ncodex = false\n").unwrap();
        assert_eq!(config.tick_secs, 10);
        assert_eq!(config.api_interval_secs, 300);
        assert!(config.providers.claude);
        assert!(!config.providers.codex);
    }
}
