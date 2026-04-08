use crate::errors::Result;
use crate::pipeline::{Pipeline, Task, compute_task_hash};
use serde::Deserialize;
use serde_yaml::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};

// ── GitHub Actions YAML structures ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Workflow {
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    jobs: HashMap<String, Job>,
}

#[derive(Debug, Deserialize)]
struct Job {
    #[serde(default)]
    steps: Vec<Step>,
    /// `needs` can be a single string or a list of strings.
    #[serde(default, deserialize_with = "string_or_vec")]
    needs: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    /// GitHub Actions `if:` condition on the job.
    #[serde(default, rename = "if")]
    condition: Option<String>,
    /// Strategy with optional matrix.
    #[serde(default)]
    strategy: Option<Strategy>,
}

#[derive(Debug, Deserialize)]
struct Strategy {
    #[serde(default)]
    matrix: Option<MatrixConfig>,
}

#[derive(Debug, Deserialize)]
struct MatrixConfig {
    #[serde(default)]
    include: Option<Vec<HashMap<String, Value>>>,
    #[serde(default)]
    exclude: Option<Vec<HashMap<String, Value>>>,
    #[serde(flatten)]
    dimensions: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct Step {
    pub run: Option<String>,
    pub name: Option<String>,
    pub uses: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// GitHub Actions `if:` condition on the step.
    #[serde(default, rename = "if")]
    pub condition: Option<String>,
    /// `with` parameters (for uses: actions).
    #[serde(default)]
    pub with: HashMap<String, String>,
}

// ── Known actions ────────────────────────────────────────────────────────────

/// Classify a `uses:` action. Returns (action_base, is_noop, warning_msg).
fn classify_uses_action(uses: &str) -> (&'static str, bool, &'static str) {
    let base = uses.split('@').next().unwrap_or(uses);
    match base {
        "actions/checkout" => (
            "actions/checkout",
            true,
            "no-op: working directory is already the repo",
        ),
        "dtolnay/rust-toolchain" => (
            "dtolnay/rust-toolchain",
            true,
            "no-op: assumes system Rust is installed",
        ),
        "actions/cache" => (
            "actions/cache",
            true,
            "no-op: rustyochestrator's task-level cache covers this",
        ),
        "actions/upload-artifact" => ("actions/upload-artifact", false, ""),
        "actions/download-artifact" => ("actions/download-artifact", false, ""),
        _ => ("unknown", true, "unrecognised action — skipped"),
    }
}

// ── GitHub Actions condition evaluation ──────────────────────────────────────

/// Evaluate a GitHub Actions `if:` expression.
/// Supports: success(), failure(), always(), cancelled(), simple comparisons.
fn evaluate_gha_condition(expr: &str, job_status: &str) -> bool {
    let expr = expr.trim();

    // Boolean literals
    if expr.eq_ignore_ascii_case("true") || expr == "${{ true }}" {
        return true;
    }
    if expr.eq_ignore_ascii_case("false") || expr == "${{ false }}" {
        return false;
    }

    // Strip outer ${{ }} if present
    let inner = if expr.starts_with("${{") && expr.ends_with("}}") {
        expr[3..expr.len() - 2].trim()
    } else {
        expr
    };

    // Status functions
    match inner {
        "always()" => return true,
        "cancelled()" => return job_status == "cancelled",
        "failure()" => return job_status == "failure",
        "success()" => return job_status == "success" || job_status == "pending",
        _ => {}
    }

    // Simple comparison: X == 'Y' or X != 'Y'
    if inner.contains("==") || inner.contains("!=") {
        // For now, conditions requiring runtime context are assumed true
        // (we can't fully evaluate them without running)
        return true;
    }

    // Default: assume true (don't skip if we can't evaluate)
    true
}

// ── Matrix expansion ─────────────────────────────────────────────────────────

/// Expand a matrix config into a list of env var maps (one per combination).
fn expand_matrix(config: &MatrixConfig) -> Vec<HashMap<String, String>> {
    // Collect dimension keys and their values
    let mut dim_keys: Vec<String> = Vec::new();
    let mut dim_values: Vec<Vec<String>> = Vec::new();

    for (key, value) in &config.dimensions {
        if let Value::Sequence(seq) = value {
            dim_keys.push(key.clone());
            let vals: Vec<String> = seq
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    other => serde_yaml::to_string(other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                })
                .collect();
            dim_values.push(vals);
        }
    }

    if dim_keys.is_empty() && config.include.is_none() {
        return vec![HashMap::new()];
    }

