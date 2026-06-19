pub mod config_cmd;
pub mod run;
pub mod tui;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::context::PromptContext;
use crate::llm::Provider;
use crate::llm::message::ChatMessage;
use crate::llm::openai::OpenAiProvider;
use crate::mode::{Mode, SPEC_DIR};

pub struct Session {
    pub provider: Arc<dyn Provider>,
    pub ctx: PromptContext,
    pub cwd: PathBuf,
}

pub fn build_session(
    cfg: &Config,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
) -> Session {
    let provider: Arc<dyn Provider> = Arc::new(OpenAiProvider::from_config(cfg));
    let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);
    let cwd = project_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Session { provider, ctx, cwd }
}

/// List existing specification files (relative paths under docs/edds/), sorted.
pub fn list_spec_docs(cwd: &Path) -> Vec<String> {
    let dir = cwd.join(SPEC_DIR);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                out.push(format!("{SPEC_DIR}/{name}"));
            }
        }
    }
    out.sort();
    out
}

/// Build the system message for a turn, listing existing spec documents into
/// context. Used by both the TUI and the non-interactive run so the prompt is
/// assembled one consistent way.
pub fn system_message_for(ctx: &PromptContext, mode: Mode, cwd: &Path) -> ChatMessage {
    let specs = list_spec_docs(cwd);
    ChatMessage::system(ctx.system_prompt(mode, &specs))
}
