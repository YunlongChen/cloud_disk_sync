// src/cli/mod.rs
use clap::{Parser, Subcommand};

#[derive(Parser, Subcommand)]
#[command(name = "disksync")]
#[command(about = "A powerful cloud storage sync tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short, long, default_value = "~/.config/disksync/config.yaml")]
    pub config: String,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Add a new cloud storage account
    AddAccount {
        #[arg(short, long)]
        name: String,

        #[arg(short, long)]
        provider: String,

        #[arg(short, long)]
        token: Option<String>,
    },

    /// Create a new sync task
    CreateTask {
        #[arg(short, long)]
        name: String,

        #[arg(short, long)]
        source: String,

        #[arg(short, long)]
        target: String,

        #[arg(short = 's', long)]
        schedule: Option<String>,

        #[arg(short, long)]
        encrypt: bool,
    },

    /// Run a sync task
    Run {
        #[arg(short, long)]
        task: String,

        #[arg(short, long)]
        dry_run: bool,

        #[arg(short, long)]
        resume: bool,
    },

    /// List all tasks
    List,

    /// Show sync report
    Report {
        #[arg(short, long)]
        task: String,

        #[arg(short, long)]
        detailed: bool,
    },

    /// Verify data integrity
    Verify {
        #[arg(short, long)]
        task: String,

        #[arg(short = 'a', long)]
        all: bool,
    },

    /// Generate encryption key
    GenKey {
        #[arg(short, long)]
        name: String,

        #[arg(short, long)]
        strength: Option<u32>,
    },
}