// src/cli/mod.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cloud-disk-sync")]
#[command(about = "A powerful cloud storage sync tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(short, long, default_value = "~/.config/cloud-disk-sync/config.yaml")]
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

    /// List all plugins
    Plugins,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_cli_parse_run() {
        let args = ["cloud-disk-sync", "run", "--task", "t1", "--dry-run"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Run { task, dry_run, .. } => {
                assert_eq!(task, "t1");
                assert!(dry_run);
            }
            _ => panic!("unexpected command"),
        }
    }
}
