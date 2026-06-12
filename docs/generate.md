# Handoff: generate

## What was built

`odysseus-code generate <lang> "<description>" [--format pretty|compact]` —
asks the model for code-only output, extracts the snippet, prints it fenced
(`pretty`, default) or raw (`compact`).

## Files

- `src/actions/generate.rs` — handler + `extract_code` (4 unit tests)
- `src/main.rs` — dispatch
- `tests/generate.rs` — 2 integration tests (both formats)

## Behavior

- Reuses `actions::resolve_session` (same session flow as prompt; see
  docs/prompt.md). `<lang>` overrides the context language.
- Instruction sent: `Generate <lang> code: <description>` + "reply with ONLY
  the code in a single fenced code block".
- `extract_code`: first fenced block (info string skipped, unterminated fence
  tolerated), else whole reply trimmed. Re-fenced with the *requested* lang in
  pretty mode so output is consistent even if the model picked another tag.

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --test generate -q && cargo test generate -q
# Live: cargo run -q -- generate rust "a generic merge sort" --format compact
```

## Gotchas

- Integration tests pre-seed `sessions.json` with `{"odysseus-code":"srv-1"}`
  to skip session-resolution mocks — copy `seed_session` for new tests.

## Next steps

sandbox (`run`) is independent; `generate … | odysseus-code run` becomes the
end-to-end story once both exist.
