# AGENTS.md — skagit-flats

Guidance for AI agents (Claude, Copilot, etc.) working in this repository.

---

## What this project is

`skagit-flats` is a Rust daemon that drives a Waveshare 7.5 inch e-ink display
on a Raspberry Pi Zero 2 W. It fetches data from pluggable public-data sources
(weather, river gauges, ferry schedules, trail conditions) and renders them as
panels on the display. A local web interface allows configuration and preview.

Read [`docs/product/overview.md`](docs/product/overview.md) before starting
any feature work. It defines goals, non-goals, and success criteria.

---

## Architecture

```
config → sources → domain → presentation → render ──→ display (SPI)
                                                   ↘
                                                    web (preview + config UI)
```

Full design: [`docs/architecture/overview.md`](docs/architecture/overview.md)

Key invariants:
- Data flows **one direction**: sources → domain → presentation → render → output
- Sources **never** call presentation or render code
- The `web` layer reuses the render pipeline — it does not have its own renderer
- Each source implements the `Source` trait; adding a source means adding a module, not changing the core

---

## Working in this repo

### Before writing code

1. Check `docs/product/overview.md` — understand what's in scope
2. Check `docs/architecture/overview.md` — understand where your change fits
3. Check `bd ready` for existing issues before creating new ones

### Source trait

Every data source must implement the `Source` trait (defined in `src/sources/mod.rs`).
A source:
- Has a name and a refresh interval
- Fetches data and returns a `Result<DataPoint, SourceError>`
- Handles its own retry/backoff — the scheduler does not retry on your behalf
- Must not panic; return `Err` instead

### Error handling

- Use `Result` everywhere. No `.unwrap()` in production paths.
- Source errors are logged and the previous value is kept on the display — stale
  data is acceptable, crashes are not.
- Network timeouts must be explicit; never block a thread indefinitely.

### Display constraints

- The Waveshare 7.5 inch panel is **800×480, 1-bit** (black/white only)
- No antialiasing. Font choice and size matter — test readability at small sizes
- Full refresh (~2s) clears ghosting; partial refresh (~0.3s) repaints a region
- Prefer partial refresh; schedule full refresh hourly via the app layer

### Web interface

- Served on the local network only — no authentication required, but also no
  external exposure assumed
- Must render the same `PixelBuffer` used by the display driver — do not build
  a separate preview renderer
- Config writes go to `config.toml`; the daemon reloads on change (SIGHUP or
  file watcher, TBD)

### Testing

- Unit test source parsing logic against fixture responses (not live API calls)
- Integration tests that hit real APIs are opt-in and must be gated behind a
  feature flag or environment variable
- The render pipeline should be testable by comparing pixel buffers to golden
  files

### Cross-compilation

Target: `aarch64-unknown-linux-gnu`

```sh
cargo build --release --target aarch64-unknown-linux-gnu
```

Do not introduce dependencies that require native C libraries unless
absolutely necessary — cross-compilation becomes significantly harder.

---

## What not to do

- Do not add a source by modifying the core scheduler or render loop — use the
  `Source` trait
- Do not add authentication or cloud sync — these are explicit non-goals
- Do not add forecast data — current conditions only
- Do not use `unwrap()` or `expect()` in any path that runs on the Pi
- Do not introduce async runtimes (Tokio, async-std) without discussing first —
  the current design uses threads and channels intentionally
- Do not create a separate renderer for the web preview — reuse the render pipeline

---

## Open questions (as of initial design)

- Trail/campsite data source: WTA, Recreation.gov, and USFS have no unified API.
  The approach is TBD — likely scraping or a thin aggregator.
- Config reload mechanism: SIGHUP vs. file watcher — not yet decided.
- Web framework: no decision made; keep it minimal (no heavy frontend framework).

---

## References

- Product overview: `docs/product/overview.md`
- Architecture: `docs/architecture/overview.md`
- Sample config: `config.sample.toml`
- Waveshare 7.5" v2 datasheet: in `docs/hardware/` (TBD)
