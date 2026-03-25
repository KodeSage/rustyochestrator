use crate::errors::{Result, RustyError};
use crate::pipeline::Task;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const MAX_RETRIES: u32 = 2;

// ── Public API ────────────────────────────────────────────────────────────────

/// Execute a task with up to MAX_RETRIES retries. Returns `true` on success.
pub async fn execute_task(task: &Task) -> Result<bool> {
    for attempt in 1..=(MAX_RETRIES + 1) {
        if attempt > 1 {
            println!(
                "[RETRY] Attempt {}/{} for task '{}'",
                attempt,
                MAX_RETRIES + 1,
                task.id
            );
        }

        match run_command(&task.id, &task.command).await {
            Ok(true) => return Ok(true),

            Ok(false) if attempt <= MAX_RETRIES => {
                println!("[WARN] Task '{}' failed, retrying…", task.id);
                continue;
            }
            Ok(false) => {
                println!(
                    "[FAILED] Task '{}' failed after {} attempt(s)",
                    task.id, attempt
                );
                return Ok(false);
            }

            Err(e) if attempt <= MAX_RETRIES => {
                println!("[WARN] Task '{}' error: {} – retrying…", task.id, e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(false) // unreachable, but satisfies the type checker
}

// ── Internal ──────────────────────────────────────────────────────────────────

async fn run_command(task_id: &str, command: &str) -> Result<bool> {
    println!("[INFO] Starting task: {}", task_id);
    tracing::info!(task = task_id, "starting");

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
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

    let h_out = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            println!("  [{}] {}", tid_out, line);
        }
    });

    let h_err = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("  [{}|err] {}", tid_err, line);
        }
    });

    // Wait for all output to be consumed, then collect exit status.
    let _ = tokio::join!(h_out, h_err);
    let status = child.wait().await.map_err(RustyError::Io)?;

    if status.success() {
        println!("[INFO] Completed task: {}", task_id);
        tracing::info!(task = task_id, "completed");
        Ok(true)
    } else {
        println!("[FAIL] Task '{}' exited with status {}", task_id, status);
        tracing::error!(task = task_id, %status, "failed");
        Ok(false)
    }
}
