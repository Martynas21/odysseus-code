# tests-ci — handoff

## What was built

- `tests/cli.rs` — CLI-surface integration tests via assert_cmd/predicates
  (6 tests, no network or config access):
  - `--help` lists every subcommand (prompt, generate, run, session, models,
    config, tui) and the global context flags (`--session-id`,
    `--project-path`, `--current-file`).
  - `--version` prints the crate version.
  - Unknown subcommand fails with a Usage message; `prompt` without text
    fails; every subcommand has a working `--help`.
- `.github/workflows/ci.yml` — single `test` job on ubuntu-latest, triggered
  on push to main and on pull requests: stable toolchain (rustfmt + clippy
  components), `Swatinem/rust-cache@v2`, then
  `cargo fmt --check` → `cargo clippy --all-targets -- -D warnings` →
  `cargo build` → `cargo test`.
- Clippy cleanup: removed the unused `Config::save` (everything persists via
  `Config::save_to`; it was the only `-D warnings` finding).

## Files touched

- `tests/cli.rs` — new
- `.github/workflows/ci.yml` — new
- `src/config.rs` — removed dead `Config::save`

## Verification (same commands CI runs)

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

All green locally on 2026-06-12: 51 passed across 5 binaries
(37 unit, 6 cli, 2 generate, 4 prompt, 2 session), 1 ignored.

## Gotchas

- The Docker-dependent sandbox test is `#[ignore]`d, so plain `cargo test`
  (and CI) skips it; run `cargo test -- --ignored` on a machine with Docker
  to exercise it.
- `tests/cli.rs` only uses `--help`/argument-validation paths, so it needs no
  env isolation; any test that actually executes a command must set
  `ODYSSEUS_CODE_CONFIG_DIR`/`ODYSSEUS_CODE_CACHE_DIR` (see `tests/prompt.rs`).
- The workflow has not run on GitHub yet (no remote configured in this repo);
  it will activate on first push to a GitHub remote.
