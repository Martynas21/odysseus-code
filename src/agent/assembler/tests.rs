use super::*;

#[test]
fn assembler_reassembles_fragmented_call() {
    let mut a = ToolCallAssembler::default();
    a.push(0, Some("call_1".into()), Some("shell".into()), "{\"cmd\":");
    a.push(0, None, None, "\"ls\"}");
    let calls = a.finish();
    assert_eq!(
        calls,
        vec![ToolCall {
            id: "call_1".into(),
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into()
        }]
    );
}

#[test]
fn assembler_keeps_parallel_calls_by_index() {
    let mut a = ToolCallAssembler::default();
    a.push(0, Some("a".into()), Some("read_file".into()), "{}");
    a.push(1, Some("b".into()), Some("grep".into()), "{}");
    let calls = a.finish();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "a");
    assert_eq!(calls[1].id, "b");
}
