use crate::errors::{Result, RustyError};
use crate::pipeline::Task;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const MAX_RETRIES: u32 = 2;

/// Returns `true` if the env key looks like a secret (should be redacted in logs).
fn is_sensitive(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    k.contains("SECRET") || k.contains("TOKEN") || k.contains("KEY") || k.contains("PASSWORD")
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Execute a task with up to MAX_RETRIES retries. Returns `true` on success.
/// `prefix` is prepended to every log line (e.g. `"[ci] "` for workflow isolation).
/// `quiet` suppresses all stdout/stderr output (used when the TUI dashboard is active).
/// `env` is the fully resolved (secrets substituted) environment map for the process.
pub async fn execute_task(
    task: &Task,
    prefix: &str,
    quiet: bool,
    env: &HashMap<String, String>,
) -> Result<bool> {
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

    for attempt in 1..=(MAX_RETRIES + 1) {
        if attempt > 1 && !quiet {
            println!(
                "{}[RETRY] Attempt {}/{} for task '{}'",
                prefix,
                attempt,
                MAX_RETRIES + 1,
                task.id
            );
        }

        match run_command(&task.id, &task.command, prefix, quiet, env).await {
            Ok(true) => return Ok(true),

            Ok(false) if attempt <= MAX_RETRIES => {
                if !quiet {
                    println!("{}[WARN] Task '{}' failed, retrying…", prefix, task.id);
                }
                continue;
            }
            Ok(false) => {
                if !quiet {
                    println!(
                        "{}[FAILED] Task '{}' failed after {} attempt(s)",
                        prefix, task.id, attempt
                    );
                }
                return Ok(false);
            }

            Err(e) if attempt <= MAX_RETRIES => {
                if !quiet {
                    println!(
                        "{}[WARN] Task '{}' error: {} – retrying…",
                        prefix, task.id, e
                    );
                }
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(false) // unreachable, but satisfies the type checker
}

// ── Internal ──────────────────────────────────────────────────────────────────

async fn run_command(
    task_id: &str,
    command: &str,
    prefix: &str,
    quiet: bool,
    env: &HashMap<String, String>,
) -> Result<bool> {
    if !quiet {
        println!("{}[INFO] Starting task: {}", prefix, task_id);
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

    let h_out = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !quiet {
                println!("{}  [{}] {}", pfx_out, tid_out, line);
            }
        }
    });

    let h_err = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !quiet {
                eprintln!("{}  [{}|err] {}", pfx_err, tid_err, line);
            }
        }
    });

    // Wait for all output to be consumed, then collect exit status.
    let _ = tokio::join!(h_out, h_err);
    let status = child.wait().await.map_err(RustyError::Io)?;

    if status.success() {
        if !quiet {
            println!("{}[INFO] Completed task: {}", prefix, task_id);
        }
        tracing::info!(task = task_id, "completed");
        Ok(true)
    } else {
        if !quiet {
            println!(
                "{}[FAIL] Task '{}' exited with status {}",
                prefix, task_id, status
            );
        }
        tracing::error!(task = task_id, %status, "failed");
        Ok(false)
    }
}
