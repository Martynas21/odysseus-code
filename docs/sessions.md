# Handoff: sessions

## What was built

`odysseus-code session start|end <id>` — named, server-backed session contexts
for multi-step interactions (CI pipelines, long dev sessions).

## Files

- `src/actions/session.rs` — start/end handlers
- `src/main.rs` — dispatch
- `tests/session.rs` — lifecycle integration test (start → prompt routes to
  active session → end deletes server-side), raw-server-id end

## Behavior

- `start <id>`: creates an Odysseus server session named `<id>` (endpoint/model
  resolution as in docs/prompt.md), stores the name→server-id mapping locally
  (`sessions.json`) and marks it **active**. Starting an existing name just
  re-activates it.
- Subsequent `prompt`/`generate` calls without `--session-id` use the active
  session; `--session-id <name-or-server-id>` overrides.
- `end <id>`: resolves `<id>` (local name first, else raw server ID), DELETEs
  the server session, removes the mapping and clears active if it pointed there
  (`SessionStore::remove` handles that).
- History lives server-side; the same conversation is visible in the Odysseus
  web UI under the session's name.

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --test session -q
# Live: session start demo && prompt "remember 42" && prompt "what number?" && session end demo
```

## Gotchas

- Ending a session that another machine/UI already deleted returns an HTTP
  error from Odysseus; the local mapping is then NOT removed (delete happens
  first). Manually edit `~/.cache/odysseus-code/sessions.json` if that bites.

## Next steps

tui (loads history via `client.history`), then tests-ci, docs.
