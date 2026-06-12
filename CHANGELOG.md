# Changelog

## 0.1.0 — 2026-06-12

Initial release.

- **skeleton** — Cargo project, clap CLI surface (`prompt`, `generate`,
  `run`, `session`, `models`, `config`, `tui`), global context flags
  (`--session-id`, `--project-path`, `--current-file`).
- **config** — YAML config at `~/.config/odysseus-code/config.yaml` with
  `config set/get/path`; `ODYSSEUS_URL` / `ODYSSEUS_API_TOKEN` env overrides.
- **api-client** — `OdysseusClient` for the Odysseus REST API (chat, session
  create/delete/list, models, history) with typed errors, a 401 setup hint,
  and one 429 retry; prompts wrapped with code-centric context metadata.
- **prompt** — one-shot prompting with a lazily created/reused default server
  session; `models` lists backend endpoints.
- **generate** — code-only generation with fence extraction;
  `--format pretty` (fenced) or `compact` (raw, pipeable).
- **sandbox** — `run` executes snippets in ephemeral, network-less,
  resource-limited Docker containers (rust, python, sh), reads stdin via
  `-`, and relays the snippet's exit code.
- **sessions** — `session start/end` named server-side sessions with a local
  name-to-ID map and `--session-id` routing.
- **tui** — full-screen ratatui chat: preloaded history, non-blocking sends,
  exact row-based scrolling, status bar.
- **tests-ci** — 51 tests (unit + assert_cmd/mockito integration) and a
  GitHub Actions workflow (fmt, clippy -D warnings, build, test).
