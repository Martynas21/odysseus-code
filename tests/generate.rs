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

/// Pre-seed the cache dir so generate skips session resolution.
fn seed_session(cache_dir: &std::path::Path) {
    std::fs::write(
        cache_dir.join("sessions.json"),
        r#"{"sessions":{"odysseus-code":"srv-1"},"active":null}"#,
    )
    .unwrap();
}

#[test]
fn generate_pretty_prints_fenced_block() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    seed_session(cache_dir.path());

    server
        .mock("POST", "/api/chat")
        .match_body(mockito::Matcher::Regex(
            r#""message":"\[context\].*Generate rust code: factorial function"#.into(),
        ))
        .with_body(r#"{"response":"Sure!\n```rust\nfn fact(n: u64) -> u64 { (1..=n).product() }\n```\nDone."}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["generate", "rust", "factorial function"])
        .assert()
        .success()
        .stdout(predicate::str::diff(
            "```rust\nfn fact(n: u64) -> u64 { (1..=n).product() }\n```\n",
        ));
}

#[test]
fn generate_compact_prints_raw_code() {
    let mut server = mockito::Server::new();
    let config_dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    seed_session(cache_dir.path());

    server
        .mock("POST", "/api/chat")
        .with_body(r#"{"response":"```python\nprint('hi')\n```"}"#)
        .create();

    cmd(&server.url(), config_dir.path(), cache_dir.path())
        .args(["generate", "python", "say hi", "--format", "compact"])
        .assert()
        .success()
        .stdout(predicate::str::diff("print('hi')\n"));
}
