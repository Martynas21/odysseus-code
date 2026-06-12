use assert_cmd::Command;
use predicates::prelude::*;

fn cmd(server_url: &str, config_dir: &std::path::Path, cache_dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("odysseus-code").unwrap();
    c.env("ODYSSEUS_CODE_CONFIG_DIR", config_dir)
        .env("ODYSSEUS_CODE_CACHE_DIR", cache_dir)
        .env("ODYSSEUS_URL", server_url)
        .env("ODYSSEUS_API_TOKEN", "ody_inttest");
    c
}

#[test]
fn session_lifecycle_start_prompt_end() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();

    // start: creates a server session named after the local id
    let models = server
        .mock("GET", "/api/models")
        .with_body(r#"[{"endpoint_id":"ep1","endpoint_name":"local","models":["qwen3"],"models_extra":[]}]"#)
        .create();
    let create = server
        .mock("POST", "/api/session")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("name".into(), "my-project".into()),
            mockito::Matcher::UrlEncoded("endpoint_id".into(), "ep1".into()),
            mockito::Matcher::UrlEncoded("model".into(), "qwen3".into()),
        ]))
        .with_body(r#"{"id":"srv-42","name":"my-project","model":"qwen3"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["session", "start", "my-project"])
        .assert()
        .success()
        .stdout(predicate::str::contains("srv-42").and(predicate::str::contains("now active")));
    models.assert();
    create.assert();

    // prompt without --session-id routes to the active session
    let chat = server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"session":"srv-42"}"#.into(),
        ))
        .with_body(r#"{"response":"in session"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["prompt", "hello there"])
        .assert()
        .success()
        .stdout(predicate::str::contains("in session"));
    chat.assert();

    // end: deletes the server session and clears the mapping
    let delete = server
        .mock("DELETE", "/api/session/srv-42")
        .with_body(r#"{"status":"deleted"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["session", "end", "my-project"])
        .assert()
        .success()
        .stdout(predicate::str::contains("srv-42 deleted"));
    delete.assert();

    let store = std::fs::read_to_string(cache_dir.path().join("sessions.json")).unwrap();
    assert!(
        !store.contains("srv-42"),
        "mapping should be removed: {store}"
    );
}

#[test]
fn session_end_accepts_raw_server_id() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();

    let delete = server
        .mock("DELETE", "/api/session/raw-id-9")
        .with_body(r#"{"status":"deleted"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["session", "end", "raw-id-9"])
        .assert()
        .success();
    delete.assert();
}
