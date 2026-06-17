mod actions;
mod agent;
mod cli;
mod config;
mod context;
mod llm;
mod tools;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let project_path = cli.project_path.as_deref();
    let current_file = cli.current_file.as_deref();
    let model = cli.model.as_deref();
    let base_url = cli.base_url.as_deref();

    match cli.command {
        None | Some(Command::Tui) => {
            actions::tui::handle(project_path, current_file, model, base_url).await
        }
        Some(Command::Config { action }) => actions::config_cmd::handle(action),
        Some(Command::Run { prompt, yes }) => {
            actions::run::handle(prompt, yes, project_path, current_file, model, base_url).await
        }
    }
}
