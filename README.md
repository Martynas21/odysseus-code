# odysseus-code

A command-line harness that turns a self-hosted [Odysseus](https://github.com/pewdiepie-archdaemon/odysseus)
AI workspace into a local coding assistant. Odysseus itself is web-only;
odysseus-code wraps its REST API in a full-screen terminal chat so you can
pair with it on code without leaving the terminal.

## Core philosophy

- **Specialization** — every prompt is wrapped with code-centric context
  (project path, current file, inferred language), so the model behaves like
  a coding companion rather than a generic chat.
- **Extensibility** — small Rust codebase with one module per concern; adding
  a subcommand is a localized change (see [Extending](#extending)).

## Features

| Feature | Subcommand | Notes |
|---|---|---|
| Interactive chat | `tui` (default) | ratatui full-screen chat, context-wrapped, with server-side history |
| Model discovery | `models` | Lists backend endpoints and models |
| Configuration | `config set/get/path` | YAML file + env overrides |

## Install

```sh
cargo install --path .
```

Requires Rust (edition 2024 toolchain).

## Setup: API token

1. Open the Odysseus web UI (default `http://localhost:7000`).
2. Go to Settings -> Integrations and scroll past the integrations list to
   the "API Tokens" card (admin only). Name the token, leave scopes blank
   (defaults to `chat`), and copy the `ody_...` value -- it is shown only once.
3. Tell odysseus-code about it:

```sh
odysseus-code config set api_key ody_yourtokenhere
```

`ODYSSEUS_URL` and `ODYSSEUS_API_TOKEN` environment variables override the
config file without being written back to disk.

## Configuration

Stored at `~/.config/odysseus-code/config.yaml` (`odysseus-code config path`).

| Key | Default | Meaning |
|---|---|---|
| `endpoint` | `http://localhost:7000` | Base URL of the Odysseus instance |
| `api_key` | (empty) | `ody_...` API token |
| `model` | (empty = first available) | Preferred model for new sessions |
| `endpoint_id` | (empty = resolve from `/api/models`) | Odysseus model-endpoint ID |
| `default_language` | `rust` | Language assumed when none can be inferred |

```sh
odysseus-code config get            # whole config
odysseus-code config get endpoint   # one key
odysseus-code config set model qwen3
```

## Usage

```sh
# Bare invocation opens the full-screen interactive chat (same as `tui`).
# Esc stops the model mid-reply; Ctrl-C quits; /clear starts a fresh server session.
odysseus-code

# What can the backend serve?
odysseus-code models
```

Global flags on `tui`: `--session-id <id>` (attach to a specific server
session instead of the default), `--project-path <dir>`, `--current-file
<file>` (context sent to the model; language is inferred from the current
file's extension).

## Extending

- **New subcommand** — add a variant in `src/cli.rs`, a handler module under
  `src/actions/`, and a dispatch arm in `src/main.rs`.
- **New context language** — extend `language_for_extension` in
  `src/context.rs`.
- **API surface** — `src/client.rs` is the only place that talks HTTP;
  mockito tests next to it show the wire format.

## Development

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # if cargo is not on PATH
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI (GitHub Actions) runs the same four steps on every push and pull request.
