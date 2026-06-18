use super::*;

#[test]
fn parse_chunk_reasoning_delta() {
    let events = parse_chunk(r#"{"choices":[{"delta":{"reasoning_content":"Hmm"}}]}"#).unwrap();
    assert_eq!(events, vec![StreamEvent::ReasoningDelta("Hmm".into())]);
}

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
fn parse_chunk_tool_call_fragment_without_index_defaults_to_zero() {
    let data = r#"{"choices":[{"delta":{"tool_calls":[
        {"function":{"arguments":"}"}}]}}]}"#;
    let events = parse_chunk(data).unwrap();
    assert_eq!(
        events,
        vec![StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: "}".into(),
        }]
    );
}

#[test]
fn parse_chunk_tool_call_continuation_fragment() {
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
