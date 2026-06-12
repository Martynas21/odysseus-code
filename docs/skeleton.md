# Handoff: skeleton

## What was built

Cargo binary project with the full clap CLI surface declared and all handlers
stubbed (`anyhow::bail!("…: not implemented yet")`). Establishes the module
layout and dependency set so subsequent features only fill in handlers.

## Files

- `Cargo.toml` — deps: clap (derive), serde/serde_json/serde_yaml, reqwest
  (json, rustls, form — **form** is needed because Odysseus session creation is
  form-encoded), tokio (rt-multi-thread, macros), ratatui + crossterm,
  anyhow/thiserror, dirs, tempfile; dev: mockito, assert_cmd, predicates.
- `src/cli.rs` — `Cli` (clap Parser) with global flags `--session-id`,
  `--project-path`, `--current-file`; `Command` enum: Prompt, Generate
  (`--format pretty|compact` via `OutputFormat`), Run (optional positional
  `code`, `--lang`), Session (Start/End), Models, Config (Set/Get/Path), Tui.
- `src/main.rs` — `#[tokio::main]`, parses `Cli`, match dispatch (stubs).
- `docs/MVP.md` — the roadmap/handoff index.

## Public interfaces consumed by later features

- `cli::Cli { command, session_id, project_path, current_file }`
- `cli::{Command, SessionAction, ConfigAction, OutputFormat}`

Add a new subcommand by extending `cli::Command` and adding a match arm in
`src/main.rs` (handlers go in `src/actions/<name>.rs` once `actions.rs` exists).

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo run -q -- --help   # lists prompt, generate, run, session, models, config, tui
```

## Gotchas

- `cargo`/`rustc` live in `~/.cargo/bin`, which is NOT on the default PATH of
  this machine's shells — export it per command.
- reqwest ≥0.13 renamed feature `rustls-tls` → `rustls`.
- Edition 2024.

## Next steps

`config` feature (see docs/MVP.md). Keep config I/O behind
`$ODYSSEUS_CODE_CONFIG_DIR` override for testability.
