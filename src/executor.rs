use crate::errors::{Result, RustyError};
use crate::pipeline::{RetryDelay, Task};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

/// Returns `true` if the env key looks like a secret (should be redacted in logs).
fn is_sensitive(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    k.contains("SECRET") || k.contains("TOKEN") || k.contains("KEY") || k.contains("PASSWORD")
}

// ── Output capture ───────────────────────────────────────────────────────────

/// Captured output values from a task (NAME=value lines written to stdout).
pub type TaskOutputs = HashMap<String, String>;

// ── Log file writer ──────────────────────────────────────────────────────────

/// Shared log file handle for writing combined output.
pub type LogWriter = Arc<Mutex<std::fs::File>>;

pub fn create_log_writer(path: &str) -> std::io::Result<LogWriter> {
    let file = std::fs::File::create(path)?;
    Ok(Arc::new(Mutex::new(file)))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Execute a task with configurable retries and optional timeout.
///
/// Returns `(success: bool, captured_outputs: TaskOutputs)`.
#[allow(clippy::too_many_arguments)]
pub async fn execute_task(
    task: &Task,
    prefix: &str,
    quiet: bool,
    env: &HashMap<String, String>,
    retries: u32,
    retry_delay: Option<&RetryDelay>,
    timeout: Option<std::time::Duration>,
    output_names: &[String],
    log_writer: Option<&LogWriter>,
) -> Result<(bool, TaskOutputs)> {
    // Debug-log env keys so users can verify what was passed (values redacted for secrets).
    if !env.is_empty() {
        for (k, v) in env {
            if is_sensitive(k) {
                tracing::debug!(task = task.id.as_str(), key = k, value = "***", "env var");
            } else {
                tracing::debug!(task = task.id.as_str(), key = k, value = v, "env var");
            }
        }
    }

    let total_attempts = retries + 1;
    for attempt in 1..=total_attempts {
        if attempt > 1 {
            // Apply retry delay
            if let Some(delay) = retry_delay {
                let wait = delay.delay_for_attempt(attempt - 2); // 0-indexed attempt
                tracing::debug!(
                    task = task.id.as_str(),
                    attempt,
                    delay_ms = wait.as_millis() as u64,
                    "retry delay"
                );
                tokio::time::sleep(wait).await;
            }

            if !quiet {
                println!(
                    "{}[RETRY] Attempt {}/{} for task '{}'",
                    prefix, attempt, total_attempts, task.id
                );
            }
            if let Some(lw) = log_writer {
                let mut f = lw.lock().await;
                use std::io::Write;
                let _ = writeln!(
                    f,
                    "{}[RETRY] Attempt {}/{} for task '{}'",
                    prefix, attempt, total_attempts, task.id
                );
            }
        }

        let result = if let Some(dur) = timeout {
            match tokio::time::timeout(
                dur,
                run_command(
                    &task.id,
                    &task.command,
                    prefix,
                    quiet,
                    env,
                    output_names,
                    log_writer,
                ),
            )
            .await
            {
                Ok(inner) => inner,
                Err(_) => {
                    // Timeout expired
                    let msg = format!(
                        "{}[TIMEOUT] Task '{}' exceeded timeout of {:.0}s",
                        prefix,
                        task.id,
                        dur.as_secs_f64()
                    );
                    if !quiet {
                        println!("{}", msg);
                    }
                    if let Some(lw) = log_writer {
                        let mut f = lw.lock().await;
                        use std::io::Write;
                        let _ = writeln!(f, "{}", msg);
                    }
                    tracing::error!(
                        task = task.id.as_str(),
                        timeout_secs = dur.as_secs(),
                        "timeout"
                    );
                    Ok((false, HashMap::new()))
                }
            }
        } else {
            run_command(
                &task.id,
                &task.command,
                prefix,
                quiet,
                env,
                output_names,
                log_writer,
            )
            .await
        };

        match result {
            Ok((true, outputs)) => return Ok((true, outputs)),

            Ok((false, _)) if attempt < total_attempts => {
                if !quiet {
                    println!("{}[WARN] Task '{}' failed, retrying…", prefix, task.id);
                }
                if let Some(lw) = log_writer {
                    let mut f = lw.lock().await;
                    use std::io::Write;
                    let _ = writeln!(f, "{}[WARN] Task '{}' failed, retrying…", prefix, task.id);
                }
                continue;
            }
            Ok((false, outputs)) => {
                if !quiet {
                    println!(
                        "{}[FAILED] Task '{}' failed after {} attempt(s)",
                        prefix, task.id, attempt
                    );
                }
                if let Some(lw) = log_writer {
                    let mut f = lw.lock().await;
                    use std::io::Write;
                    let _ = writeln!(
                        f,
                        "{}[FAILED] Task '{}' failed after {} attempt(s)",
                        prefix, task.id, attempt
                    );
                }
                return Ok((false, outputs));
            }

            Err(e) if attempt < total_attempts => {
                if !quiet {
                    println!(
                        "{}[WARN] Task '{}' error: {} – retrying…",
                        prefix, task.id, e
                    );
                }
                if let Some(lw) = log_writer {
                    let mut f = lw.lock().await;
                    use std::io::Write;
                    let _ = writeln!(
                        f,
                        "{}[WARN] Task '{}' error: {} – retrying…",
                        prefix, task.id, e
                    );
                }
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok((false, HashMap::new())) // unreachable, but satisfies the type checker
}

// ── Internal ──────────────────────────────────────────────────────────────────

async fn run_command(
    task_id: &str,
    command: &str,
    prefix: &str,
    quiet: bool,
    env: &HashMap<String, String>,
    output_names: &[String],
    log_writer: Option<&LogWriter>,
) -> Result<(bool, TaskOutputs)> {
    if !quiet {
        println!("{}[INFO] Starting task: {}", prefix, task_id);
    }
    if let Some(lw) = log_writer {
        let mut f = lw.lock().await;
        use std::io::Write;
        let _ = writeln!(f, "{}[INFO] Starting task: {}", prefix, task_id);
    }
    tracing::info!(task = task_id, "starting");

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .envs(env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(RustyError::Io)?;

    // Drain stdout and stderr concurrently to avoid pipe-buffer deadlocks.
    let stdout = child.stdout.take().expect("stdout should be piped");
    let stderr = child.stderr.take().expect("stderr should be piped");

    let tid_out = task_id.to_string();
    let tid_err = task_id.to_string();
    let pfx_out = prefix.to_string();
    let pfx_err = prefix.to_string();
    let capture_names: HashSet<String> = output_names.iter().cloned().collect();
    let captured: Arc<Mutex<TaskOutputs>> = Arc::new(Mutex::new(HashMap::new()));
    let captured_clone = captured.clone();
    let lw_out = log_writer.cloned();
    let lw_err = log_writer.cloned();

    let h_out = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // Check for output capture: lines matching NAME=value
            if !capture_names.is_empty()
                && let Some(eq_pos) = line.find('=')
            {
                let name = &line[..eq_pos];
                if capture_names.contains(name) {
                    let value = line[eq_pos + 1..].to_string();
                    captured_clone.lock().await.insert(name.to_string(), value);
                }
            }

            if !quiet {
                println!("{}  [{}] {}", pfx_out, tid_out, line);
            }
            if let Some(ref lw) = lw_out {
                let mut f = lw.lock().await;
                use std::io::Write;
                let _ = writeln!(f, "{}  [{}] {}", pfx_out, tid_out, line);
            }
        }
    });

    let h_err = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !quiet {
                eprintln!("{}  [{}|err] {}", pfx_err, tid_err, line);
            }
            if let Some(ref lw) = lw_err {
                let mut f = lw.lock().await;
                use std::io::Write;
                let _ = writeln!(f, "{}  [{}|err] {}", pfx_err, tid_err, line);
            }
        }
    });

    // Wait for all output to be consumed, then collect exit status.
    let _ = tokio::join!(h_out, h_err);
    let status = child.wait().await.map_err(RustyError::Io)?;

    let outputs = match Arc::try_unwrap(captured) {
        Ok(mutex) => mutex.into_inner(),
        Err(arc) => arc.blocking_lock().clone(),
    };

    if status.success() {
        if !quiet {
            println!("{}[INFO] Completed task: {}", prefix, task_id);
        }
        if let Some(lw) = log_writer {
            let mut f = lw.lock().await;
            use std::io::Write;
            let _ = writeln!(f, "{}[INFO] Completed task: {}", prefix, task_id);
        }
        tracing::info!(task = task_id, "completed");
        Ok((true, outputs))
    } else {
        if !quiet {
            println!(
                "{}[FAIL] Task '{}' exited with status {}",
                prefix, task_id, status
            );
        }
        if let Some(lw) = log_writer {
            let mut f = lw.lock().await;
            use std::io::Write;
            let _ = writeln!(
                f,
                "{}[FAIL] Task '{}' exited with status {}",
                prefix, task_id, status
            );
        }
        tracing::error!(task = task_id, %status, "failed");
        Ok((false, outputs))
    }
}

use std::collections::HashSet;
