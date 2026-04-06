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
        timeout: None,
        retries: None,
        retry_delay: None,
        outputs: vec![],
        condition: None,
    }
}

/// Helper: call execute_task with v0.1.4 defaults (2 retries, no delay, no timeout, no outputs, no log).
async fn run_task(
    task: &Task,
    prefix: &str,
    quiet: bool,
    env: &HashMap<String, String>,
) -> Result<bool, rustyochestrator::errors::RustyError> {
    let (success, _outputs) =
        execute_task(task, prefix, quiet, env, 2, None, None, &[], None).await?;
    Ok(success)
}

#[tokio::test]
async fn test_successful_echo_command_returns_true() {
    let task = make_task("exec_echo", "echo hello_world");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_exit_1_returns_false() {
    let task = make_task("exec_fail", "exit 1");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_false_command_returns_false() {
    let task = make_task("exec_false", "false");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_true_command_returns_true() {
    let task = make_task("exec_true", "true");
    let result = run_task(&task, "", true, &HashMap::new()).await;
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
    let result = run_task(&task, "", true, &env).await;
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
    let result = run_task(&task, "", true, &env).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_multiline_command_all_lines_run() {
    let task = make_task("exec_multi", "echo line1\necho line2\necho line3");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_multiline_command_fails_if_any_line_fails() {
    let task = make_task("exec_multi_fail", "echo ok\nexit 1\necho unreachable");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_nonzero_exit_code_returns_false() {
    let task = make_task("exec_exit42", "exit 42");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_quiet_mode_suppresses_output() {
    let task = make_task("exec_quiet", "echo this_should_be_quiet");
    let result = run_task(&task, "[prefix] ", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_empty_env_map_is_fine() {
    let task = make_task("exec_no_env", "echo no_env");
    let result = run_task(&task, "", true, &HashMap::new()).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// ── v0.1.4 tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_timeout_kills_long_running_task() {
    let task = make_task("exec_timeout", "sleep 60");
    let timeout = Some(std::time::Duration::from_secs(1));
    let (success, _) = execute_task(
        &task,
        "",
        true,
        &HashMap::new(),
        0,
        None,
        timeout,
        &[],
        None,
    )
    .await
    .unwrap();
    assert!(!success, "task should have been killed by timeout");
}

#[tokio::test]
async fn test_zero_retries_runs_once() {
    let task = make_task("exec_no_retry", "exit 1");
    let (success, _) = execute_task(&task, "", true, &HashMap::new(), 0, None, None, &[], None)
        .await
        .unwrap();
    assert!(!success);
}

#[tokio::test]
async fn test_output_capture() {
    let task = make_task("exec_output", "echo VERSION=1.2.3\necho BUILD_ID=abc123");
    let output_names = vec!["VERSION".to_string(), "BUILD_ID".to_string()];
    let (success, outputs) = execute_task(
        &task,
        "",
        true,
        &HashMap::new(),
        0,
        None,
        None,
        &output_names,
        None,
    )
    .await
    .unwrap();
    assert!(success);
    assert_eq!(outputs.get("VERSION").map(String::as_str), Some("1.2.3"));
    assert_eq!(outputs.get("BUILD_ID").map(String::as_str), Some("abc123"));
}
