# AGENTS.md — skagit-flats

Guidance for AI agents working in this repository.

## What this is

A thin e-ink display client. The hot path is ~30 lines:

```
fetch image_url → decode PNG → 1-bit pack → push to display → sleep → repeat
```

Data fetching, layout, and rendering all happen upstream in a separate
service (the `cascades` project). This binary's job is to put pixels on the
panel and survive flaky networks.

## Module layout

| Path                | Responsibility                                       |
|---------------------|------------------------------------------------------|
| `src/main.rs`       | Parse args, load config, hand off to `app::run`      |
| `src/app/mod.rs`    | Fetch loop and `AppOptions` parsing                  |
| `src/config/mod.rs` | TOML schema (`[device]`, `[display]`)                |
| `src/display/`      | `DisplayDriver` trait, `NullDisplay`, Waveshare SPI  |
| `src/render/`       | `PixelBuffer` (1-bit, packed)                        |

## What not to add

- **No web UI.** Configuration is via `config.toml` + restart. The upstream
  service owns everything user-facing.
- **No data sources.** No NOAA, USGS, ferries, weather APIs. Those belong
  upstream. If the display needs new info, add it server-side.
- **No rendering logic.** No fonts, layout, panels, sparklines. The fetched
  PNG is the whole picture — we just decode and forward bytes.
- **No async runtime.** The whole binary is a blocking single-threaded loop.
  Don't pull in tokio.

## Display constraints

- **800×480, 1-bit.** Size mismatch fails the Waveshare driver hard.
- Full refresh (~2s) is the only mode in use right now. Partial refresh
  exists in the trait but isn't wired up to the loop.
- The driver inverts and sends both DTM1 (old frame) and DTM2 (new frame)
  per refresh — see `src/display/waveshare.rs` for command sequencing.

## Cross-compilation

Target `aarch64-unknown-linux-gnu` via `cargo zigbuild`. The `rppal` crate
fails to build on macOS (Linux-only termios) — that's expected and not a
regression. Use `cargo check` for default builds and `cargo zigbuild
--features hardware` (or build on the Pi) to validate the hardware path.

Avoid native C dependencies — they make the cross-compile setup brittle.

## Testing

- Unit tests live alongside the code.
- `tests/hardware_tests.rs` is gated behind both the `hardware` feature
  and `SKAGIT_HARDWARE_TESTS=1`. Run on a Pi with the panel connected.
- No integration tests — the upstream contract is just "GET returns
  image/png", which is trivial enough to smoke-test by hand.

## Error handling

- A failed fetch logs and retries on the next tick. Last successful frame
  stays on the display.
- Network calls have an explicit 10s timeout — never block forever.
- No `unwrap()`/`expect()` in `app::run` or below.