    // Generate cartesian product
    let mut combos: Vec<HashMap<String, String>> = vec![HashMap::new()];
    for (i, key) in dim_keys.iter().enumerate() {
        let mut new_combos = Vec::new();
        for combo in &combos {
            for val in &dim_values[i] {
                let mut new = combo.clone();
                new.insert(key.clone(), val.clone());
                new_combos.push(new);
            }
        }
        combos = new_combos;
    }

    // Apply include: add extra combinations
    if let Some(includes) = &config.include {
        for inc in includes {
            let map: HashMap<String, String> = inc
                .iter()
                .map(|(k, v)| {
                    let vs = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        other => serde_yaml::to_string(other)
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    };
                    (k.clone(), vs)
                })
                .collect();

            // Check if this matches an existing combo (extends it) or is new
            let mut matched = false;
            for combo in combos.iter_mut() {
                let all_match = dim_keys.iter().all(|k| {
                    match (combo.get(k), map.get(k)) {
                        (Some(a), Some(b)) => a == b,
                        (_, None) => true, // include doesn't specify this dim
                        _ => false,
                    }
                });
                if all_match && dim_keys.iter().any(|k| map.contains_key(k)) {
                    // Extend existing combo with extra keys
                    for (k, v) in &map {
                        combo.insert(k.clone(), v.clone());
                    }
                    matched = true;
                    break;
                }
            }
            if !matched {
                combos.push(map);
            }
        }
    }

    // Apply exclude: remove matching combinations
    if let Some(excludes) = &config.exclude {
        combos.retain(|combo| {
            !excludes.iter().any(|exc| {
                exc.iter().all(|(k, v)| {
                    let vs = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        other => serde_yaml::to_string(other)
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    };
                    combo.get(k).map(|cv| cv == &vs).unwrap_or(false)
                })
            })
        });
    }

    combos
}

// ── Context variable resolution ──────────────────────────────────────────────

/// Resolve basic `${{ github.* }}` and `${{ env.* }}` context variables.
fn resolve_context_vars(s: &str, env: &HashMap<String, String>) -> String {
    let mut result = s.to_string();

    // Collect all ${{ ... }} patterns and resolve them
    while let Some(start) = result.find("${{") {
        let Some(end_offset) = result[start..].find("}}") else {
            break;
        };
        let end = start + end_offset + 2;
        let inner = result[start + 3..end - 2].trim();

        let replacement = if let Some(gh_key) = inner.strip_prefix("github.") {
            // Resolve from git or environment
            resolve_github_context(gh_key.trim())
        } else if let Some(runner_key) = inner.strip_prefix("runner.") {
            resolve_runner_context(runner_key.trim())
        } else if let Some(env_key) = inner.strip_prefix("env.") {
            env.get(env_key.trim())
                .cloned()
                .or_else(|| std::env::var(env_key.trim()).ok())
        } else if let Some(matrix_key) = inner.strip_prefix("matrix.") {
            // Resolve matrix.foo → look up "foo" in the env map
            env.get(matrix_key.trim()).cloned()
        } else if inner.starts_with("secrets.") {
            // Keep secrets references — they're resolved at runtime
            None
        } else {
            None
        };

        if let Some(val) = replacement {
            result = format!("{}{}{}", &result[..start], val, &result[end..]);
        } else if inner.starts_with("secrets.") {
            // Keep the original reference
            break;
        } else {
            // Can't resolve — remove the expression to avoid shell errors
            result = format!("{}{}", &result[..start], &result[end..]);
        }
    }

    result
}

/// Resolve `github.*` context variables from git and environment.
fn resolve_github_context(key: &str) -> Option<String> {
    match key {
        "sha" => git_cmd(&["rev-parse", "HEAD"]),
        "ref" => {
            git_cmd(&["rev-parse", "--abbrev-ref", "HEAD"]).map(|b| format!("refs/heads/{}", b))
        }
        "ref_name" => git_cmd(&["rev-parse", "--abbrev-ref", "HEAD"]),
        "repository" => {
            git_cmd(&["remote", "get-url", "origin"]).map(|url| {
                // Extract owner/repo from URL
                let url = url.trim_end_matches(".git");
                url.rsplit('/')
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("/")
            })
        }
        "actor" => std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok(),
        "workspace" => std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from)),
        "event_name" => Some("local".to_string()),
        _ => None,
    }
}

