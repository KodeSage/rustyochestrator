mod cache;
mod cli;
mod config;
mod errors;
mod executor;
mod github;
mod pipeline;
mod report;
mod reporter;
mod scheduler;
mod tui;

use clap::Parser;
use cli::{CacheCommands, Cli, Commands};
use pipeline::Pipeline;
use scheduler::Scheduler;
use std::io::IsTerminal;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

/// Sync entry point — loads .env before the tokio runtime (and its worker threads) start,
/// which is the only point at which calling `set_var` is guaranteed to be single-threaded.
fn main() {
    load_dotenv();
    async_main();
}

#[tokio::main]
async fn async_main() {
    // Parse CLI first so we can detect TUI mode before initialising tracing.
    let cli = Cli::parse();

    // In TUI mode suppress info-level tracing so it doesn't bleed into the display.
    let tui_active = match &cli.command {
        Commands::Run {
            no_tui, verbose, ..
        } => !no_tui && !verbose && std::io::stdout().is_terminal(),
        Commands::RunAll {
            no_tui, verbose, ..
        } => !no_tui && !verbose && std::io::stdout().is_terminal(),
        _ => false,
    };
    let default_level = if tui_active { "warn" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Commands::Run {
            pipeline: path,
            concurrency,
            no_tui,
            verbose,
            dry_run,
            trace_deps,
            log_file,
            keep_artifacts,
        } => {
            let pipeline = load_pipeline(&path);
            let workers = concurrency.unwrap_or_else(num_cpus::get);

            // Derive a pipeline name from the file path
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("pipeline")
                .to_string();

            let use_tui = !no_tui && !verbose && !dry_run && std::io::stdout().is_terminal();

            if !use_tui && !dry_run {
                tracing::info!(workers, tasks = pipeline.tasks.len(), "pipeline starting");
            }

            // Set up artifact run ID
            let run_id = format!(
                "{}-{}",
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
            unsafe {
                std::env::set_var("RUSTYORCH_RUN_ID", &run_id);
            }

            // Build scheduler
            let mut scheduler = Scheduler::new(pipeline.clone(), workers)
                .with_name(name.clone())
                .with_dry_run(dry_run)
                .with_trace_deps(trace_deps);

            // Log file
            if let Some(ref log_path) = log_file {
                match executor::create_log_writer(log_path) {
                    Ok(lw) => {
                        scheduler = scheduler.with_log_writer(lw);
                        tracing::info!("logging to file: {}", log_path);
                    }
                    Err(e) => die(&format!("cannot create log file '{}': {}", log_path, e)),
                }
            }

            if let Some(cfg) = config::load() {
                let r = reporter::Reporter::new(cfg.dashboard_url.clone(), cfg.token.clone());
                scheduler = scheduler.with_reporter(r);
                tracing::info!("reporting to dashboard: {}", cfg.dashboard_url);
            }

            if use_tui {
                // Tasks displayed in topological order so the layout matches execution flow.
                let task_ids: Vec<String> = pipeline.levels().into_iter().flatten().collect();
                let dashboard = Arc::new(tui::Dashboard::new(&name, &task_ids));
                scheduler = scheduler.with_dashboard(dashboard);
            }

            match scheduler.run().await {
                Ok(true) => {
                    if !use_tui && !dry_run {
                        tracing::info!("pipeline completed successfully");
                    }
                }
                Ok(false) => {
                    if !use_tui {
                        eprintln!("error: pipeline finished with failures");
                    }
                    cleanup_artifacts(&run_id, keep_artifacts);
                    std::process::exit(1);
                }
                Err(e) => {
                    cleanup_artifacts(&run_id, keep_artifacts);
                    die(&e.to_string());
                }
            }

            cleanup_artifacts(&run_id, keep_artifacts);
        }

        Commands::Validate { pipeline: path } => {
            let pipeline = load_pipeline(&path);
            println!("  {} tasks", pipeline.tasks.len());
            for task in &pipeline.tasks {
                let mut info_parts = Vec::new();
                if !task.depends_on.is_empty() {
                    info_parts.push(format!("needs: {}", task.depends_on.join(", ")));
                }
                if let Some(ref t) = task.timeout {
                    info_parts.push(format!("timeout: {}", t));
                }
                if let Some(r) = task.retries {
                    info_parts.push(format!("retries: {}", r));
                }
                if !task.outputs.is_empty() {
                    info_parts.push(format!("outputs: [{}]", task.outputs.join(", ")));
                }
                if let Some(ref c) = task.condition {
                    info_parts.push(format!("if: {}", c));
                }

                if info_parts.is_empty() {
                    println!("  [ok] {}", task.id);
                } else {
                    println!("  [ok] {}  ({})", task.id, info_parts.join(", "));
                }
            }

            // Show pipeline defaults if set
            if let Some(ref defaults) = pipeline.defaults {
                let mut default_parts = Vec::new();
                if let Some(ref t) = defaults.timeout {
                    default_parts.push(format!("timeout: {}", t));
                }
                if let Some(r) = defaults.retries {
                    default_parts.push(format!("retries: {}", r));
                }
                if defaults.retry_delay.is_some() {
                    default_parts.push("retry_delay: set".to_string());
                }
                if !default_parts.is_empty() {
                    println!("\n  defaults: {}", default_parts.join(", "));
                }
            }

            println!("\npipeline '{}' is valid.", path);
        }

        Commands::List { pipeline: path } => {
            let pipeline = load_pipeline(&path);
            let levels = pipeline.levels();
            println!("\nExecution order for '{}':\n", path);
            let mut n = 1;
            for (stage, tasks) in levels.iter().enumerate() {
                println!(
                    "  Stage {} — {} task(s) run in parallel:",
                    stage,
                    tasks.len()
                );
                for id in tasks {
                    let task = pipeline.tasks.iter().find(|t| &t.id == id).unwrap();
                    if task.depends_on.is_empty() {
                        println!("    {}. {}", n, id);
                    } else {
                        println!("    {}. {}  (after: {})", n, id, task.depends_on.join(", "));
                    }
                    n += 1;
                }
                println!();
            }
        }

        Commands::Graph { pipeline: path } => {
            let pipeline = load_pipeline(&path);
            println!("\nDependency graph for '{}':", path);
            pipeline.print_graph();
        }

        Commands::Cache(args) => match args.command {
            CacheCommands::Show => {
                let cache = match cache::Cache::load() {
                    Ok(c) => c,
                    Err(e) => die(&format!("cannot load cache: {}", e)),
                };
                if cache.entries.is_empty() {
                    println!("Cache is empty. (.rustyochestrator/cache.json)");
                } else {
                    println!("\n  {:<30} {:<10} hash", "task", "status");
                    println!("  {}", "-".repeat(72));
                    let mut entries: Vec<_> = cache.entries.iter().collect();
                    entries.sort_by_key(|(id, _)| id.as_str());
                    for (id, entry) in entries {
                        let status = if entry.success { "ok" } else { "failed" };
                        println!("  {:<30} {:<10} {}", id, status, &entry.hash[..16]);
                    }
                    println!("\n  {} cached task(s).", cache.entries.len());
                }
            }
            CacheCommands::Clean => {
                if std::path::Path::new(".rustyochestrator/cache.json").exists() {
                    std::fs::remove_file(".rustyochestrator/cache.json")
                        .unwrap_or_else(|e| die(&e.to_string()));
                    println!("Cache cleared.");
                } else {
                    println!("Nothing to clean (no cache found).");
                }
            }
        },

        Commands::Init { output } => {
            if std::path::Path::new(&output).exists() {
                eprintln!("error: '{}' already exists. Remove it first.", output);
                std::process::exit(1);
            }
            let template = r#"# Pipeline-level defaults (applied to all tasks unless overridden)
# defaults:
#   timeout: "5m"
#   retries: 2
#   retry_delay: "5s"

# Pipeline-level environment variables
# env:
#   NODE_ENV: production

tasks:
  - id: build
    command: "echo building..."
    # timeout: "300s"
    # retries: 3
    # retry_delay:
    #   strategy: exponential
    #   base: "1s"

  - id: test
    command: "echo testing..."
    depends_on: [build]
    # if: "$CI == 'true'"
    # outputs: [TEST_RESULT]

  - id: deploy
    command: "echo deploying..."
    depends_on: [test]
    # if: "tasks.test.result == 'success'"
"#;
            std::fs::write(&output, template).unwrap_or_else(|e| die(&e.to_string()));
            println!("Created '{}'.", output);
            println!("Run it with:  rustyochestrator run {}", output);
        }

        Commands::Connect { url, token } => {
            // Test the connection before saving
            println!("Connecting to {}...", url);
            let client = reqwest::Client::new();
            match client
                .get(format!("{}/api/pipelines", url.trim_end_matches('/')))
                .bearer_auth(&token)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 401 => {
                    let user_login = decode_jwt_sub(&token).unwrap_or_else(|| "user".to_string());
                    let cfg = config::ConnectConfig {
                        dashboard_url: url.clone(),
                        token,
                        user_login: user_login.clone(),
                    };
                    config::save(&cfg).unwrap_or_else(|e| die(&e.to_string()));
                    println!("Connected as @{}", user_login);
                    println!("Dashboard: {}", url);
                    println!("Pipelines will now report live to the dashboard.");
                }
                Ok(resp) => {
                    die(&format!("dashboard returned HTTP {}", resp.status()));
                }
                Err(e) => {
                    die(&format!("could not reach dashboard: {}", e));
                }
            }
        }

        Commands::Disconnect => {
            config::delete().unwrap_or_else(|e| die(&e.to_string()));
            println!("Disconnected. Pipelines will no longer report to the dashboard.");
        }

        Commands::Status => match config::load() {
            Some(cfg) => {
                println!("Connected");
                println!("  Dashboard : {}", cfg.dashboard_url);
                println!("  User      : @{}", cfg.user_login);
            }
            None => {
                println!("Not connected.");
                println!("Run:  rustyochestrator connect --token <token> --url <dashboard-url>");
            }
        },

        Commands::RunAll {
            dir,
            concurrency,
            no_tui,
            verbose,
            log_file,
            keep_artifacts,
        } => {
            let workers = concurrency.unwrap_or_else(num_cpus::get);

            let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| {
                die(&format!("cannot read directory '{}': {}", dir, e));
            });

            let mut workflow_paths: Vec<std::path::PathBuf> = entries
                .filter_map(|e| {
                    let path = e.ok()?.path();
                    let ext = path.extension()?.to_str()?;
                    if ext == "yml" || ext == "yaml" {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect();

            workflow_paths.sort();

            if workflow_paths.is_empty() {
                eprintln!("error: no .yml/.yaml files found in '{}'", dir);
                std::process::exit(1);
            }

            tracing::info!(
                count = workflow_paths.len(),
                dir = %dir,
                "running workflows simultaneously"
            );

            // Set up artifact run ID
            let run_id = format!(
                "run-all-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
            unsafe {
                std::env::set_var("RUSTYORCH_RUN_ID", &run_id);
            }

            let reporter_cfg = config::load();

            // Shared log writer if specified
            let shared_log_writer = log_file.as_ref().map(|path| {
                executor::create_log_writer(path)
                    .unwrap_or_else(|e| die(&format!("cannot create log file '{}': {}", path, e)))
            });

            let workflows: Vec<(String, Pipeline)> = workflow_paths
                .iter()
                .map(|path| {
                    let path_str = path.to_string_lossy().to_string();
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("workflow")
                        .to_string();
                    let pipeline = load_pipeline(&path_str);
                    tracing::info!(workflow = %name, tasks = pipeline.tasks.len(), "loaded");
                    (name, pipeline)
                })
                .collect();

            let use_tui = !no_tui && !verbose && std::io::stdout().is_terminal();

            let mut handles = Vec::new();
            for (name, pipeline) in workflows {
                let cfg = reporter_cfg.clone();
                let lw = shared_log_writer.clone();
                let handle = tokio::spawn(async move {
                    let mut scheduler =
                        Scheduler::new(pipeline.clone(), workers).with_name(name.clone());
                    if let Some(c) = cfg {
                        let r = reporter::Reporter::new(c.dashboard_url.clone(), c.token.clone());
                        scheduler = scheduler.with_reporter(r);
                    }
                    if let Some(lw) = lw {
                        scheduler = scheduler.with_log_writer(lw);
                    }
                    if use_tui {
                        let task_ids: Vec<String> =
                            pipeline.levels().into_iter().flatten().collect();
                        let dashboard = Arc::new(tui::Dashboard::new(&name, &task_ids));
                        scheduler = scheduler.with_dashboard(dashboard);
                    }
                    let result = scheduler.run().await;
                    (name, result)
                });
                handles.push(handle);
            }

            let mut all_ok = true;
            for handle in handles {
                match handle.await {
                    Ok((name, Ok(true))) => {
                        tracing::info!(workflow = %name, "completed successfully");
                    }
                    Ok((name, Ok(false))) => {
                        eprintln!("error: workflow '{}' finished with failures", name);
                        all_ok = false;
                    }
                    Ok((name, Err(e))) => {
                        eprintln!("error: workflow '{}' error: {}", name, e);
                        all_ok = false;
                    }
                    Err(e) => {
                        eprintln!("error: a workflow task panicked: {}", e);
                        all_ok = false;
                    }
                }
            }

            cleanup_artifacts(&run_id, keep_artifacts);

            if !all_ok {
                std::process::exit(1);
            }
        }

        Commands::Report { markdown, json } => match report::RunReport::load() {
            Ok(r) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&r).unwrap());
                } else if markdown {
                    r.print_markdown();
                } else {
                    r.print_timing_summary();
                }
            }
            Err(_) => {
                eprintln!("No run report found. Run a pipeline first.");
                eprintln!("Reports are saved to .rustyochestrator/last-run.json");
                std::process::exit(1);
            }
        },
    }
}

