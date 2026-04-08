use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── Colour helpers (ANSI) ─────────────────────────────────────────────────────

fn green(s: &str) -> String {
    format!("\x1b[32m{s}\x1b[0m")
}
fn yellow(s: &str) -> String {
    format!("\x1b[33m{s}\x1b[0m")
}
fn red(s: &str) -> String {
    format!("\x1b[31m{s}\x1b[0m")
}
fn dim(s: &str) -> String {
    format!("\x1b[2m{s}\x1b[0m")
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

pub struct Dashboard {
    #[allow(dead_code)] // keeps MultiProgress alive; rendering is driven via ProgressBar handles
    mp: MultiProgress,
    header: ProgressBar,
    /// Ordered list of task IDs (topological order for display).
    task_order: Vec<String>,
    task_bars: HashMap<String, ProgressBar>,
    summary: ProgressBar,
    total: usize,
    // Counters updated atomically from concurrent scheduler tasks.
    done: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    cached: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
}

impl Dashboard {
    /// Create a dashboard for `pipeline_name`.
    /// `task_ids` should be in topological (display) order.
    pub fn new(pipeline_name: &str, task_ids: &[String]) -> Self {
        // Draw to stdout so tracing on stderr stays separate.
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::stdout());
        let total = task_ids.len();

        // ── Header ────────────────────────────────────────────────────────────
        let header = mp.add(ProgressBar::new_spinner());
        header.set_style(ProgressStyle::with_template("{msg}").unwrap());
        // Kick off a background thread that refreshes the elapsed timer every 500 ms.
        {
            let hdr = header.clone();
            let name = pipeline_name.to_string();
            let start = Instant::now();
            std::thread::spawn(move || {
                loop {
                    if hdr.is_finished() {
                        break;
                    }
                    let e = start.elapsed().as_secs();
                    hdr.set_message(format!(
                        "rustyochestrator — {}   {}",
                        name,
                        dim(&format!(
                            "elapsed {:02}:{:02}:{:02}",
                            e / 3600,
                            (e % 3600) / 60,
                            e % 60
                        ))
                    ));
                    std::thread::sleep(Duration::from_millis(500));
                }
            });
        }

        // ── One row per task ──────────────────────────────────────────────────
        let wait_style = ProgressStyle::with_template("  {msg}").unwrap();
        let mut task_bars: HashMap<String, ProgressBar> = HashMap::new();

        for id in task_ids {
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_style(wait_style.clone());
            pb.set_message(format!("{}  {}", dim("◌"), dim(id)));
            task_bars.insert(id.clone(), pb);
        }

        // ── Summary progress bar ──────────────────────────────────────────────
        let summary = mp.add(ProgressBar::new(total as u64));
        summary.set_style(
            ProgressStyle::with_template("\n  {bar:24.cyan/white.dim}  {pos}/{len}  {msg}")
                .unwrap()
                .progress_chars("██░"),
        );
        summary.set_message(format!(
            "{}  {}  {}",
            dim("0 done"),
            dim("0 running"),
            dim("0 failed")
        ));

        Self {
            mp,
            header,
            task_order: task_ids.to_vec(),
            task_bars,
            summary,
            total,
            done: Arc::new(AtomicUsize::new(0)),
            failed: Arc::new(AtomicUsize::new(0)),
            cached: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    // ── State transitions ─────────────────────────────────────────────────────

    pub fn task_started(&self, id: &str) {
        if let Some(pb) = self.task_bars.get(id) {
            // reset() restarts the elapsed clock from now.
            pb.reset();
            pb.set_style(
                ProgressStyle::with_template("  {spinner:.cyan} {prefix:<32} {elapsed}  {msg}")
                    .unwrap()
                    .tick_strings(SPINNER),
            );
            pb.set_prefix(id.to_string());
            pb.set_message(dim("[running]"));
            pb.enable_steady_tick(Duration::from_millis(80));
        }
        self.running.fetch_add(1, Ordering::Relaxed);
        self.refresh_summary();
    }

    pub fn task_completed(&self, id: &str, duration_ms: u64, cached: bool) {
        if let Some(pb) = self.task_bars.get(id) {
            pb.set_style(ProgressStyle::with_template("  {msg}").unwrap());
            let t = fmt_ms(duration_ms);
            let msg = if cached {
                format!(
                    "{}  {:<32}  {}  {}",
                    yellow("✓"),
                    id,
                    dim(&t),
                    yellow("[cached]")
                )
            } else {
                format!("{}  {:<32}  {}", green("✓"), id, dim(&t))
            };
            pb.finish_with_message(msg);
        }
        if cached {
            self.cached.fetch_add(1, Ordering::Relaxed);
        }
        self.running.fetch_sub(1, Ordering::Relaxed);
        let done = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        self.summary.inc(1);
        self.refresh_summary_with(done);
    }

    pub fn task_failed(&self, id: &str, duration_ms: u64) {
        if let Some(pb) = self.task_bars.get(id) {
            pb.set_style(ProgressStyle::with_template("  {msg}").unwrap());
            let t = fmt_ms(duration_ms);
            pb.finish_with_message(format!(
                "{}  {:<32}  {}  {}",
                red("✗"),
                id,
                dim(&t),
                red("[failed]")
            ));
        }
        self.failed.fetch_add(1, Ordering::Relaxed);
        self.running.fetch_sub(1, Ordering::Relaxed);
        let done = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        self.summary.inc(1);
        self.refresh_summary_with(done);
    }

    /// A downstream task that was cancelled because a dependency failed (never ran).
    pub fn task_cancelled(&self, id: &str) {
        if let Some(pb) = self.task_bars.get(id) {
            pb.set_style(ProgressStyle::with_template("  {msg}").unwrap());
            pb.finish_with_message(format!(
                "{}  {}",
                dim("⊘"),
                dim(&format!("{id:<32}  [skipped]"))
            ));
        }
        self.failed.fetch_add(1, Ordering::Relaxed);
        let done = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        self.summary.inc(1);
        self.refresh_summary_with(done);
    }

    /// A task that was skipped because its `if:` condition evaluated to false.
    pub fn task_condition_skipped(&self, id: &str) {
        if let Some(pb) = self.task_bars.get(id) {
            pb.set_style(ProgressStyle::with_template("  {msg}").unwrap());
            pb.finish_with_message(format!(
                "{}  {}",
                dim("○"),
                dim(&format!("{id:<32}  [if: skipped]"))
            ));
        }
        let done = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        self.summary.inc(1);
        self.refresh_summary_with(done);
    }

    /// Freeze the dashboard with a final status line.
    pub fn finish(&self, success: bool) {
        let status = if success {
            green("✓ passed")
        } else {
            red("✗ failed")
        };
        self.header
            .finish_with_message(format!("rustyochestrator — {status}"));
        // Finish any bar that's still spinning (shouldn't happen, but be safe).
        for id in &self.task_order {
            if let Some(pb) = self.task_bars.get(id)
                && !pb.is_finished()
            {
                pb.finish();
            }
        }
        self.summary.finish();
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn refresh_summary(&self) {
        let done = self.done.load(Ordering::Relaxed);
        self.refresh_summary_with(done);
    }

    fn refresh_summary_with(&self, done: usize) {
        let failed = self.failed.load(Ordering::Relaxed);
        let running = self.running.load(Ordering::Relaxed);
        let cached = self.cached.load(Ordering::Relaxed);
        let pending = self.total.saturating_sub(done + running);
        let ok = done.saturating_sub(failed);

        let failed_str = if failed > 0 {
            red(&format!("{failed} failed"))
        } else {
            dim("0 failed")
        };

        let msg = if cached > 0 {
            format!(
                "{}  {running} running  {}  {}  {}",
                green(&format!("{ok} done")),
                dim(&format!("{pending} pending")),
                failed_str,
                yellow(&format!("{cached} cached")),
            )
        } else {
            format!(
                "{}  {running} running  {}  {}",
                green(&format!("{ok} done")),
                dim(&format!("{pending} pending")),
                failed_str,
            )
        };

        self.summary.set_message(msg);
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

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
