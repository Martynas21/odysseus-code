# Prompt for the next agent (paste into a fresh Claude Code session)

You are finishing **odysseus-code**, a Rust CLI harness at
`/home/omega/projects/odysseus-code` that turns a self-hosted Odysseus AI
workspace (FastAPI app at `~/projects/odysseus`, live on
`http://localhost:7000`) into a local coding assistant.

**Start by reading `docs/MVP.md`** — it's the single source of truth: feature
checklist, completion ritual, and links to per-feature handoff docs (skeleton,
config, api-client, prompt, generate, sandbox, sessions — all done, committed,
36 tests passing). Read the handoff docs adjacent to each task before
implementing it.

## Environment gotchas (also in docs/skeleton.md)

- `cargo` is at `~/.cargo/bin`, NOT on PATH — `export PATH="$HOME/.cargo/bin:$PATH"` per shell.
- Never touch real user config/cache in tests: isolate via
  `ODYSSEUS_CODE_CONFIG_DIR`, `ODYSSEUS_CODE_CACHE_DIR`, `ODYSSEUS_URL`,
  `ODYSSEUS_API_TOKEN` env vars (see `tests/prompt.rs` for the pattern).
- `OdysseusClient` already derives `Clone` (committed) — the TUI's background
  send task relies on it.

## Completion ritual for every task (non-negotiable, from docs/MVP.md)

implement → verify → write `docs/<feature>.md` handoff (what/files/interfaces/
verify-commands/gotchas) → tick the box in `docs/MVP.md` → `cargo fmt` →
commit referencing the doc.

## Remaining tasks, in order

1. **tui** — `odysseus-code tui [--session-id <id>]`: basic ratatui (v0.30)
   chat screen. Resolve session like prompt does (`actions::resolve_session`),
   load history via `client.history(sid)`, scrollable message pane + input
   line + status bar (endpoint/model/session). Enter sends via `client.chat`
   on a spawned tokio task with a "thinking…" indicator (poll a channel with
   `try_recv`; don't block the draw loop); Esc/Ctrl-C quits. Use
   `ratatui::init()/restore()` and import crossterm types from
   `ratatui::crossterm` to avoid version mismatch. Keep rendering/scroll logic
   in pure functions with unit tests (e.g. strip the `[context] {…} [/context]`
   prefix from displayed user messages — see `PromptContext::wrap` in
   `src/context.rs`). Manual live verification only if a token is configured;
   otherwise verify it builds, unit tests pass, and `tui` errors gracefully
   without a token.

2. **tests-ci** — fill any unit-test gaps, add `tests/cli.rs` (assert_cmd:
   `--help` lists all subcommands, unknown subcommand fails), and
   `.github/workflows/ci.yml`: rustfmt `--check`, `clippy -- -D warnings`
   (fix any clippy findings), build, `cargo test` (Docker-dependent tests are
   already `#[ignore]`d). Run the same commands locally as verification.

3. **docs** — README.md based on the product spec (the user supplied one — see
   the features matrix and usage examples below if not otherwise available):
   purpose, core philosophy (specialization / safety & reproducibility /
   extensibility), features matrix, install (`cargo install --path .`), config
   table (endpoint, api_key, model, endpoint_id, default_language,
   sandbox_image), usage examples (prompt, generate both formats,
   `cat x.rs | odysseus-code run -`, session start/end, models, tui),
   API-token setup (Odysseus web UI → Settings → API Tokens), extending guide
   pointing at real files (`src/cli.rs`, `src/actions/`, `src/sandbox.rs`
   language table). ASCII hyphens in all command examples. Plus CHANGELOG.md
   (0.1.0, one entry per feature).

4. **Live MVP gate (may be blocked):** if `cargo run -q -- config get api_key`
   shows an `ody_` token (the user was asked to create one at localhost:7000 →
   Settings → API Tokens), verify live: `cargo run -q -- models`, then
   `cargo run -q -- prompt "Say hello in five words"`, and note the result in
   docs/prompt.md. If there's no token, leave the ⚠ note in docs/MVP.md and
   tell the user at the end.

## Working style

Work autonomously, commit per task, don't pause between tasks. When everything
is done run the full suite
(`cargo fmt --check && cargo clippy -- -D warnings && cargo test`) and report:
tasks completed, test counts, anything still pending (e.g. the token).
