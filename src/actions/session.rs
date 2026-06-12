use anyhow::Result;

use crate::cli::SessionAction;
use crate::client::OdysseusClient;
use crate::config::Config;
use crate::session::SessionStore;

pub async fn handle(action: SessionAction) -> Result<()> {
    let cfg = Config::load()?;
    let client = OdysseusClient::from_config(&cfg)?;
    let mut store = SessionStore::load()?;

    match action {
        SessionAction::Start { id } => {
            if let Some(server_id) = store.server_id(&id) {
                println!("session '{id}' already exists ({server_id}); now active");
                store.set_active(&id);
                store.save()?;
                return Ok(());
            }
            let info = super::create_session(&client, &cfg, &id).await?;
            store.insert(&id, &info.id);
            store.set_active(&id);
            store.save()?;
            println!(
                "session '{id}' started (server id {}, model {}); now active",
                info.id, info.model
            );
        }
        SessionAction::End { id } => {
            // Known local name, else assume a raw server session ID.
            let server_id = store
                .server_id(&id)
                .map(str::to_string)
                .unwrap_or_else(|| id.clone());
            client.delete_session(&server_id).await?;
            store.remove(&id);
            store.save()?;
            println!("session '{id}' ended ({server_id} deleted)");
        }
    }
    Ok(())
}
