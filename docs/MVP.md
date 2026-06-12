# odysseus-code — MVP Roadmap & Handoff Index

Single source of truth for what is done and what is next. Agents picking up
work: read this file first, then the handoff docs of completed features
adjacent to your task.

**Backend:** the user's self-hosted Odysseus instance (`~/projects/odysseus`),
live at `http://localhost:7000`. Auth is a `Bearer ody_…` API token created in
Odysseus Settings → API Tokens. Wire protocol details live in
[api-client.md](api-client.md) once that chunk lands; until then see the plan
notes below.

## Completion ritual (every feature)

implement → verify → write `docs/<feature>.md` handoff → tick the box here →
`cargo fmt` → commit referencing the doc.

Parallel agents: pick an unticked feature whose dependencies are ticked; touch
only your own feature doc plus your row here.

## Features

### MVP (priority: working `prompt` against the live backend)

- [x] **skeleton** — Cargo project, clap CLI surface, this roadmap. Handoff: [skeleton.md](skeleton.md)
- [x] **config** — `config.rs`, `~/.config/odysseus-code/config.yaml`, `config set/get/path`. Handoff: [config.md](config.md)
- [x] **api-client** — `client.rs` + `context.rs`: chat, session create/delete, models, history against the Odysseus REST API; mockito tests. Handoff: [api-client.md](api-client.md)
- [ ] **prompt** — `prompt` subcommand incl. lazy default server session + `models` helper; live verification against localhost:7000. Handoff: docs/prompt.md

### Post-MVP

- [ ] **generate** — `generate <lang> "<desc>" --format pretty|compact`. Handoff: docs/generate.md (depends: api-client, prompt)
- [ ] **sandbox** — `run` subcommand + Docker sandbox, stdin support. Handoff: docs/sandbox.md (independent of api-client)
- [ ] **sessions** — `session start/end`, local name→server-id map, `--session-id` routing. Handoff: docs/sessions.md (depends: api-client)
- [ ] **tui** — ratatui chat screen. Handoff: docs/tui.md (depends: api-client, sessions)
- [ ] **tests-ci** — remaining unit tests, `tests/cli.rs`, GitHub Actions (fmt, clippy, build, test). Handoff: docs/tests-ci.md
- [ ] **docs** — README from the product spec + CHANGELOG. Handoff: docs/docs.md

## Plan notes (until api-client.md exists)

- `POST /api/chat` JSON `{message, session}` → `{"response": "..."}`
- `POST /api/session` **form-encoded** `name`, `endpoint_id`, `model` → `{id, ...}` (raw `endpoint_url` is admin-only)
- `DELETE /api/session/{sid}`, `GET /api/history/{sid}`, `GET /api/models`
- History is server-side; context metadata (project_path, current_file, language) is embedded as a prefix block in the message text.
- Full approved plan: `/home/omega/.claude/plans/stateful-fluttering-koala.md`
