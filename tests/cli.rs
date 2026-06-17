use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("odysseus-code").unwrap()
}

#[test]
fn help_lists_remaining_subcommands() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("config").and(predicate::str::contains("tui")));
}

#[test]
fn help_has_no_models_subcommand() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("models").not());
}

#[test]
fn help_lists_global_flags() {
    bin().arg("--help").assert().success().stdout(
        predicate::str::contains("--project-path")
            .and(predicate::str::contains("--current-file"))
            .and(predicate::str::contains("--model"))
            .and(predicate::str::contains("--base-url")),
    );
}

#[test]
fn help_dropped_session_id() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--session-id").not());
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
fn subcommands_have_their_own_help() {
    for sub in ["config", "tui"] {
        bin().args([sub, "--help"]).assert().success();
    }
}

#[test]
fn run_subcommand_has_help() {
    bin().args(["run", "--help"]).assert().success();
}