fn load_pipeline(path: &str) -> Pipeline {
    let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
        die(&format!("cannot read '{}': {}", path, e));
    });

    let pipeline = if path.contains(".github") || path.contains("workflows") {
        tracing::info!("detected GitHub Actions workflow format");
        github::parse_github_workflow(&content).unwrap_or_else(|e| {
            die(&format!("failed to parse GitHub Actions workflow: {}", e));
        })
    } else {
        match Pipeline::from_yaml(&content) {
            Ok(p) => p,
            Err(_) => {
                tracing::info!("native parse failed – trying GitHub Actions format");
                github::parse_github_workflow(&content).unwrap_or_else(|e| {
                    die(&format!("failed to parse pipeline: {}", e));
                })
            }
        }
    };

    pipeline.validate().unwrap_or_else(|e| {
        die(&format!("invalid pipeline: {}", e));
    });

    pipeline
}

/// Clean up artifact directory unless --keep-artifacts was passed.
fn cleanup_artifacts(run_id: &str, keep: bool) {
    let artifact_dir = format!(".rustyochestrator/artifacts/{}", run_id);
    if std::path::Path::new(&artifact_dir).exists() {
        if keep {
            tracing::info!("keeping artifacts at: {}", artifact_dir);
        } else {
            let _ = std::fs::remove_dir_all(&artifact_dir);
        }
    }
}

