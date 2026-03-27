use rustyochestrator::pipeline::{Pipeline, TaskState, compute_task_hash};
use std::collections::BTreeMap;

// ── Parsing ──────────────────────────────────────────────────────────────────

#[test]
fn test_parse_single_task() {
    let yaml = r#"
tasks:
  - id: build
    command: "cargo build"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 1);
    assert_eq!(pipeline.tasks[0].id, "build");
    assert_eq!(pipeline.tasks[0].command, "cargo build");
    assert!(pipeline.tasks[0].depends_on.is_empty());
    assert!(pipeline.tasks[0].hash.is_some());
}

#[test]
fn test_parse_multiple_tasks_with_deps() {
    let yaml = r#"
tasks:
  - id: build
    command: "cargo build"
  - id: test
    command: "cargo test"
    depends_on: [build]
  - id: deploy
    command: "echo deploying"
    depends_on: [test]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(pipeline.tasks.len(), 3);
    let test_task = pipeline.tasks.iter().find(|t| t.id == "test").unwrap();
    assert_eq!(test_task.depends_on, vec!["build"]);
    let deploy_task = pipeline.tasks.iter().find(|t| t.id == "deploy").unwrap();
    assert_eq!(deploy_task.depends_on, vec!["test"]);
}

#[test]
fn test_parse_pipeline_level_env() {
    let yaml = r#"
env:
  NODE_ENV: production
  PORT: "3000"
tasks:
  - id: start
    command: "node app.js"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(
        pipeline.env.get("NODE_ENV"),
        Some(&"production".to_string())
    );
    assert_eq!(pipeline.env.get("PORT"), Some(&"3000".to_string()));
}

#[test]
fn test_parse_task_level_env() {
    let yaml = r#"
tasks:
  - id: build
    command: "cargo build"
    env:
      RUST_LOG: debug
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(
        pipeline.tasks[0].env.get("RUST_LOG"),
        Some(&"debug".to_string())
    );
}

#[test]
fn test_parse_empty_tasks_list() {
    let yaml = "tasks: []";
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.tasks.is_empty());
}

#[test]
fn test_parse_invalid_yaml_returns_error() {
    let yaml = "invalid: {unclosed bracket: [";
    assert!(Pipeline::from_yaml(yaml).is_err());
}

#[test]
fn test_parse_task_hash_is_computed() {
    let yaml = r#"
tasks:
  - id: build
    command: "cargo build"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let hash = pipeline.tasks[0].hash.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_parse_task_multiple_deps() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
  - id: b
    command: "echo b"
  - id: c
    command: "echo c"
    depends_on: [a, b]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let c = pipeline.tasks.iter().find(|t| t.id == "c").unwrap();
    assert_eq!(c.depends_on.len(), 2);
    assert!(c.depends_on.contains(&"a".to_string()));
    assert!(c.depends_on.contains(&"b".to_string()));
}

// ── Validation ────────────────────────────────────────────────────────────────

#[test]
fn test_validate_valid_linear_pipeline() {
    let yaml = r#"
tasks:
  - id: build
    command: "cargo build"
  - id: test
    command: "cargo test"
    depends_on: [build]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.validate().is_ok());
}

#[test]
fn test_validate_valid_parallel_pipeline() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
  - id: b
    command: "echo b"
  - id: c
    command: "echo c"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.validate().is_ok());
}

#[test]
fn test_validate_missing_dependency_returns_error() {
    let yaml = r#"
tasks:
  - id: test
    command: "cargo test"
    depends_on: [nonexistent_task]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let err = pipeline.validate().unwrap_err();
    assert!(err.to_string().contains("nonexistent_task"));
}

#[test]
fn test_validate_circular_dependency_two_nodes() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
    depends_on: [b]
  - id: b
    command: "echo b"
    depends_on: [a]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let err = pipeline.validate().unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(msg.contains("circular"));
}

#[test]
fn test_validate_self_dependency_returns_error() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
    depends_on: [a]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.validate().is_err());
}

#[test]
fn test_validate_three_node_cycle() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
    depends_on: [c]
  - id: b
    command: "echo b"
    depends_on: [a]
  - id: c
    command: "echo c"
    depends_on: [b]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.validate().is_err());
}

#[test]
fn test_validate_empty_pipeline() {
    let yaml = "tasks: []";
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.validate().is_ok());
}

// ── Levels ────────────────────────────────────────────────────────────────────

#[test]
fn test_levels_no_dependencies_all_in_stage_0() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
  - id: b
    command: "echo b"
  - id: c
    command: "echo c"
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let levels = pipeline.levels();
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].len(), 3);
}

