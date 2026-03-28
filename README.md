# skagit-flats

A general-purpose, pluggable public-data dashboard that runs on a Raspberry Pi
and drives a Waveshare 7.5 inch e-ink display. Built for the Skagit Valley —
river levels, weather, ferry schedules, trail conditions — but designed so any
data source can be added or removed without touching the core.

A companion web interface (served from the Pi) lets users configure which
sources are active, adjust formatting, and preview the e-ink layout in the
browser before it hits the display.

---

## What it does

The daemon fetches data from configurable sources on independent schedules,
formats results into panels, renders them to a pixel buffer, and pushes that
buffer to the e-ink display over SPI. The web interface exposes the same
render pipeline as a live preview.

**Default sources (examples — not the fixed set):**

| Source | What it shows | API |
|--------|--------------|-----|
| NOAA / NWS | Conditions at a local observation station (temp, wind, sky) | `api.weather.gov` |
| USGS NWIS | River gauge — water level and streamflow | `waterservices.usgs.gov` |
| WSDOT Ferries | Vessel status and departure times for a configured route | `wsdot.wa.gov/ferries/api` |
| Trail / Campsite | Weekend suitability for configured hiking and camping destinations | WTA, Recreation.gov, USFS |
| Road Closures | Closure and restriction status for roads leading to configured destinations | WSDOT, USFS, county APIs |

All default APIs are public and require no authentication.

---

## Who it's for

A household or small group (roughly 5–10 people) who want glanceable,
always-on local data without opening an app. The display and web interface
should be self-describing — a new user should understand what's shown and
how to change it without being told.

---

## Architecture

```
config → sources → domain → presentation → render ──→ display (SPI)
                                                   ↘
                                                    web (preview + config UI)
```

Each source is an independent module implementing a common `Source` trait.
Sources run on their own timer threads and push `DataPoint` values to the main
loop over a channel. The main loop rebuilds affected panels, re-renders, and
updates the display. The web server renders the same pixel buffer as an
in-browser preview.

See [`docs/architecture/overview.md`](docs/architecture/overview.md) for the
full design.

---

The system also evaluates configurable go/no-go criteria per destination
(temperature range, precipitation limits, river level thresholds, road access)
and renders a clear **GO / NO GO** decision panel alongside the raw data.

---

## Running locally (no hardware)

```sh
docker compose up
```

Opens the web UI at `http://localhost:8080`. The SPI display driver is disabled;
the browser preview is the only output. Set `SKAGIT_FIXTURE_DATA=1` to use
static fixture data instead of live API calls.

---

## Why Rust

- **Low overhead** — runs continuously on a Pi Zero 2 W (512 MB RAM)
- **Reliability** — ownership model eliminates memory bugs; `Result` forces
  explicit error handling at every source call
- **SPI access** — `rppal` gives safe, idiomatic GPIO/SPI without a C shim
- **Concurrency** — per-source threads and channels map naturally to Rust's
  ownership model

## Why internal scheduling instead of cron

Each source owns its retry logic and backoff. This enables partial display
updates (only repaint the panel whose data changed), avoids IPC between
processes, and keeps errors local to the source that failed. One process,
one log stream, one systemd unit.

---

## Configuration

Copy `config.sample.toml` and edit for your location:

```toml
[display]
width  = 800
height = 480

[location]
latitude  = 48.4231
longitude = -122.3368
timezone  = "America/Los_Angeles"

[sources.noaa]
refresh_interval_secs = 300

[sources.usgs]
refresh_interval_secs = 900

[sources.wsdot]
refresh_interval_secs = 120
```

---

## Building

```sh
cargo build --release
```

Cross-compile for Raspberry Pi (from x86):

```sh
cargo build --release --target aarch64-unknown-linux-gnu
```

---

## Docs

- [`docs/product/overview.md`](docs/product/overview.md) — problem, goals, non-goals, success criteria
- [`docs/architecture/overview.md`](docs/architecture/overview.md) — component design, data flow, extension points

---

## Status

Product document and architecture design complete. Implementation in progress.