/// Decode the `sub` claim from a JWT payload without verifying the signature.
fn decode_jwt_sub(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return None;
    }
    let padded = {
        let p = parts[1];
        let pad = (4 - p.len() % 4) % 4;
        format!("{}{}", p, "=".repeat(pad))
    };
    let bytes = base64_decode(&padded)?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    payload["sub"].as_str().map(|s| s.to_string())
}

/// Minimal base64url decoder (avoids adding a base64 crate dependency)
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let input = input.replace('-', "+").replace('_', "/");
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [0u8; 256];
    for (i, &c) in alphabet.iter().enumerate() {
        table[c as usize] = i as u8;
    }
    let chars: Vec<u8> = input.bytes().filter(|&b| b != b'=').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < chars.len() {
        let a = table[chars[i] as usize] as u32;
        let b = table[chars[i + 1] as usize] as u32;
        let c = table[chars[i + 2] as usize] as u32;
        let d = table[chars[i + 3] as usize] as u32;
        let n = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
        i += 4;
    }
    if i + 2 < chars.len() {
        let a = table[chars[i] as usize] as u32;
        let b = table[chars[i + 1] as usize] as u32;
        let c = table[chars[i + 2] as usize] as u32;
        let n = (a << 18) | (b << 12) | (c << 6);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
    } else if i + 1 < chars.len() {
        let a = table[chars[i] as usize] as u32;
        let b = table[chars[i + 1] as usize] as u32;
        let n = (a << 18) | (b << 12);
        out.push((n >> 16) as u8);
    }
    Some(out)
}

