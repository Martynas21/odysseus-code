use anyhow::Result;

use crate::client::OdysseusClient;
use crate::config::Config;

pub async fn handle() -> Result<()> {
    let cfg = Config::load()?;
    let client = OdysseusClient::from_config(&cfg)?;
    let endpoints = client.list_models().await?;

    if endpoints.is_empty() {
        println!("No model endpoints configured on the Odysseus backend.");
        println!("Add one in the Odysseus web UI (Settings → Models).");
        return Ok(());
    }

    for ep in endpoints {
        println!("{} (endpoint_id: {})", ep.endpoint_name, ep.endpoint_id);
        for model in ep.models.iter().chain(&ep.models_extra) {
            println!("  {model}");
        }
    }
    println!();
    println!("Pick defaults for new sessions with:");
    println!("  odysseus-code config set endpoint_id <endpoint_id>");
    println!("  odysseus-code config set model <model>");
    Ok(())
}
