// src/cli/mod.rs
pub mod info;
pub mod browse;

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
    /// Account management commands
    #[command(subcommand)]
    Account(AccountCmd),

    /// Task management commands
    #[command(subcommand)]
    Tasks(TaskCmd),

    /// Run a sync task
    Run {
        #[arg(short, long)]
        task: String,

        #[arg(short, long)]
        dry_run: bool,

        #[arg(short, long)]
        resume: bool,
    },

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

    /// Generate shell completions
    Completion {
        /// Shell type (bash, zsh, fish, powershell, elvish)
        #[arg(short, long)]
        shell: Option<String>,
    },

    /// Show file differences for a sync task
    Diff {
        /// Task ID or Name (positional)
        name_or_id: Option<String>,

        /// Task ID or Name
        #[arg(short = 't', long = "task")]
        id: Option<String>,
    },

    /// Show system and program info
    Info,
}

#[derive(Subcommand)]
pub enum AccountCmd {
    /// Create a new cloud storage account
    Create {
        /// Account Name (positional)
        name_or_id: Option<String>,

        #[arg(short, long)]
        name: Option<String>,

        #[arg(short, long)]
        provider: Option<String>,

        #[arg(short, long)]
        token: Option<String>,
    },
    /// List all accounts
    List,
    /// Remove an account
    Remove {
        /// Account ID or Name
        #[arg(short, long)]
        id: Option<String>,

        /// Account ID or Name (positional)
        name_or_id: Option<String>,

        /// Force removal without confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Update an account
    Update {
        /// Account ID or Name
        #[arg(short, long)]
        id: Option<String>,

        /// Account ID or Name (positional)
        name_or_id: Option<String>,

        /// New name (optional)
        #[arg(short, long)]
        name: Option<String>,

        /// New token/credential (optional)
        #[arg(short, long)]
        token: Option<String>,
    },
    /// Check account status
    Status {
        /// Account ID or Name
        #[arg(short, long)]
        id: Option<String>,

        /// Account ID or Name (positional)
        name_or_id: Option<String>,
    },
    /// Browse files in an account
    Browse {
        /// Account ID or Name
        #[arg(short, long)]
        id: Option<String>,

        /// Account ID or Name (positional)
        name_or_id: Option<String>,

        /// Path to browse
        #[arg(short, long)]
        path: Option<String>,

        /// Path to browse (positional)
        path_pos: Option<String>,

        /// Recursively list files
        #[arg(short, long)]
        recursive: bool,

        /// Show detailed file info
        #[arg(short, long)]
        detail: bool,
    },
}

#[derive(Subcommand)]
pub enum TaskCmd {
    /// Create a new sync task
    Create {
        /// Task Name (positional)
        name_or_id: Option<String>,

        #[arg(short, long)]
        name: Option<String>,

        #[arg(short, long)]
        source: Option<String>,

        #[arg(short, long)]
        target: Option<String>,

        #[arg(long)] // Removed short alias 's' to avoid conflict
        schedule: Option<String>,

        #[arg(short, long)]
        encrypt: bool,
    },
    /// List all tasks
    List,
    /// Remove a sync task
    Remove {
        /// Task ID or Name
        #[arg(short, long)]
        id: Option<String>,

        /// Task ID or Name (positional)
        name_or_id: Option<String>,

        /// Task Name (optional, deprecated)
        #[arg(short, long)]
        name: Option<String>,

        /// Force removal without confirmation
        #[arg(short, long)]
        force: bool,
    },
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

