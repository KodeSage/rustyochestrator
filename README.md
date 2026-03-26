# 🦀 Rusty Orchestrator
[![crates.io](https://img.shields.io/crates/v/rustyochestrator.svg)](https://crates.io/crates/rustyochestrator)
[![license](https://img.shields.io/crates/l/rustyochestrator.svg)](LICENSE)


A high-performance CI/CD pipeline runner written in Rust.

Executes task pipelines defined in YAML, runs independent tasks in parallel, skips unchanged work via content-addressable caching, and understands GitHub Actions workflow syntax natively.

---

## Installation

### Option 1 — cargo install (recommended)

Requires Rust 1.70+. Installs the `rustyochestrator` binary directly from [crates.io](https://crates.io/crates/rustyochestrator):

```bash
cargo install rustyochestrator
```

Upgrade to the latest version at any time:

```bash
cargo install rustyochestrator --force
```

### Option 2 — pre-built binary

Download the binary for your platform from the [latest GitHub release](https://github.com/yourname/rusty/releases/latest), then move it onto your PATH:

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
cd rusty
cargo build --release
./target/release/rustyochestrator --version
```

---

## Quick start

### No folder setup needed

`rustyochestrator init` creates the pipeline file in your current directory — no manual folder creation required:

```bash
rustyochestrator init
rustyochestrator run pipeline.yaml
```

### Dependencies are not auto-installed

`rustyochestrator` only runs the commands you define. If your project needs packages installed, declare it as a task:

```yaml
tasks:
  - id: install
    command: "npm install"   # or pip install, cargo fetch, etc.

  - id: build
    command: "npm run build"
    depends_on: [install]
```

---

## Local usage

Everything below works offline with no dashboard, no account, and no network access required.

### Run a pipeline

```bash
rustyochestrator run pipeline.yaml
```

Run a second time — every unchanged task is skipped instantly from cache:

```bash
rustyochestrator run pipeline.yaml
# [CACHE HIT] Skipping task: build
# [CACHE HIT] Skipping task: test
# [CACHE HIT] Skipping task: deploy
```

### All CLI commands

```bash
# Execute a pipeline
rustyochestrator run pipeline.yaml
rustyochestrator run pipeline.yaml --concurrency 4   # limit worker count
rustyochestrator run .github/workflows/ci.yml        # GitHub Actions format

# Validate without running
rustyochestrator validate pipeline.yaml

# Show execution order by stage
rustyochestrator list pipeline.yaml

# Print the dependency graph
rustyochestrator graph pipeline.yaml

# Inspect the local cache
rustyochestrator cache show

# Clear the local cache (forces full re-run next time)
rustyochestrator cache clean

# Scaffold a new pipeline.yaml
rustyochestrator init
rustyochestrator init my-pipeline.yaml   # custom filename

# Run all workflows in a directory simultaneously
rustyochestrator run-all .github/workflows
rustyochestrator run-all ./my-pipelines --concurrency 4

# Debug logging
RUST_LOG=debug rustyochestrator run pipeline.yaml
```

### Developing from source

Clone the repo and use `cargo run` in place of the installed binary:

```bash
git clone https://github.com/yourname/rusty
cd rusty

cargo run -- run examples/pipeline.yaml
cargo run -- run examples/pipeline.yaml --concurrency 2
cargo run -- validate examples/pipeline.yaml
cargo run -- list examples/pipeline.yaml
cargo run -- graph examples/pipeline.yaml
cargo run -- cache show
cargo run -- cache clean
cargo run -- init
cargo run -- run-all .github/workflows

# Build and run the release binary directly
cargo build --release
./target/release/rustyochestrator run examples/pipeline.yaml
```

---

### Manage the connection

```bash
rustyochestrator status       # show connected dashboard and user
rustyochestrator disconnect   # remove connection
```

---

## Language support

rustyochestrator is **completely language-agnostic**. The `command` field runs anything your shell can execute.

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

## Features

- **Parallel execution** — worker pool backed by Tokio; concurrency defaults to the number of logical CPUs
- **DAG scheduling** — dependencies are resolved at runtime; tasks run as soon as their deps finish
- **Content-addressable cache** — each task is hashed by its command + dependency IDs; unchanged tasks are skipped instantly
- **GitHub Actions compatibility** — parse and run `.github/workflows/*.yml` files directly
- **Parallel workflow execution** — `run-all` runs every workflow file in a directory simultaneously, with each workflow's output prefixed by its name
- **Retry logic** — failed tasks are retried up to 2 times before being marked failed
- **Failure propagation** — when a task fails its entire transitive dependent subtree is cancelled immediately
- **Real-time output** — stdout and stderr from every task are streamed line-by-line as they run
- **Cycle detection** — circular dependencies are caught before execution starts
- **Live dashboard** — optional dashhy integration streams pipeline events to a hosted monitoring UI

---

## Pipeline format

### Native YAML

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

**Task fields:**

| Field        | Required | Description                                 |
| ------------ | -------- | ------------------------------------------- |
| `id`         | yes      | Unique identifier for the task              |
| `command`    | yes      | Shell command to run (executed via `sh -c`) |
| `depends_on` | no       | List of task IDs that must succeed first    |

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

### GitHub Actions format

Rusty can run GitHub Actions workflow files directly — useful for local testing before pushing.

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

Mapping rules:

- Each `run:` step becomes one task
- Steps within a job are sequential
- `needs:` wires the first step of a job to the last step of each required job
- `uses:` steps (actions) are silently skipped

---

## CLI reference

### `run` — execute a pipeline

```bash
rustyochestrator run <pipeline.yaml> [--concurrency <N>]
```

```bash
rustyochestrator run pipeline.yaml
rustyochestrator run pipeline.yaml --concurrency 4
rustyochestrator run .github/workflows/ci.yml      # GitHub Actions format
RUST_LOG=debug rustyochestrator run pipeline.yaml  # verbose logging
```

---

### `run-all` — run all workflows in a directory simultaneously

Discovers every `.yml` and `.yaml` file in the given directory, loads them all, then runs them concurrently — just like GitHub Actions fires multiple workflow files in parallel. Each workflow's output is prefixed with its filename so interleaved logs are always identifiable.

```bash
rustyochestrator run-all <dir> [--concurrency <N>]
```

```bash
rustyochestrator run-all .github/workflows          # default directory
rustyochestrator run-all ./pipelines                # any folder
rustyochestrator run-all examples --concurrency 2   # limit workers per workflow
```

Example output with two workflows running simultaneously:

```
INFO  running workflows simultaneously count=2 dir=.github/workflows
INFO  loaded workflow=ci tasks=7
INFO  loaded workflow=release tasks=7
[ci] [INFO] Starting task: lint__cargo_fmt___check
[release] [INFO] Starting task: build__Install_cross_...
[ci]   [lint__cargo_fmt___check] ...
[release]   [build__Install_cross_...|err] Compiling libc v0.2.183
[ci] [INFO] Completed task: lint__cargo_fmt___check
[release] [INFO] Completed task: build__Install_cross_...
```

- All pipelines are validated before any execution starts — parse errors surface immediately
- Each workflow runs its own independent DAG scheduler with its own cache
- Exit code is non-zero if any workflow fails; the failing workflow name is reported
- Works with both native pipeline format and GitHub Actions format files in the same directory

---

### `validate` — check without running

Parses the file, checks for missing dependencies and cycles, prints each task and its deps. Exits non-zero on any error.

```bash
rustyochestrator validate pipeline.yaml
```

```
  7 tasks
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
    3. clippy  (after: fmt)
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
    clippy  ◄── [fmt]
    build-debug  ◄── [fmt]

  Stage 2:
    test  ◄── [build-debug, clippy]
```

---

### `cache show` — inspect the cache

```bash
rustyochestrator cache show
```

```
  task                           status     hash
  ------------------------------------------------------------------------
  build                          ok         b3d10802f5217f42
  clippy                         ok         9eeaf9f8bc055df3
  fmt                            ok         810e75f0d3dee10e
  test                           ok         257080d6e3e17348

  4 cached task(s).
```

---

### `cache clean` — clear the cache

Forces every task to re-run on the next `rustyochestrator run`.

```bash
rustyochestrator cache clean
# Cache cleared.
```

---

### `init` — scaffold a new pipeline

Creates a starter `pipeline.yaml` (or a custom filename) in the current directory.

```bash
rustyochestrator init                    # creates pipeline.yaml
rustyochestrator init my-pipeline.yaml   # custom filename
```

---

### `connect` — link to dashhy dashboard

```bash
rustyochestrator connect --token <jwt> --url <dashboard-url>
```

Saves the connection to `~/.rustyochestrator/connect.json`. All subsequent `run` commands will report live to the dashboard.

---

### `disconnect` — remove dashboard connection

```bash
rustyochestrator disconnect
```

---

### `status` — show connection status

```bash
rustyochestrator status
# Connected
#   Dashboard : https://your-dashhy.vercel.app
#   User      : @your-github-username
```

---


## Caching

Cache entries are stored in `.rustyochestrator/cache.json`.

A task is a **cache hit** when:

1. Its SHA-256 hash (of `command + dependency IDs`) matches the stored entry
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

To force a full re-run, delete the cache:

```bash
rustyochestrator cache clean
# or
rm -rf .rustyochestrator
```

---

## License

MIT
