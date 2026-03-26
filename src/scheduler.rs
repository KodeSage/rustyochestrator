use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, Semaphore};
use tracing::{error, info, warn};

use crate::cache::Cache;
use crate::errors::Result;
use crate::executor::execute_task;
use crate::pipeline::{Pipeline, Task, TaskState};
use crate::reporter::{Event, PipelineCompletedArgs, Reporter};
use crate::tui::Dashboard;

// ── Channel message ───────────────────────────────────────────────────────────

#[derive(Debug)]
struct TaskOutcome {
    id: String,
    success: bool,
    skipped: bool, // cache hit
    duration_ms: u64,
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

pub struct Scheduler {
    pipeline: Pipeline,
    concurrency: usize,
    reporter: Option<Reporter>,
    dashboard: Option<Arc<Dashboard>>,
    pipeline_id: String,
    pipeline_name: String,
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

    /// Run the pipeline and return `true` if every task succeeded / was skipped.
    pub async fn run(self) -> Result<bool> {
        let cache = Arc::new(Mutex::new(Cache::load()?));
        let sem = Arc::new(Semaphore::new(self.concurrency));

        let tasks_map: Arc<HashMap<String, Task>> = Arc::new(
            self.pipeline
                .tasks
                .iter()
                .map(|t| (t.id.clone(), t.clone()))
                .collect(),
        );

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

        // Emit pipeline_started
        if let Some(ref r) = self.reporter {
            // get user_login from config
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
            );
        }

        if running == 0 {
            return Ok(true);
        }

        // Main event loop
        while running > 0 {
            let outcome = match rx.recv().await {
                Some(msg) => msg,
                None => break,
            };
            running -= 1;

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

            let new_state = if outcome.skipped {
                info!("task '{}' skipped (cache hit)", outcome.id);
                cached_tasks += 1;
                if let Some(ref d) = self.dashboard {
                    d.task_completed(&outcome.id, outcome.duration_ms, true);
                }
                TaskState::Skipped
            } else if outcome.success {
                info!("task '{}' succeeded", outcome.id);
                if let Some(ref d) = self.dashboard {
                    d.task_completed(&outcome.id, outcome.duration_ms, false);
                }
                TaskState::Success
            } else {
                error!("task '{}' failed", outcome.id);
                failed_tasks += 1;
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
                    states.insert(dep_id, TaskState::Failed);
                }
                TaskState::Failed
            };
            states.insert(outcome.id, new_state);

            let ready = collect_ready(&states, &tasks_map);
            for id in ready {
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
                );
            }
        }

        let n_failed = states.values().filter(|s| **s == TaskState::Failed).count();
        let n_pending = states
            .values()
            .filter(|s| **s == TaskState::Pending)
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
                    Some(TaskState::Success) | Some(TaskState::Skipped)
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

fn spawn_task(
    task: Task,
    tx: mpsc::Sender<TaskOutcome>,
    sem: Arc<Semaphore>,
    cache: Arc<Mutex<Cache>>,
    prefix: String,
    quiet: bool,
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
                    duration_ms: 0,
                })
                .await;
            return;
        }

        let t0 = Instant::now();
        let success = execute_task(&task, &prefix, quiet)
            .await
            .unwrap_or_else(|e| {
                error!("task '{}' I/O error: {}", task.id, e);
                false
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
                duration_ms,
            })
            .await;
    });
}