    #[test]
    fn test_cli_parse_account_create() {
        let args = [
            "cloud-disk-sync",
            "account",
            "create",
            "--name",
            "ali",
            "--provider",
            "aliyun",
        ];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Create { name, provider, .. }) => {
                assert_eq!(name, Some("ali".to_string()));
                assert_eq!(provider, Some("aliyun".to_string()));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_create_positional() {
        let args = [
            "cloud-disk-sync",
            "account",
            "create",
            "ali",
        ];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Create { name_or_id, name, provider, .. }) => {
                assert_eq!(name_or_id, Some("ali".to_string()));
                assert_eq!(name, None);
                assert_eq!(provider, None);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_remove() {
        let args = ["cloud-disk-sync", "account", "remove", "--id", "acc1"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Remove { id, name_or_id, force }) => {
                assert_eq!(id, Some("acc1".to_string()));
                assert_eq!(name_or_id, None);
                assert_eq!(force, false);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_update() {
        let args = ["cloud-disk-sync", "account", "update", "--id", "acc1", "--name", "new_name"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Update { id, name_or_id, name, token }) => {
                assert_eq!(id, Some("acc1".to_string()));
                assert_eq!(name_or_id, None);
                assert_eq!(name, Some("new_name".to_string()));
                assert_eq!(token, None);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_remove_positional() {
        let args = ["cloud-disk-sync", "account", "remove", "my-webdav"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Remove { id, name_or_id, force }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, Some("my-webdav".to_string()));
                assert_eq!(force, false);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_remove_force() {
        let args = ["cloud-disk-sync", "account", "remove", "my-webdav", "-f"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Remove { id, name_or_id, force }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, Some("my-webdav".to_string()));
                assert_eq!(force, true);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_update_positional() {
        let args = ["cloud-disk-sync", "account", "update", "my-webdav", "--name", "new_name"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Update { id, name_or_id, name, token }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, Some("my-webdav".to_string()));
                assert_eq!(name, Some("new_name".to_string()));
                assert_eq!(token, None);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_account_status_positional() {
        let args = ["cloud-disk-sync", "account", "status", "my-webdav"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Account(AccountCmd::Status { id, name_or_id }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, Some("my-webdav".to_string()));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_task_create_positional() {
        let args = ["cloud-disk-sync", "tasks", "create", "test-task"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Tasks(TaskCmd::Create { name_or_id, name, .. }) => {
                assert_eq!(name_or_id, Some("test-task".to_string()));
                assert_eq!(name, None);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_task_create_full() {
        let args = ["cloud-disk-sync", "tasks", "create", "test-task", "--source", "acc1", "--target", "acc2"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Tasks(TaskCmd::Create { name_or_id, source, target, .. }) => {
                assert_eq!(name_or_id, Some("test-task".to_string()));
                assert_eq!(source, Some("acc1".to_string()));
                assert_eq!(target, Some("acc2".to_string()));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_task_remove_positional() {
        let args = ["cloud-disk-sync", "tasks", "remove", "task_35d"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Tasks(TaskCmd::Remove { id, name_or_id, name, force }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, Some("task_35d".to_string()));
                assert_eq!(name, None);
                assert_eq!(force, false);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_task_remove_id() {
        let args = ["cloud-disk-sync", "tasks", "remove", "--id", "task_35d"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Tasks(TaskCmd::Remove { id, name_or_id, name, force }) => {
                assert_eq!(id, Some("task_35d".to_string()));
                assert_eq!(name_or_id, None);
                assert_eq!(name, None);
                assert_eq!(force, false);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_task_remove_name() {
        let args = ["cloud-disk-sync", "tasks", "remove", "--name", "test-task"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Tasks(TaskCmd::Remove { id, name_or_id, name, force }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, None);
                assert_eq!(name, Some("test-task".to_string()));
                assert_eq!(force, false);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_task_remove_force() {
        let args = ["cloud-disk-sync", "tasks", "remove", "task_35d", "-f"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Tasks(TaskCmd::Remove { id, name_or_id, name, force }) => {
                assert_eq!(id, None);
                assert_eq!(name_or_id, Some("task_35d".to_string()));
                assert_eq!(name, None);
                assert_eq!(force, true);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_diff_positional() {
        let args = ["cloud-disk-sync", "diff", "test-task"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Diff { name_or_id, id } => {
                assert_eq!(name_or_id, Some("test-task".to_string()));
                assert_eq!(id, None);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_cli_parse_diff_named() {
        let args = ["cloud-disk-sync", "diff", "--task", "task_123"];
        let cli = Cli::parse_from(&args);
        match cli.command {
            Commands::Diff { name_or_id, id } => {
                assert_eq!(name_or_id, None);
                assert_eq!(id, Some("task_123".to_string()));
            }
            _ => panic!("unexpected command"),
        }
    }
}
