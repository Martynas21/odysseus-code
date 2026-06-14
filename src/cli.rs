use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A command-line harness that turns the web-only Odysseus API into a
/// local coding assistant.
#[derive(Debug, Parser)]
#[command(name = "odysseus-code", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Server session to attach to (local name or raw ID); defaults to a
    /// session named "odysseus-code"
    #[arg(long, global = true)]
    pub session_id: Option<String>,

    /// Absolute path of the workspace sent as context (defaults to ".")
    #[arg(long, global = true)]
    pub project_path: Option<PathBuf>,

    /// Path of the file being edited, sent as context
    #[arg(long, global = true)]
    pub current_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List models available on the Odysseus backend
    Models,

    /// Read or modify the configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Open an interactive chat TUI
    Tui,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Set a configuration key (endpoint, api_key, model, endpoint_id, default_language)
    Set { key: String, value: String },
    /// Print one configuration value, or the whole config if no key is given
    Get { key: Option<String> },
    /// Print the path of the configuration file
    Path,
}
