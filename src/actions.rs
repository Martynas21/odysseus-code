pub mod config_cmd;
pub mod models;
pub mod tui;

use anyhow::{Context, Result, bail};

use crate::client::OdysseusClient;
use crate::config::Config;
use crate::session::{DEFAULT_SESSION_NAME, SessionStore};

/// Resolve which server session the TUI should talk to, returning the friendly
/// session name the resolved ID is stored under (when there is one).
///
/// Order: explicit `--session-id` (local name, else raw server ID) → a lazily
/// created/reused server session named "odysseus-code". A raw server-ID launch
/// has no friendly name, so the name is `None`. Callers that may later remap the
/// session (e.g. the TUI's `/clear`) need the name to update the store. New
/// mappings are persisted to the store.
pub async fn resolve_session_named(
    client: &OdysseusClient,
    cfg: &Config,
    store: &mut SessionStore,
    explicit: Option<&str>,
) -> Result<(Option<String>, String)> {
    if let Some(wanted) = explicit {
        if let Some(id) = store.server_id(wanted) {
            return Ok((Some(wanted.to_string()), id.to_string()));
        }
        // Not a known local name — assume it's a raw server session ID.
        return Ok((None, wanted.to_string()));
    }

    // Cached default from a previous run?
    if let Some(id) = store.server_id(DEFAULT_SESSION_NAME) {
        return Ok((Some(DEFAULT_SESSION_NAME.to_string()), id.to_string()));
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
    Ok((Some(DEFAULT_SESSION_NAME.to_string()), session.id))
}

/// Create a server session, resolving endpoint/model from config when set,
/// else from the first endpoint advertised by `GET /api/models`.
// The configured endpoint id was dropped from Config in the agent pivot, so the
// "honor configured endpoint" branch is now dead; this whole helper is removed
// in a later phase.
#[allow(clippy::const_is_empty)]
pub async fn create_session(
    client: &OdysseusClient,
    cfg: &Config,
    name: &str,
) -> Result<crate::client::SessionInfo> {
    let configured_endpoint_id = "";
    let (endpoint_id, model) = if !configured_endpoint_id.is_empty() {
        // Honor the configured endpoint_id; never override it from /api/models.
        if !cfg.model.is_empty() {
            // Both set: fast path, no need to hit the backend.
            (configured_endpoint_id.to_string(), cfg.model.clone())
        } else {
            // Endpoint set but model left to "first available": resolve a model
            // for THAT endpoint from /api/models without changing the endpoint.
            let endpoints = client.list_models().await?;
            let pick = endpoints
                .iter()
                .find(|e| e.endpoint_id == configured_endpoint_id)
                .with_context(|| {
                    format!(
                        "configured endpoint_id '{}' not found on the Odysseus backend; \
                         run `odysseus-code models` to see what is available",
                        configured_endpoint_id
                    )
                })?;
            let model = pick.first_model().with_context(|| {
                format!(
                    "configured endpoint_id '{}' has no available models; \
                     run `odysseus-code models` to see what is available",
                    configured_endpoint_id
                )
            })?;
            (configured_endpoint_id.to_string(), model)
        }
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
            pick.first_model().unwrap_or_default()
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