#[test]
fn test_levels_linear_chain_three_stages() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
  - id: b
    command: "echo b"
    depends_on: [a]
  - id: c
    command: "echo c"
    depends_on: [b]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let levels = pipeline.levels();
    assert_eq!(levels.len(), 3);
    assert!(levels[0].contains(&"a".to_string()));
    assert!(levels[1].contains(&"b".to_string()));
    assert!(levels[2].contains(&"c".to_string()));
}

#[test]
fn test_levels_diamond_shape() {
    let yaml = r#"
tasks:
  - id: root
    command: "echo root"
  - id: left
    command: "echo left"
    depends_on: [root]
  - id: right
    command: "echo right"
    depends_on: [root]
  - id: merge
    command: "echo merge"
    depends_on: [left, right]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let levels = pipeline.levels();
    assert_eq!(levels.len(), 3);
    assert!(levels[0].contains(&"root".to_string()));
    assert!(levels[1].contains(&"left".to_string()));
    assert!(levels[1].contains(&"right".to_string()));
    assert!(levels[2].contains(&"merge".to_string()));
}

#[test]
fn test_levels_covers_all_tasks() {
    let yaml = r#"
tasks:
  - id: a
    command: "echo a"
  - id: b
    command: "echo b"
    depends_on: [a]
  - id: c
    command: "echo c"
    depends_on: [a]
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let levels = pipeline.levels();
    let all: Vec<_> = levels.into_iter().flatten().collect();
    assert_eq!(all.len(), 3);
    assert!(all.contains(&"a".to_string()));
    assert!(all.contains(&"b".to_string()));
    assert!(all.contains(&"c".to_string()));
}

// ── Hashing ───────────────────────────────────────────────────────────────────

#[test]
fn test_hash_deterministic_same_inputs() {
    let env: BTreeMap<&str, &str> = BTreeMap::new();
    let h1 = compute_task_hash("cargo build", &[], &env);
    let h2 = compute_task_hash("cargo build", &[], &env);
    assert_eq!(h1, h2);
}

#[test]
fn test_hash_differs_on_command_change() {
    let env: BTreeMap<&str, &str> = BTreeMap::new();
    let h1 = compute_task_hash("cargo build", &[], &env);
    let h2 = compute_task_hash("cargo test", &[], &env);
    assert_ne!(h1, h2);
}

#[test]
fn test_hash_differs_on_deps_change() {
    let env: BTreeMap<&str, &str> = BTreeMap::new();
    let h1 = compute_task_hash("echo hi", &[], &env);
    let h2 = compute_task_hash("echo hi", &["dep1".to_string()], &env);
    assert_ne!(h1, h2);
}

#[test]
fn test_hash_differs_on_env_key_add() {
    let env1: BTreeMap<&str, &str> = BTreeMap::new();
    let mut env2: BTreeMap<&str, &str> = BTreeMap::new();
    env2.insert("KEY", "VALUE");
    let h1 = compute_task_hash("echo hi", &[], &env1);
    let h2 = compute_task_hash("echo hi", &[], &env2);
    assert_ne!(h1, h2);
}

#[test]
fn test_hash_differs_on_env_value_change() {
    let mut env1: BTreeMap<&str, &str> = BTreeMap::new();
    env1.insert("KEY", "val1");
    let mut env2: BTreeMap<&str, &str> = BTreeMap::new();
    env2.insert("KEY", "val2");
    let h1 = compute_task_hash("echo hi", &[], &env1);
    let h2 = compute_task_hash("echo hi", &[], &env2);
    assert_ne!(h1, h2);
}

#[test]
fn test_hash_is_64_char_hex() {
    let env: BTreeMap<&str, &str> = BTreeMap::new();
    let h = compute_task_hash("cargo build", &["dep".to_string()], &env);
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_hash_dep_order_matters() {
    let env: BTreeMap<&str, &str> = BTreeMap::new();
    let h1 = compute_task_hash("echo", &["a".to_string(), "b".to_string()], &env);
    let h2 = compute_task_hash("echo", &["b".to_string(), "a".to_string()], &env);
    // Different dep orders produce different hashes (they're fed sequentially)
    assert_ne!(h1, h2);
}

// ── TaskState ─────────────────────────────────────────────────────────────────

#[test]
fn test_task_state_variants_are_distinct() {
    let states = [
        TaskState::Pending,
        TaskState::Running,
        TaskState::Success,
        TaskState::Failed,
        TaskState::Skipped,
    ];
    for (i, s1) in states.iter().enumerate() {
        for (j, s2) in states.iter().enumerate() {
            if i == j {
                assert_eq!(s1, s2);
            } else {
                assert_ne!(s1, s2);
            }
        }
    }
}

#[test]
fn test_task_state_clone() {
    let s = TaskState::Running;
    let cloned = s.clone();
    assert_eq!(s, cloned);
}
