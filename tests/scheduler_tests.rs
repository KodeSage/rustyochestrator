use rustyochestrator::pipeline::Pipeline;
use rustyochestrator::scheduler::Scheduler;

fn build_scheduler(yaml: &str, workers: usize) -> Scheduler {
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    pipeline.validate().unwrap();
    Scheduler::new(pipeline, workers)
}

#[tokio::test]
async fn test_empty_pipeline_returns_ok_true() {
    let yaml = "tasks: []";
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let result = Scheduler::new(pipeline, 1).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_single_successful_task() {
    let yaml = r#"
tasks:
  - id: sched_single_ok
    command: "echo sched_single_ok"
"#;
    let result = build_scheduler(yaml, 1).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_single_failing_task_returns_false() {
    let yaml = r#"
tasks:
  - id: sched_single_fail_xyz_abc
    command: "exit 1"
"#;
    let result = build_scheduler(yaml, 1).run().await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_failure_propagates_to_dependent_tasks() {
    let yaml = r#"
tasks:
  - id: sched_fail_parent_abc
    command: "exit 1"
  - id: sched_skipped_child_abc
    command: "echo should_not_run"
    depends_on: [sched_fail_parent_abc]
"#;
    let result = build_scheduler(yaml, 2).run().await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_sequential_pipeline_all_succeed() {
    let yaml = r#"
tasks:
  - id: sched_seq_a
    command: "echo seq_a"
  - id: sched_seq_b
    command: "echo seq_b"
    depends_on: [sched_seq_a]
  - id: sched_seq_c
    command: "echo seq_c"
    depends_on: [sched_seq_b]
"#;
    let result = build_scheduler(yaml, 1).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_parallel_independent_tasks_all_succeed() {
    let yaml = r#"
tasks:
  - id: sched_par_a
    command: "echo par_a"
  - id: sched_par_b
    command: "echo par_b"
  - id: sched_par_c
    command: "echo par_c"
"#;
    let result = build_scheduler(yaml, 4).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_diamond_pipeline_succeeds() {
    let yaml = r#"
tasks:
  - id: sched_diamond_root
    command: "echo root"
  - id: sched_diamond_left
    command: "echo left"
    depends_on: [sched_diamond_root]
  - id: sched_diamond_right
    command: "echo right"
    depends_on: [sched_diamond_root]
  - id: sched_diamond_merge
    command: "echo merge"
    depends_on: [sched_diamond_left, sched_diamond_right]
"#;
    let result = build_scheduler(yaml, 4).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_missing_secret_returns_error() {
    let yaml = r#"
tasks:
  - id: sched_secret_task
    command: "echo secret_task"
    env:
      MY_KEY: "${{ secrets.RUSTYTEST_NONEXISTENT_SECRET_XYZ_999 }}"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let result = Scheduler::new(pipeline, 1).run().await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("RUSTYTEST_NONEXISTENT_SECRET_XYZ_999")
    );
}

#[tokio::test]
async fn test_pipeline_env_passed_to_task() {
    let yaml = r#"
env:
  SCHED_TEST_PIPELINE_ENV: "pipeline_value"
tasks:
  - id: sched_env_check
    command: "test \"$SCHED_TEST_PIPELINE_ENV\" = pipeline_value"
"#;
    let result = build_scheduler(yaml, 1).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_task_env_overrides_pipeline_env() {
    let yaml = r#"
env:
  SCHED_TEST_OVERRIDE_VAR: "pipeline"
tasks:
  - id: sched_override_check
    command: "test \"$SCHED_TEST_OVERRIDE_VAR\" = task"
    env:
      SCHED_TEST_OVERRIDE_VAR: "task"
"#;
    let result = build_scheduler(yaml, 1).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_with_name_does_not_break_run() {
    let yaml = r#"
tasks:
  - id: sched_named_task
    command: "echo named"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let result = Scheduler::new(pipeline, 1)
        .with_name("custom-pipeline-name".to_string())
        .run()
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_concurrency_1_still_runs_all_tasks() {
    let yaml = r#"
tasks:
  - id: sched_c1_a
    command: "echo c1_a"
  - id: sched_c1_b
    command: "echo c1_b"
  - id: sched_c1_c
    command: "echo c1_c"
    depends_on: [sched_c1_a, sched_c1_b]
"#;
    let result = build_scheduler(yaml, 1).run().await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}
