# Rusty Orchestrator

[![crates.io](https://img.shields.io/crates/v/rustyochestrator.svg)](https://crates.io/crates/rustyochestrator)
[![CI](https://github.com/KodeSage/rustyochestrator/actions/workflows/ci.yml/badge.svg)](https://github.com/KodeSage/rustyochestrator/actions)
[![license](https://img.shields.io/crates/l/rustyochestrator.svg)](LICENSE)

A high-performance, open-source CI/CD pipeline runner written in Rust.

Define your build pipeline in YAML. Rusty Orchestrator runs independent tasks in parallel, skips unchanged work via content-addressable caching, and understands GitHub Actions workflow syntax natively — all from your terminal, with no external service required.

---

## Table of Contents

- [Rusty Orchestrator](#rusty-orchestrator)
  - [Table of Contents](#table-of-contents)
  - [Why Rusty Orchestrator?](#why-rusty-orchestrator)
  - [How it works](#how-it-works)
  - [Prerequisites](#prerequisites)
  - [Installation](#installation)
    - [Option 1 — cargo install (recommended)](#option-1--cargo-install-recommended)
    - [Option 2 — pre-built binary](#option-2--pre-built-binary)
    - [Option 3 — one-liner installer](#option-3--one-liner-installer)
    - [Option 4 — build from source](#option-4--build-from-source)
  - [Quick start](#quick-start)
  - [CLI reference](#cli-reference)
    - [`run` — execute a pipeline](#run--execute-a-pipeline)
    - [`run-all` — run all workflows in a directory](#run-all--run-all-workflows-in-a-directory)
    - [`validate` — check without running](#validate--check-without-running)
    - [`list` — show execution order](#list--show-execution-order)
    - [`graph` — ASCII dependency graph](#graph--ascii-dependency-graph)
    - [`cache show` — inspect the cache](#cache-show--inspect-the-cache)
    - [`cache clean` — clear the cache](#cache-clean--clear-the-cache)
    - [`init` — scaffold a new pipeline](#init--scaffold-a-new-pipeline)
    - [`report` — view last run summary](#report--view-last-run-summary)
    - [`connect` — link to dashhy dashboard](#connect--link-to-dashhy-dashboard)
    - [`disconnect` — remove dashboard connection](#disconnect--remove-dashboard-connection)
    - [`status` — show connection status](#status--show-connection-status)
  - [Pipeline format](#pipeline-format)
    - [Task fields](#task-fields)
    - [Pipeline defaults](#pipeline-defaults)
    - [Task timeouts](#task-timeouts)
    - [Configurable retries](#configurable-retries)
    - [Task output capture & reuse](#task-output-capture--reuse)
    - [Conditional task execution](#conditional-task-execution)
    - [Environment variables & secrets](#environment-variables--secrets)
    - [GitHub Actions format](#github-actions-format)
    - [Matrix build strategy](#matrix-build-strategy)
    - [Context variable resolution](#context-variable-resolution)
    - [Conditional steps (`if:`)](#conditional-steps-if)
    - [Reusable actions support (`uses:`)](#reusable-actions-support-uses)
    - [Local artifact store](#local-artifact-store)
  - [Language examples](#language-examples)
    - [Node.js / npm](#nodejs--npm)
    - [Python](#python)
    - [Terraform / infrastructure](#terraform--infrastructure)
    - [Mixed stack](#mixed-stack)
  - [Caching](#caching)
  - [Features](#features)
  - [Contributing](#contributing)
  - [License](#license)

---

## Why Rusty Orchestrator?

Most CI tools require you to push code before you can see whether your pipeline works. Rusty Orchestrator runs the same pipeline locally — including your existing GitHub Actions workflows — so you can iterate fast before ever opening a pull request.

| Problem | How Rusty Orchestrator solves it |
| --- | --- |
| "My CI is slow" | Parallel execution + smart caching skips unchanged tasks instantly |
| "I have to push to test my workflow" | Run `.github/workflows/*.yml` files locally with one command |
| "Debugging CI failures is painful" | Live TUI with per-task progress, output streaming, and debug logging |
| "I don't want another SaaS dependency" | Fully offline — no account, no network, no dashboard required |
| "My tasks keep timing out in CI" | Per-task timeouts with subprocess kill on expiry |
| "Flaky tasks need retries" | Configurable retry count with fixed or exponential backoff delay |

---

## How it works

```
pipeline.yaml  ──►  Parser  ──►  DAG Resolver  ──►  Scheduler  ──►  Worker Pool
                                      │                                    │
                                 Cycle check                    Tokio async tasks
                                 Dep ordering                   Parallel execution
                                                                Content cache check
                                                                Timeout enforcement
                                                                Output capture
```

1. **Parse** — Rusty reads your YAML (native format or GitHub Actions) and builds a list of tasks with dependencies.
2. **Resolve** — A directed acyclic graph (DAG) is constructed. Circular dependencies are caught before anything runs.
3. **Evaluate conditions** — Tasks with `if:` expressions are evaluated. Tasks whose conditions are false are skipped without being marked as failed.
4. **Schedule** — Tasks are grouped into parallel stages. A task starts as soon as all its dependencies succeed.
5. **Cache** — Each task is identified by a SHA-256 hash of its command, dependency IDs, and env values. If the hash matches a previous successful run, the task is skipped.
6. **Execute** — Tasks run as shell subprocesses with optional timeouts. Stdout and stderr are streamed live. Configurable retries with backoff on failure.
7. **Capture** — Tasks with declared outputs capture `NAME=value` lines from stdout, making them available to downstream tasks.
8. **Report** — A JSON run summary is saved to `.rustyochestrator/last-run.json` with per-task durations, cache hits, and failure details.

---

## Prerequisites

- **Rust 1.70+** — only needed if installing via `cargo install` or building from source. Not required for pre-built binaries.

---

## Installation

### Option 1 — cargo install (recommended)

```bash
cargo install rustyochestrator
```

Upgrade to the latest version at any time:

```bash
cargo install rustyochestrator --force
```

### Option 2 — pre-built binary

Download the binary for your platform from the [latest GitHub release](https://github.com/KodeSage/rustyochestrator/releases/latest), then move it onto your PATH:

```bash
# macOS / Linux
tar xzf rustyochestrator-*.tar.gz
sudo mv rustyochestrator /usr/local/bin/

# verify
rustyochestrator --version
```

### Option 3 — one-liner installer

```bash
curl -fsSL https://github.com/KodeSage/rustyochestrator/releases/latest/download/install.sh | sh
```

### Option 4 — build from source

```bash
git clone https://github.com/KodeSage/rustyochestrator
cd rustyochestrator
cargo build --release
./target/release/rustyochestrator --version
```

---

## Quick start

**1. Scaffold a pipeline in your project:**

```bash
rustyochestrator init
```

This creates a `pipeline.yaml` in the current directory. No folder setup required.

**2. Edit the generated file** to match your project's build steps:

```yaml
tasks:
  - id: install
    command: "npm install"

  - id: lint
    command: "npm run lint"
    depends_on: [install]

  - id: test
    command: "npm test"
    depends_on: [install]

  - id: build
    command: "npm run build"
    depends_on: [lint, test]
```

**3. Run it:**

```bash
rustyochestrator run pipeline.yaml
```

**4. Run it again** — unchanged tasks are skipped from cache:

```bash
rustyochestrator run pipeline.yaml
# [CACHE HIT] Skipping task: install
# [CACHE HIT] Skipping task: lint
# [CACHE HIT] Skipping task: test
# [CACHE HIT] Skipping task: build
```

> **Note:** Rusty Orchestrator only runs commands you define. If your project needs packages installed, declare it as a task (as shown above) — nothing is auto-installed.

---

## CLI reference

### `run` — execute a pipeline

```bash
rustyochestrator run <pipeline.yaml> [OPTIONS]
```

| Flag | Description |
| --- | --- |
| `-c, --concurrency <N>` | Maximum concurrent tasks (default: num_cpus) |
| `--no-tui` | Disable the TUI dashboard and use plain log output |
| `--verbose` | Stream all task stdout/stderr inline (forces plain output) |
| `--dry-run` | Print what would execute without running anything |
| `--trace-deps` | Show dependency resolution steps before execution |
| `--log-file <path>` | Write combined task output to a file |
| `--keep-artifacts` | Keep artifacts after run completes (for debugging) |

```bash
rustyochestrator run pipeline.yaml
rustyochestrator run pipeline.yaml --concurrency 4       # limit worker count
rustyochestrator run .github/workflows/ci.yml            # GitHub Actions format
rustyochestrator run pipeline.yaml --no-tui              # force plain log output
rustyochestrator run pipeline.yaml --verbose             # stream all output inline
rustyochestrator run pipeline.yaml --dry-run             # see what would run
rustyochestrator run pipeline.yaml --trace-deps          # show dep resolution
rustyochestrator run pipeline.yaml --log-file build.log  # write output to file
RUST_LOG=debug rustyochestrator run pipeline.yaml        # verbose debug logging
```

**Dry run output:**

```
[dry-run] Would execute pipeline 'pipeline' with 4 task(s):

  Stage 0 (1 parallel):
    install → `npm install`  (retries=2)
  Stage 1 (2 parallel):
    lint → `npm run lint`  (retries=2 timeout=60s)  after: [install]
    test → `npm test`  (retries=3 timeout=300s)  after: [install]
  Stage 2 (1 parallel):
    build → `npm run build`  (retries=2)  after: [lint, test]

[dry-run] No tasks were executed.
```

When stdout is a TTY, the live TUI dashboard is shown automatically:

```
rustyochestrator — pipeline.yaml   elapsed 00:00:12

  ✓ toolchain                        0.8s   [cached]
  ✓ fmt                              1.2s
  ⠸ clippy                           12s    [running]
  ⠸ build-debug                      9s     [running]
  ◌ test                                    [waiting]
  ◌ build-release                           [waiting]
  ○ optional-check                          [if: skipped]

  ████████░░░░░░░░░░░░░░░░  2/7  2 done  2 running  3 pending  0 failed
```

In non-TTY environments (CI runners, `| tee`, `> file`) the TUI is suppressed automatically and plain log output is used. Use `--no-tui` to force plain output locally.

**Run summary** (printed after every non-TUI run):

```
  ── Run Summary ──────────────────────────────────────────────
  Pipeline: pipeline  Status: passed  Duration: 12.4s
  Tasks: 7 total, 1 cached, 0 failed, 1 skipped

  Task                             Duration  Status
  ------------------------------------------------------------
  test                                8.2s  success
  build-debug                         6.1s  success
  clippy                              5.9s  success
  fmt                                 1.2s  success
  toolchain                           0.8s  cached [cached]
  optional-check                      0ms  condition_skip [if: skipped]

  Bottleneck: 'test' took 8.2s (66% of total)
```

---

### `run-all` — run all workflows in a directory

Discovers every `.yml` and `.yaml` file in the given directory and runs them all concurrently — just like GitHub Actions fires multiple workflow files in parallel. Each workflow's output is prefixed with its filename.

```bash
rustyochestrator run-all <dir> [OPTIONS]
```

| Flag | Description |
| --- | --- |
| `-c, --concurrency <N>` | Maximum concurrent tasks per workflow (default: num_cpus) |
| `--no-tui` | Disable the TUI dashboard and use plain log output |
| `--verbose` | Stream all task stdout/stderr inline |
| `--log-file <path>` | Write combined output to a file |
| `--keep-artifacts` | Keep artifacts after run completes |

```bash
rustyochestrator run-all .github/workflows
rustyochestrator run-all ./my-pipelines
rustyochestrator run-all examples --concurrency 2
```

Example output with two workflows running simultaneously:

```
INFO  running workflows simultaneously count=2 dir=.github/workflows
[ci]      Starting task: lint__cargo_fmt___check
[release] Starting task: build__Install_cross_...
[ci]      Completed task: lint__cargo_fmt___check
[release] Completed task: build__Install_cross_...
```

- All pipelines are validated before any execution starts — parse errors surface immediately
- Each workflow runs its own independent DAG scheduler with its own cache
- Exit code is non-zero if any workflow fails
- Works with both native pipeline format and GitHub Actions format files in the same directory

---

### `validate` — check without running

Parses the file, resolves dependencies, checks for cycles. Shows timeout, retry, output, and condition info per task. Exits non-zero on any error.

```bash
rustyochestrator validate pipeline.yaml
```

```
  4 tasks
  [ok] build  (timeout: 5m, retries: 3)
  [ok] test  (needs: build, outputs: [RESULT])
  [ok] lint  (needs: build)
  [ok] deploy  (needs: test, lint, if: tasks.test.result == 'success')

  defaults: timeout: 5m, retries: 2

pipeline 'pipeline.yaml' is valid.
```

---

### `list` — show execution order

Groups tasks into parallel stages so you can see exactly what runs when.

```bash
rustyochestrator list pipeline.yaml
```

```
Execution order for 'pipeline.yaml':

  Stage 0 — 2 task(s) run in parallel:
    1. toolchain
    2. fmt

  Stage 1 — 2 task(s) run in parallel:
    3. clippy       (after: fmt)
    4. build-debug  (after: fmt)

  Stage 2 — 1 task(s) run in parallel:
    5. test  (after: build-debug, clippy)
```

---

### `graph` — ASCII dependency graph

```bash
rustyochestrator graph pipeline.yaml
```

```
Dependency graph for 'pipeline.yaml':

  Stage 0  (no deps):
    toolchain
    fmt

  Stage 1:
    clippy      ◄── [fmt]
    build-debug ◄── [fmt]

  Stage 2:
    test  ◄── [build-debug, clippy]
```

---

### `cache show` — inspect the cache

```bash
rustyochestrator cache show
```

```
  task       status     hash
  ─────────────────────────────────────────
  build      ok         b3d10802f5217f42
  clippy     ok         9eeaf9f8bc055df3
  fmt        ok         810e75f0d3dee10e
  test       ok         257080d6e3e17348

  4 cached task(s).
```

---

### `cache clean` — clear the cache

Forces every task to re-run on the next `run`.

```bash
rustyochestrator cache clean
# Cache cleared.
```

---

### `init` — scaffold a new pipeline

Creates a starter `pipeline.yaml` (or a custom filename) in the current directory. The template includes commented examples of all available fields.

```bash
rustyochestrator init                    # creates pipeline.yaml
rustyochestrator init my-pipeline.yaml  # custom filename
```

---

### `report` — view last run summary

Displays the results of the most recent pipeline run from `.rustyochestrator/last-run.json`.

```bash
rustyochestrator report               # plain text summary
rustyochestrator report --markdown    # Markdown table output
rustyochestrator report --json        # raw JSON
```

**Plain text:**

```
  ── Run Summary ──────────────────────────────────────────────
  Pipeline: pipeline  Status: passed  Duration: 12.4s
  Tasks: 5 total, 1 cached, 0 failed, 0 skipped

  Task                             Duration  Status
  ------------------------------------------------------------
  test                                8.2s  success
  build                               3.1s  success
  lint                                2.5s  success
  install                             0ms  cached [cached]

  Bottleneck: 'test' took 8.2s (66% of total)
```

**Markdown (`--markdown`):**

```markdown
# Pipeline Report: pipeline

- **Status:** Passed
- **Duration:** 12.4s
- **Tasks:** 5 total | 1 cached | 0 failed | 0 skipped

| Task | Duration | Status |
|------|----------|--------|
| test | 8.2s | success |
| build | 3.1s | success |
| lint | 2.5s | success |
| install | 0ms | cached |
```

---

### `connect` — link to dashhy dashboard

Stream live pipeline events to [Dashhy](https://github.com/KodeSage/Dashhy), the hosted monitoring UI built for Rusty Orchestrator.

```bash
rustyochestrator connect --token <jwt> --url <dashboard-url>
```

Saves the connection to `~/.rustyochestrator/connect.json`. All subsequent `run` commands will report live to the dashboard.

### `disconnect` — remove dashboard connection

```bash
rustyochestrator disconnect
```

### `status` — show connection status

```bash
rustyochestrator status
# Connected
#   Dashboard : https://your-dashhy.vercel.app
#   User      : @your-github-username
```

---

## Pipeline format

### Task fields

```yaml
tasks:
  - id: build
    command: "cargo build"
    timeout: "5m"
    retries: 3
    retry_delay: "5s"

  - id: lint
    command: "cargo clippy -- -D warnings"
    depends_on: [build]
    timeout: "2m"

  - id: test
    command: "cargo test"
    depends_on: [build]
    outputs: [TEST_COUNT, COVERAGE]

  - id: deploy
    command: "echo deploying version ${{ tasks.test.outputs.COVERAGE }}"
    depends_on: [lint, test]
    if: "tasks.test.result == 'success'"
```

| Field | Required | Description |
| --- | --- | --- |
| `id` | yes | Unique identifier for the task |
| `command` | yes | Shell command to run (executed via `sh -c`) |
| `depends_on` | no | List of task IDs that must succeed before this task starts |
| `env` | no | Map of environment variables scoped to this task |
| `timeout` | no | Maximum duration before the task is killed (e.g. `"300s"`, `"5m"`, `"1h"`, `"1h30m"`) |
| `retries` | no | Number of retries on failure (default: 2). Set to 0 for no retries |
| `retry_delay` | no | Delay between retries — a duration string or structured config (see below) |
| `outputs` | no | List of variable names to capture from stdout (`NAME=value` lines) |
| `if` | no | Condition expression — task is skipped (not failed) when false |

Multi-line commands work with YAML block scalars:

```yaml
tasks:
  - id: report
    command: |
      echo "=== build info ==="
      rustc --version
      du -sh target/release/myapp
    depends_on: [build]
```

---

### Pipeline defaults

Set default `timeout`, `retries`, and `retry_delay` for all tasks at the pipeline level. Task-level values always override these defaults.

```yaml
defaults:
  timeout: "5m"
  retries: 3
  retry_delay: "2s"

tasks:
  - id: build
    command: "cargo build"
    # inherits timeout=5m, retries=3, retry_delay=2s

  - id: test
    command: "cargo test"
    timeout: "10m"   # overrides default
    retries: 5       # overrides default
    depends_on: [build]
```

---

### Task timeouts

Timeouts kill the subprocess and mark the task as failed when the duration is exceeded. Supports `s` (seconds), `m` (minutes), and `h` (hours), including combinations:

```yaml
tasks:
  - id: quick-check
    command: "cargo clippy"
    timeout: "60s"

  - id: full-build
    command: "cargo build --release"
    timeout: "30m"

  - id: integration
    command: "./run-integration-tests.sh"
    timeout: "1h30m"
```

```
[TIMEOUT] Task 'integration' exceeded timeout of 5400s
```

---

### Configurable retries

Override the default retry count (2) per task. Use `retry_delay` for a fixed delay or exponential backoff between attempts:

```yaml
tasks:
  - id: flaky-test
    command: "npm test"
    retries: 5
    retry_delay: "3s"           # fixed 3-second delay between retries

  - id: deploy
    command: "./deploy.sh"
    retries: 3
    retry_delay:
      strategy: exponential     # 1s → 2s → 4s
      base: "1s"

  - id: critical
    command: "echo must succeed first try"
    retries: 0                  # no retries — fail immediately
```

---

### Task output capture & reuse

Tasks can export named values by printing `NAME=value` lines to stdout. Downstream tasks reference them via `${{ tasks.task_id.outputs.NAME }}`:

```yaml
tasks:
  - id: version
    command: |
      echo "VERSION=$(cat VERSION)"
      echo "BUILD_ID=$(git rev-parse --short HEAD)"
    outputs: [VERSION, BUILD_ID]

  - id: build
    command: "echo Building version ${{ tasks.version.outputs.VERSION }}"
    depends_on: [version]

  - id: tag
    command: "git tag ${{ tasks.version.outputs.VERSION }}-${{ tasks.version.outputs.BUILD_ID }}"
    depends_on: [build]
```

Only lines whose key matches a declared output name are captured. Other stdout lines are printed normally.

---

### Conditional task execution

The `if` field accepts simple expressions. When the condition evaluates to false, the task is skipped without being marked as failed — downstream tasks that depend on it can still proceed.

```yaml
tasks:
  - id: build
    command: "cargo build"

  - id: test
    command: "cargo test"
    depends_on: [build]

  - id: deploy
    command: "./deploy.sh"
    depends_on: [test]
    if: "$DEPLOY_ENV == 'production'"

  - id: notify
    command: "echo 'Tests passed'"
    depends_on: [test]
    if: "tasks.test.result == 'success'"

  - id: cleanup
    command: "echo 'Cleaning up'"
    depends_on: [test]
    if: "tasks.test.result == 'failure'"
```

**Supported expressions:**

| Expression | Description |
| --- | --- |
| `"true"` / `"false"` | Boolean literals |
| `"$ENV_VAR"` | Truthy if set and non-empty |
| `"$ENV_VAR == 'value'"` | Compare env var to string literal |
| `"$ENV_VAR != 'value'"` | Inequality check |
| `"tasks.task_id.result == 'success'"` | Check task outcome (`success`, `failure`, `skipped`) |

---

### Environment variables & secrets

Declare `env:` at the pipeline level (applied to every task) or at the task level (overrides the pipeline-level value for that task only).

```yaml
env:
  NODE_ENV: production
  API_URL: https://api.example.com

tasks:
  - id: build
    command: "npm run build"

  - id: deploy
    command: "npm run deploy"
    env:
      API_URL: https://staging.example.com  # overrides pipeline-level value
      API_KEY: "${{ secrets.DEPLOY_KEY }}"  # read from shell environment at runtime
```

**Secret references** use `${{ secrets.NAME }}` syntax. At runtime, the value is read from the process environment and passed to the task process — it is never written to disk.

**`.env` file support:** Rusty Orchestrator automatically loads a `.env` file from the current directory before running any pipeline. This means you can store secrets locally without exporting them to your shell every session:

```bash
# .env  (add to .gitignore — never commit this file)
DEPLOY_KEY=ghp_yourtoken
DATABASE_URL=postgres://localhost/mydb
```

```bash
rustyochestrator run pipeline.yaml   # DEPLOY_KEY is resolved from .env automatically
```

Precedence rules:
- A variable already exported in your shell always wins over the `.env` value
- `.env` wins over "not set at all"
- Values may be quoted with `"..."` or `'...'`; both `KEY=VALUE` and `export KEY=VALUE` are accepted

**Pre-flight validation:** all secrets are resolved before any task starts. If a referenced secret is missing from both the shell environment and `.env`, the run aborts immediately:

```
Error: secret 'DEPLOY_KEY' referenced by env key 'API_KEY' in task 'deploy' is not set in the environment
```

**Automatic redaction:** debug logging (`RUST_LOG=debug`) prints env keys but redacts values whose key contains `SECRET`, `TOKEN`, `KEY`, or `PASSWORD` (case-insensitive):

```bash
RUST_LOG=debug rustyochestrator run pipeline.yaml
# DEBUG task=deploy key=API_URL value=https://staging.example.com
# DEBUG task=deploy key=API_KEY value=***
```

**Cache invalidation:** changing any env value (including secrets) invalidates the task's cache hash, forcing a re-run.

---

### GitHub Actions format

Rusty Orchestrator can run GitHub Actions workflow files directly — useful for local testing before pushing:

```yaml
# .github/workflows/ci.yml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Compile
        run: cargo build --release

  test:
    runs-on: ubuntu-latest
    needs: [build]
    steps:
      - name: Unit tests
        run: cargo test --all
```

```bash
rustyochestrator run .github/workflows/ci.yml
```

**Mapping rules:**

- Each `run:` step becomes one task
- Steps within a job run sequentially
- `needs:` wires the first step of a downstream job to the last step of each required job
- `uses:` steps are handled with visible warnings (see [Reusable actions support](#reusable-actions-support-uses))
- `env:` blocks at the workflow, job, and step levels are parsed and merged
- `${{ secrets.NAME }}` references are forwarded; other `${{ }}` expressions are resolved where possible
- `if:` conditions on jobs and steps are evaluated
- `strategy.matrix` is expanded into parallel task combinations

---

### Matrix build strategy

Rusty Orchestrator parses `strategy.matrix` from GitHub Actions workflows and expands them into one task per combination. Matrix values are injected as environment variables.

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
        rust: [stable, nightly]
        exclude:
          - os: macos-latest
            rust: nightly
        include:
          - os: ubuntu-latest
            rust: beta
            experimental: true
    steps:
      - name: Test
        run: cargo test
```

This generates separate tasks for each matrix combination:

```
test__ubuntu-latest_stable__Test
test__ubuntu-latest_nightly__Test
test__macos-latest_stable__Test
test__ubuntu-latest_beta__Test
```

Each task receives its matrix values as environment variables (e.g. `os=ubuntu-latest`, `rust=stable`).

**Supported modifiers:**
- `include:` — adds extra combinations or extends existing ones with additional keys
- `exclude:` — removes specific combinations from the cartesian product

---

### Context variable resolution

Rusty Orchestrator resolves basic GitHub Actions context variables from git and the shell environment:

| Expression | Resolved from |
| --- | --- |
| `${{ github.sha }}` | `git rev-parse HEAD` |
| `${{ github.ref }}` | `refs/heads/<branch>` |
| `${{ github.ref_name }}` | Current branch name |
| `${{ github.repository }}` | Remote origin URL (owner/repo) |
| `${{ github.actor }}` | `$USER` / `$USERNAME` |
| `${{ github.workspace }}` | Current working directory |
| `${{ github.event_name }}` | `"local"` |
| `${{ env.NAME }}` | Environment variable |
| `${{ secrets.NAME }}` | Environment variable (kept for runtime resolution) |

Expressions that cannot be resolved locally are stripped from commands to prevent shell errors.

---

### Conditional steps (`if:`)

GitHub Actions `if:` expressions are evaluated on both jobs and steps:

```yaml
jobs:
  deploy:
    if: success()
    needs: [test]
    steps:
      - name: Deploy
        if: ${{ github.ref == 'refs/heads/main' }}
        run: ./deploy.sh
```

**Supported status functions:**

| Function | Description |
| --- | --- |
| `success()` | True when all previous steps/jobs succeeded (default) |
| `failure()` | True when any previous step/job failed |
| `always()` | Always true — run regardless of outcome |
| `cancelled()` | True when the run was cancelled |

Jobs or steps whose condition evaluates to false are skipped entirely.

---

### Reusable actions support (`uses:`)

Instead of silently skipping `uses:` steps, Rusty Orchestrator emits a visible warning for each one:

| Action | Behaviour |
| --- | --- |
| `actions/checkout@v*` | No-op with warning — working directory is already the repo |
| `dtolnay/rust-toolchain@*` | No-op with warning — assumes system Rust is installed |
| `actions/cache@v*` | No-op with warning — rustyochestrator's task-level cache covers this |
| `actions/upload-artifact@v*` | Emulated via local artifact store (see below) |
| `actions/download-artifact@v*` | Emulated via local artifact store (see below) |
| Any other `uses:` | Named warning emitted, step skipped, run continues |

```
  [warn] build/Checkout: uses: actions/checkout@v4 — no-op: working directory is already the repo
  [warn] build/Setup Rust: uses: dtolnay/rust-toolchain@stable — no-op: assumes system Rust is installed
```

---

### Local artifact store

`actions/upload-artifact` and `actions/download-artifact` are emulated with a scoped temp directory, enabling local execution of release workflows that pass files between jobs.

**How it works:**
- Artifacts are written to `.rustyochestrator/artifacts/<run-id>/<name>/` during a run
- The store is keyed by the pipeline run ID so concurrent `run` invocations don't collide
- Cleaned up automatically when the run completes; `--keep-artifacts` flag preserves them for debugging

**Upload:**
- `with.name` → artifact name (subdirectory in the store)
- `with.path` → file path or glob pattern; matched files are copied into the store

**Download:**
- `with.name` → artifact name to fetch
- `with.path` → destination directory (default: artifact name as a subdirectory)
- Fails fast with a clear error if the named artifact was never uploaded in this run

**Limitations:**
- Artifacts do not persist across separate `rustyochestrator run` invocations
- Binary files are copied as-is; no compression or size limits are enforced locally

---

## Language examples

Rusty Orchestrator is **completely language-agnostic**. The `command` field runs anything your shell can execute.

### Node.js / npm

```yaml
defaults:
  timeout: "5m"

tasks:
  - id: install
    command: "npm install"

  - id: lint
    command: "npm run lint"
    depends_on: [install]

  - id: test
    command: "npm test"
    depends_on: [install]
    retries: 3
    retry_delay: "2s"

  - id: build
    command: "npm run build"
    depends_on: [lint, test]
```

### Python

```yaml
tasks:
  - id: install
    command: "pip install -r requirements.txt"

  - id: lint
    command: "flake8 src/"
    depends_on: [install]

  - id: test
    command: "pytest tests/"
    depends_on: [install]
    timeout: "10m"
    outputs: [COVERAGE]

  - id: docker-build
    command: "docker build -t myapp:latest ."
    depends_on: [lint, test]
    if: "tasks.test.result == 'success'"
```

### Terraform / infrastructure

```yaml
tasks:
  - id: tf-init
    command: "terraform init"

  - id: tf-plan
    command: "terraform plan -out=plan.tfplan"
    depends_on: [tf-init]
    timeout: "10m"

  - id: tf-apply
    command: "terraform apply plan.tfplan"
    depends_on: [tf-plan]
    if: "$TF_AUTO_APPROVE == 'true'"
```

### Mixed stack

```yaml
defaults:
  retries: 2
  timeout: "5m"

tasks:
  - id: backend-test
    command: "cargo test"

  - id: frontend-test
    command: "npm test"

  - id: build-image
    command: "docker build -t myapp ."
    depends_on: [backend-test, frontend-test]
    timeout: "15m"

  - id: push-image
    command: "docker push myapp:latest"
    depends_on: [build-image]
    retries: 3
    retry_delay:
      strategy: exponential
      base: "2s"
```

Any command that runs in `sh -c` works — shell scripts, Python scripts, Makefiles, Docker, cloud CLIs, or anything else.

---

## Caching

Cache entries are stored in `.rustyochestrator/cache.json`.

A task is a **cache hit** when:

1. Its SHA-256 hash of `command + dependency IDs + env key/value pairs` matches the stored entry
2. The previous run recorded `success: true`

```json
{
  "entries": {
    "build": {
      "hash": "a3f1c2...",
      "success": true
    }
  }
}
```

To force a full re-run, clear the cache:

```bash
rustyochestrator cache clean
# or
rm -rf .rustyochestrator
```

---

## Features

| Feature | Description |
| --- | --- |
| **Parallel execution** | Worker pool backed by Tokio; concurrency defaults to the number of logical CPUs |
| **DAG scheduling** | Dependencies resolved at runtime; tasks start as soon as their deps finish |
| **Content-addressable cache** | Tasks hashed by command + deps + env; unchanged tasks skipped instantly |
| **Task timeouts** | Per-task timeout with subprocess kill on expiry; pipeline-level default timeout |
| **Configurable retries** | Per-task retry count with fixed or exponential backoff delay |
| **Task output capture** | Capture `NAME=value` from stdout; downstream tasks reference via `${{ tasks.id.outputs.NAME }}` |
| **Conditional execution** | `if:` field with env var comparisons and task outcome checks; skipped tasks don't block dependents |
| **GitHub Actions compatibility** | Parse and run `.github/workflows/*.yml` files directly |
| **Matrix build strategy** | Expand `strategy.matrix` into parallel task combinations with `include`/`exclude` support |
| **Context variable resolution** | Resolve `${{ github.sha }}`, `${{ github.ref }}`, `${{ env.* }}` from git and shell environment |
| **Conditional steps** | Evaluate `if:` on GitHub Actions jobs and steps with `success()`, `failure()`, `always()` |
| **Reusable actions handling** | Visible warnings for `uses:` steps; no-op stubs for common actions |
| **Local artifact store** | Emulate `actions/upload-artifact` and `actions/download-artifact` via scoped temp directory |
| **Parallel workflow execution** | `run-all` runs every workflow file in a directory simultaneously |
| **Live TUI dashboard** | Colour-coded per-task progress with spinners, elapsed time, and a summary bar |
| **CI-friendly output** | Auto-detects non-TTY environments and falls back to plain log output |
| **Run reports** | JSON/Markdown summary with per-task durations, cache hits, bottleneck identification |
| **Dry run** | `--dry-run` prints the execution plan without running anything |
| **Dependency tracing** | `--trace-deps` shows dependency resolution steps |
| **Log file output** | `--log-file` writes combined task output to a file for post-mortem debugging |
| **Environment variables & secrets** | Declare `env:` at pipeline or task level; secret refs resolved from shell env or `.env` file |
| **`.env` file support** | Automatically loads `.env` from the current directory; shell exports always take precedence |
| **Pre-flight secret validation** | All secrets validated before execution starts; missing secrets abort immediately |
| **Pipeline defaults** | Set default `timeout`, `retries`, and `retry_delay` for all tasks |
| **Retry logic** | Failed tasks retried with configurable count and delay strategy |
| **Failure propagation** | When a task fails, its entire transitive dependent subtree is cancelled |
| **Real-time output streaming** | Stdout and stderr from every task streamed line-by-line as they run |
| **Cycle detection** | Circular dependencies caught and reported before execution starts |
| **Dashboard integration** | Optional [Dashhy](https://github.com/KodeSage/Dashhy) integration streams live pipeline events to a hosted monitoring UI |

---

## Contributing

Contributions are welcome. To get started:

```bash
git clone https://github.com/KodeSage/rustyochestrator
cd rustyochestrator
cargo build
cargo test
```

Use `cargo run -- <command>` in place of the installed binary during development:

```bash
cargo run -- run examples/pipeline.yaml
cargo run -- run examples/pipeline.yaml --dry-run
cargo run -- run examples/pipeline.yaml --trace-deps
cargo run -- validate examples/pipeline.yaml
cargo run -- list examples/pipeline.yaml
cargo run -- graph examples/pipeline.yaml
cargo run -- cache show
cargo run -- init
cargo run -- run-all .github/workflows
cargo run -- report
cargo run -- report --markdown
```

Please open an issue before submitting large changes so we can align on the approach.

---

## License

MIT — see [LICENSE](LICENSE).
