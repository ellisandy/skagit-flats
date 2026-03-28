# skagit-flats — Architecture Overview

## System Summary

`skagit-flats` is a single Rust daemon that runs on a Raspberry Pi Zero 2 W.
It fetches data from pluggable public-data sources, renders the results as a
panel layout, and drives a Waveshare 7.5 inch (800×480) e-ink display over SPI.
A local web interface serves configuration and a live preview of the display.

---

## Data Flow

```
┌──────────┐     ┌─────────────┐     ┌────────┐     ┌──────────────┐
│  config  │────▶│   sources   │────▶│ domain │────▶│ presentation │
└──────────┘     └─────────────┘     └────────┘     └──────┬───────┘
                   (per-source                              │
                    threads)                                ▼
                                                     ┌────────────┐
                                                     │   render   │
                                                     └──────┬─────┘
                                                            │
                                          ┌─────────────────┼──────────────┐
                                          ▼                                 ▼
                                   ┌────────────┐                   ┌─────────────┐
                                   │  display   │                   │     web     │
                                   │ (SPI / Pi) │                   │  (preview)  │
                                   └────────────┘                   └─────────────┘
```

Data flows in one direction. No layer calls back into an earlier layer.

---

## Layers

### `config`

Owns two configuration files with clearly separated responsibilities:

| File | Owns | Written by |
|------|------|-----------|
| `config.toml` | Hardware settings, display geometry, location, per-source intervals | Human/agent editing only |
| `destinations.toml` | Destination definitions + per-destination go/no-go criteria | Web UI (and human/agent editing) |

All other layers receive typed, shared references — they do not read files directly.
The web UI writes **only** to `destinations.toml`; `config.toml` is never touched at runtime.

Responsibilities:
- Parse and validate both files at startup; fail fast with a clear error on either
- Provide typed `Config` and `DestinationsConfig` structs to the rest of the system
- Reload `destinations.toml` on change (file watcher); `config.toml` requires restart

### `sources`

One module per data provider. Each source implements the `Source` trait:

```rust
pub trait Source: Send {
    fn name(&self) -> &str;
    fn refresh_interval(&self) -> Duration;
    fn fetch(&self) -> Result<DataPoint, SourceError>;
}
```

Each source runs on its own thread, controlled by the `app` scheduler. When
`fetch()` returns `Ok(DataPoint)`, the value is sent on a channel to the main
loop. On `Err`, the source logs the error and applies its own backoff before
retrying. The main loop is never blocked by a slow or failing source.

**Adding a source**: implement `Source`, register in `app`. No other changes.

**Initial sources**: NOAA/NWS, USGS NWIS, WSDOT Ferries, Trail/Campsite (TBD), Road Closures (TBD).

### `domain`

Shared data types. No logic — pure data structures.

```
WeatherObservation  — temperature, wind, sky condition, observation time
RiverGauge          — water level (ft), streamflow (cfs), site ID, timestamp
FerryStatus         — route, vessel name, estimated departures
TrailCondition      — destination name, suitability summary, last updated
RoadStatus          — road name, closure/restriction status, affected segment
TripCriteria        — per-destination thresholds (min/max temp, max precip,
                       max river level, road open required, etc.)
TripDecision        — Go | NoGo { reasons: Vec<String> }
DataPoint           — enum wrapping all source outputs
```

Domain types are `Clone + Send`. Sources produce them; presentation consumes them.

### `evaluation`

Applies `TripCriteria` to current domain values and produces a `TripDecision`
(Go or NoGo with reasons). This is pure logic — no I/O, no rendering.

```rust
pub fn evaluate(destination: &Destination, state: &DomainState) -> TripDecision
```

Criteria are loaded from config and editable via the web interface. The
evaluation result is passed to `presentation` like any other domain value.

### `presentation`

Transforms domain values and evaluation results into `Panel` structs — a title
and a list of text rows. No knowledge of pixels, fonts, or layout geometry.

```rust
pub struct Panel {
    pub title: String,
    pub rows: Vec<String>,
}
```

Each source type has a corresponding formatter function. Trip decisions render
as a prominent GO / NO GO panel with the blocking reasons listed. The web
interface can also use presentation to render text previews independently of
the pixel pipeline.

### `render`

Lays out `Panel` structs into a `PixelBuffer` (800×480, 1-bit). Handles:
- Panel placement and border geometry
- Font rasterization (embedded bitmap font; no system fonts)
- Line wrapping and truncation within panel bounds

```rust
pub struct PixelBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,  // 1 bit per pixel, packed
}
```

The `PixelBuffer` is the single source of truth for what the display shows.
Both `display` and `web` consume it.

### `display`

SPI driver for the Waveshare 7.5 inch e-ink panel (v2). Wraps `rppal`.

- **Full refresh**: clears ghosting, ~2 seconds. Run hourly.
- **Partial refresh**: repaints a region, ~0.3 seconds. Run when a panel's
  data changes.

The display layer accepts a `PixelBuffer` and a `RefreshMode`. It has no
knowledge of panels, sources, or layout.

### `web`

Built on **axum** (tokio-based). Chosen for agent development friendliness:
axum has strong compile-time typing via extractors and typed responses, explicit
routing with no conventions magic, and wide training corpus coverage. The tokio
runtime is isolated to a single `std::thread::spawn` call in `app`; the rest of
the system remains on `std` threads with channels.

Endpoints:

