mod cli;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Prompt { .. } => anyhow::bail!("prompt: not implemented yet"),
        Command::Generate { .. } => anyhow::bail!("generate: not implemented yet"),
        Command::Run { .. } => anyhow::bail!("run: not implemented yet"),
        Command::Session { .. } => anyhow::bail!("session: not implemented yet"),
        Command::Models => anyhow::bail!("models: not implemented yet"),
        Command::Config { .. } => anyhow::bail!("config: not implemented yet"),
        Command::Tui => anyhow::bail!("tui: not implemented yet"),
    }
}
