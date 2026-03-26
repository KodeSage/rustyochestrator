use crate::errors::{Result, RustyError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

// ── Data structures ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Environment variables for this task (merged with pipeline-level env at runtime).
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// SHA-256 of (command + dep ids + env keys/values); populated after parsing, never in YAML.
    #[serde(skip)]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Pipeline {
    pub tasks: Vec<Task>,
    /// Pipeline-level environment variables applied to every task unless overridden.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl Pipeline {
    /// Parse a native rustyochestrator YAML and compute task hashes.
    pub fn from_yaml(content: &str) -> Result<Self> {
        let mut pipeline: Pipeline = serde_yaml::from_str(content)?;
        for task in &mut pipeline.tasks {
            // Merge pipeline env + task env (task wins) for hashing — uses BTreeMap for
            // deterministic key ordering so the hash is stable across runs.
            let merged: BTreeMap<&str, &str> = pipeline
                .env
                .iter()
                .chain(task.env.iter())
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            task.hash = Some(compute_task_hash(&task.command, &task.depends_on, &merged));
        }
        Ok(pipeline)
    }

    /// Validate the pipeline: missing deps + cycle detection.
    pub fn validate(&self) -> Result<()> {
        let ids: HashSet<&str> = self.tasks.iter().map(|t| t.id.as_str()).collect();

        for task in &self.tasks {
            for dep in &task.depends_on {
                if !ids.contains(dep.as_str()) {
                    return Err(RustyError::MissingDependency {
                        task: task.id.clone(),
                        dep: dep.clone(),
                    });
                }
            }
        }

        self.detect_cycles()
    }

    // ── Cycle detection (DFS colouring) ──────────────────────────────────────

    fn detect_cycles(&self) -> Result<()> {
        // Build adjacency list: task → its dependencies
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for task in &self.tasks {
            adj.entry(task.id.as_str()).or_default();
            for dep in &task.depends_on {
                adj.entry(task.id.as_str()).or_default().push(dep.as_str());
            }
        }

        // 0 = unvisited  1 = in-stack  2 = done
        let mut colour: HashMap<&str, u8> = HashMap::new();

        for task in &self.tasks {
            if !colour.contains_key(task.id.as_str()) {
                let mut path = Vec::new();
                Self::dfs(task.id.as_str(), &adj, &mut colour, &mut path)?;
            }
        }
        Ok(())
    }

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        colour: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Result<()> {
        colour.insert(node, 1);
        path.push(node);

        if let Some(neighbours) = adj.get(node) {
            for &nb in neighbours {
                match colour.get(nb).copied() {
                    Some(1) => {
                        return Err(RustyError::CircularDependency(format!(
                            "{} -> {}",
                            path.join(" -> "),
                            nb
                        )));
                    }
                    Some(2) => {}
                    _ => Self::dfs(nb, adj, colour, path)?,
                }
            }
        }

        path.pop();
        colour.insert(node, 2);
        Ok(())
    }

    /// Return tasks grouped by depth level (level 0 = no deps).
    /// Used by `list` and `graph` commands.
    pub fn levels(&self) -> Vec<Vec<String>> {
        let mut depth: HashMap<String, usize> = HashMap::new();

        // Iteratively resolve depths until stable
        let mut changed = true;
        while changed {
            changed = false;
            for task in &self.tasks {
                let d = task
                    .depends_on
                    .iter()
                    .map(|dep| depth.get(dep).copied().unwrap_or(0) + 1)
                    .max()
                    .unwrap_or(0);
                let entry = depth.entry(task.id.clone()).or_insert(0);
                if *entry != d {
                    *entry = d;
                    changed = true;
                }
            }
        }

        let max = depth.values().copied().max().unwrap_or(0);
        let mut levels: Vec<Vec<String>> = vec![Vec::new(); max + 1];
        for task in &self.tasks {
            let d = depth[&task.id];
            levels[d].push(task.id.clone());
        }
        levels
    }

    /// Print a compact ASCII dependency graph to stdout.
    pub fn print_graph(&self) {
        let levels = self.levels();
        let task_map: HashMap<&str, &Task> =
            self.tasks.iter().map(|t| (t.id.as_str(), t)).collect();

        println!();
        for (i, level) in levels.iter().enumerate() {
            let label = if i == 0 {
                "  (no deps)".to_string()
            } else {
                String::new()
            };
            println!("  Stage {}{}:", i, label);

            for id in level {
                let task = task_map[id.as_str()];
                if task.depends_on.is_empty() {
                    println!("    {}", id);
                } else {
                    println!("    {}  ◄── [{}]", id, task.depends_on.join(", "));
                }
            }
            println!();
        }
    }
}

// ── Hashing ──────────────────────────────────────────────────────────────────

pub fn compute_task_hash(
    command: &str,
    depends_on: &[String],
    env: &BTreeMap<&str, &str>,
) -> String {
    let mut h = Sha256::new();
    h.update(command.as_bytes());
    for dep in depends_on {
        h.update(dep.as_bytes());
    }
    // Include env key=value pairs so cache is invalidated when env changes.
    for (k, v) in env {
        h.update(k.as_bytes());
        h.update(b"=");
        h.update(v.as_bytes());
    }
    hex::encode(h.finalize())
}
