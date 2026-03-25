use clap::{Args, Parser, Subcommand};

/// High-performance CI/CD pipeline runner
#[derive(Parser)]
#[command(name = "rustyochestrator", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Execute a pipeline
    Run {
        /// Path to pipeline YAML (native or GitHub Actions format)
        pipeline: String,
        /// Maximum concurrent tasks [default: num_cpus]
        #[arg(short, long)]
        concurrency: Option<usize>,
    },

    /// Validate a pipeline without running it
    Validate {
        /// Path to pipeline YAML
        pipeline: String,
    },

    /// List tasks in execution order
    List {
        /// Path to pipeline YAML
        pipeline: String,
    },

    /// Print an ASCII dependency graph
    Graph {
        /// Path to pipeline YAML
        pipeline: String,
    },

    /// Manage the task cache
    Cache(CacheArgs),

    /// Scaffold a new pipeline.yaml in the current directory
    Init {
        /// Output file name [default: pipeline.yaml]
        #[arg(default_value = "pipeline.yaml")]
        output: String,
    },

    /// Connect this CLI to a dashhy dashboard
    Connect {
        /// Dashboard URL (e.g. https://dashhy.vercel.app)
        #[arg(long)]
        url: String,
        /// Authentication token from the dashboard
        #[arg(long)]
        token: String,
    },

    /// Disconnect from the dashhy dashboard
    Disconnect,

    /// Show connection status
    Status,
}

#[derive(Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommands,
}

#[derive(Subcommand)]
pub enum CacheCommands {
    /// Show all cached task entries
    Show,
    /// Delete the cache
    Clean,
}
