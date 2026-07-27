use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

use crate::rules::RuleCode;

#[derive(Debug, Parser)]
#[command(
    name = "ddd-lint",
    version,
    about = "Opinionated domain-driven design lints for Python"
)]
pub struct Cli {
    /// Increase diagnostic logging. Repeat for more detail.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check Python files and directories.
    Check {
        /// Files or directories to check. Defaults to the current directory.
        paths: Vec<PathBuf>,
    },

    /// Explain a rule.
    Rule {
        /// Rule code, for example DM001.
        code: RuleCode,
    },

    /// Show the current configuration.
    Config,
}
