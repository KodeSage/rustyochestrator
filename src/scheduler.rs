use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, Semaphore};
use tracing::{error, info, warn};

use crate::cache::Cache;
use crate::errors::{Result, RustyError};
use crate::executor::{self, LogWriter, TaskOutputs};
use crate::pipeline::{Pipeline, Task, TaskState, evaluate_condition};
use crate::report::RunReport;
use crate::reporter::{Event, PipelineCompletedArgs, Reporter};
use crate::tui::Dashboard;

/// Per-task config: (retries, retry_delay, timeout, output_names).
type TaskConfigMap = HashMap<
    String,
    (
        u32,
        Option<crate::pipeline::RetryDelay>,
        Option<std::time::Duration>,
        Vec<String>,
    ),
>;

// ── Env helpers ───────────────────────────────────────────────────────────────

/// Merge pipeline-level env with task-level env (task wins on conflict).
fn merge_env(
    pipeline_env: &HashMap<String, String>,
    task_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    pipeline_env
        .iter()
        .chain(task_env.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// If `value` is a `${{ secrets.NAME }}` reference return `Some("NAME")`.
fn secret_ref(value: &str) -> Option<&str> {
    let t = value.trim();
    if t.starts_with("${{") && t.ends_with("}}") {
        let inner = t[3..t.len() - 2].trim();
        inner.strip_prefix("secrets.").map(str::trim)
    } else {
        None
    }
}

/// Resolve all env values: substitute `${{ secrets.NAME }}` from the shell environment.
/// Also resolves `${{ tasks.task_id.outputs.NAME }}` from captured outputs.
fn resolve_env(
    task_id: &str,
    env: &HashMap<String, String>,
    task_outputs: Option<&HashMap<String, TaskOutputs>>,
) -> Result<HashMap<String, String>> {
    let mut resolved = HashMap::with_capacity(env.len());
    for (key, value) in env {
        if let Some(secret_name) = secret_ref(value) {
            match std::env::var(secret_name) {
                Ok(val) => {
                    resolved.insert(key.clone(), val);
                }
                Err(_) => {
                    return Err(RustyError::MissingSecret {
                        key: key.clone(),
                        secret: secret_name.to_string(),
                        task: task_id.to_string(),
                    });
                }
            }
        } else {
            // Resolve ${{ tasks.X.outputs.Y }} references in values
            let val = resolve_task_output_refs(value, task_outputs);
            resolved.insert(key.clone(), val);
        }
    }
    Ok(resolved)
}

/// Replace `${{ tasks.task_id.outputs.NAME }}` references in a string with actual values.
fn resolve_task_output_refs(
    value: &str,
    task_outputs: Option<&HashMap<String, TaskOutputs>>,
) -> String {
    let Some(outputs) = task_outputs else {
        return value.to_string();
    };

    let mut result = value.to_string();
    // Find and replace all ${{ tasks.X.outputs.Y }} patterns
    while let Some(start) = result.find("${{") {
        let Some(end) = result[start..].find("}}") else {
            break;
        };
        let end = start + end + 2;
        let inner = result[start + 3..end - 2].trim();

        if let Some(rest) = inner.strip_prefix("tasks.") {
            // Parse: task_id.outputs.name
            let parts: Vec<&str> = rest.splitn(3, '.').collect();
            if parts.len() == 3 && parts[1] == "outputs" {
                let task_id = parts[0];
                let output_name = parts[2];
                let replacement = outputs
                    .get(task_id)
                    .and_then(|o| o.get(output_name))
                    .cloned()
                    .unwrap_or_default();
                result = format!("{}{}{}", &result[..start], replacement, &result[end..]);
                continue;
            }
        }
        // Not a recognized pattern — skip past it
        break;
    }
    result
}

// ── Channel message ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct TaskOutcome {
    id: String,
    success: bool,
    skipped: bool,        // cache hit
    condition_skip: bool, // skipped by if: condition
    duration_ms: u64,
    outputs: TaskOutputs,
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

pub struct Scheduler {
    pipeline: Pipeline,
    concurrency: usize,
    reporter: Option<Reporter>,
    dashboard: Option<Arc<Dashboard>>,
    pipeline_id: String,
    pipeline_name: String,
    dry_run: bool,
    trace_deps: bool,
    log_writer: Option<LogWriter>,
}

impl Scheduler {
    pub fn new(pipeline: Pipeline, concurrency: usize) -> Self {
        // Derive a pipeline name from the task list (use first task id as base)
        let name = pipeline
            .tasks
            .first()
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "pipeline".to_string());
        let id = format!("{}-{:x}", name, rand_u32());
        Self {
            pipeline,
            concurrency,
            reporter: None,
            dashboard: None,
            pipeline_id: id,
            pipeline_name: name,
            dry_run: false,
            trace_deps: false,
            log_writer: None,
        }
    }

    pub fn with_reporter(mut self, reporter: Reporter) -> Self {
        self.reporter = Some(reporter);
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.pipeline_name = name.clone();
        self.pipeline_id = format!("{}-{:x}", name, rand_u32());
        self
    }

    pub fn with_dashboard(mut self, dashboard: Arc<Dashboard>) -> Self {
        self.dashboard = Some(dashboard);
        self
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn with_trace_deps(mut self, trace_deps: bool) -> Self {
        self.trace_deps = trace_deps;
        self
    }

    pub fn with_log_writer(mut self, log_writer: LogWriter) -> Self {
        self.log_writer = Some(log_writer);
        self
    }

    /// Run the pipeline and return `true` if every task succeeded / was skipped.
    pub async fn run(self) -> Result<bool> {
        // ── Trace deps ──────────────────────────────────────────────────────
        if self.trace_deps {
            println!(
                "\n[trace-deps] Dependency resolution for '{}':",
                self.pipeline_name
            );
            let levels = self.pipeline.levels();
            for (stage, ids) in levels.iter().enumerate() {
                println!("  Stage {}:", stage);
                for id in ids {
                    let task = self.pipeline.tasks.iter().find(|t| &t.id == id).unwrap();
                    if task.depends_on.is_empty() {
                        println!("    {} (no deps → ready immediately)", id);
                    } else {
                        println!(
                            "    {} (blocked by: {} → ready when all complete)",
                            id,
                            task.depends_on.join(", ")
                        );
                    }
                }
            }
            println!();
        }

        // ── Dry run ─────────────────────────────────────────────────────────
        if self.dry_run {
            println!(
                "\n[dry-run] Would execute pipeline '{}' with {} task(s):\n",
                self.pipeline_name,
                self.pipeline.tasks.len()
            );
            let levels = self.pipeline.levels();
            for (stage, ids) in levels.iter().enumerate() {
                println!("  Stage {} ({} parallel):", stage, ids.len());
                for id in ids {
                    let task = self.pipeline.tasks.iter().find(|t| &t.id == id).unwrap();
                    let timeout_str = self
                        .pipeline
                        .effective_timeout(task)
                        .map(|d| format!(" timeout={}s", d.as_secs()))
                        .unwrap_or_default();
                    let retries = self.pipeline.effective_retries(task);
                    let cond_str = task
                        .condition
                        .as_deref()
                        .map(|c| format!(" if=\"{}\"", c))
                        .unwrap_or_default();
                    println!(
                        "    {} → `{}`  (retries={}{}{}){}",
                        id,
                        task.command.lines().next().unwrap_or(""),
                        retries,
                        timeout_str,
                        cond_str,
                        if task.depends_on.is_empty() {
                            String::new()
                        } else {
                            format!("  after: [{}]", task.depends_on.join(", "))
                        }
                    );
                }
            }
            println!("\n[dry-run] No tasks were executed.");
            return Ok(true);
        }

        // ── Pre-flight: resolve all secrets before any task runs ─────────────
        let resolved_envs: Arc<HashMap<String, HashMap<String, String>>> = {
            let mut map = HashMap::new();
            for task in &self.pipeline.tasks {
                let merged = merge_env(&self.pipeline.env, &task.env);
                let resolved = resolve_env(&task.id, &merged, None)?;
                map.insert(task.id.clone(), resolved);
            }
            Arc::new(map)
        };

        let cache = Arc::new(Mutex::new(Cache::load()?));
        let sem = Arc::new(Semaphore::new(self.concurrency));

        let tasks_map: Arc<HashMap<String, Task>> = Arc::new(
            self.pipeline
                .tasks
                .iter()
                .map(|t| (t.id.clone(), t.clone()))
                .collect(),
        );

        // Per-task config: (retries, retry_delay, timeout, output_names)
        let task_configs: Arc<TaskConfigMap> = Arc::new(
            self.pipeline
                .tasks
                .iter()
                .map(|t| {
                    let retries = self.pipeline.effective_retries(t);
                    let retry_delay = self.pipeline.effective_retry_delay(t);
                    let timeout = self.pipeline.effective_timeout(t);
                    let outputs = t.outputs.clone();
                    (t.id.clone(), (retries, retry_delay, timeout, outputs))
                })
                .collect(),
        );

        // Shared task outputs store
        let task_outputs: Arc<Mutex<HashMap<String, TaskOutputs>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let buf = (self.pipeline.tasks.len() + 4).max(8);
        let (tx, mut rx) = mpsc::channel::<TaskOutcome>(buf);

        let mut states: HashMap<String, TaskState> = tasks_map
            .keys()
            .map(|id| (id.clone(), TaskState::Pending))
            .collect();

        let mut running: usize = 0;
        let mut cached_tasks: usize = 0;
        let mut failed_tasks: usize = 0;
        let total_tasks = self.pipeline.tasks.len();
        let start = Instant::now();

        // For run report: per-task timing
        let mut task_timings: HashMap<String, (u64, String)> = HashMap::new(); // id → (duration_ms, status)

        // Emit pipeline_started
        if let Some(ref r) = self.reporter {
            let user_login = crate::config::load()
                .map(|c| c.user_login)
                .unwrap_or_else(|| "cli".to_string());
            r.send(Event::pipeline_started(
                &self.pipeline_id,
                &self.pipeline_name,
                total_tasks,
                &user_login,
            ));
        }

        // Kick off tasks with no dependencies
        let boot: Vec<String> = self
            .pipeline
            .tasks
            .iter()
            .filter(|t| t.depends_on.is_empty())
            .map(|t| t.id.clone())
            .collect();

        let prefix = format!("[{}] ", self.pipeline_name);
        let quiet = self.dashboard.is_some();

        for id in boot {
            // Check condition before starting
            let task = &tasks_map[&id];
            if let Some(ref cond) = task.condition {
                let env = resolved_envs.get(&id).cloned().unwrap_or_default();
                if !evaluate_condition(cond, &env, &states) {
                    if !quiet {
                        println!(
                            "{}[SKIP] Task '{}' condition evaluated to false: {}",
                            prefix, id, cond
                        );
                    }
                    states.insert(id.clone(), TaskState::ConditionSkip);
                    task_timings.insert(id.clone(), (0, "condition_skip".to_string()));
                    if let Some(ref d) = self.dashboard {
                        d.task_condition_skipped(&id);
                    }
                    continue;
                }
            }

            states.insert(id.clone(), TaskState::Running);
            running += 1;
            if let Some(ref d) = self.dashboard {
                d.task_started(&id);
            }
            spawn_task(
                tasks_map[&id].clone(),
                tx.clone(),
                sem.clone(),
                cache.clone(),
                prefix.clone(),
                quiet,
                resolved_envs.clone(),
                task_configs.clone(),
                task_outputs.clone(),
                self.log_writer.clone(),
            );
        }

        if running == 0 && states.values().all(|s| *s != TaskState::Pending) {
            // All tasks were either condition-skipped or there were no tasks
            if let Some(ref d) = self.dashboard {
                d.finish(true);
            }
            return Ok(true);
        }

        // Main event loop
        while running > 0 {
            let outcome = match rx.recv().await {
                Some(msg) => msg,
                None => break,
            };
            running -= 1;

            // Store captured outputs
            if !outcome.outputs.is_empty() {
                let mut outputs = task_outputs.lock().await;
                outputs.insert(outcome.id.clone(), outcome.outputs.clone());
            }

            // Report task completion to remote dashboard
            if let Some(ref r) = self.reporter {
                r.send(Event::task_completed(
                    &self.pipeline_id,
                    &outcome.id,
                    outcome.skipped,
                    outcome.duration_ms,
                    outcome.success || outcome.skipped,
                ));
            }

            let new_state = if outcome.condition_skip {
                info!("task '{}' skipped (condition false)", outcome.id);
                task_timings.insert(outcome.id.clone(), (0, "condition_skip".to_string()));
                if let Some(ref d) = self.dashboard {
                    d.task_condition_skipped(&outcome.id);
                }
                TaskState::ConditionSkip
            } else if outcome.skipped {
                info!("task '{}' skipped (cache hit)", outcome.id);
                cached_tasks += 1;
                task_timings.insert(
                    outcome.id.clone(),
                    (outcome.duration_ms, "cached".to_string()),
                );
                if let Some(ref d) = self.dashboard {
                    d.task_completed(&outcome.id, outcome.duration_ms, true);
                }
                TaskState::Skipped
            } else if outcome.success {
                info!("task '{}' succeeded", outcome.id);
                task_timings.insert(
                    outcome.id.clone(),
                    (outcome.duration_ms, "success".to_string()),
                );
                if let Some(ref d) = self.dashboard {
                    d.task_completed(&outcome.id, outcome.duration_ms, false);
                }
                TaskState::Success
            } else {
                error!("task '{}' failed", outcome.id);
                failed_tasks += 1;
                task_timings.insert(
                    outcome.id.clone(),
                    (outcome.duration_ms, "failed".to_string()),
                );
                if let Some(ref d) = self.dashboard {
                    d.task_failed(&outcome.id, outcome.duration_ms);
                }
                let deps = transitive_dependents(&outcome.id, &states, &tasks_map);
                for dep_id in deps {
                    warn!(
                        "[SKIP] '{}' skipped because '{}' failed",
                        dep_id, outcome.id
                    );
                    if let Some(ref d) = self.dashboard {
                        d.task_cancelled(&dep_id);
                    }
                    task_timings.insert(dep_id.clone(), (0, "cancelled".to_string()));
                    states.insert(dep_id, TaskState::Failed);
                }
                TaskState::Failed
            };
            states.insert(outcome.id, new_state);

            // Collect ready tasks (checking conditions)
            let ready_candidates = collect_ready(&states, &tasks_map);
            for id in ready_candidates {
                let task = &tasks_map[&id];
                if let Some(ref cond) = task.condition {
                    let env = resolved_envs.get(&id).cloned().unwrap_or_default();
                    if !evaluate_condition(cond, &env, &states) {
                        if !quiet {
                            println!(
                                "{}[SKIP] Task '{}' condition evaluated to false: {}",
                                prefix, id, cond
                            );
                        }
                        states.insert(id.clone(), TaskState::ConditionSkip);
                        task_timings.insert(id.clone(), (0, "condition_skip".to_string()));
                        if let Some(ref d) = self.dashboard {
                            d.task_condition_skipped(&id);
                        }
                        continue;
                    }
                }

                states.insert(id.clone(), TaskState::Running);
                running += 1;
                if let Some(ref d) = self.dashboard {
                    d.task_started(&id);
                }
                spawn_task(
                    tasks_map[&id].clone(),
                    tx.clone(),
                    sem.clone(),
                    cache.clone(),
                    prefix.clone(),
                    quiet,
                    resolved_envs.clone(),
                    task_configs.clone(),
                    task_outputs.clone(),
                    self.log_writer.clone(),
                );
            }

            // Check if there are still pending tasks that might become ready
            // (needed when condition-skipped tasks free up downstream tasks)
            if running == 0 {
                let more_ready = collect_ready(&states, &tasks_map);
                if more_ready.is_empty() {
                    break;
                }
                for id in more_ready {
                    let task = &tasks_map[&id];
                    if let Some(ref cond) = task.condition {
                        let env = resolved_envs.get(&id).cloned().unwrap_or_default();
                        if !evaluate_condition(cond, &env, &states) {
                            states.insert(id.clone(), TaskState::ConditionSkip);
                            task_timings.insert(id.clone(), (0, "condition_skip".to_string()));
                            if let Some(ref d) = self.dashboard {
                                d.task_condition_skipped(&id);
                            }
                            continue;
                        }
                    }
                    states.insert(id.clone(), TaskState::Running);
                    running += 1;
                    if let Some(ref d) = self.dashboard {
                        d.task_started(&id);
                    }
                    spawn_task(
                        tasks_map[&id].clone(),
                        tx.clone(),
                        sem.clone(),
                        cache.clone(),
                        prefix.clone(),
                        quiet,
                        resolved_envs.clone(),
                        task_configs.clone(),
                        task_outputs.clone(),
                        self.log_writer.clone(),
                    );
                }
            }
        }

        let n_failed = states.values().filter(|s| **s == TaskState::Failed).count();
        let n_pending = states
            .values()
            .filter(|s| **s == TaskState::Pending)
            .count();
        let n_condition_skip = states
            .values()
            .filter(|s| **s == TaskState::ConditionSkip)
            .count();
        let duration_ms = start.elapsed().as_millis() as u64;
        let pipeline_success = n_failed == 0 && n_pending == 0;

        if n_failed > 0 {
            error!("{} task(s) failed", n_failed);
        }

        // Freeze the TUI dashboard
        if let Some(ref d) = self.dashboard {
            d.finish(pipeline_success);
        }

        // ── Write run report ────────────────────────────────────────────────
        let report = RunReport::new(
            &self.pipeline_name,
            pipeline_success,
            total_tasks,
            cached_tasks,
            failed_tasks,
            n_condition_skip,
            duration_ms,
            &task_timings,
            &self.pipeline.tasks,
        );
        if let Err(e) = report.save() {
            warn!("failed to save run report: {}", e);
        }

        // Print timing summary (non-TUI mode)
        if !quiet && total_tasks > 0 {
            report.print_timing_summary();
        }

        // Emit pipeline_completed
        if let Some(ref r) = self.reporter {
            let user_login = crate::config::load()
                .map(|c| c.user_login)
                .unwrap_or_else(|| "cli".to_string());
            // Give in-flight task events a moment to land
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            r.send(Event::pipeline_completed(PipelineCompletedArgs {
                id: &self.pipeline_id,
                name: &self.pipeline_name,
                success: pipeline_success,
                total_tasks,
                cached_tasks,
                failed_tasks,
                duration_ms,
                user_login: &user_login,
            }));
            // Give the completed event a moment to send before process exits
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        }

        Ok(pipeline_success)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn rand_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}

fn collect_ready(
    states: &HashMap<String, TaskState>,
    tasks_map: &HashMap<String, Task>,
) -> Vec<String> {
    states
        .iter()
        .filter_map(|(id, s)| {
            if *s != TaskState::Pending {
                return None;
            }
            let task = tasks_map.get(id)?;
            let ready = task.depends_on.iter().all(|dep| {
                matches!(
                    states.get(dep),
                    Some(TaskState::Success)
                        | Some(TaskState::Skipped)
                        | Some(TaskState::ConditionSkip)
                )
            });
            if ready { Some(id.clone()) } else { None }
        })
        .collect()
}

fn transitive_dependents(
    root: &str,
    states: &HashMap<String, TaskState>,
    tasks_map: &HashMap<String, Task>,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut frontier = vec![root.to_string()];
    let mut seen: std::collections::HashSet<String> = std::iter::once(root.to_string()).collect();

    while let Some(current) = frontier.pop() {
        for (id, state) in states {
            if *state != TaskState::Pending || seen.contains(id) {
                continue;
            }
            if tasks_map
                .get(id)
                .map(|t| t.depends_on.contains(&current))
                .unwrap_or(false)
            {
                seen.insert(id.clone());
                result.push(id.clone());
                frontier.push(id.clone());
            }
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn spawn_task(
    task: Task,
    tx: mpsc::Sender<TaskOutcome>,
    sem: Arc<Semaphore>,
    cache: Arc<Mutex<Cache>>,
    prefix: String,
    quiet: bool,
    resolved_envs: Arc<HashMap<String, HashMap<String, String>>>,
    task_configs: Arc<TaskConfigMap>,
    task_outputs: Arc<Mutex<HashMap<String, TaskOutputs>>>,
    log_writer: Option<LogWriter>,
) {
    tokio::spawn(async move {
        let _permit = sem.acquire().await.expect("semaphore closed");

        let hit = {
            let c = cache.lock().await;
            task.hash
                .as_deref()
                .map(|h| c.is_hit(&task.id, h))
                .unwrap_or(false)
        };

        if hit {
            if !quiet {
                println!("{}[CACHE HIT] Skipping task: {}", prefix, task.id);
            }
            let _ = tx
                .send(TaskOutcome {
                    id: task.id,
                    success: false,
                    skipped: true,
                    condition_skip: false,
                    duration_ms: 0,
                    outputs: HashMap::new(),
                })
                .await;
            return;
        }

        let empty_env = HashMap::new();
        let env = resolved_envs.get(&task.id).unwrap_or(&empty_env);

        // Resolve task output references in env values
        let outputs_snapshot = task_outputs.lock().await.clone();
        let mut resolved_env = env.clone();
        for (_key, value) in resolved_env.iter_mut() {
            if value.contains("${{") {
                *value = resolve_task_output_refs(value, Some(&outputs_snapshot));
            }
        }

        let (retries, retry_delay, timeout, output_names) = task_configs
            .get(&task.id)
            .cloned()
            .unwrap_or((2, None, None, Vec::new()));

        let t0 = Instant::now();
        let (success, captured_outputs) = executor::execute_task(
            &task,
            &prefix,
            quiet,
            &resolved_env,
            retries,
            retry_delay.as_ref(),
            timeout,
            &output_names,
            log_writer.as_ref(),
        )
        .await
        .unwrap_or_else(|e| {
            error!("task '{}' I/O error: {}", task.id, e);
            (false, HashMap::new())
        });
        let duration_ms = t0.elapsed().as_millis() as u64;

        if success && let Some(hash) = task.hash.clone() {
            let mut c = cache.lock().await;
            c.record(task.id.clone(), hash, true);
            if let Err(e) = c.save() {
                warn!("cache save error: {}", e);
            }
        }

        let _ = tx
            .send(TaskOutcome {
                id: task.id,
                success,
                skipped: false,
                condition_skip: false,
                duration_ms,
                outputs: captured_outputs,
            })
            .await;
    });
}
