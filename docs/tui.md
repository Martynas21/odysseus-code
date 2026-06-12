# tui — handoff

## What was built

`odysseus-code tui [--session-id <id>]` opens a full-screen ratatui (v0.30)
chat client against the Odysseus backend:

- Session is resolved exactly like `prompt` (`actions::resolve_session`:
  explicit `--session-id` → active session → lazy default "odysseus-code").
- Existing conversation history is loaded via `client.history(sid)` before the
  terminal is initialized, so auth/network errors print normally to stderr.
- Layout: scrollable transcript pane (bordered, title `odysseus-code`),
  3-row input line, 1-row status bar showing endpoint | model | session.
- Enter sends the input through `client.chat` on a spawned tokio task;
  replies come back over a `tokio::sync::mpsc` unbounded channel polled with
  `try_recv()` each frame, so the draw loop never blocks. While pending, a
  "thinking…" line is appended to the transcript and the status bar.
- Prompts are wrapped with `PromptContext::wrap` (same `[context] {…}
  [/context]` block as `prompt`); displayed user messages have that prefix
  stripped again (`strip_context_prefix`).
- Keys: Enter send, Esc / Ctrl-C quit, Up/Down scroll one row,
  PageUp/PageDown scroll 10 rows, Backspace edits. Input is ignored while a
  reply is pending (no concurrent sends).

## Files touched

- `src/actions/tui.rs` — new; all TUI code and its unit tests.
- `src/actions.rs` — `pub mod tui;`
- `src/main.rs` — `Command::Tui` now calls `actions::tui::handle`.

## Design notes / public interfaces

- `actions::tui::handle(session_id, project_path, current_file) -> Result<()>`
  is the only public item.
- Rendering and scroll logic are pure functions with unit tests:
  - `strip_context_prefix(&str) -> &str`
  - `wrap_text(text, width) -> Vec<String>` — hard-wraps at char boundaries
    (text is pre-wrapped instead of using `Paragraph::wrap`, so the scroll
    offset in rows is exact).
  - `message_lines(&[DisplayMessage], width, thinking) -> Vec<Line>` — label
    line per message (You/Odysseus/Error, color-coded) + content + blank line.
  - `scroll_offset(total, viewport, from_bottom) -> u16` — scroll state is
    "rows up from the bottom"; 0 sticks to the newest message, and `App::push`
    resets it to 0 so new replies are always visible.
- `ratatui::init()/restore()` manage the terminal; crossterm types are
  imported from `ratatui::crossterm` to avoid version mismatch.
- `OdysseusClient` is `Clone`; the background send task clones it plus the
  session ID and the wrapped message.

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build                      # compiles clean
cargo test actions::tui          # 8 unit tests pass (46 total in the crate)
# graceful no-token failure (isolated dirs, no terminal corruption):
tmp=$(mktemp -d); ODYSSEUS_CODE_CONFIG_DIR=$tmp ODYSSEUS_CODE_CACHE_DIR=$tmp \
  ODYSSEUS_API_TOKEN= ODYSSEUS_URL= cargo run -q -- tui; rm -rf $tmp
# → prints the "Settings → API Tokens" hint, exit 1
```

Live verification against localhost:7000 is still pending an `ody_` token
(none configured as of 2026-06-12).

## Gotchas

- `event::poll(50ms)` blocks a tokio worker thread; fine because the runtime
  is multi-threaded (`rt-multi-thread`) and the chat call runs on another
  worker.
- Char-based wrapping treats wide (CJK) glyphs as width 1 — scroll math can
  be slightly off for those; acceptable for now.
- Cursor position is clamped to the input box width; very long input scrolls
  the cursor visually but not the text (single-line input, no horizontal
  scroll yet).

## Next steps (optional)

- Horizontal input scrolling / multi-line input.
- Mouse-wheel scroll, `/quit`-style commands, model switcher.
