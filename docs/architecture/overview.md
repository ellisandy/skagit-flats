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

Loads `config.toml` at startup and owns the runtime configuration. All other
layers receive a shared reference; they do not read files directly.

Responsibilities:
- Parse and validate TOML on startup; fail fast with a clear error
- Provide typed access to display settings, location, and per-source intervals
- Reload on SIGHUP or file change (mechanism TBD)

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

**Initial sources**: NOAA/NWS, USGS NWIS, WSDOT Ferries, Trail/Campsite (TBD).

### `domain`

Shared data types. No logic — pure data structures.

```
WeatherObservation  — temperature, wind, sky condition, observation time
RiverGauge          — water level (ft), streamflow (cfs), site ID, timestamp
FerryStatus         — route, vessel name, estimated departures
TrailCondition      — destination name, suitability summary, last updated
DataPoint           — enum wrapping all of the above
```

Domain types are `Clone + Send`. Sources produce them; presentation consumes them.

### `presentation`

Transforms domain values into `Panel` structs — a title and a list of text rows.
No knowledge of pixels, fonts, or layout geometry.

```rust
pub struct Panel {
    pub title: String,
    pub rows: Vec<String>,
}
```

Each source type has a corresponding formatter function. The web interface can
also use presentation to render text previews independently of the pixel pipeline.

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

A lightweight HTTP server (framework TBD; minimal, no heavy frontend) running
on the local network. Serves:

- **Preview endpoint** — renders the current `PixelBuffer` as a PNG or SVG;
  shows exactly what the physical display is showing
- **Config UI** — lists available sources, allows enable/disable and parameter
  changes; writes back to `config.toml`
- **Source status** — last fetch time, last error, next scheduled fetch

The web layer shares the `PixelBuffer` via an `Arc<RwLock<PixelBuffer>>`. It
does **not** have its own rendering logic — it reuses the render pipeline.

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
main thread
  ├── spawns: source/noaa thread  ──sends DataPoint──▶ channel
  ├── spawns: source/usgs thread  ──sends DataPoint──▶ channel
  ├── spawns: source/wsdot thread ──sends DataPoint──▶ channel
  ├── spawns: web server thread   ──reads Arc<RwLock<PixelBuffer>>
  └── owns:   main loop (recv channel, render, display)
```

- Sources are isolated; one panicking source does not take down others
- The main loop is single-threaded — no concurrent writes to the display or PixelBuffer
- The web server reads PixelBuffer via `Arc<RwLock>` (read lock only)
- No async runtime; threads and `std::sync::mpsc` channels throughout

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

## Open Questions

| Question | Status |
|----------|--------|
| Config reload mechanism: SIGHUP vs. file watcher | Undecided |
| Web framework selection | Undecided — leaning minimal (axum or tiny_http) |
| Trail/campsite data source strategy | No unified API; approach TBD |
| Font: embedded bitmap vs. runtime loaded | Undecided |
| Partial refresh region granularity: per-panel vs. full buffer | Undecided |

---

## References

- Product overview: [`docs/product/overview.md`](../product/overview.md)
- Waveshare 7.5" v2 spec: `docs/hardware/` (TBD)
- `rppal` crate: SPI/GPIO access for Raspberry Pi
