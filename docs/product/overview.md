# Skagit Flats — Product Overview

## Problem

Living in the Skagit Valley and surrounding islands means tracking a handful of
hyper-local conditions that generic weather apps don't surface well: is the
Skagit running high enough to flood lowland roads? Which ferry is running late?
What are actual conditions at a nearby NOAA observation station rather than an
interpolated forecast? Is this a good weekend to hike or camp at a specific site?

Pulling this from a phone works, but requires intent — opening three different
apps, navigating to the right locations, and mentally assembling a picture.
The information is ambient but the retrieval is not.

A wall-mounted e-ink display changes the equation: glanceable, always-on,
no interaction required. The display is visible from across the room and
consumes near-zero power when static.

## Goals

- Provide a general-purpose, pluggable dashboard framework; NOAA, USGS, WSDOT,
  and trail/campsite conditions are the initial example sources, not the fixed
  set — users can add or remove sources
- Show river gauge level and streamflow for a configured USGS site (Skagit River
  at Mount Vernon, or similar)
- Show current weather conditions from a nearby NOAA observation station
- Show vessel status and departure times for a configured WSDOT ferry route
- Allow users to input specific local campsites or hiking destinations and answer
  "Is this a good weekend to go?" based on weather, river levels, and trail
  conditions
- Serve a web interface from the Pi for configuring which sources are displayed,
  how they are formatted, and showing a live mock preview of the e-ink layout
- Update each panel on its own schedule, appropriate to how fast that data
  changes (ferry: fast; river: moderate; weather: slow)
- Run unattended for months on a Raspberry Pi Zero 2 W without intervention
- Survive network outages and API errors gracefully — stale data is acceptable,
  crashes are not
- Be self-describing: a new user should be able to understand what is displayed
  and why without prior explanation

## Non-Goals

- Touch input directly on the e-ink display hardware
- Forecasts beyond what public APIs provide — this is not a custom forecast engine
- Cloud sync or any externally-accessible network interface (local network only)
- Support for display hardware other than the Waveshare 7.5 inch e-ink panel
- Mobile app

## Users

A small household or shared living group — roughly 5–10 people. Each user should
be able to understand what the display shows without being told. Configuration
(adding sources, adjusting layout) happens via the web interface and should not
require editing config files directly.

## Architecture Summary

A single daemon process running on a Raspberry Pi Zero 2 W drives a Waveshare
7.5 inch (800×480) e-ink panel over SPI, and also serves a local web interface
for configuration and preview.

```
config → sources → domain → presentation → render → display
                                                  ↘ web (preview + config UI)
```

| Layer | Responsibility |
|-------|---------------|
| `config` | Load and own runtime configuration from TOML |
| `sources` | Plugin-style modules, one per data provider; each runs on its own timer thread |
| `domain` | Shared data types: `WeatherObservation`, `RiverGauge`, `FerryStatus`, etc. |
| `presentation` | Format domain values into `Panel` structs (title + rows of text) |
| `render` | Lay out panels into a `PixelBuffer`; font rasterization and geometry |
| `display` | SPI driver for the Waveshare panel; full and partial refresh |
| `web` | Local HTTP server: configuration UI, source management, mock e-ink preview |
| `app` | Scheduler, channel plumbing, refresh coordination |

Each source runs independently. When a source produces new data it sends a
`DataPoint` on a channel to the main loop, which rebuilds the affected panel,
re-renders, and pushes to the display. The web interface renders the same
`PixelBuffer` as a preview, so what you see in the browser matches the physical
display. A full refresh (to clear e-ink ghosting) runs on a separate hourly timer.

## Key Technical Decisions

**Rust** — the daemon runs continuously on hardware with 512 MB RAM. Rust's
ownership model eliminates a class of memory and concurrency bugs that would
be hard to reproduce on a Pi. The `rppal` crate provides safe SPI/GPIO access
without a C shim.

**Plugin-style sources** — each source implements a common `Source` trait.
Adding a new data source means adding a new module; no changes to the core
pipeline. The web UI reflects available sources dynamically.

**Internal scheduling over cron** — each source owns its retry logic and
backoff. This enables partial display updates (only repaint the panel whose
data changed), avoids IPC between processes, and keeps error handling local
to the source that failed. One process, one log stream.

**E-ink over LCD** — near-zero power when static, readable in direct sunlight,
no backlight glow in a dark room. Refresh latency (1–2 seconds for a full
refresh) is acceptable for this use case; partial updates are faster.

**TOML configuration** — human-editable, no external dependencies, maps
naturally to the nested settings structure (per-source intervals, location,
display geometry). The web UI writes back to the same TOML file.

## Data Sources

Initial sources (examples, not the fixed set):

| Source | API | What it provides |
|--------|-----|-----------------|
| NOAA / NWS | `api.weather.gov` | Temperature, wind speed/direction, sky conditions at a configured observation station |
| USGS NWIS | `waterservices.usgs.gov` | Water level (ft) and streamflow (cfs) at a configured gauge site |
| WSDOT Ferries | `wsdot.wa.gov/ferries/api` | Vessel location, estimated departure times for a configured route |
| Trail/Campsite Conditions | Recreation.gov / USFS / Wta.org | Trail and campsite status, recent trip reports, weekend suitability summary |

All initial APIs are public and require no authentication.

## Risks and Open Questions

| Risk | Severity | Notes |
|------|----------|-------|
| WSDOT ferry API instability | Medium | The WSDOT API has changed endpoints before; may need a thin adapter layer |
| Trail/campsite data source quality | Medium | No single authoritative API; may need to aggregate WTA, Recreation.gov, and USFS |
| Web UI complexity on Pi Zero 2 W | Medium | Serving HTTP + rendering + SPI driver on one process; may need to profile memory |
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
3. All initial source panels show live data
4. The web interface allows a user to add/remove sources and see a mock preview
5. A new user can understand the display and reconfigure it without help
6. The display updates without intervention for 30 days
7. A power cycle recovers automatically via systemd

## Current Status

Product document drafted. No code committed yet. Next step: scaffold the
repository structure (Rust project, module layout, CI configuration).
