use rustyochestrator::errors::{Result, RustyError};
use std::io;

#[test]
fn test_circular_dependency_error_contains_path() {
    let err = RustyError::CircularDependency("a -> b -> a".to_string());
    let msg = err.to_string();
    assert!(msg.contains("a -> b -> a"));
    assert!(msg.to_lowercase().contains("circular"));
}

#[test]
fn test_missing_dependency_error_mentions_task_and_dep() {
    let err = RustyError::MissingDependency {
        task: "test_task".to_string(),
        dep: "missing_dep".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("test_task"), "msg: {}", msg);
    assert!(msg.contains("missing_dep"), "msg: {}", msg);
}

#[test]
fn test_missing_secret_error_mentions_key_secret_and_task() {
    let err = RustyError::MissingSecret {
        key: "API_KEY".to_string(),
        secret: "MY_SECRET_NAME".to_string(),
        task: "deploy_task".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("API_KEY"), "msg: {}", msg);
    assert!(msg.contains("MY_SECRET_NAME"), "msg: {}", msg);
    assert!(msg.contains("deploy_task"), "msg: {}", msg);
}

#[test]
fn test_io_error_converted_via_from() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let err: RustyError = io_err.into();
    let msg = err.to_string();
    assert!(!msg.is_empty());
}

#[test]
fn test_error_debug_format_is_non_empty() {
    let err = RustyError::CircularDependency("cycle".to_string());
    let debug = format!("{:?}", err);
    assert!(!debug.is_empty());
    assert!(debug.contains("CircularDependency") || debug.contains("cycle"));
}

#[test]
fn test_result_ok_variant() {
    let ok: Result<i32> = Ok(42);
    assert!(ok.is_ok());
    assert_eq!(ok.as_ref().ok(), Some(&42));
}

#[test]
fn test_result_err_variant() {
    let err: Result<i32> = Err(RustyError::CircularDependency("x -> x".to_string()));
    assert!(err.is_err());
}

#[test]
fn test_yaml_parse_error_via_from() {
    let bad_yaml = "invalid: {unclosed: [";
    let result = serde_yaml::from_str::<serde_yaml::Value>(bad_yaml);
    if let Err(e) = result {
        let err: RustyError = e.into();
        let msg = err.to_string();
        assert!(msg.to_lowercase().contains("yaml"));
    }
}

#[test]
fn test_json_error_via_from() {
    let bad_json = "{invalid json}";
    let result = serde_json::from_str::<serde_json::Value>(bad_json);
    if let Err(e) = result {
        let err: RustyError = e.into();
        let msg = err.to_string();
        assert!(msg.to_lowercase().contains("json") || !msg.is_empty());
    }
}

#[test]
fn test_missing_dependency_error_mentions_not_exist() {
    let err = RustyError::MissingDependency {
        task: "my_task".to_string(),
        dep: "ghost_dep".to_string(),
    };
    let msg = err.to_string();
    // Should mention that dep does not exist
    assert!(
        msg.contains("does not exist") || msg.contains("missing") || msg.contains("not exist"),
        "msg: {}",
        msg
    );
}
