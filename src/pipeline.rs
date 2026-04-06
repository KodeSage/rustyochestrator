use crate::errors::{Result, RustyError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

// ── Duration parsing ─────────────────────────────────────────────────────────

/// Parse a human-readable duration string (e.g. "300s", "5m", "1h", "1h30m").
pub fn parse_duration(s: &str) -> Option<std::time::Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total_secs: u64 = 0;
    let mut num_buf = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num_buf.push(ch);
        } else {
            let n: u64 = num_buf.parse().ok()?;
            num_buf.clear();
            match ch {
                's' => total_secs += n,
                'm' => total_secs += n * 60,
                'h' => total_secs += n * 3600,
                _ => return None,
            }
        }
    }
    // bare number with no suffix → seconds
    if !num_buf.is_empty() {
        total_secs += num_buf.parse::<u64>().ok()?;
    }
    if total_secs == 0 {
        return None;
    }
    Some(std::time::Duration::from_secs(total_secs))
}

// ── Retry delay ──────────────────────────────────────────────────────────────

/// Retry delay strategy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RetryDelay {
    /// A fixed delay string like "5s" or "1m".
    Fixed(String),
    /// A structured delay with a strategy field.
    Structured {
        strategy: String,
        /// Base delay string (e.g. "1s"). Defaults to "1s".
        #[serde(default = "default_base_delay")]
        base: String,
    },
}

fn default_base_delay() -> String {
    "1s".to_string()
}

impl RetryDelay {
    /// Compute the delay duration for the given attempt (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        match self {
            RetryDelay::Fixed(s) => parse_duration(s).unwrap_or(std::time::Duration::from_secs(1)),
            RetryDelay::Structured { strategy, base } => {
                let base_dur = parse_duration(base).unwrap_or(std::time::Duration::from_secs(1));
                if strategy == "exponential" {
                    base_dur * 2u32.saturating_pow(attempt)
                } else {
                    // "fixed" or unknown → fixed
                    base_dur
                }
            }
        }
    }
}

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

    // ── v0.1.4 fields ────────────────────────────────────────────────────────
    /// Per-task timeout (e.g. "300s", "5m", "1h"). Overrides pipeline-level default.
    #[serde(default)]
    pub timeout: Option<String>,

    /// Number of retries on failure. Overrides pipeline-level default (which defaults to 2).
    #[serde(default)]
    pub retries: Option<u32>,

    /// Delay between retries. Can be a fixed duration string or a structured config.
    #[serde(default)]
    pub retry_delay: Option<RetryDelay>,

    /// Named outputs this task exports. Captured from stdout lines matching `NAME=value`.
    #[serde(default)]
    pub outputs: Vec<String>,

    /// Condition expression. If it evaluates to false, the task is skipped (not failed).
    /// Supports simple expressions like `"$ENV_VAR == 'production'"`.
    #[serde(default, rename = "if")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,       // cache hit
    ConditionSkip, // skipped due to `if:` evaluating to false
}

// ── Pipeline defaults ────────────────────────────────────────────────────────

/// Pipeline-level defaults that apply to all tasks unless overridden.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PipelineDefaults {
    /// Default timeout for all tasks.
    #[serde(default)]
    pub timeout: Option<String>,
    /// Default retry count for all tasks.
    #[serde(default)]
    pub retries: Option<u32>,
    /// Default retry delay for all tasks.
    #[serde(default)]
    pub retry_delay: Option<RetryDelay>,
}

// ── Pipeline ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Pipeline {
    pub tasks: Vec<Task>,
    /// Pipeline-level environment variables applied to every task unless overridden.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Pipeline-level defaults for timeout, retries, retry_delay.
    #[serde(default)]
    pub defaults: Option<PipelineDefaults>,
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

    /// Resolve the effective timeout for a task (task overrides pipeline default).
    pub fn effective_timeout(&self, task: &Task) -> Option<std::time::Duration> {
        task.timeout
            .as_deref()
            .or(self.defaults.as_ref().and_then(|d| d.timeout.as_deref()))
            .and_then(parse_duration)
    }

    /// Resolve the effective retry count for a task.
    pub fn effective_retries(&self, task: &Task) -> u32 {
        task.retries
            .or(self.defaults.as_ref().and_then(|d| d.retries))
            .unwrap_or(2)
    }

    /// Resolve the effective retry delay for a task.
    pub fn effective_retry_delay(&self, task: &Task) -> Option<RetryDelay> {
        task.retry_delay
            .clone()
            .or(self.defaults.as_ref().and_then(|d| d.retry_delay.clone()))
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

// ── Condition evaluation ─────────────────────────────────────────────────────

/// Evaluate a simple condition expression.
///
/// Supported forms:
/// - `"true"` / `"false"`
/// - `"$ENV_VAR == 'value'"` — compares env var to string literal
/// - `"$ENV_VAR != 'value'"`
/// - `"$ENV_VAR"` — truthy if set and non-empty
/// - `"tasks.task_id.result == 'success'"` — check task outcome
pub fn evaluate_condition(
    expr: &str,
    env: &HashMap<String, String>,
    task_results: &HashMap<String, TaskState>,
) -> bool {
    let expr = expr.trim();

    if expr.eq_ignore_ascii_case("true") {
        return true;
    }
    if expr.eq_ignore_ascii_case("false") {
        return false;
    }

    // Try comparison: LHS == RHS or LHS != RHS
    let (lhs, op, rhs) = if let Some(pos) = expr.find("!=") {
        let l = expr[..pos].trim();
        let r = expr[pos + 2..].trim();
        (l, "!=", r)
    } else if let Some(pos) = expr.find("==") {
        let l = expr[..pos].trim();
        let r = expr[pos + 2..].trim();
        (l, "==", r)
    } else {
        // Bare variable check: truthy if set and non-empty
        let var_name = expr.strip_prefix('$').unwrap_or(expr);
        let val = env
            .get(var_name)
            .cloned()
            .or_else(|| std::env::var(var_name).ok())
            .unwrap_or_default();
        return !val.is_empty() && val != "0" && val.to_lowercase() != "false";
    };

    let resolve_value = |s: &str| -> String {
        // Strip quotes from string literals
        let s = s.trim();
        if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
            return s[1..s.len() - 1].to_string();
        }
        // $ENV_VAR reference
        if let Some(var) = s.strip_prefix('$') {
            return env
                .get(var)
                .cloned()
                .or_else(|| std::env::var(var).ok())
                .unwrap_or_default();
        }
        // tasks.task_id.result
        if let Some(rest) = s.strip_prefix("tasks.")
            && let Some(dot) = rest.find('.')
        {
            let task_id = &rest[..dot];
            let field = &rest[dot + 1..];
            if field == "result" {
                return match task_results.get(task_id) {
                    Some(TaskState::Success) | Some(TaskState::Skipped) => "success".to_string(),
                    Some(TaskState::Failed) => "failure".to_string(),
                    Some(TaskState::ConditionSkip) => "skipped".to_string(),
                    _ => "pending".to_string(),
                };
            }
        }
        s.to_string()
    };

    let lhs_val = resolve_value(lhs);
    let rhs_val = resolve_value(rhs);

    match op {
        "==" => lhs_val == rhs_val,
        "!=" => lhs_val != rhs_val,
        _ => false,
    }
}
