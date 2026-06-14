use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// A command-line harness that turns the web-only Odysseus API into a
/// local coding assistant.
#[derive(Debug, Parser)]
#[command(name = "odysseus-code", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Session ID to use for this command (overrides the active session)
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
    /// Send a natural-language prompt and print the reply as plain text
    Prompt {
        /// The prompt text
        text: String,
    },

    /// Generate a code snippet in the given language
    Generate {
        /// Target programming language (e.g. rust, python)
        lang: String,
        /// What the snippet should do
        description: String,
        /// Output format
        #[arg(long, value_enum, default_value_t = OutputFormat::Pretty)]
        format: OutputFormat,
    },

    /// Compile and run a code snippet inside a sandboxed container
    Run {
        /// The code snippet; omit or pass "-" to read from stdin
        code: Option<String>,
        /// Language of the snippet (defaults to config default_language)
        #[arg(long)]
        lang: Option<String>,
    },

    /// Create or close a session context for multi-step interactions
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },

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
pub enum SessionAction {
    /// Start a new session with the given ID and make it active
    Start { id: String },
    /// End the session with the given ID
    End { id: String },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Set a configuration key (endpoint, api_key, model, default_language, sandbox_image)
    Set { key: String, value: String },
    /// Print one configuration value, or the whole config if no key is given
    Get { key: Option<String> },
    /// Print the path of the configuration file
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Fenced markdown code block with a language tag
    Pretty,
    /// Raw code only
    Compact,
}
