# Changelog

All notable changes to Rusty Orchestrator are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.4] — 2026-04-06

A major feature release adding task timeouts, configurable retries, output capture,
conditional execution, matrix builds, artifact storage, run reports, and new CLI flags.

### Added

- **Task Timeouts** — optional `timeout` field per task (e.g. `timeout: "300s"`, `"5m"`, `"1h"`).
  Subprocess is killed and marked failed when the timeout is exceeded.
  Pipeline-level `defaults.timeout` serves as fallback for all tasks.

- **Configurable Retry Count** — `retries: N` per task replaces the previously
  hardcoded 2-retry limit. Pipeline-level `defaults.retries` for global override.

- **Retry Delay** — `retry_delay` option supporting fixed delay (`"5s"`) or
  exponential backoff (`{ strategy: exponential, base: "1s" }`).
  Pipeline-level `defaults.retry_delay` for global override.

- **Task Output Capture & Reuse** — tasks can declare `outputs: [VERSION, BUILD_ID]`.
  Stdout lines matching `NAME=value` are captured and available to downstream tasks
  via `${{ tasks.task_id.outputs.NAME }}` in commands and env values.

- **Conditional Task Execution** — `if:` field per task with expression support.
  Evaluates env vars (`$ENV_VAR == 'production'`), task outcomes
  (`tasks.build.result == 'success'`), and boolean literals.
  Skipped tasks are shown as `[if: skipped]` and do not block dependents.

- **Matrix Build Strategy** (GitHub Actions) — parses `strategy.matrix` and expands
  the cartesian product into one task per combination. Matrix values are injected as
  env vars. `include` and `exclude` modifiers are supported.

- **Context Variable Resolution** (GitHub Actions) — resolves `${{ github.sha }}`,
  `${{ github.ref }}`, `${{ github.ref_name }}`, `${{ github.repository }}`,
  `${{ github.actor }}`, `${{ github.workspace }}`, and `${{ env.* }}` from
  git state and the shell environment.

- **Conditional Steps** (GitHub Actions) — evaluates `if:` expressions on jobs and
  steps. Supports `success()`, `failure()`, `always()`, `cancelled()` status
  functions and simple boolean/string comparisons.

- **Reusable Actions Warnings** (GitHub Actions) — replaces silent `uses:` skipping
  with visible per-step warnings. Known no-op stubs for `actions/checkout`,
  `dtolnay/rust-toolchain`, and `actions/cache`. All other unrecognised `uses:`
  steps emit a named warning and are skipped; the run continues.

- **Local Artifact Store** (GitHub Actions) — `actions/upload-artifact` and
  `actions/download-artifact` are emulated via a scoped temp directory at
  `.rustyochestrator/artifacts/<run-id>/<name>/`. Artifacts are cleaned up
  automatically; `--keep-artifacts` preserves them for debugging.

- **Structured Pipeline Report** — after every run, a JSON summary is written to
  `.rustyochestrator/last-run.json` with task durations, cache hits, failures, and
  a timestamp. New `rustyochestrator report` command displays the last run summary
  with `--json` and `--markdown` output options.

- **Task Timing Breakdown** — per-task start/end timestamps recorded and displayed
  in the TUI and final summary. Slowest task highlighted as a bottleneck candidate
  with percentage of total duration.

- **`--verbose` flag** — streams all task stdout/stderr inline, forces plain output
  (disables TUI).

- **`--dry-run` flag** — prints the full execution plan (stages, commands, retries,
  timeouts, conditions) without running anything.

- **`--trace-deps` flag** — shows dependency resolution steps before execution,
  listing which tasks block which and when each becomes ready.

- **`--log-file <path>` flag** — writes combined stdout/stderr of all tasks to a
  file for CI artifacts and post-mortem debugging.

- **Pipeline-level `defaults` block** — new top-level YAML key for setting default
  `timeout`, `retries`, and `retry_delay` across all tasks.

- **`ConditionSkip` task state** — new state for tasks skipped by `if:` conditions,
  distinct from cache-skip and failure-skip. Displayed as `[if: skipped]` in TUI.

### Changed

- `execute_task()` now returns `(bool, TaskOutputs)` instead of `bool`, enabling
  output capture without a separate collection mechanism.
