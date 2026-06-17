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
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallChunk>>,
}

#[derive(Deserialize)]
struct ToolCallChunk {
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

/// Parse one non-`[DONE]` SSE data payload into zero or more events.
pub(crate) fn parse_chunk(data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
    let chunk: ChatChunk = serde_json::from_str(data)
        .map_err(|e| ProviderError::BadStream(format!("{e} — payload: {data}")))?;
    let mut events = Vec::new();
    for choice in chunk.choices {
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
mod tests {
    use super::*;
    use crate::llm::StreamEvent;

    #[test]
    fn parse_chunk_text_delta() {
        let events = parse_chunk(r#"{"choices":[{"delta":{"content":"Hel"}}]}"#).unwrap();
        assert_eq!(events, vec![StreamEvent::TextDelta("Hel".into())]);
    }

    #[test]
    fn parse_chunk_tool_call_fragment() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_1","function":{"name":"shell","arguments":"{\"cmd\":"}}]}}]}"#;
        let events = parse_chunk(data).unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("call_1".into()),
                name: Some("shell".into()),
                arguments: "{\"cmd\":".into(),
            }]
        );
    }

    #[test]
    fn parse_chunk_usage() {
        let data = r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7}}"#;
        let events = parse_chunk(data).unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::Usage {
                prompt_tokens: 12,
                completion_tokens: 7
            }]
        );
    }

    #[test]
    fn parse_chunk_empty_delta_is_no_events() {
        let events = parse_chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn parse_chunk_malformed_is_bad_stream() {
        assert!(parse_chunk("not json").is_err());
    }

    #[test]
    fn parse_chunk_tool_call_continuation_fragment() {
        // The dominant live shape: after the first fragment, continuation chunks
        // carry only `index` + more `arguments` (no `id`, no `name`).
        let data = r#"{"choices":[{"delta":{"tool_calls":[
            {"index":0,"function":{"arguments":"\"ls\"}"}}]}}]}"#;
        let events = parse_chunk(data).unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments: "\"ls\"}".into(),
            }]
        );
    }
}
