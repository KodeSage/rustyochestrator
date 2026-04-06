use crate::pipeline::Task;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const REPORT_DIR: &str = ".rustyochestrator";
const REPORT_FILE: &str = ".rustyochestrator/last-run.json";

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    pub id: String,
    pub status: String,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub pipeline_name: String,
    pub success: bool,
    pub total_tasks: usize,
    pub cached_tasks: usize,
    pub failed_tasks: usize,
    pub condition_skipped: usize,
    pub total_duration_ms: u64,
    pub tasks: Vec<TaskReport>,
    pub timestamp: String,
}

impl RunReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pipeline_name: &str,
        success: bool,
        total_tasks: usize,
        cached_tasks: usize,
        failed_tasks: usize,
        condition_skipped: usize,
        total_duration_ms: u64,
        task_timings: &HashMap<String, (u64, String)>,
        pipeline_tasks: &[Task],
    ) -> Self {
        let mut tasks: Vec<TaskReport> = pipeline_tasks
            .iter()
            .map(|t| {
                let (duration_ms, status) = task_timings
                    .get(&t.id)
                    .cloned()
                    .unwrap_or((0, "pending".to_string()));
                TaskReport {
                    id: t.id.clone(),
                    status,
                    duration_ms,
                    depends_on: t.depends_on.clone(),
                    command: Some(t.command.clone()),
                }
            })
            .collect();

        // Sort by duration descending (slowest first) for easy bottleneck identification
        tasks.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));

        Self {
            pipeline_name: pipeline_name.to_string(),
            success,
            total_tasks,
            cached_tasks,
            failed_tasks,
            condition_skipped,
            total_duration_ms,
            tasks,
            timestamp: now_iso(),
        }
    }

    /// Save the report to `.rustyochestrator/last-run.json`.
    pub fn save(&self) -> crate::errors::Result<()> {
        std::fs::create_dir_all(REPORT_DIR)?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(REPORT_FILE, json)?;
        Ok(())
    }

    /// Load the last run report from disk.
    pub fn load() -> crate::errors::Result<Self> {
        let raw = std::fs::read_to_string(REPORT_FILE)?;
        let report: Self = serde_json::from_str(&raw)?;
        Ok(report)
    }

    /// Print a timing summary to stdout, highlighting the slowest tasks.
    pub fn print_timing_summary(&self) {
        println!("\n  ── Run Summary ──────────────────────────────────────────────");
        println!(
            "  Pipeline: {}  Status: {}  Duration: {}",
            self.pipeline_name,
            if self.success { "passed" } else { "FAILED" },
            fmt_ms(self.total_duration_ms),
        );
        println!(
            "  Tasks: {} total, {} cached, {} failed, {} skipped",
            self.total_tasks, self.cached_tasks, self.failed_tasks, self.condition_skipped,
        );

        if !self.tasks.is_empty() {
            println!("\n  {:<32} {:>10}  Status", "Task", "Duration");
            println!("  {}", "-".repeat(60));

            for task in &self.tasks {
                let dur = fmt_ms(task.duration_ms);
                let marker = match task.status.as_str() {
                    "success" => "",
                    "cached" => " [cached]",
                    "failed" => " << FAILED",
                    "cancelled" => " [cancelled]",
                    "condition_skip" => " [if: skipped]",
                    _ => "",
                };
                println!("  {:<32} {:>10}  {}{}", task.id, dur, task.status, marker);
            }

            // Highlight slowest task as bottleneck
            let slowest = self
                .tasks
                .iter()
                .filter(|t| t.status == "success" || t.status == "failed")
                .max_by_key(|t| t.duration_ms);
            if let Some(s) = slowest
                && s.duration_ms > 0
            {
                println!(
                    "\n  Bottleneck: '{}' took {} ({:.0}% of total)",
                    s.id,
                    fmt_ms(s.duration_ms),
                    if self.total_duration_ms > 0 {
                        (s.duration_ms as f64 / self.total_duration_ms as f64) * 100.0
                    } else {
                        0.0
                    }
                );
            }
        }
        println!();
    }

    /// Print the report as formatted Markdown.
    pub fn print_markdown(&self) {
        println!("# Pipeline Report: {}", self.pipeline_name);
        println!();
        println!(
            "- **Status:** {}",
            if self.success { "Passed" } else { "Failed" }
        );
        println!("- **Duration:** {}", fmt_ms(self.total_duration_ms));
        println!("- **Timestamp:** {}", self.timestamp);
        println!(
            "- **Tasks:** {} total | {} cached | {} failed | {} skipped",
            self.total_tasks, self.cached_tasks, self.failed_tasks, self.condition_skipped
        );
        println!();
        println!("| Task | Duration | Status |");
        println!("|------|----------|--------|");
        for task in &self.tasks {
            println!(
                "| {} | {} | {} |",
                task.id,
                fmt_ms(task.duration_ms),
                task.status
            );
        }
        println!();

        let slowest = self
            .tasks
            .iter()
            .filter(|t| t.status == "success" || t.status == "failed")
            .max_by_key(|t| t.duration_ms);
        if let Some(s) = slowest
            && s.duration_ms > 0
        {
            println!(
                "**Bottleneck:** `{}` — {} ({:.0}% of total)",
                s.id,
                fmt_ms(s.duration_ms),
                if self.total_duration_ms > 0 {
                    (s.duration_ms as f64 / self.total_duration_ms as f64) * 100.0
                } else {
                    0.0
                }
            );
        }
    }
}

fn fmt_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let s = ms / 1000;
        format!("{}m {}s", s / 60, s % 60)
    }
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let sec = secs % 60;
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let mut days = (secs / 86400) as u32;

    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = [
        31u32,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &len in &month_lengths {
        if days < len {
            break;
        }
        days -= len;
        month += 1;
    }
    let day = days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, min, sec
    )
}

#[inline]
fn is_leap(year: u32) -> bool {
    year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100))
}
