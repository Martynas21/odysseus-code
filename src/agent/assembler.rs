use std::collections::BTreeMap;

use crate::llm::message::ToolCall;

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Reassembles streamed tool-call fragments keyed by their `index`.
#[derive(Default)]
pub struct ToolCallAssembler {
    calls: BTreeMap<usize, PartialCall>,
}

impl ToolCallAssembler {
    pub fn push(&mut self, index: usize, id: Option<String>, name: Option<String>, args: &str) {
        let call = self.calls.entry(index).or_default();
        if let Some(id) = id
            && !id.is_empty()
        {
            call.id = id;
        }
        if let Some(name) = name
            && !name.is_empty()
        {
            call.name = name;
        }
        call.arguments.push_str(args);
    }

    pub fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_values()
            .map(|c| ToolCall {
                id: c.id,
                name: c.name,
                arguments: c.arguments,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
