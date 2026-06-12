# Handoff: config

## What was built

YAML config persisted at `~/.config/odysseus-code/config.yaml`, plus the
`config set|get|path` subcommands. First load writes defaults to disk.

## Files

- `src/config.rs` — `Config` struct + load/save + key get/set + unit tests
- `src/actions.rs` — actions module root
- `src/actions/config_cmd.rs` — `handle(ConfigAction)`
- `src/main.rs` — dispatch for `Command::Config`

## Public interface

```rust
config::Config { endpoint, api_key, model, endpoint_id, default_language, sandbox_image }
Config::load()            // file + ODYSSEUS_URL / ODYSSEUS_API_TOKEN env overrides — use this in runtime code
Config::load_file(&path)  // disk only, no env — used by `config set` so env values never persist
cfg.save() / cfg.save_to(&path)
cfg.set(key, value) / cfg.get(key)   // string-keyed, unknown keys error
config::config_dir() / config::config_path()
```

## Decisions

- Defaults: `endpoint: http://localhost:7000` (the live local Odysseus),
  `default_language: rust`, `sandbox_image: rust:slim`. The original spec said
  `odysseus/sandbox-rust:latest` but that image doesn't exist publicly; a
  default that can actually pull keeps `run` working out of the box.
- `endpoint` values are normalized (trailing `/` stripped).
- Extra keys `model` and `endpoint_id` exist because Odysseus session creation
  needs a model + model-endpoint id (see docs/MVP.md plan notes).
- `$ODYSSEUS_CODE_CONFIG_DIR` overrides the config dir — tests and parallel
  agents should always set it instead of touching the real user config.

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test config -q
ODYSSEUS_CODE_CONFIG_DIR=/tmp/c cargo run -q -- config set endpoint http://x:1
ODYSSEUS_CODE_CONFIG_DIR=/tmp/c cargo run -q -- config get endpoint   # http://x:1
```

## Gotchas

- Unit tests use `load_file`/`save_to` with temp paths, never env vars (Rust
  tests run in parallel threads; setting process-global env in tests races).

## Next steps

api-client: `Config::load()` is the entry point; `api_key` empty string means
"no token yet" — surface a helpful error pointing at Odysseus Settings → API Tokens.
