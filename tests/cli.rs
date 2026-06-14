use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("odysseus-code").unwrap()
}

#[test]
fn help_lists_all_subcommands() {
    bin().arg("--help").assert().success().stdout(
        predicate::str::contains("models")
            .and(predicate::str::contains("config"))
            .and(predicate::str::contains("tui")),
    );
}

#[test]
fn help_lists_global_context_flags() {
    bin().arg("--help").assert().success().stdout(
        predicate::str::contains("--session-id")
            .and(predicate::str::contains("--project-path"))
            .and(predicate::str::contains("--current-file")),
    );
}

#[test]
fn version_prints_crate_version() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_subcommand_fails_with_usage() {
    bin()
        .arg("frobnicate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn subcommands_have_their_own_help() {
    for sub in ["models", "config", "tui"] {
        bin().args([sub, "--help"]).assert().success();
    }
}
