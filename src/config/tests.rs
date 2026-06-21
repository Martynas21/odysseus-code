use super::*;

#[test]
fn first_load_writes_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    let cfg = Config::load_file(&path).unwrap();
    assert_eq!(cfg, Config::default());
    assert!(path.exists(), "defaults should be persisted on first load");
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("base_url: http://localhost:1234"));
    assert!(raw.contains("default_language: rust"));
}

#[test]
fn set_persists_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    let mut cfg = Config::load_file(&path).unwrap();
    cfg.set("base_url", "http://example.com:9999/").unwrap();
    cfg.set("api_key", "ody_test123").unwrap();
    cfg.save_to(&path).unwrap();

    let reloaded = Config::load_file(&path).unwrap();
    assert_eq!(reloaded.base_url, "http://example.com:9999");
    assert_eq!(reloaded.api_key, "ody_test123");
}

#[test]
fn endpoint_alias_migrates_to_base_url() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, "endpoint: http://old:7000\napi_key: ody_x\n").unwrap();
    let cfg = Config::load_file(&path).unwrap();
    assert_eq!(cfg.base_url, "http://old:7000");
}

#[test]
fn unknown_legacy_keys_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, "base_url: http://x\nendpoint_id: ep1\n").unwrap();
    let cfg = Config::load_file(&path).unwrap();
    assert_eq!(cfg.base_url, "http://x");
}

#[test]
fn agent_defaults_are_sane() {
    let cfg = Config::default();
    assert_eq!(cfg.base_url, "http://localhost:1234");
    assert_eq!(cfg.temperature, 0.2);
    assert_eq!(cfg.max_tokens, 32768);
    assert_eq!(cfg.tool_timeout_secs, 60);
    assert_eq!(cfg.approval_policy, "prompt");
}

#[test]
fn set_and_get_new_keys() {
    let mut cfg = Config::default();
    cfg.set("base_url", "http://h:1/").unwrap();
    cfg.set("approval_policy", "auto").unwrap();
    assert_eq!(cfg.get("base_url").unwrap(), "http://h:1");
    assert_eq!(cfg.get("approval_policy").unwrap(), "auto");
}

#[test]
fn unknown_keys_are_rejected() {
    let mut cfg = Config::default();
    assert!(cfg.set("nope", "x").is_err());
    assert!(cfg.get("nope").is_err());
}

#[cfg(unix)]
#[test]
fn save_to_restricts_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    Config::default().save_to(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn get_returns_each_key() {
    let cfg = Config::default();
    for key in Config::keys() {
        cfg.get(&key).unwrap();
    }
}

#[test]
fn every_key_round_trips_through_set_and_get() {
    let cases = [
        ("base_url", "http://h:1"),
        ("api_key", "k"),
        ("model", "m"),
        ("temperature", "0.5"),
        ("max_tokens", "100"),
        ("tool_timeout_secs", "30"),
        ("approval_policy", "auto"),
        ("default_language", "go"),
        ("searxng_url", "http://localhost:8080"),
    ];
    for key in Config::keys() {
        assert!(
            cases.iter().any(|(c, _)| *c == key),
            "no set case for config key '{key}'"
        );
    }
    for (key, value) in cases {
        let mut cfg = Config::default();
        cfg.set(key, value).unwrap();
        cfg.get(key).unwrap();
    }
}

#[test]
fn searxng_url_round_trips_and_trims_slash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    let mut cfg = Config::load_file(&path).unwrap();
    assert_eq!(cfg.searxng_url, "", "default searxng_url is empty");
    cfg.set("searxng_url", "http://localhost:8080/").unwrap();
    cfg.save_to(&path).unwrap();
    let reloaded = Config::load_file(&path).unwrap();
    assert_eq!(reloaded.searxng_url, "http://localhost:8080");
}

#[test]
fn searxng_url_get_returns_value() {
    let mut cfg = Config::default();
    cfg.set("searxng_url", "http://h:8080").unwrap();
    assert_eq!(cfg.get("searxng_url").unwrap(), "http://h:8080");
}