- The scheduler resolves `${{ tasks.X.outputs.Y }}` references in env values at
  task spawn time using a shared output store.
- `collect_ready()` now treats `ConditionSkip` as a completed state, so downstream
  tasks of condition-skipped tasks can proceed.
- The `init` command scaffold now includes commented examples of the new fields
  (`timeout`, `retries`, `retry_delay`, `if`, `outputs`).
- `validate` command output now shows timeout, retries, outputs, and condition info.

---

## [0.1.3] — 2025-06-15

### Added

- **Full test suite** — 100+ unit and integration tests across all modules
  (cache, config, errors, executor, github, pipeline, reporter, scheduler).
- **Library target** (`lib.rs`) — all modules exported as a public library crate,
  enabling programmatic use of the parser, executor, and scheduler.
- **`.env` file support** — automatically loads `.env` from the current directory
  before running any pipeline. Shell exports always take precedence. Supports
  `KEY=VALUE`, `export KEY=VALUE`, and quoted values.
- **Pre-flight secret validation** — all `${{ secrets.NAME }}` references are
  resolved before any task starts. Missing secrets abort immediately with a clear
  error naming the task and key.
- **Automatic secret redaction** — debug logging redacts values whose key contains
  `SECRET`, `TOKEN`, `KEY`, or `PASSWORD`.
- **CI pipeline** (`.github/workflows/ci.yml`) — lint, build, test, and
  integration test jobs for the project itself.

### Changed

- Bumped version from 0.1.2 to 0.1.3.

---

## [0.1.2] — 2025-05-28

### Added

- **Live TUI dashboard** — colour-coded per-task progress with Unicode spinners,
  elapsed time, and a summary progress bar. Auto-detects TTY; falls back to plain
  log output in CI or when piped. `--no-tui` flag to force plain output.
- **`run-all` command** — discovers every `.yml`/`.yaml` file in a directory and
  runs them all concurrently, each with its own independent DAG scheduler and cache.
  Prefixes output with workflow filename.
- **Environment variables & secrets** — `env:` blocks at pipeline and task level.
  `${{ secrets.NAME }}` references resolved from the shell environment at runtime.
  Cache invalidation on env value change.
- **Retry logic** — failed tasks retried up to 2 times before being marked failed.
- **Failure propagation** — when a task fails, its entire transitive dependent
  subtree is cancelled immediately.
- **Real-time output streaming** — stdout and stderr from every task streamed
  line-by-line via concurrent draining to avoid pipe-buffer deadlocks.
- **Dashboard integration** — optional connection to
  [Dashhy](https://github.com/KodeSage/Dashhy) for live pipeline event streaming.
  `connect`, `disconnect`, and `status` commands.
- **`indicatif` dependency** for progress bar rendering.
- **`reqwest` dependency** for dashboard HTTP reporting.

### Changed

- Bumped version from 0.1.0 to 0.1.2.

---

## [0.1.0] — 2025-05-20

### Added

- **Initial release** of Rusty Orchestrator.
- **YAML pipeline parser** — define tasks with `id`, `command`, and `depends_on`.
- **DAG scheduler** — directed acyclic graph resolution with cycle detection
  (3-colour DFS). Tasks grouped into parallel stages by depth level.
- **Parallel execution** — Tokio-based worker pool with configurable concurrency
  (defaults to number of logical CPUs).
- **Content-addressable cache** — SHA-256 hash of command + dependencies + env.
  Unchanged tasks skipped instantly. Atomic file writes for cache persistence.
- **GitHub Actions compatibility** — parse and run `.github/workflows/*.yml` files.
  `run:` steps become tasks, `needs:` wires cross-job dependencies, `uses:` steps
  silently skipped.
- **CLI commands** — `run`, `validate`, `list`, `graph`, `cache show`, `cache clean`,
  `init`.
- **One-liner installer** (`install.sh`) and pre-built binary support.
- Published to [crates.io](https://crates.io/crates/rustyochestrator).

[0.1.4]: https://github.com/KodeSage/rustyochestrator/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/KodeSage/rustyochestrator/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/KodeSage/rustyochestrator/compare/v0.1.0...v0.1.2
[0.1.0]: https://github.com/KodeSage/rustyochestrator/releases/tag/v0.1.0
