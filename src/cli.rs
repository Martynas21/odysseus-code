use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A local coding agent that talks directly to an OpenAI-compatible endpoint.
#[derive(Debug, Parser)]
#[command(name = "odysseus-code", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Absolute path of the workspace the agent operates in (defaults to ".")
    #[arg(long, global = true)]
    pub project_path: Option<PathBuf>,

    /// Path of the file currently being edited (context only)
    #[arg(long, global = true)]
    pub current_file: Option<PathBuf>,

    /// Override the configured model id for this run
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// Override the configured base URL for this run
    #[arg(long, global = true)]
    pub base_url: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read or modify the configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Open the interactive agent TUI
    Tui,
    /// Run a single agent turn non-interactively, streaming the reply to stdout
    Run {
        /// The prompt to send to the agent
        prompt: String,
        /// Auto-approve mutating tools (otherwise they are auto-denied)
        #[arg(long)]
        yes: bool,
        /// Skip the model's chain-of-thought for a faster, direct answer
        #[arg(long)]
        no_think: bool,
        /// Agent mode: "implement" (default) builds the feature; "spec" produces
        /// a specification document and makes no code changes
        #[arg(long, default_value = "implement")]
        mode: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Set a configuration key (base_url, api_key, model, temperature,
    /// max_tokens, tool_timeout_secs, approval_policy, default_language)
    Set { key: String, value: String },
    /// Print one configuration value, or the whole config if no key is given
    Get { key: Option<String> },
    /// Print the path of the configuration file
    Path,
}
