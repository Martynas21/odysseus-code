use assert_cmd::Command;
use predicates::prelude::*;

/// Run the binary with config/cache isolated to temp dirs and pointed at the
/// mock Odysseus server.
fn cmd(server_url: &str, config_dir: &std::path::Path, cache_dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("odysseus-code").unwrap();
    c.env("ODYSSEUS_CODE_CONFIG_DIR", config_dir)
        .env("ODYSSEUS_CODE_CACHE_DIR", cache_dir)
        .env("ODYSSEUS_URL", server_url)
        .env("ODYSSEUS_API_TOKEN", "ody_inttest");
    c
}

#[test]
fn prompt_lazily_creates_default_session_and_prints_reply() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();

    // No existing sessions → CLI must discover models and create the default
    // session, then chat into it.
    let sessions = server.mock("GET", "/api/sessions").with_body("[]").create();
    let models = server
        .mock("GET", "/api/models")
        .with_body(r#"{"hosts":[],"items":[{"endpoint_id":"ep1","endpoint_name":"local","models":["qwen3"],"models_extra":[]}]}"#)
        .create();
    let create = server
        .mock("POST", "/api/session")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("name".into(), "odysseus-code".into()),
            mockito::Matcher::UrlEncoded("endpoint_id".into(), "ep1".into()),
            mockito::Matcher::UrlEncoded("model".into(), "qwen3".into()),
        ]))
        .with_body(r#"{"id":"srv-1","name":"odysseus-code","model":"qwen3"}"#)
        .create();
    let chat = server
        .mock("POST", "/api/chat")
        .match_header("authorization", "Bearer ody_inttest")
        // The message must carry the context block and the prompt text,
        // routed to the newly created session.
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::PartialJsonString(r#"{"session":"srv-1"}"#.into()),
            mockito::Matcher::Regex(r#""message":"\[context\].*Hello"#.into()),
        ]))
        .with_body(r#"{"response":"Hi! How can I help?"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["prompt", "Hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hi! How can I help?"));

    sessions.assert();
    models.assert();
    create.assert();
    chat.assert();

    // Second prompt reuses the cached session: no session/model calls again.
    let chat2 = server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"session":"srv-1"}"#.into(),
        ))
        .with_body(r#"{"response":"Again!"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["prompt", "And again"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Again!"));
    chat2.assert();
}

#[test]
fn explicit_session_id_skips_session_resolution() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();

    let chat = server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"session":"raw-server-id"}"#.into(),
        ))
        .with_body(r#"{"response":"scoped"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["prompt", "hi", "--session-id", "raw-server-id"])
        .assert()
        .success()
        .stdout(predicate::str::contains("scoped"));
    chat.assert();
}

#[test]
fn missing_api_token_fails_with_setup_hint() {
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();

    let mut c = Command::cargo_bin("odysseus-code").unwrap();
    c.env("ODYSSEUS_CODE_CONFIG_DIR", config_dir.path())
        .env("ODYSSEUS_CODE_CACHE_DIR", cache_dir.path())
        .env_remove("ODYSSEUS_API_TOKEN")
        .args(["prompt", "hi"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Settings → Integrations → API Tokens",
        ));
}

#[test]
fn models_lists_endpoints() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();

    server
        .mock("GET", "/api/models")
        .with_body(r#"{"hosts":[],"items":[{"endpoint_id":"ep1","endpoint_name":"local llama","models":["qwen3","gpt-oss"],"models_extra":["extra-model"]}]}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .arg("models")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("local llama (endpoint_id: ep1)")
                .and(predicate::str::contains("qwen3"))
                .and(predicate::str::contains("extra-model")),
        );
}
