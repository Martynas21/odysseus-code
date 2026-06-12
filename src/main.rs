mod actions;
mod cli;
mod client;
mod config;
mod context;
mod session;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let session_id = cli.session_id.as_deref();
    let project_path = cli.project_path.as_deref();
    let current_file = cli.current_file.as_deref();

    match cli.command {
        Command::Prompt { text } => {
            actions::prompt::handle(&text, session_id, project_path, current_file).await
        }
        Command::Generate {
            lang,
            description,
            format,
        } => {
            actions::generate::handle(
                &lang,
                &description,
                format,
                session_id,
                project_path,
                current_file,
            )
            .await
        }
        Command::Run { .. } => anyhow::bail!("run: not implemented yet"),
        Command::Session { .. } => anyhow::bail!("session: not implemented yet"),
        Command::Models => actions::models::handle().await,
        Command::Config { action } => actions::config_cmd::handle(action),
        Command::Tui => anyhow::bail!("tui: not implemented yet"),
    }
}
