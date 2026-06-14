mod actions;
mod cli;
mod client;
mod config;
mod context;
mod sandbox;
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
        // No subcommand: drop straight into the interactive chat TUI,
        // the way `claude` does when invoked bare.
        None | Some(Command::Tui) => {
            actions::tui::handle(session_id, project_path, current_file).await
        }
        Some(Command::Prompt { text }) => {
            actions::prompt::handle(&text, session_id, project_path, current_file).await
        }
        Some(Command::Generate {
            lang,
            description,
            format,
        }) => {
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
        Some(Command::Run { code, lang }) => {
            let exit = actions::run::handle(code.as_deref(), lang.as_deref())?;
            std::process::exit(exit);
        }
        Some(Command::Session { action }) => actions::session::handle(action).await,
        Some(Command::Models) => actions::models::handle().await,
        Some(Command::Config { action }) => actions::config_cmd::handle(action),
    }
}
