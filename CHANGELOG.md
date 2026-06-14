# Changelog

## 0.1.0 — 2026-06-12

Initial release.

- **skeleton** — Cargo project, clap CLI surface (`models`, `config`, `tui`),
  global context flags (`--session-id`, `--project-path`, `--current-file`).
- **config** — YAML config at `~/.config/odysseus-code/config.yaml` with
  `config set/get/path`; `ODYSSEUS_URL` / `ODYSSEUS_API_TOKEN` env overrides.
- **api-client** — `OdysseusClient` for the Odysseus REST API (chat, session
  create/list, models, history) with typed errors, a 401 setup hint, and one
  429 retry; prompts wrapped with code-centric context metadata.
- **models** — `models` lists backend endpoints and their models; the TUI
  lazily creates/reuses a default server session named "odysseus-code", with
  `--session-id` to attach to a specific one.
- **tui** — full-screen ratatui chat: preloaded history, non-blocking sends,
  exact row-based scrolling, status bar, `/clear` for a fresh session.
- **tests-ci** — 48 tests (unit + assert_cmd/mockito integration) and a
  GitHub Actions workflow (fmt, clippy -D warnings, build, test).
