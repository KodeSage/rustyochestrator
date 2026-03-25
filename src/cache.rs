use crate::errors::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const CACHE_DIR: &str = ".rustyochestrator";
const CACHE_FILE: &str = ".rustyochestrator/cache.json";

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub hash: String,
    pub success: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    pub entries: HashMap<String, CacheEntry>,
}

// ── Implementation ────────────────────────────────────────────────────────────

impl Cache {
    /// Load cache from `.rustyochestrator/cache.json`, or return an empty cache if absent.
    pub fn load() -> Result<Self> {
        if !Path::new(CACHE_FILE).exists() {
            return Ok(Cache::default());
        }
        let raw = std::fs::read_to_string(CACHE_FILE)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Persist cache to disk.
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(CACHE_DIR)?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(CACHE_FILE, json)?;
        Ok(())
    }

    /// Return `true` if `task_id` has been run successfully with the same hash.
    pub fn is_hit(&self, task_id: &str, hash: &str) -> bool {
        self.entries
            .get(task_id)
            .map(|e| e.success && e.hash == hash)
            .unwrap_or(false)
    }

    /// Record the result of a task execution.
    pub fn record(&mut self, task_id: String, hash: String, success: bool) {
        self.entries.insert(task_id, CacheEntry { hash, success });
    }
}
