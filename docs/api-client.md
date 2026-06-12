# Handoff: api-client

## What was built

`OdysseusClient` — typed async client for the Odysseus REST API — plus
`PromptContext`, which wraps every prompt with code-centric metadata. Fully
covered by mockito tests (no live server needed).

## Files

- `src/client.rs` — client, response types, `ClientError`, 10 tests
- `src/context.rs` — `PromptContext` + extension→language table, 4 tests

## Public interface

```rust
let client = OdysseusClient::from_config(&cfg)?;  // errors early if api_key empty/placeholder
client.chat(session_id, message).await? -> String        // POST /api/chat
client.create_session(name, endpoint_id, model).await? -> SessionInfo // form POST /api/session
client.delete_session(sid).await?                         // DELETE /api/session/{sid}
client.list_sessions().await? -> Vec<SessionInfo>         // GET /api/sessions (bare array)
client.list_models().await? -> Vec<ModelEndpoint>         // GET /api/models  (bare array)
client.history(sid).await? -> Vec<HistoryMessage>         // GET /api/history/{sid} ({history: […]})

let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);
ctx.wrap("Explain X") // "[context] {json metadata} [/context]\n\nExplain X"
```

`ModelEndpoint { endpoint_id, endpoint_name, models, models_extra }` — pass
`endpoint_id` + one of `models` to `create_session`.

## Error semantics (`ClientError`)

- `Unauthorized` — 401 or missing/placeholder token; message tells the user to
  create a token in Odysseus Settings → API Tokens.
- `RateLimited` — 429 twice; `chat` honors `Retry-After` once (capped 10s).
- `Http {status, body}` — other non-2xx, body truncated to 300 chars.
- `Network {url, source}` — connection-level failures (server down).
- `BadResponse` — 2xx but unparseable JSON.

## Decisions / gotchas

- `POST /api/session` is **form-encoded** (FastAPI `Form(...)` params), not
  JSON — reqwest needs the `form` feature (already enabled).
- Raw `endpoint_url` in session creation is admin-only server-side; always use
  `endpoint_id` from `list_models()`.
- Context metadata is embedded as a `[context] {…json…} [/context]` prefix in
  the message because `/api/chat` has no metadata fields.
- Chat timeout is 300s (local models are slow); connect timeout 5s.
- Dead-code warnings expected until the `prompt` chunk wires these modules in.

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test client -q && cargo test context -q   # 14 tests
```

## Next steps

`prompt` action: resolve session (--session-id → active → lazy "odysseus-code"
default via list_sessions/create_session), wrap with PromptContext, print reply.
