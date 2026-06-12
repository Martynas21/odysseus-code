# Handoff: sandbox

## What was built

`odysseus-code run "<code>" [--lang <l>]` — compiles/executes a snippet in an
ephemeral Docker container and relays stdout/stderr + the snippet's exit code.
Reads from stdin when the snippet is omitted or `-` (`cat x.rs | odysseus-code run`).

## Files

- `src/sandbox.rs` — language table, `image_for`, `run` (docker invocation);
  2 unit tests + 1 `#[ignore]`d live test
- `src/actions/run.rs` — stdin/arg handling, prints output, returns exit code
- `src/main.rs` — propagates the snippet's exit code via `std::process::exit`

## Container invocation

```
docker run --rm --network=none --memory=512m --pids-limit=256 \
  -v <tempdir>:/work -w /work <image> sh -c "timeout 120 sh -c '<compile&run>'"
```

Temp dir holds the snippet (`main.rs`/`main.py`/`main.sh`), auto-deleted.
Languages: rust (`rustc main.rs -o main && ./main`), python, sh — extend
`spec_for` in `src/sandbox.rs` for more.

## Image resolution (decision)

`config sandbox_image` applies to **rust** (per the product spec it names a
Rust toolchain image; default `rust:slim`). python → `python:3-slim`,
sh → `alpine:3`, so non-rust snippets work without reconfiguring. docker
exit 125 (image/daemon failure) is reported as our error, not the snippet's.

## Verify

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test sandbox -q                # unit tests, no docker
cargo test -- --ignored              # live: needs docker + alpine:3
echo 'echo hi' | cargo run -q -- run --lang sh -   # prints hi, exit 0
cargo run -q -- run 'fn main(){println!("hi")}'    # needs rust:slim pulled
```

Verified live on this machine: sh path (alpine:3) incl. stdin and exit-code
propagation, and rust path (`rust:slim`) compiling and printing successfully.

## Gotchas

- 120s in-container timeout guards infinite loops; exit 124 = timed out.
- WSL2: docker mounts of /tmp tempdirs work via the Docker Desktop/WSL
  integration; no action needed.

## Next steps

sessions feature; later the README should document the `generate … --format
compact | odysseus-code run -` pipeline.
