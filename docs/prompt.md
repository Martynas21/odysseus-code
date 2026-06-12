# Handoff: prompt

## What was built

The MVP path: `odysseus-code prompt "<text>"` sends a context-wrapped message
into an Odysseus server session and prints the plain-text reply. Plus the
`models` helper and the local `SessionStore`.

## Files

- `src/session.rs` — `SessionStore` (name→server-id map + active pointer),
  `sessions.json` in cache dir, 4 tests
- `src/actions.rs` — shared `resolve_session` + `create_session` helpers
- `src/actions/prompt.rs` — prompt handler
- `src/actions/models.rs` — `models` subcommand
- `src/main.rs` — dispatch for Prompt/Models
- `tests/prompt.rs` — 4 integration tests (assert_cmd + mockito): lazy default
  session creation & reuse, explicit `--session-id`, missing-token hint,
  models listing

## Session resolution (used by prompt; generate will reuse it)

`actions::resolve_session(client, cfg, store, explicit)`:
1. `--session-id <x>` → local name lookup, else treated as raw server session ID
2. active session from `session start`
3. cached default mapping in sessions.json
4. reuse server session named `odysseus-code` (GET /api/sessions)
5. create it: endpoint/model from config (`endpoint_id`+`model`) else first
   endpoint from GET /api/models (honoring `model` if set); helpful errors
   point at `odysseus-code models`

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --test prompt -q     # mock-server integration tests
# Live (needs ody_ token from Odysseus Settings → API Tokens):
cargo run -q -- config set api_key ody_...
cargo run -q -- models
cargo run -q -- prompt "Say hello in five words"
```

## Gotchas

- Integration tests isolate state via `ODYSSEUS_CODE_CONFIG_DIR` /
  `ODYSSEUS_CODE_CACHE_DIR` and inject the mock server via `ODYSSEUS_URL` /
  `ODYSSEUS_API_TOKEN`. Follow the same pattern for new CLI tests.
- **Live verification still pending**: requires the user-created API token —
  blocked on user input, everything else is verified against mocks.

## Next steps

generate (reuses resolve_session + adds formatting), sessions feature
(`session start/end` on top of SessionStore + client.delete_session).
