//! Last-good readings, persisted so a restart doesn't blank the overlay.
//!
//! What is remembered is deliberately only the numeric half of a reading:
//! sessions and activity are live facts and would be lies the moment they
//! were written down.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Usage;
use crate::util::paths;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    readings: BTreeMap<String, Usage>,
    #[serde(skip)]
    path: PathBuf,
}

impl Store {
    pub fn load() -> Self {
        let path = paths::state_dir().join("readings.json");
        let mut store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Store>(&raw).ok())
            .unwrap_or_default();
        store.path = path;
        store
    }

    pub fn get(&self, provider_id: &str) -> Option<&Usage> {
        self.readings.get(provider_id)
    }

    pub fn put(&mut self, provider_id: &str, usage: Usage) {
        self.readings.insert(provider_id.to_owned(), usage);
    }

    /// Write via a temp file + rename so a crash mid-write cannot leave a
    /// truncated file that would silently lose every remembered reading.
    pub fn save(&self) -> anyhow::Result<()> {
        let Some(dir) = self.path.parent() else {
            return Ok(());
        };
        std::fs::create_dir_all(dir)?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }
}
