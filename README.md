# 🦀 Rusty Orchestrator

A high-performance CI/CD pipeline runner written in Rust.

Executes task pipelines defined in YAML, runs independent tasks in parallel, skips unchanged work via content-addressable caching, and understands GitHub Actions workflow syntax natively.


## Installation


### Developing from source

Clone the repo and use `cargo run` in place of the installed binary:

```bash
git clone https://github.com//rusty
cd rusty

cargo run -- run examples/pipeline.yaml
cargo run -- run examples/pipeline.yaml --concurrency 2
cargo run -- validate examples/pipeline.yaml
cargo run -- list examples/pipeline.yaml
cargo run -- graph examples/pipeline.yaml
cargo run -- cache show
cargo run -- cache clean
cargo run -- init

# Build and run the release binary directly
cargo build --release
./target/release/rustyochestrator run examples/pipeline.yaml
```


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



## License

MIT