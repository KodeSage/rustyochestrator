mod cache;
mod cli;
mod config;
mod errors;
mod executor;
mod github;
mod pipeline;
mod reporter;
mod scheduler;

use clap::Parser;
use cli::{CacheCommands, Cli, Commands};
use pipeline::Pipeline;
use scheduler::Scheduler;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            pipeline: path,
            concurrency,
        } => {
            let pipeline = load_pipeline(&path);
            let workers = concurrency.unwrap_or_else(num_cpus::get);
            tracing::info!(workers, tasks = pipeline.tasks.len(), "pipeline starting");

            // Derive a pipeline name from the file path
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("pipeline")
                .to_string();

            // Build scheduler, attach reporter if connected to a dashboard
            let mut scheduler = Scheduler::new(pipeline, workers).with_name(name);
            if let Some(cfg) = config::load() {
                let r = reporter::Reporter::new(cfg.dashboard_url.clone(), cfg.token.clone());
                scheduler = scheduler.with_reporter(r);
                tracing::info!("reporting to dashboard: {}", cfg.dashboard_url);
            }

            match scheduler.run().await {
                Ok(true) => tracing::info!("pipeline completed successfully"),
                Ok(false) => {
                    eprintln!("error: pipeline finished with failures");
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

        Commands::RunAll { dir, concurrency } => {
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

            // Spawn each workflow as a concurrent tokio task
            let mut handles = Vec::new();
            for (name, pipeline) in workflows {
                let cfg = reporter_cfg.clone();
                let handle = tokio::spawn(async move {
                    let mut scheduler = Scheduler::new(pipeline, workers).with_name(name.clone());
                    if let Some(c) = cfg {
                        let r = reporter::Reporter::new(c.dashboard_url.clone(), c.token.clone());
                        scheduler = scheduler.with_reporter(r);
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

fn die(msg: &str) -> ! {
    eprintln!("error: {}", msg);
    std::process::exit(1);
}
