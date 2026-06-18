use serde::Deserialize;

use crate::llm::{ProviderError, StreamEvent};

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallChunk>>,
}

#[derive(Deserialize)]
struct ToolCallChunk {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FnChunk>,
}

#[derive(Deserialize)]
struct FnChunk {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

pub(super) fn parse_chunk(data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
    let chunk: ChatChunk = serde_json::from_str(data)
        .map_err(|e| ProviderError::BadStream(format!("{e} — payload: {data}")))?;
    let mut events = Vec::new();
    for choice in chunk.choices {
        if let Some(reasoning) = choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            events.push(StreamEvent::ReasoningDelta(reasoning));
        }
        if let Some(text) = choice.delta.content
            && !text.is_empty()
        {
            events.push(StreamEvent::TextDelta(text));
        }
        for call in choice.delta.tool_calls.unwrap_or_default() {
            let (name, arguments) = match call.function {
                Some(f) => (f.name, f.arguments.unwrap_or_default()),
                None => (None, String::new()),
            };
            events.push(StreamEvent::ToolCallDelta {
                index: call.index,
                id: call.id,
                name,
                arguments,
            });
        }
    }
    if let Some(u) = chunk.usage {
        events.push(StreamEvent::Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests;
