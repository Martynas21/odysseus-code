//! Tool registry. Phase-3 stub: an empty registry that advertises no tools.
//! Task 4.1 replaces this file with the real trait, registry, and built-in
//! tools (fs_read, list_dir, write_file, edit_file, grep, shell).

use crate::llm::ToolDef;

/// The set of tools available to the agent. Empty for Phase 3.
#[derive(Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    /// The default tool set. Empty until Phase 4 wires in the built-ins.
    pub fn default_set() -> ToolRegistry {
        ToolRegistry
    }

    /// Tool definitions advertised to the model.
    pub fn defs(&self) -> Vec<ToolDef> {
        vec![]
    }
}
