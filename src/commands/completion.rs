use crate::cli::Cli;
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use std::io;

pub fn cmd_generate_completion(shell: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let shell_type = match shell.as_deref() {
        Some("bash") => Shell::Bash,
        Some("zsh") => Shell::Zsh,
        Some("fish") => Shell::Fish,
        Some("powershell") | Some("pwsh") => Shell::PowerShell,
        Some("elvish") => Shell::Elvish,
        _ => {
            // 如果未指定，尝试根据环境判断，或默认为 bash
            Shell::Bash
        }
    };

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(shell_type, &mut cmd, bin_name, &mut io::stdout());

    Ok(())
}