| Route | Method | Description |
|-------|--------|-------------|
| `GET /preview` | — | Current `PixelBuffer` as PNG — pixel-identical to the physical display |
| `GET /sources` | — | List all sources with last fetch time, last error, next scheduled fetch |
| `GET /destinations` | — | List configured destinations and their current `TripDecision` |
| `POST /destinations` | JSON | Create or update a destination and its `TripCriteria` |
| `DELETE /destinations/:name` | — | Remove a destination |
| `POST /sources/:name/enable` | — | Enable a source |
| `POST /sources/:name/disable` | — | Disable a source |

The web layer shares state via `Arc<RwLock<T>>`:
- `PixelBuffer` — read-only (serves preview)
- `DestinationsConfig` — read/write (config UI writes here, reloads evaluation)

It does **not** have its own rendering logic — it reuses the render pipeline.

### `app`

The runtime: scheduler, channel plumbing, and refresh coordination.

Responsibilities:
- Spawn one thread per source; pass each a `Sender<DataPoint>`
- Own the `Receiver<DataPoint>` main loop
- On receipt of a `DataPoint`: update domain state → re-run presentation →
  re-render affected panels → partial-refresh display → update shared PixelBuffer
- Run a periodic full-refresh timer (hourly)
- Start the web server
- Handle shutdown signals (SIGTERM, SIGINT)

---

## Concurrency Model

```
main thread (std)
  ├── spawns: source/noaa thread (std)   ──sends DataPoint──▶ mpsc channel
  ├── spawns: source/usgs thread (std)   ──sends DataPoint──▶ mpsc channel
  ├── spawns: source/wsdot thread (std)  ──sends DataPoint──▶ mpsc channel
  ├── spawns: web server thread (std)
  │     └── starts tokio runtime
  │           └── axum router ──reads Arc<RwLock<PixelBuffer>>
  │                           ──reads/writes Arc<RwLock<DestinationsConfig>>
  └── owns: main loop (recv channel, evaluate, render, display)
```

- Sources are isolated; one panicking source does not take down others
- The main loop is single-threaded — no concurrent writes to the display or PixelBuffer
- The tokio runtime is confined to the web server thread; all other concurrency uses `std`
- The web server holds read locks on `PixelBuffer`; write locks on `DestinationsConfig`
  are short-lived (config update only, not on the hot path)

---

## Local Development (Docker, No Hardware)

The daemon supports a `--no-hardware` mode (or `SKAGIT_NO_HARDWARE=1` env var)
that disables the SPI display driver entirely. In this mode:

- The `display` layer is replaced by a no-op stub
- The web interface is the only output — the preview endpoint serves the current
  `PixelBuffer` as a PNG
- Sources still run on their normal schedules (or can be pointed at fixture data)

A `Dockerfile` and `docker-compose.yml` at the repo root provide a ready-to-run
local environment:

```sh
docker compose up
# Web UI available at http://localhost:8080
```

This allows full end-to-end testing of the config UI, source pipeline, and
render output without a Pi or e-ink panel. The preview in the browser is
pixel-identical to what the physical display would show.

**Fixture mode**: set `SKAGIT_FIXTURE_DATA=1` to have sources return static
fixture responses instead of making live API calls. Useful for UI development
and CI.

---

## Extension Points

### Adding a new data source

1. Create `src/sources/<name>.rs` implementing `Source`
2. Add the source variant to the `DataPoint` enum in `domain`
3. Add a presenter function in `presentation`
4. Register the source in `app` (add to scheduler startup)
5. Add config fields in `config` if needed

No changes to `render`, `display`, or `web`.

### Adding a new panel layout

Modify `render` to change panel geometry. Sources and presentation are unaffected.

### Adding a web UI feature

Add endpoints to `web`. The render pipeline is already shared; new features
read from `Arc<RwLock<PixelBuffer>>` and the config.

---

## Error Handling Philosophy

- Sources return `Result<DataPoint, SourceError>` — never panic
- On source error: log, keep the previous panel value, back off and retry
- On display error: log, attempt recovery; a blank display is preferable to a crash
- On config parse error at startup: fail fast with a descriptive message
- The daemon must survive indefinitely without human intervention; every error
  path must be handled

---

## Hardware Constraints

| Constraint | Value | Impact |
|-----------|-------|--------|
| CPU | Pi Zero 2 W (4× ARM Cortex-A53 @ 1 GHz) | Keep render loop lightweight; no heavy image processing |
| RAM | 512 MB | No large in-memory caches; bounded buffers per source |
| Display | 800×480, 1-bit | No grayscale; bitmap fonts only; full refresh is slow |
| SPI bus | Single shared bus | Display writes are serialized; no concurrent SPI access |
| Network | Wi-Fi only | Sources must handle intermittent connectivity gracefully |

---

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Web framework | **axum** | Strong compile-time typing; explicit routing; wide agent training corpus coverage; tokio isolated to one thread |
| Criteria storage | **`destinations.toml`** | Separates user data from hardware config; web UI writes only this file; schema maps directly to `Destination` + `TripCriteria` types |
| Config reload | **file watcher for `destinations.toml`; restart for `config.toml`** | Destinations change via web UI at runtime; hardware config changes are rare and benefit from a clean restart |

## Open Questions

| Question | Status |
|----------|--------|
| Trail/campsite data source strategy | No unified API; approach TBD — spike required |
| Road closure data coverage | WSDOT covers state roads; USFS/county coverage inconsistent |
| Font: embedded bitmap vs. runtime loaded | Undecided |
| Partial refresh region granularity: per-panel vs. full buffer | Undecided |

---

## References

- Product overview: [`docs/product/overview.md`](../product/overview.md)
- Waveshare 7.5" v2 spec: `docs/hardware/` (TBD)
- `rppal` crate: SPI/GPIO access for Raspberry Pi
