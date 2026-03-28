# Skagit Flats — Product Overview

## Problem

Living in the Skagit Valley and surrounding islands means tracking a handful of
hyper-local conditions that generic weather apps don't surface well: is the
Skagit running high enough to flood lowland roads? Which ferry is running late?
What are actual conditions at a nearby NOAA observation station rather than an
interpolated forecast?

Pulling this from a phone works, but requires intent — opening three different
apps, navigating to the right locations, and mentally assembling a picture.
The information is ambient but the retrieval is not.

A wall-mounted e-ink display changes the equation: glanceable, always-on,
no interaction required. The display is visible from across the room and
consumes near-zero power when static.

## Goals

- Show river gauge level and streamflow for a configured USGS site (Skagit River
  at Mount Vernon, or similar)
- Show current weather conditions from a nearby NOAA observation station
- Show vessel status and departure times for a configured WSDOT ferry route
- Update each panel on its own schedule, appropriate to how fast that data
  changes (ferry: fast; river: moderate; weather: slow)
- Run unattended for months on a Raspberry Pi Zero 2 W without intervention
- Survive network outages and API errors gracefully — stale data is acceptable,
  crashes are not
- Be configurable for a different location without code changes

## Non-Goals

- General-purpose dashboard framework — this is purpose-built for one display
  and one household's data needs
- Touch input or any user interaction with the display hardware
- Forecasts — current conditions only; this is not a weather app
- Cloud sync, remote management, or any network-accessible interface
- Support for display hardware other than the Waveshare 7.5 inch e-ink panel
- Mobile or web companion app

## Users

One: the person who built it. Possibly also anyone who lives in the same house
and glances at the wall.

## Architecture Summary

A single daemon process running on a Raspberry Pi Zero 2 W drives a Waveshare
7.5 inch (800×480) e-ink panel over SPI.

```
config → sources → domain → presentation → render → display
```

| Layer | Responsibility |
|-------|---------------|
| `config` | Load and own runtime configuration from TOML |
| `sources` | One module per provider; each runs on its own timer thread |
| `domain` | Shared data types: `WeatherObservation`, `RiverGauge`, `FerryStatus` |
| `presentation` | Format domain values into `Panel` structs (title + text rows) |
| `render` | Lay out panels into a `PixelBuffer`; font rasterization and geometry |
| `display` | SPI driver for the Waveshare panel; full and partial refresh |
| `app` | Scheduler, channel plumbing, refresh coordination |

Each source runs independently. When a source produces new data it sends a
`DataPoint` on a channel to the main loop, which rebuilds the affected panel,
re-renders, and pushes to the display. A full refresh (to clear e-ink ghosting)
runs on a separate hourly timer.

## Key Technical Decisions

**Rust** — the daemon runs continuously on hardware with 512 MB RAM. Rust's
ownership model eliminates a class of memory and concurrency bugs that would
be hard to reproduce on a Pi. The `rppal` crate provides safe SPI/GPIO access
without a C shim.

**Internal scheduling over cron** — each source owns its retry logic and
backoff. This enables partial display updates (only repaint the panel whose
data changed), avoids IPC between processes, and keeps error handling local
to the source that failed. One process, one log stream.

**E-ink over LCD** — near-zero power when static, readable in direct sunlight,
no backlight glow in a dark room. Refresh latency (1–2 seconds for a full
refresh) is acceptable for this use case; partial updates are faster.

**TOML configuration** — human-editable, no external dependencies, maps
naturally to the nested settings structure (per-source intervals, location,
display geometry).

## Data Sources

| Source | API | What it provides |
|--------|-----|-----------------|
| NOAA / NWS | `api.weather.gov` | Temperature, wind speed/direction, sky conditions at a configured observation station |
| USGS NWIS | `waterservices.usgs.gov` | Water level (ft) and streamflow (cfs) at a configured gauge site |
| WSDOT Ferries | `wsdot.wa.gov/ferries/api` | Vessel location, estimated departure times for a configured route |

All three APIs are public and require no authentication.

## Risks and Open Questions

| Risk | Severity | Notes |
|------|----------|-------|
| WSDOT ferry API instability | Medium | The WSDOT API has changed endpoints before; may need a thin adapter layer |
| SPI driver compatibility across Pi models | Low | `rppal` supports Pi Zero 2 W; untested on other models |
| E-ink partial update artifacts | Low | Partial refresh accumulates ghosting; mitigated by hourly full refresh |
| NOAA station coverage gaps | Low | Some areas have sparse observation stations; nearest may be far from location |
| Cross-compilation toolchain setup | Low | Requires `aarch64-unknown-linux-gnu` target; documented in README |
| Font rendering on 1-bit display | Low | No antialiasing; choice of font and size matters for readability |

## Out of Scope (Explicit Deferrals)

- Tides (NOAA CO-OPS API) — obvious addition but deferred until core sources work
- Air quality (AirNow API) — relevant in wildfire season; deferred
- Road/bridge closure alerts — no suitable public API identified
- NWS alerts/warnings overlay — would require layout changes; deferred

## Success Criteria

The project is done when:

1. The daemon compiles for `aarch64-unknown-linux-gnu`
2. It runs on a Pi Zero 2 W with a real Waveshare 7.5 inch panel attached
3. All three panels show live data from the configured sources
4. The display updates without intervention for 30 days
5. A power cycle recovers automatically via systemd

## Current Status

Initial scaffold committed. Source modules, rendering pipeline, and display
driver are stubs. No real API calls are made yet. Next step: implement the
NOAA source module end-to-end as a reference implementation for the others.