/// Resolve `runner.*` context variables from the local system.
fn resolve_runner_context(key: &str) -> Option<String> {
    match key {
        "os" => Some(
            match std::env::consts::OS {
                "macos" => "macOS",
                "linux" => "Linux",
                "windows" => "Windows",
                other => other,
            }
            .to_string(),
        ),
        "arch" => Some(
            match std::env::consts::ARCH {
                "x86_64" => "X64",
                "aarch64" => "ARM64",
                "x86" => "X86",
                other => other,
            }
            .to_string(),
        ),
        "temp" => std::env::temp_dir().to_str().map(String::from),
        "tool_cache" => std::env::temp_dir()
            .join("runner-tool-cache")
            .to_str()
            .map(String::from),
        _ => None,
    }
}

fn git_cmd(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Convert a GitHub Actions workflow YAML into a native `Pipeline`.
pub fn parse_github_workflow(content: &str) -> Result<Pipeline> {
    let workflow: Workflow = serde_yaml::from_str(content)?;

    // Process jobs in dependency order
    let job_order = topological_job_order(&workflow.jobs);

    let mut tasks: Vec<Task> = Vec::new();
    let mut job_tail: HashMap<String, String> = HashMap::new();
    // Track upload artifact task IDs for download references
    let mut artifact_uploads: HashMap<String, String> = HashMap::new();

    let filter_env = |raw: &HashMap<String, String>| -> HashMap<String, String> {
        raw.iter()
            .filter_map(|(k, v)| {
                if !v.contains("${{") {
                    return Some((k.clone(), v.clone()));
                }
                let t = v.trim();
                if t.starts_with("${{") && t.ends_with("}}") {
                    let inner = t[3..t.len() - 2].trim();
                    if inner.starts_with("secrets.") {
                        return Some((k.clone(), v.clone()));
                    }
                }
                tracing::debug!(key = k, value = v, "skipping GitHub Actions env expression");
                None
            })
            .collect()
    };

    let workflow_env = filter_env(&workflow.env);

    for job_name in &job_order {
        let job = &workflow.jobs[job_name];

        // Evaluate job-level condition
        if let Some(ref cond) = job.condition
            && !evaluate_gha_condition(cond, "pending")
        {
            tracing::info!(
                job = job_name.as_str(),
                condition = cond.as_str(),
                "job skipped by if: condition"
            );
            continue;
        }

        // Expand matrix if present
        let matrix_combos = if let Some(ref strategy) = job.strategy {
            if let Some(ref matrix) = strategy.matrix {
                expand_matrix(matrix)
            } else {
                vec![HashMap::new()]
            }
        } else {
            vec![HashMap::new()]
        };

        let job_env = filter_env(&job.env);

        for matrix_vars in &matrix_combos {
            let matrix_suffix = if matrix_combos.len() > 1 {
                // Create a readable suffix from matrix values
                let vals: Vec<&str> = matrix_vars.values().map(|v| v.as_str()).collect();
                format!("__{}", vals.join("_"))
            } else {
                String::new()
            };

            let mut prev: Option<String> = None;

            // Base env for context variable resolution (workflow + job level)
            let base_env: HashMap<String, String> = workflow_env
                .iter()
                .chain(job_env.iter())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            for (idx, step) in job.steps.iter().enumerate() {
                // Evaluate step-level condition
                if let Some(ref cond) = step.condition
                    && !evaluate_gha_condition(cond, "pending")
                {
                    let name = step.name.as_deref().unwrap_or("unnamed step");
                    tracing::info!(
                        job = job_name.as_str(),
                        step = name,
                        condition = cond.as_str(),
                        "step skipped by if: condition"
                    );
                    continue;
                }

                // Handle `uses:` steps
                if let Some(ref uses) = step.uses {
                    let (action_base, is_noop, warning) = classify_uses_action(uses);
                    let step_name = step.name.as_deref().unwrap_or(uses);

                    if action_base == "actions/upload-artifact" {
                        // Generate a synthetic upload task
                        let artifact_name = resolve_context_vars(
                            &step
                                .with
                                .get("name")
                                .cloned()
                                .unwrap_or_else(|| "artifact".to_string()),
                            &base_env,
                        );
                        let path_glob = resolve_context_vars(
                            &step
                                .with
                                .get("path")
                                .cloned()
                                .unwrap_or_else(|| ".".to_string()),
                            &base_env,
                        );
                        let task_id = format!(
                            "{}{}__{}",
                            job_name,
                            matrix_suffix,
                            normalize_name(&format!("upload_{}", artifact_name))
                        );

                        let cmd = format!(
                            "mkdir -p .rustyochestrator/artifacts/$RUSTYORCH_RUN_ID/{name} && \
                             cp -r {path} .rustyochestrator/artifacts/$RUSTYORCH_RUN_ID/{name}/ 2>/dev/null || \
                             echo '[warn] no files matched: {path}'",
                            name = artifact_name,
                            path = path_glob,
                        );

                        let mut depends_on: Vec<String> = Vec::new();
                        if let Some(p) = prev.clone() {
                            depends_on.push(p);
                        }
                        if prev.is_none() {
                            for needed in &job.needs {
                                let tail_key = if matrix_combos.len() > 1 {
                                    format!("{}{}", needed, matrix_suffix)
                                } else {
                                    needed.clone()
                                };
                                if let Some(tail) = job_tail.get(&tail_key).or(job_tail.get(needed))
                                {
                                    depends_on.push(tail.clone());
                                }
                            }
                        }

                        prev = Some(task_id.clone());
                        artifact_uploads.insert(artifact_name, task_id.clone());
                        tasks.push(Task {
                            id: task_id,
                            command: cmd,
                            depends_on,
                            env: HashMap::new(),
                            hash: None,
                            timeout: None,
                            retries: Some(0),
                            retry_delay: None,
                            outputs: Vec::new(),
                            condition: None,
                        });
                        continue;
                    }

                    if action_base == "actions/download-artifact" {
                        let artifact_name = resolve_context_vars(
                            &step
                                .with
                                .get("name")
                                .cloned()
                                .unwrap_or_else(|| "artifact".to_string()),
                            &base_env,
                        );
                        let dest = resolve_context_vars(
                            &step
                                .with
                                .get("path")
                                .cloned()
                                .unwrap_or_else(|| artifact_name.clone()),
                            &base_env,
                        );
                        let task_id = format!(
                            "{}{}__{}",
                            job_name,
                            matrix_suffix,
                            normalize_name(&format!("download_{}", artifact_name))
                        );

                        let cmd = format!(
                            "mkdir -p {dest} && \
                             cp -r .rustyochestrator/artifacts/$RUSTYORCH_RUN_ID/{name}/* {dest}/ 2>/dev/null || \
                             {{ echo '[error] artifact not found: {name}'; exit 1; }}",
                            name = artifact_name,
                            dest = dest,
                        );

                        let mut depends_on: Vec<String> = Vec::new();
                        if let Some(p) = prev.clone() {
                            depends_on.push(p);
                        }
                        // Also depend on the upload task
                        if let Some(upload_id) = artifact_uploads.get(&artifact_name)
                            && !depends_on.contains(upload_id)
                        {
                            depends_on.push(upload_id.clone());
                        }
                        if prev.is_none() {
                            for needed in &job.needs {
                                let tail_key = if matrix_combos.len() > 1 {
                                    format!("{}{}", needed, matrix_suffix)
                                } else {
                                    needed.clone()
                                };
                                if let Some(tail) = job_tail.get(&tail_key).or(job_tail.get(needed))
                                    && !depends_on.contains(tail)
                                {
                                    depends_on.push(tail.clone());
                                }
                            }
                        }

                        prev = Some(task_id.clone());
                        tasks.push(Task {
                            id: task_id,
                            command: cmd,
                            depends_on,
                            env: HashMap::new(),
                            hash: None,
                            timeout: None,
                            retries: Some(0),
                            retry_delay: None,
                            outputs: Vec::new(),
                            condition: None,
                        });
                        continue;
                    }

                    // Known no-op or unknown action → emit warning
                    if is_noop {
                        tracing::warn!(
                            job = job_name.as_str(),
                            step = step_name,
                            uses = uses.as_str(),
                            "⚠ uses: {} — {}",
                            uses,
                            warning
                        );
                        println!(
                            "  [warn] {}/{}: uses: {} — {}",
                            job_name, step_name, uses, warning
                        );
                    }
                    continue;
                }

                let run_cmd = match &step.run {
                    Some(r) => {
                        let cmd = r.trim().to_string();
                        if cmd.contains("${{") {
                            // Try to resolve context variables
                            let step_env = filter_env(&step.env);
                            let merged_env: HashMap<String, String> = workflow_env
                                .iter()
                                .chain(job_env.iter())
                                .chain(step_env.iter())
                                .chain(matrix_vars.iter())
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            let resolved = resolve_context_vars(&cmd, &merged_env);
                            // If still has unresolvable expressions, skip
                            if resolved.contains("${{") {
                                let name = step.name.as_deref().unwrap_or("unnamed step");
                                tracing::debug!(
                                    job = job_name.as_str(),
                                    step = name,
                                    "skipping step — contains unresolvable ${{{{...}}}} expression"
                                );
                                continue;
                            }
                            resolved
                        } else {
                            cmd
                        }
                    }
                    None => continue,
                };

                let label = step
                    .name
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .replace([' ', '-', '/'], "_");

                let task_id = if label.is_empty() {
                    format!("{}{}_step_{}", job_name, matrix_suffix, idx)
                } else {
                    format!("{}{}__{}", job_name, matrix_suffix, label)
                };

                let mut depends_on: Vec<String> = Vec::new();

                // Sequential within-job dependency
                if let Some(p) = prev.clone() {
                    depends_on.push(p);
                }

                // Cross-job dependency via `needs:`
                if prev.is_none() {
                    for needed in &job.needs {
                        let tail_key = if matrix_combos.len() > 1 {
                            format!("{}{}", needed, matrix_suffix)
                        } else {
                            needed.clone()
                        };
                        if let Some(tail) = job_tail.get(&tail_key).or(job_tail.get(needed)) {
                            depends_on.push(tail.clone());
                        }
                    }
                }

                // Merge env: workflow → job → step → matrix vars (later wins).
                let step_env = filter_env(&step.env);
                let task_env: HashMap<String, String> = workflow_env
                    .iter()
                    .chain(job_env.iter())
                    .chain(step_env.iter())
                    .chain(matrix_vars.iter())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                // Convert step condition to native condition
                let condition = step.condition.as_ref().map(|c| {
                    let c = c.trim();
                    if c.starts_with("${{") && c.ends_with("}}") {
                        c[3..c.len() - 2].trim().to_string()
                    } else {
                        c.to_string()
                    }
                });

                prev = Some(task_id.clone());
                tasks.push(Task {
                    id: task_id,
                    command: run_cmd,
                    depends_on,
                    env: task_env,
                    hash: None,
                    timeout: None,
                    retries: None,
                    retry_delay: None,
                    outputs: Vec::new(),
                    condition,
                });
            }

            if let Some(tail_id) = prev {
                let tail_key = if matrix_combos.len() > 1 {
                    format!("{}{}", job_name, matrix_suffix)
                } else {
                    job_name.clone()
                };
                job_tail.insert(tail_key, tail_id.clone());
                // Also store under the plain job name (for cross-job deps without matrix)
                if matrix_combos.len() > 1 {
                    job_tail.entry(job_name.clone()).or_insert(tail_id);
                }
            }
        }
    }

    for task in &mut tasks {
        let env_btree: BTreeMap<&str, &str> = task
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        task.hash = Some(compute_task_hash(
            &task.command,
            &task.depends_on,
            &env_btree,
        ));
    }

    Ok(Pipeline {
        tasks,
        env: HashMap::new(),
        defaults: None,
    })
}

fn normalize_name(s: &str) -> String {
    s.replace([' ', '-', '/', '.'], "_")
}

/// Return job names sorted in topological order (required jobs come first).
fn topological_job_order(jobs: &HashMap<String, Job>) -> Vec<String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for (name, job) in jobs {
        in_degree.entry(name.as_str()).or_insert(0);
        for needed in &job.needs {
            dependents
                .entry(needed.as_str())
                .or_default()
                .push(name.as_str());
            *in_degree.entry(name.as_str()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = {
        let mut roots: Vec<&str> = in_degree
            .iter()
            .filter(|e| *e.1 == 0)
            .map(|e| *e.0)
            .collect();
        roots.sort_unstable();
        roots.into_iter().collect()
    };

    let mut order: Vec<String> = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.to_string());
        if let Some(deps) = dependents.get(node) {
            let mut next: Vec<&str> = deps.clone();
            next.sort_unstable();
            for dep in next {
                let d = in_degree.get_mut(dep).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(dep);
                }
            }
        }
    }

    for name in jobs.keys() {
        if !order.contains(name) {
            order.push(name.clone());
        }
    }

    order
}

// ── Serde helper: accept "string" or ["string", …] ───────────────────────────

fn string_or_vec<'de, D>(de: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};
    use std::fmt;

    struct Sv;

    impl<'de> Visitor<'de> for Sv {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or a sequence of strings")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Vec<String>, E> {
            Ok(vec![v.to_owned()])
        }

        fn visit_string<E: serde::de::Error>(
            self,
            v: String,
        ) -> std::result::Result<Vec<String>, E> {
            Ok(vec![v])
        }

        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Vec<String>, A::Error> {
            let mut out = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                out.push(s);
            }
            Ok(out)
        }
    }

    de.deserialize_any(Sv)
}
