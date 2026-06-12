pub mod config_cmd;
pub mod models;
pub mod prompt;

use anyhow::{Context, Result, bail};

use crate::client::OdysseusClient;
use crate::config::Config;
use crate::session::{DEFAULT_SESSION_NAME, SessionStore};

/// Resolve which server session a command should talk to.
///
/// Order: explicit `--session-id` (local name, else raw server ID) → the
/// active session set by `session start` → a lazily created/reused server
/// session named "odysseus-code". New mappings are persisted to the store.
pub async fn resolve_session(
    client: &OdysseusClient,
    cfg: &Config,
    store: &mut SessionStore,
    explicit: Option<&str>,
) -> Result<String> {
    if let Some(wanted) = explicit {
        if let Some(id) = store.server_id(wanted) {
            return Ok(id.to_string());
        }
        // Not a known local name — assume it's a raw server session ID.
        return Ok(wanted.to_string());
    }

    if let Some((_, id)) = store.active() {
        return Ok(id.to_string());
    }

    // Cached default from a previous run?
    if let Some(id) = store.server_id(DEFAULT_SESSION_NAME) {
        return Ok(id.to_string());
    }

    // Reuse a server session named "odysseus-code" if one exists…
    let existing = client
        .list_sessions()
        .await?
        .into_iter()
        .find(|s| s.name == DEFAULT_SESSION_NAME);
    let session = match existing {
        Some(s) => s,
        // …otherwise create one.
        None => create_session(client, cfg, DEFAULT_SESSION_NAME).await?,
    };
    store.insert(DEFAULT_SESSION_NAME, &session.id);
    store.save()?;
    Ok(session.id)
}

/// Create a server session, resolving endpoint/model from config when set,
/// else from the first endpoint advertised by `GET /api/models`.
pub async fn create_session(
    client: &OdysseusClient,
    cfg: &Config,
    name: &str,
) -> Result<crate::client::SessionInfo> {
    let (endpoint_id, model) = if !cfg.endpoint_id.is_empty() && !cfg.model.is_empty() {
        (cfg.endpoint_id.clone(), cfg.model.clone())
    } else {
        let endpoints = client.list_models().await?;
        let pick = endpoints
            .iter()
            .find(|e| {
                !e.endpoint_id.is_empty()
                    && (!e.models.is_empty() || !e.models_extra.is_empty())
                    && (cfg.model.is_empty()
                        || e.models
                            .iter()
                            .chain(&e.models_extra)
                            .any(|m| m == &cfg.model))
            })
            .with_context(|| {
                if cfg.model.is_empty() {
                    "no model endpoints available on the Odysseus backend; \
                     add one in Odysseus Settings or run `odysseus-code models`"
                        .to_string()
                } else {
                    format!(
                        "model '{}' not found on any Odysseus endpoint; \
                         run `odysseus-code models` to see what is available",
                        cfg.model
                    )
                }
            })?;
        let model = if cfg.model.is_empty() {
            pick.models
                .first()
                .or_else(|| pick.models_extra.first())
                .cloned()
                .unwrap_or_default()
        } else {
            cfg.model.clone()
        };
        (pick.endpoint_id.clone(), model)
    };

    if model.is_empty() {
        bail!(
            "could not resolve a model for new sessions; set one with \
             `odysseus-code config set model <id>` (see `odysseus-code models`)"
        );
    }
    Ok(client.create_session(name, &endpoint_id, &model).await?)
}
