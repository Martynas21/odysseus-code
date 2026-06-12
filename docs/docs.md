# docs — handoff

## What was built

- `README.md` — user-facing documentation from the product spec: purpose,
  core philosophy (specialization / safety & reproducibility /
  extensibility), features matrix, install (`cargo install --path .`),
  API-token setup (Odysseus web UI -> Settings -> API Tokens), config table
  (endpoint, api_key, model, endpoint_id, default_language, sandbox_image),
  usage examples (prompt, generate in both formats,
  `cat x.rs | odysseus-code run -`, generate-pipe-run, session start/end,
  models, tui), extending guide pointing at the real extension points
  (`src/cli.rs`, `src/actions/`, `src/sandbox.rs` `spec_for` table,
  `src/context.rs` `language_for_extension`), and the dev/CI commands.
- `CHANGELOG.md` — 0.1.0 with one entry per feature (skeleton, config,
  api-client, prompt, generate, sandbox, sessions, tui, tests-ci).

## Files touched

- `README.md` — new
- `CHANGELOG.md` — new

## Conventions

- All command examples use plain ASCII hyphens (verify with
  `grep -nP '[^\x00-\x7F]' README.md` — only prose arrows/dashes outside
  code blocks, none inside command examples).
- README links into `docs/MVP.md` for the development roadmap; per-feature
  handoff docs stay the source of truth for design detail.

## Verification

```sh
grep -c 'odysseus-code ' README.md   # usage examples present
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## Next steps

- Update README's features matrix and CHANGELOG when post-0.1.0 features
  land (release ritual: bump Cargo.toml version + CHANGELOG entry).
