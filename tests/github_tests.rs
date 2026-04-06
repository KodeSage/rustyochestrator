use rustyochestrator::github::parse_github_workflow;

#[test]
fn test_parse_simple_single_job_workflow() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Build
        run: cargo build
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 1);
    assert_eq!(pipeline.tasks[0].command, "cargo build");
}

#[test]
fn test_uses_steps_are_silently_skipped() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build
        run: cargo build
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 1);
    assert_eq!(pipeline.tasks[0].command, "cargo build");
}

#[test]
fn test_steps_within_a_job_are_sequential() {
    let yaml = r#"
on: push
jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - name: Step_A
        run: echo step_a
      - name: Step_B
        run: echo step_b
      - name: Step_C
        run: echo step_c
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 3);

    let a = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "echo step_a")
        .unwrap();
    let b = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "echo step_b")
        .unwrap();
    let c = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "echo step_c")
        .unwrap();

    assert!(a.depends_on.is_empty());
    assert!(b.depends_on.contains(&a.id));
    assert!(c.depends_on.contains(&b.id));
}

#[test]
fn test_cross_job_dependency_via_needs() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Build
        run: cargo build
  test:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Test
        run: cargo test
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 2);

    let build_id = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "cargo build")
        .unwrap()
        .id
        .clone();
    let test_task = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "cargo test")
        .unwrap();
    assert!(test_task.depends_on.contains(&build_id));
}

#[test]
fn test_needs_as_array_string() {
    let yaml = r#"
on: push
jobs:
  job_a:
    runs-on: ubuntu-latest
    steps:
      - run: echo a
  job_b:
    runs-on: ubuntu-latest
    steps:
      - run: echo b
  job_c:
    needs: [job_a, job_b]
    runs-on: ubuntu-latest
    steps:
      - run: echo c
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let c_task = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "echo c")
        .unwrap();
    assert_eq!(c_task.depends_on.len(), 2);
}

#[test]
fn test_needs_as_single_string() {
    let yaml = r#"
on: push
jobs:
  job_a:
    runs-on: ubuntu-latest
    steps:
      - run: echo a
  job_b:
    needs: job_a
    runs-on: ubuntu-latest
    steps:
      - run: echo b
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let b_task = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "echo b")
        .unwrap();
    assert_eq!(b_task.depends_on.len(), 1);
}

#[test]
fn test_secrets_env_reference_is_preserved() {
    let yaml = r#"
on: push
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - name: Deploy
        run: ./deploy.sh
        env:
          API_KEY: "${{ secrets.API_KEY }}"
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let task = &pipeline.tasks[0];
    assert_eq!(
        task.env.get("API_KEY"),
        Some(&"${{ secrets.API_KEY }}".to_string())
    );
}

#[test]
fn test_github_context_env_is_filtered() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Build
        run: echo building
        env:
          PLAIN_VAR: hello
          GITHUB_REF: "${{ github.ref }}"
          SHA: "${{ github.sha }}"
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let task = &pipeline.tasks[0];
    assert_eq!(task.env.get("PLAIN_VAR"), Some(&"hello".to_string()));
    assert!(!task.env.contains_key("GITHUB_REF"));
    assert!(!task.env.contains_key("SHA"));
}

#[test]
fn test_step_with_expression_in_run_command_resolved_or_stripped() {
    // Since v0.1.4, context variables like ${{ matrix.os }} are resolved where possible.
    // Unresolvable expressions are stripped from the command (not skipped entirely).
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Matrix Step
        run: echo ${{ matrix.os }}
      - name: Static Step
        run: cargo build
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 2);
    // The matrix expression was stripped, leaving "echo "
    let matrix_task = pipeline
        .tasks
        .iter()
        .find(|t| t.id.contains("Matrix"))
        .unwrap();
    assert_eq!(matrix_task.command.trim(), "echo");
    // Static step still works
    let static_task = pipeline
        .tasks
        .iter()
        .find(|t| t.command == "cargo build")
        .unwrap();
    assert_eq!(static_task.command, "cargo build");
}

#[test]
fn test_workflow_level_env_propagated_to_tasks() {
    let yaml = r#"
on: push
env:
  GLOBAL_VAR: global_value
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Build
        run: cargo build
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let task = &pipeline.tasks[0];
    assert_eq!(
        task.env.get("GLOBAL_VAR"),
        Some(&"global_value".to_string())
    );
}

#[test]
fn test_job_level_env_propagated_to_tasks() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    env:
      JOB_VAR: job_value
    steps:
      - name: Build
        run: cargo build
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let task = &pipeline.tasks[0];
    assert_eq!(task.env.get("JOB_VAR"), Some(&"job_value".to_string()));
}

#[test]
fn test_step_env_overrides_job_env() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    env:
      KEY: job_value
    steps:
      - name: Build
        run: cargo build
        env:
          KEY: step_value
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let task = &pipeline.tasks[0];
    assert_eq!(task.env.get("KEY"), Some(&"step_value".to_string()));
}

#[test]
fn test_step_env_overrides_workflow_env() {
    let yaml = r#"
on: push
env:
  KEY: workflow_value
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Build
        run: cargo build
        env:
          KEY: step_value
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let task = &pipeline.tasks[0];
    assert_eq!(task.env.get("KEY"), Some(&"step_value".to_string()));
}

#[test]
fn test_task_id_includes_job_and_step_name() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: Run Tests
        run: cargo test
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let id = &pipeline.tasks[0].id;
    assert!(
        id.contains("build"),
        "id '{}' should contain job name 'build'",
        id
    );
}

#[test]
fn test_task_id_fallback_uses_step_index() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: cargo test
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    let id = &pipeline.tasks[0].id;
    assert!(id.contains("build"), "id '{}' should contain job name", id);
    assert!(id.contains("step"), "id '{}' should contain 'step'", id);
}

#[test]
fn test_all_tasks_have_hashes_computed() {
    let yaml = r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: cargo build
      - run: cargo test
      - run: cargo clippy
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    for task in &pipeline.tasks {
        let hash = task.hash.as_ref().expect("hash should be computed");
        assert_eq!(hash.len(), 64);
    }
}

#[test]
fn test_empty_jobs_map_produces_empty_pipeline() {
    let yaml = r#"
on: push
jobs: {}
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    assert!(pipeline.tasks.is_empty());
}

#[test]
fn test_pipeline_env_is_empty_workflow_env_merged_per_task() {
    let yaml = r#"
on: push
env:
  GLOBAL: value
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#;
    let pipeline = parse_github_workflow(yaml).unwrap();
    // Workflow env is merged into task env, pipeline-level env remains empty
    assert!(pipeline.env.is_empty());
    assert_eq!(
        pipeline.tasks[0].env.get("GLOBAL"),
        Some(&"value".to_string())
    );
}