/// Load `.env` from the current directory into the process environment.
fn load_dotenv() {
    let Ok(content) = std::fs::read_to_string(".env") else {
        return;
    };
    for (key, value) in parse_dotenv(&content) {
        if std::env::var(&key).is_err() {
            unsafe {
                std::env::set_var(&key, &value);
            }
        }
    }
}

/// Parse the contents of a `.env` file into a list of (key, value) pairs.
fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        pairs.push((key.to_string(), value.to_string()));
    }
    pairs
}

fn die(msg: &str) -> ! {
    eprintln!("error: {}", msg);
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::parse_dotenv;

    // ── Parsing: basic formats ────────────────────────────────────────────────

    #[test]
    fn test_basic_key_value() {
        assert_eq!(
            parse_dotenv("FOO=bar"),
            vec![("FOO".to_string(), "bar".to_string())]
        );
    }

    #[test]
    fn test_export_prefix_stripped() {
        assert_eq!(
            parse_dotenv("export FOO=bar"),
            vec![("FOO".to_string(), "bar".to_string())]
        );
    }

    #[test]
    fn test_export_prefix_with_extra_spaces() {
        assert_eq!(
            parse_dotenv("export   FOO=bar"),
            vec![("FOO".to_string(), "bar".to_string())]
        );
    }

    #[test]
    fn test_whitespace_around_key_and_value() {
        assert_eq!(
            parse_dotenv("  FOO  =  bar  "),
            vec![("FOO".to_string(), "bar".to_string())]
        );
    }

    // ── Parsing: quoted values ────────────────────────────────────────────────

    #[test]
    fn test_double_quoted_value() {
        assert_eq!(
            parse_dotenv(r#"FOO="bar baz""#),
            vec![("FOO".to_string(), "bar baz".to_string())]
        );
    }

    #[test]
    fn test_single_quoted_value() {
        assert_eq!(
            parse_dotenv("FOO='bar baz'"),
            vec![("FOO".to_string(), "bar baz".to_string())]
        );
    }

    #[test]
    fn test_mismatched_quotes_not_stripped() {
        assert_eq!(
            parse_dotenv("FOO=\"bar'"),
            vec![("FOO".to_string(), "\"bar'".to_string())]
        );
    }

    #[test]
    fn test_value_with_equals_sign() {
        assert_eq!(
            parse_dotenv("URL=https://example.com?a=1&b=2"),
            vec![("URL".to_string(), "https://example.com?a=1&b=2".to_string())]
        );
    }

    #[test]
    fn test_empty_value() {
        assert_eq!(
            parse_dotenv("FOO="),
            vec![("FOO".to_string(), "".to_string())]
        );
    }

    // ── Parsing: skipped lines ────────────────────────────────────────────────

    #[test]
    fn test_comment_lines_skipped() {
        assert_eq!(parse_dotenv("# this is a comment"), vec![]);
    }

    #[test]
    fn test_inline_comment_not_stripped() {
        let pairs = parse_dotenv("FOO=bar # not a comment");
        assert_eq!(pairs[0].1, "bar # not a comment");
    }

    #[test]
    fn test_blank_lines_skipped() {
        assert_eq!(parse_dotenv("\n\n   \n"), vec![]);
    }

    #[test]
    fn test_line_without_equals_skipped() {
        assert_eq!(parse_dotenv("NOEQUALS"), vec![]);
    }

    #[test]
    fn test_empty_key_skipped() {
        assert_eq!(parse_dotenv("=value"), vec![]);
    }

    // ── Parsing: multi-line content ───────────────────────────────────────────

    #[test]
    fn test_multiple_entries_parsed_in_order() {
        let content = "FOO=1\nBAR=2\nBAZ=3";
        assert_eq!(
            parse_dotenv(content),
            vec![
                ("FOO".to_string(), "1".to_string()),
                ("BAR".to_string(), "2".to_string()),
                ("BAZ".to_string(), "3".to_string()),
            ]
        );
    }

    #[test]
    fn test_mixed_valid_and_skipped_lines() {
        let content = "\
# comment
FOO=bar

export BAR=baz
NOEQUALS
BAZ=qux";
        let pairs = parse_dotenv(content);
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], ("FOO".to_string(), "bar".to_string()));
        assert_eq!(pairs[1], ("BAR".to_string(), "baz".to_string()));
        assert_eq!(pairs[2], ("BAZ".to_string(), "qux".to_string()));
    }

    #[test]
    fn test_empty_content_returns_empty() {
        assert_eq!(parse_dotenv(""), vec![]);
    }

    // ── Env precedence ───────────────────────────────────────────────────────

    #[test]
    fn test_existing_env_var_not_overwritten() {
        let key = "RUSTYORCH_TEST_PRECEDENCE_A9F3";
        unsafe { std::env::set_var(key, "original") };

        let pairs = parse_dotenv(&format!("{}=should_not_win\n", key));
        assert_eq!(pairs.len(), 1);

        for (k, v) in &pairs {
            if std::env::var(k).is_err() {
                unsafe { std::env::set_var(k, v) };
            }
        }
        assert_eq!(std::env::var(key).unwrap(), "original");
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn test_absent_env_var_is_set() {
        let key = "RUSTYORCH_TEST_ABSENT_B7C1";
        unsafe { std::env::remove_var(key) };

        let pairs = parse_dotenv(&format!("{}=hello_dotenv\n", key));
        for (k, v) in &pairs {
            if std::env::var(k).is_err() {
                unsafe { std::env::set_var(k, v) };
            }
        }
        assert_eq!(std::env::var(key).unwrap(), "hello_dotenv");
        unsafe { std::env::remove_var(key) };
    }

    // ── Pipeline parsing: new v0.1.4 fields ──────────────────────────────────

    #[test]
    fn test_parse_pipeline_with_timeout_and_retries() {
        let yaml = r#"
defaults:
  timeout: "5m"
  retries: 3
  retry_delay: "2s"

tasks:
  - id: build
    command: "echo build"
    timeout: "10m"
    retries: 1

  - id: test
    command: "echo test"
    depends_on: [build]
    outputs: [RESULT]
    if: "$CI == 'true'"
"#;
        let pipeline = crate::pipeline::Pipeline::from_yaml(yaml).unwrap();
        assert_eq!(pipeline.tasks.len(), 2);

        let build = &pipeline.tasks[0];
        assert_eq!(build.timeout.as_deref(), Some("10m"));
        assert_eq!(build.retries, Some(1));

        let test = &pipeline.tasks[1];
        assert_eq!(test.outputs, vec!["RESULT".to_string()]);
        assert_eq!(test.condition.as_deref(), Some("$CI == 'true'"));

        // Pipeline defaults
        let defaults = pipeline.defaults.as_ref().unwrap();
        assert_eq!(defaults.timeout.as_deref(), Some("5m"));
        assert_eq!(defaults.retries, Some(3));

        // Effective values
        assert_eq!(
            pipeline.effective_timeout(build),
            Some(std::time::Duration::from_secs(600))
        );
        assert_eq!(pipeline.effective_retries(build), 1);
        assert_eq!(pipeline.effective_retries(test), 3); // falls back to default
    }

    #[test]
    fn test_parse_duration() {
        use crate::pipeline::parse_duration;
        assert_eq!(
            parse_duration("300s"),
            Some(std::time::Duration::from_secs(300))
        );
        assert_eq!(
            parse_duration("5m"),
            Some(std::time::Duration::from_secs(300))
        );
        assert_eq!(
            parse_duration("1h"),
            Some(std::time::Duration::from_secs(3600))
        );
        assert_eq!(
            parse_duration("1h30m"),
            Some(std::time::Duration::from_secs(5400))
        );
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("0s"), None);
    }

    #[test]
    fn test_condition_evaluation() {
        use crate::pipeline::{TaskState, evaluate_condition};
        use std::collections::HashMap;

        let env: HashMap<String, String> = [("ENV".to_string(), "production".to_string())]
            .into_iter()
            .collect();
        let results: HashMap<String, TaskState> = [
            ("build".to_string(), TaskState::Success),
            ("test".to_string(), TaskState::Failed),
        ]
        .into_iter()
        .collect();

        assert!(evaluate_condition("true", &env, &results));
        assert!(!evaluate_condition("false", &env, &results));
        assert!(evaluate_condition("$ENV == 'production'", &env, &results));
        assert!(!evaluate_condition("$ENV == 'staging'", &env, &results));
        assert!(evaluate_condition("$ENV != 'staging'", &env, &results));
        assert!(evaluate_condition(
            "tasks.build.result == 'success'",
            &env,
            &results
        ));
        assert!(evaluate_condition(
            "tasks.test.result == 'failure'",
            &env,
            &results
        ));
    }

    #[test]
    fn test_retry_delay_fixed() {
        use crate::pipeline::RetryDelay;
        let delay = RetryDelay::Fixed("5s".to_string());
        assert_eq!(
            delay.delay_for_attempt(0),
            std::time::Duration::from_secs(5)
        );
        assert_eq!(
            delay.delay_for_attempt(1),
            std::time::Duration::from_secs(5)
        );
    }

    #[test]
    fn test_retry_delay_exponential() {
        use crate::pipeline::RetryDelay;
        let delay = RetryDelay::Structured {
            strategy: "exponential".to_string(),
            base: "1s".to_string(),
        };
        assert_eq!(
            delay.delay_for_attempt(0),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            delay.delay_for_attempt(1),
            std::time::Duration::from_secs(2)
        );
        assert_eq!(
            delay.delay_for_attempt(2),
            std::time::Duration::from_secs(4)
        );
    }
}
