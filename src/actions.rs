pub mod config_cmd;
pub mod run;
pub mod tui;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::context::PromptContext;
use crate::llm::Provider;
use crate::llm::openai::OpenAiProvider;
use crate::skills::SkillTracker;
use crate::tools::ToolRegistry;

pub struct Session {
    pub provider: Arc<dyn Provider>,
    pub registry: Arc<ToolRegistry>,
    pub ctx: PromptContext,
    pub cwd: PathBuf,
}

pub fn build_session(
    cfg: &Config,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
) -> Session {
    let provider: Arc<dyn Provider> = Arc::new(OpenAiProvider::from_config(cfg));
    // The registry owns the shared skill-progress tracker; the agent loop reads
    // it back via registry.tracker().
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);
    let cwd = project_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Session {
        provider,
        registry,
        ctx,
        cwd,
    }
}
