# odysseus-code

A command-line harness that turns a self-hosted [Odysseus](http://localhost:7000)
AI workspace into a local coding assistant. Odysseus itself is web-only;
odysseus-code wraps its REST API so you can prompt, generate, and run code
without leaving the terminal.

## Core philosophy

- **Specialization** — every prompt is wrapped with code-centric context
  (project path, current file, inferred language), so the model behaves like
  a coding companion rather than a generic chat.
- **Safety & reproducibility** — generated code never runs on your host: the
  `run` subcommand executes snippets in ephemeral, network-less,
  resource-limited Docker containers.
- **Extensibility** — small Rust codebase with one module per concern; adding
  a subcommand or a sandbox language is a localized change (see
  [Extending](#extending)).

## Features

| Feature | Subcommand | Notes |
|---|---|---|
| One-shot prompting | `prompt` | Plain-text reply, context-wrapped |
| Code generation | `generate` | Code-only output, `pretty` or `compact` |
| Sandboxed execution | `run` | Docker, stdin support, exit-code relay |
| Named sessions | `session start/end` | Server-side history, local name map |
| Model discovery | `models` | Lists backend endpoints and models |
| Configuration | `config set/get/path` | YAML file + env overrides |
| Interactive chat | `tui` | ratatui full-screen chat with history |

## Install

```sh
cargo install --path .
```

Requires Rust (edition 2024 toolchain) and, for the `run` subcommand, a
working Docker daemon.

## Setup: API token

1. Open the Odysseus web UI (default `http://localhost:7000`).
2. Go to Settings -> Integrations and scroll to the "API Tokens" card
   (admin only; if it is missing, enable the "API Tokens" toggle under
   Settings -> System). Name the token, leave scopes blank (defaults to
   `chat`), and copy the `ody_...` value -- it is shown only once.
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
| `sandbox_image` | `rust:slim` | Container image for the Rust sandbox |

```sh
odysseus-code config get            # whole config
odysseus-code config get endpoint   # one key
odysseus-code config set model qwen3
```

## Usage

```sh
# One-shot prompt (lazily creates/reuses a server session named "odysseus-code")
odysseus-code prompt "How do I reverse a linked list in Rust?"

# Code generation - fenced markdown block (default) ...
odysseus-code generate rust "a generic merge sort" --format pretty

# ... or raw code only, ready to pipe
odysseus-code generate python "fizzbuzz" --format compact

# Run a snippet in the Docker sandbox (reads stdin with "-")
cat x.rs | odysseus-code run -
odysseus-code run 'print("hi")' --lang python

# Generation piped straight into the sandbox
odysseus-code generate rust "print the first 10 primes" --format compact | odysseus-code run -

# Named sessions: server-side history under a friendly local name
odysseus-code session start my-feature
odysseus-code prompt "Plan the refactor"          # goes to my-feature
odysseus-code prompt "Step 2?" --session-id my-feature
odysseus-code session end my-feature

# What can the backend serve?
odysseus-code models

# Full-screen interactive chat (Esc or Ctrl-C quits)
odysseus-code tui
```

Global flags on every subcommand: `--session-id <id>`,
`--project-path <dir>`, `--current-file <file>` (context sent to the model;
language is inferred from the current file's extension).

## Extending

- **New subcommand** — add a variant in `src/cli.rs`, a handler module under
  `src/actions/`, and a dispatch arm in `src/main.rs`.
- **New sandbox language** — extend the `spec_for` table in `src/sandbox.rs`
  (filename, run command, default image).
- **New context language** — extend `language_for_extension` in
  `src/context.rs`.
- **API surface** — `src/client.rs` is the only place that talks HTTP;
  mockito tests next to it show the wire format.

Development docs live in `docs/`: `docs/MVP.md` is the feature roadmap and
each feature has a handoff doc with design notes and verification commands.

## Development

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # if cargo is not on PATH
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                              # Docker tests are #[ignore]d
cargo test -- --ignored                 # sandbox tests (needs Docker)
```

CI (GitHub Actions) runs the same four steps on every push and pull request.
