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
    - [`connect` — link to dashhy dashboard](#connect--link-to-dashhy-dashboard)
    - [`disconnect` — remove dashboard connection](#disconnect--remove-dashboard-connection)
    - [`status` — show connection status](#status--show-connection-status)
  - [Pipeline format](#pipeline-format)
    - [Task fields](#task-fields)
    - [Environment variables \& secrets](#environment-variables--secrets)
    - [GitHub Actions format](#github-actions-format)
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

---

## How it works

```
pipeline.yaml  ──►  Parser  ──►  DAG Resolver  ──►  Scheduler  ──►  Worker Pool
                                      │                                    │
                                 Cycle check                    Tokio async tasks
                                 Dep ordering                   Parallel execution
                                                                Content cache check
```

1. **Parse** — Rusty reads your YAML (native format or GitHub Actions) and builds a list of tasks with dependencies.
2. **Resolve** — A directed acyclic graph (DAG) is constructed. Circular dependencies are caught before anything runs.
3. **Schedule** — Tasks are grouped into parallel stages. A task starts as soon as all its dependencies succeed.
4. **Cache** — Each task is identified by a SHA-256 hash of its command, dependency IDs, and env values. If the hash matches a previous successful run, the task is skipped.
5. **Execute** — Tasks run as shell subprocesses. Stdout and stderr are streamed live to the TUI or plain log output.

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
rustyochestrator run <pipeline.yaml> [--concurrency <N>] [--no-tui]
```

```bash
rustyochestrator run pipeline.yaml
rustyochestrator run pipeline.yaml --concurrency 4       # limit worker count
rustyochestrator run .github/workflows/ci.yml            # GitHub Actions format
rustyochestrator run pipeline.yaml --no-tui              # force plain log output
RUST_LOG=debug rustyochestrator run pipeline.yaml        # verbose debug logging
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
  ◌ smoke-test                              [waiting]

  ████████░░░░░░░░░░░░░░░░  2/7  2 done  2 running  3 pending  0 failed
```

In non-TTY environments (CI runners, `| tee`, `> file`) the TUI is suppressed automatically and plain log output is used. Use `--no-tui` to force plain output locally.

---

### `run-all` — run all workflows in a directory

Discovers every `.yml` and `.yaml` file in the given directory and runs them all concurrently — just like GitHub Actions fires multiple workflow files in parallel. Each workflow's output is prefixed with its filename.

```bash
rustyochestrator run-all <dir> [--concurrency <N>]
```

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

Parses the file, resolves dependencies, checks for cycles. Exits non-zero on any error.

```bash
rustyochestrator validate pipeline.yaml
```

```
  3 tasks
  [ok] build
  [ok] test  (needs: build)
  [ok] deploy  (needs: test)

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

Creates a starter `pipeline.yaml` (or a custom filename) in the current directory.

```bash
rustyochestrator init                    # creates pipeline.yaml
rustyochestrator init my-pipeline.yaml  # custom filename
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

  - id: lint
    command: "cargo clippy -- -D warnings"
    depends_on: [build]

  - id: test
    command: "cargo test"
    depends_on: [build]

  - id: package
    command: "tar czf dist.tar.gz target/release/myapp"
    depends_on: [lint, test]
```

| Field | Required | Description |
| --- | --- | --- |
| `id` | yes | Unique identifier for the task |
| `command` | yes | Shell command to run (executed via `sh -c`) |
| `depends_on` | no | List of task IDs that must succeed before this task starts |
| `env` | no | Map of environment variables scoped to this task |

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
- `uses:` steps (third-party actions) are silently skipped
- `env:` blocks at the workflow, job, and step levels are parsed and merged
- `${{ secrets.NAME }}` references are forwarded; other `${{ }}` expressions are dropped (they require a real Actions runner)

---

## Language examples

Rusty Orchestrator is **completely language-agnostic**. The `command` field runs anything your shell can execute.

### Node.js / npm

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

  - id: docker-build
    command: "docker build -t myapp:latest ."
    depends_on: [lint, test]
```

### Terraform / infrastructure

```yaml
tasks:
  - id: tf-init
    command: "terraform init"

  - id: tf-plan
    command: "terraform plan -out=plan.tfplan"
    depends_on: [tf-init]

  - id: tf-apply
    command: "terraform apply plan.tfplan"
    depends_on: [tf-plan]
```

### Mixed stack

```yaml
tasks:
  - id: backend-test
    command: "cargo test"

  - id: frontend-test
    command: "npm test"

  - id: build-image
    command: "docker build -t myapp ."
    depends_on: [backend-test, frontend-test]

  - id: push-image
    command: "docker push myapp:latest"
    depends_on: [build-image]
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
| **GitHub Actions compatibility** | Parse and run `.github/workflows/*.yml` files directly |
| **Parallel workflow execution** | `run-all` runs every workflow file in a directory simultaneously |
| **Live TUI dashboard** | Colour-coded per-task progress with spinners, elapsed time, and a summary bar |
| **CI-friendly output** | Auto-detects non-TTY environments and falls back to plain log output |
| **Environment variables & secrets** | Declare `env:` at pipeline or task level; secret refs resolved from shell env or `.env` file |
| **`.env` file support** | Automatically loads `.env` from the current directory; shell exports always take precedence |
| **Pre-flight secret validation** | All secrets validated before execution starts; missing secrets abort immediately |
| **Retry logic** | Failed tasks are retried up to 2 times before being marked failed |
| **Failure propagation** | When a task fails, its entire transitive dependent subtree is cancelled |
| **Real-time output streaming** | Stdout and stderr from every task streamed line-by-line as they run |
| **Cycle detection** | Circular dependencies caught and reported before execution starts |
| **Dashboard integration** | Optional [Dashhy](https://github.com/KodeSage/Dashhy) integration streams live pipeline events to a hosted monitoring UI |

---

## Roadmap

**v0.2.x — Reliability & usability**
- Per-task `timeout` field with pipeline-level default
- Configurable `retries: N` per task with optional retry delay / backoff
- Task output capture (`outputs:`) so downstream tasks can reference generated values
- Conditional task execution with `if:` field

**v0.3.x — GitHub Actions compatibility**
- `strategy.matrix` expansion — one task generated per matrix combination
- `${{ github.* }}` and `${{ env.* }}` context variable resolution
- `if:` expression evaluation (`success()`, `failure()`, `always()`)
- Named warnings for skipped `uses:` actions; no-op stubs for `actions/checkout`, `dtolnay/rust-toolchain`, `actions/cache`
- **Local artifact store** — first-class stubs for `actions/upload-artifact` and `actions/download-artifact` backed by `.rustyochestrator/artifacts/<run-id>/`, enabling release workflows with inter-job file transfer to run locally

**v0.4.x — Observability**
- `rustyochestrator report` command with JSON/Markdown run summary
- `--dry-run`, `--verbose`, and `--trace-deps` flags
- Per-task timing breakdown highlighting bottleneck tasks
- `--log-file <path>` for CI artifact capture

**v0.5.x — Execution control**
- `--resume` to skip previously-succeeded tasks
- `--force` to ignore cache and re-run everything
- `--only <task_id,...>` to run a subset and their dependencies
- `shell:` field per task for custom shell selection

**v0.6.x — Dashboard & remote integration**
- Real-time log streaming to the dashboard
- Webhook trigger server (`rustyochestrator serve`)
- Local run history (`rustyochestrator history`)
- Remote cache backend (S3, GCS, HTTP)
- **GitHub Release creation** — first-class stub for `softprops/action-gh-release`, calling the GitHub Releases API with a `GITHUB_TOKEN` secret to create releases and upload assets locally

**v0.7.x — Platform & distribution**
- Windows support
- Plugin / hook system (`on_task_start`, `on_task_success`, `on_task_failure`)
- Homebrew formula, Debian/RPM packages, GitHub Actions action

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
cargo run -- validate examples/pipeline.yaml
cargo run -- list examples/pipeline.yaml
cargo run -- graph examples/pipeline.yaml
cargo run -- cache show
cargo run -- init
cargo run -- run-all .github/workflows
```

Please open an issue before submitting large changes so we can align on the approach.

---

## License

MIT — see [LICENSE](LICENSE).
