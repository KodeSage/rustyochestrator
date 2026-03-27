mod cache;
mod cli;
mod config;
mod errors;
mod executor;
mod github;
mod pipeline;
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
        Commands::Run { no_tui, .. } => !no_tui && std::io::stdout().is_terminal(),
        Commands::RunAll { no_tui, .. } => !no_tui && std::io::stdout().is_terminal(),
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
        } => {
            let pipeline = load_pipeline(&path);
            let workers = concurrency.unwrap_or_else(num_cpus::get);

            // Derive a pipeline name from the file path
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("pipeline")
                .to_string();

            let use_tui = !no_tui && std::io::stdout().is_terminal();

            if !use_tui {
                tracing::info!(workers, tasks = pipeline.tasks.len(), "pipeline starting");
            }

            // Build scheduler
            let mut scheduler = Scheduler::new(pipeline.clone(), workers).with_name(name.clone());

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
                    if !use_tui {
                        tracing::info!("pipeline completed successfully");
                    }
                }
                Ok(false) => {
                    if !use_tui {
                        eprintln!("error: pipeline finished with failures");
                    }
                    std::process::exit(1);
                }
                Err(e) => die(&e.to_string()),
            }
        }

        Commands::Validate { pipeline: path } => {
            let pipeline = load_pipeline(&path);
            println!("  {} tasks", pipeline.tasks.len());
            for task in &pipeline.tasks {
                if task.depends_on.is_empty() {
                    println!("  [ok] {}", task.id);
                } else {
                    println!(
                        "  [ok] {}  (needs: {})",
                        task.id,
                        task.depends_on.join(", ")
                    );
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
            let template = r#"tasks:
  - id: build
    command: "echo building..."

  - id: test
    command: "echo testing..."
    depends_on: [build]

  - id: deploy
    command: "echo deploying..."
    depends_on: [test]
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
                    // 401 means the server is reachable (just auth check)
                    // Extract user login from JWT payload (base64url decode, no verification needed client-side)
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
        } => {
            let workers = concurrency.unwrap_or_else(num_cpus::get);

            // Collect all .yml / .yaml files in the directory
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

            workflow_paths.sort(); // deterministic order

            if workflow_paths.is_empty() {
                eprintln!("error: no .yml/.yaml files found in '{}'", dir);
                std::process::exit(1);
            }

            tracing::info!(
                count = workflow_paths.len(),
                dir = %dir,
                "running workflows simultaneously"
            );

            let reporter_cfg = config::load();

            // Load all pipelines up front so errors surface before any execution starts
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

            let use_tui = !no_tui && std::io::stdout().is_terminal();

            // Spawn each workflow as a concurrent tokio task
            let mut handles = Vec::new();
            for (name, pipeline) in workflows {
                let cfg = reporter_cfg.clone();
                let handle = tokio::spawn(async move {
                    let mut scheduler =
                        Scheduler::new(pipeline.clone(), workers).with_name(name.clone());
                    if let Some(c) = cfg {
                        let r = reporter::Reporter::new(c.dashboard_url.clone(), c.token.clone());
                        scheduler = scheduler.with_reporter(r);
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

            // Await all workflows and report final status
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

            if !all_ok {
                std::process::exit(1);
            }
        }
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

/// Decode the `sub` claim from a JWT payload without verifying the signature.
fn decode_jwt_sub(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return None;
    }
    // Base64url decode (pad to multiple of 4)
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
    // Use MIME base64 table
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
    // Handle remaining bytes
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
///
/// Rules:
/// - Lines starting with `#` and blank lines are ignored.
/// - `export KEY=VALUE` and `KEY=VALUE` are both accepted.
/// - Values wrapped in matching `"..."` or `'...'` are unquoted.
/// - Variables already present in the environment are NOT overwritten, so a
///   real shell export always takes precedence over the file.
fn load_dotenv() {
    let Ok(content) = std::fs::read_to_string(".env") else {
        return; // no .env file — silently skip
    };
    for (key, value) in parse_dotenv(&content) {
        if std::env::var(&key).is_err() {
            // SAFETY: load_dotenv() is called from the sync `main()` before `async_main()`
            // starts the tokio runtime, so no other threads exist yet.
            unsafe {
                std::env::set_var(&key, &value);
            }
        }
    }
}

/// Parse the contents of a `.env` file into a list of (key, value) pairs.
/// Pure function — no I/O, no environment mutation — safe to unit test.
fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // strip optional leading "export "
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        // strip a single layer of matching surrounding quotes
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
        // opening " but closing ' — not a matching pair, kept as-is
        assert_eq!(
            parse_dotenv("FOO=\"bar'"),
            vec![("FOO".to_string(), "\"bar'".to_string())]
        );
    }

    #[test]
    fn test_value_with_equals_sign() {
        // only the first '=' splits key/value; the rest belongs to the value
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
        // inline comments are not part of the .env spec — the '#' is part of the value
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

    // ── Env precedence (tests set_var directly; unique keys avoid collisions) ─

    #[test]
    fn test_existing_env_var_not_overwritten() {
        let key = "RUSTYORCH_TEST_PRECEDENCE_A9F3";
        unsafe { std::env::set_var(key, "original") };

        // parse_dotenv returns the pair, but load_dotenv skips keys already set
        let pairs = parse_dotenv(&format!("{}=should_not_win\n", key));
        assert_eq!(pairs.len(), 1); // parse_dotenv itself doesn't check the env

        // simulate what load_dotenv does: only set if absent
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
        // ensure it is not already set
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
}
