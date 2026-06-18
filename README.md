# odysseus-code

A command-line coding agent that runs against a local, OpenAI-compatible LLM
server. It operates directly on your repository — reading and editing files and
running shell commands through a small set of tools — from a full-screen
terminal UI or a one-shot non-interactive command. By default it targets a
local server (e.g. [LM Studio](https://lmstudio.ai/)) so your code never leaves
your machine.

## Core philosophy

- **Local-first** — talks to any OpenAI-compatible endpoint, defaulting to
  `http://localhost:1234` with no authentication required. Your code stays on
  your machine.
- **Agentic** — the model drives a tool loop (read, search, edit, run) rather
  than just chatting. Mutating actions are gated by a configurable approval
  policy.
- **Extensibility** — small Rust codebase with one module per concern; adding a
  tool or subcommand is a localized change (see [Extending](#extending)).

## Features

| Feature | Subcommand | Notes |
|---|---|---|
| Interactive agent | `tui` (default) | ratatui full-screen UI; the model reads/edits files and runs commands, with approval prompts for mutating tools |
| One-shot run | `run <prompt>` | Single non-interactive turn streamed to stdout |
| Configuration | `config set/get/path` | YAML file + env overrides |

### Tools available to the model

| Tool | Safety | Purpose |
|---|---|---|
| `read_file` | read-only | Read a file in the workspace |
| `list_dir` | read-only | List a directory |
| `grep` | read-only | Search file contents |
| `write_file` | mutating | Create or overwrite a file |
| `edit_file` | mutating | Apply a targeted edit to a file |
| `shell` | mutating | Run a shell command (per-tool timeout) |

## Install

```sh
cargo install --path .
```

Requires Rust (edition 2024 toolchain).

## Setup

odysseus-code talks to any OpenAI-compatible server. By default it targets a
local server at `http://localhost:1234` (e.g. LM Studio), which needs no
authentication — so once your server is running and serving a model, there is
nothing to configure. Point it at a different server with `config set base_url`
or the `--base-url` flag.

If your endpoint requires a bearer token, set it:

```sh
odysseus-code config set api_key sk-yourtokenhere
```

An empty `api_key` means no `Authorization` header is sent.

`ODYSSEUS_BASE_URL` (or `ODYSSEUS_URL`) and `ODYSSEUS_API_KEY` (or
`ODYSSEUS_API_TOKEN`) environment variables override the config file for a
single run without being written back to disk.

## Configuration

Stored at `~/.config/odysseus-code/config.yaml` (`odysseus-code config path`).

| Key | Default | Meaning |
|---|---|---|
| `base_url` | `http://localhost:1234` | Base URL of the OpenAI-compatible server (no `/v1` suffix) |
| `api_key` | (empty) | Bearer token; empty sends no `Authorization` header |
| `model` | (empty = server default) | Model id to request |
| `temperature` | `0.2` | Sampling temperature |
| `max_tokens` | `32768` | Max tokens generated per turn |
| `tool_timeout_secs` | `60` | Per-tool execution timeout |
| `approval_policy` | `prompt` | `prompt` (gate mutating tools), `auto` (run all), or `readonly` (auto-run read-only, auto-deny mutating) |
| `default_language` | `rust` | Language assumed when none can be inferred |

```sh
odysseus-code config get            # whole config (api_key redacted)
odysseus-code config get base_url   # one key
odysseus-code config set model qwen/qwen3-14b
```

## Usage

```sh
# Bare invocation opens the full-screen interactive agent (same as `tui`).
# Esc stops the model mid-reply; Ctrl-C quits; /clear starts a fresh session.
odysseus-code

# One-shot, non-interactive: stream a single reply to stdout.
odysseus-code run "explain what src/agent/mod.rs does"

# Let the model run mutating tools without prompting, and skip its
# chain-of-thought for a faster, direct answer.
odysseus-code run --yes --no-think "add a doc comment to Config::load"
```

Global flags: `--project-path <dir>` (workspace the agent operates in; defaults
to `.`), `--current-file <file>` (context only; language is inferred from its
extension), `--model <id>` and `--base-url <url>` (override config for this
run).

## Extending

- **New tool** — implement the `Tool` trait in a module under `src/tools/` and
  register it in `ToolRegistry::default_set` (`src/tools/mod.rs`).
- **New subcommand** — add a variant in `src/cli.rs`, a handler module under
  `src/actions/`, and a dispatch arm in `src/main.rs`.
- **New context language** — extend `language_for_extension` in
  `src/context.rs`.
- **LLM surface** — `src/llm/` is the only place that talks HTTP to the model
  server; the `openai` submodule holds the wire types and tests.

## Development

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # if cargo is not on PATH
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

CI (GitHub Actions) runs these same checks on every push and pull request.
