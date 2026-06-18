use super::*;

#[test]
fn streaming_delta_appends_to_open_assistant_bubble() {
    let cfg = Config::default();
    let mut app = App::new(&cfg, "m".into());
    app.begin_assistant();
    app.push_delta("Hel");
    app.push_delta("lo");
    assert_eq!(app.messages.last().unwrap().content, "Hello");
    assert_eq!(app.messages.last().unwrap().role, Role::Assistant);
}

#[tokio::test]
async fn stop_turn_aborts_the_task_and_resets_turn_state() {
    let cfg = Config::default();
    let mut app = App::new(&cfg, "m".into());
    app.thinking = true;
    app.begin_assistant();
    app.push_delta("partial");
    app.reasoning.push_str("thinking…");
    let (appr_tx, _appr_rx) = mpsc::unbounded_channel();
    app.appr_tx = Some(appr_tx);
    app.pending_approval = Some(PendingApproval {
        name: "shell".into(),
        args: "{}".into(),
    });
    let handle = tokio::spawn(async { std::future::pending::<()>().await });
    app.agent_task = Some(handle);

    app.stop_turn();

    assert!(!app.thinking);
    assert!(app.streaming_idx.is_none());
    assert!(app.reasoning.is_empty());
    assert!(app.appr_tx.is_none());
    assert!(app.pending_approval.is_none());
    assert!(app.agent_task.is_none());
    assert_eq!(app.messages.last().unwrap().role, Role::System);
    assert_eq!(app.messages.last().unwrap().content, "Stopped.");
}

#[test]
fn approval_keys_map_to_decisions() {
    let cfg = Config::default();
    let app = App::new(&cfg, "m".into());
    assert_eq!(
        app.approval_key(KeyCode::Char('y')),
        Some(ApprovalDecision::Approve)
    );
    assert_eq!(
        app.approval_key(KeyCode::Enter),
        Some(ApprovalDecision::Approve)
    );
    assert_eq!(
        app.approval_key(KeyCode::Char('a')),
        Some(ApprovalDecision::ApproveAlways)
    );
    assert_eq!(
        app.approval_key(KeyCode::Char('n')),
        Some(ApprovalDecision::Deny)
    );
    assert_eq!(app.approval_key(KeyCode::Esc), None);
    assert_eq!(app.approval_key(KeyCode::Char('z')), None);
}
