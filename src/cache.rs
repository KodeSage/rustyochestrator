use crate::errors::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
        // A concurrent save may have left a partial file; treat parse errors as a cache miss
        // rather than aborting the run — the cache is an optimisation, not a source of truth.
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    /// Persist cache to disk using an atomic write (temp file → rename) so concurrent
    /// readers never observe a truncated or partially-written file.
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(CACHE_DIR)?;
        let json = serde_json::to_string_pretty(self)?;
        // Build a unique temp path: PID + subsec_nanos avoids collisions across
        // threads in the same process and across concurrent processes.
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let tmp = format!("{}.{}-{}.tmp", CACHE_FILE, std::process::id(), nonce);
        std::fs::write(&tmp, &json)?;
        // rename(2) on POSIX is atomic: readers always see the old file or the new
        // file, never a half-written state.
        std::fs::rename(&tmp, CACHE_FILE)?;
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
