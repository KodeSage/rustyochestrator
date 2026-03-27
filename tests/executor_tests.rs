use rustyochestrator::executor::execute_task;
use rustyochestrator::pipeline::Task;
use std::collections::HashMap;

fn make_task(id: &str, command: &str) -> Task {
    Task {
        id: id.to_string(),
        command: command.to_string(),
        depends_on: vec![],
        env: HashMap::new(),
        hash: None,
    }
}

#[tokio::test]
async fn test_successful_echo_command_returns_true() {
    let task = make_task("exec_echo", "echo hello_world");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_exit_1_returns_false() {
    let task = make_task("exec_fail", "exit 1");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_false_command_returns_false() {
    let task = make_task("exec_false", "false");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_true_command_returns_true() {
    let task = make_task("exec_true", "true");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_env_variable_is_passed_to_subprocess() {
    let task = make_task(
        "exec_env_check",
        "test \"$RUSTYTEST_EXEC_VAR\" = \"expected_value\"",
    );
    let mut env = HashMap::new();
    env.insert(
        "RUSTYTEST_EXEC_VAR".to_string(),
        "expected_value".to_string(),
    );
    let result = execute_task(&task, "", true, &env).await;
    assert!(result.is_ok());
    assert!(result.unwrap(), "env var was not passed to subprocess");
}

#[tokio::test]
async fn test_multiple_env_variables_passed() {
    let task = make_task(
        "exec_multi_env",
        "test \"$RUSTYTEST_A\" = alpha && test \"$RUSTYTEST_B\" = beta",
    );
    let mut env = HashMap::new();
    env.insert("RUSTYTEST_A".to_string(), "alpha".to_string());
    env.insert("RUSTYTEST_B".to_string(), "beta".to_string());
    let result = execute_task(&task, "", true, &env).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_multiline_command_all_lines_run() {
    let task = make_task("exec_multi", "echo line1\necho line2\necho line3");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_multiline_command_fails_if_any_line_fails() {
    let task = make_task("exec_multi_fail", "echo ok\nexit 1\necho unreachable");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_nonzero_exit_code_returns_false() {
    let task = make_task("exec_exit42", "exit 42");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_quiet_mode_suppresses_output() {
    // quiet=true should not panic and should return result correctly
    let task = make_task("exec_quiet", "echo this_should_be_quiet");
    let result = execute_task(&task, "[prefix] ", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_empty_env_map_is_fine() {
    let task = make_task("exec_no_env", "echo no_env");
    let result = execute_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}
