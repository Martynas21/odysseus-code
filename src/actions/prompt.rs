use std::path::Path;

use anyhow::Result;

use crate::client::OdysseusClient;
use crate::config::Config;
use crate::context::PromptContext;
use crate::session::SessionStore;

pub async fn handle(
    text: &str,
    session_id: Option<&str>,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
) -> Result<()> {
    let cfg = Config::load()?;
    let client = OdysseusClient::from_config(&cfg)?;
    let mut store = SessionStore::load()?;

    let session = super::resolve_session(&client, &cfg, &mut store, session_id).await?;
    let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);
    let reply = client.chat(&session, &ctx.wrap(text)).await?;
    println!("{reply}");
    Ok(())
}
