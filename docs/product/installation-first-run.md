# Skagit Flats — Installation and First-Run Workflow

## Overview

This document defines the end-to-end installation and first-run experience for
Skagit Flats. It covers prerequisites, supported deployment paths, initial
configuration, the first successful launch, and the minimum viable path to a
useful first preview and initial destination setup.

It also identifies where the current setup is too manual, confusing, or
implementation-driven, and calls out the product changes needed to make each
pain point tractable.

---

## Supported Deployment Paths

Two deployment modes are supported. Both lead to the same web UI; only the
hardware integration differs.

| Mode | Platform | Hardware | Output |
|------|----------|----------|--------|
| **Docker (local dev/preview)** | Any laptop/desktop | None required | Web UI + PNG preview only |
| **Raspberry Pi (production)** | Raspberry Pi Zero 2 W | Waveshare 7.5" e-ink panel + SPI | Web UI + live e-ink display |

---

## Path 1: Docker Local Preview

The Docker path is the first-run experience for developers, evaluators, and
anyone who wants to see the layout before committing to hardware.

### Prerequisites

- Docker Desktop (Mac/Windows) or Docker Engine (Linux) installed
- Git

### Steps

```bash
# 1. Clone the repository
git clone <repo-url>
cd skagit-flats

# 2. Start the service
docker compose up
```

That's it. Docker Compose:
- Builds the image from the Dockerfile
- Mounts `config.sample.toml` and `destinations.sample.toml` as the active config
- Sets `SKAGIT_NO_HARDWARE=1` (SPI driver disabled)
- Sets `SKAGIT_FIXTURE_DATA=1` (pre-recorded API responses, no live network calls)
- Exposes port 8080

### First Successful Launch Checkpoints

1. **Build succeeds** — Rust compilation completes; image is ready
2. **Service is running** — `docker compose ps` shows `skagit-flats` as `Up`
3. **Health check passes** — `curl http://localhost:8080/health` returns `OK`
4. **Preview renders** — `http://localhost:8080/preview` shows a PNG of the display layout
5. **Web UI loads** — `http://localhost:8080` shows the configuration interface
6. **Data sources show** — `/sources` JSON shows all five sources listed as enabled

### First Useful Output

The `/preview` endpoint is the first useful output. With fixture data loaded, the
preview shows a pixel-accurate rendering of what the e-ink display would look like:
weather conditions in the upper-left panel, river gauge in the upper-right, ferry
status in the lower-left, and the trip decision summary for the sample destinations.

### Pain Points — Docker Path

| Pain Point | Severity | Description |
|------------|----------|-------------|
| No UI indicator for fixture mode | Medium | The web UI does not show whether live or fixture data is active. A user can't tell if what they see reflects real conditions or canned test data. |
| Sample configs aren't labeled as samples | Low | `config.sample.toml` and `destinations.sample.toml` are mounted directly; the sample destinations reference Skagit Valley regardless of where the user lives. |
| No "next steps" prompt | Low | After first load, there's no prompt directing the user to customize destinations or switch to live data. The UI is functional but not onboarding-oriented. |

---

## Path 2: Raspberry Pi Production Deployment

The Pi path is the production experience. It assumes SSH access and a physical
Waveshare 7.5" e-ink panel connected via the 40-pin GPIO header.

### Prerequisites

#### Hardware

- Raspberry Pi Zero 2 W (512 MB RAM)
- Waveshare 7.5 inch e-ink display (v2, 800×480)
- MicroSD card (8 GB+), with Raspberry Pi OS Lite (64-bit) flashed
- SPI enabled: `raspi-config` → Interface Options → SPI → Enable
- SSH access configured (either password or key-based)

#### Development Machine

- Rust 1.85+ installed (`rustup` recommended)
- aarch64 cross-compilation target and toolchain:
  ```bash
  rustup target add aarch64-unknown-linux-gnu
  # On Debian/Ubuntu:
  sudo apt install gcc-aarch64-linux-gnu
  # On macOS (Homebrew):
  brew install SergioBenitez/osxct/aarch64-unknown-linux-gnu
  ```
- `rsync` and `ssh` available
- Git

### Steps

#### 1. Configure for your location

Copy the sample config files and edit for your location:

```bash
cp config.sample.toml config.toml
cp destinations.sample.toml destinations.toml
```

Edit `config.toml`:
- Set `[location] latitude`, `longitude`, and `name` for your area
- Set `[sources.river] usgs_site_id` to the USGS gauge closest to you
  (look up at [waterdata.usgs.gov](https://waterdata.usgs.gov/nwis/rt))
- Set `[sources.trail] park_code` to the NPS park code for your area
  (find at [nps.gov/findapark](https://www.nps.gov/findapark))
- Optionally set `[sources.road] routes` to the route numbers you care about

Edit `destinations.toml`:
- Replace sample destinations with your actual destinations
- Adjust `[destinations.criteria]` thresholds to match what you consider
  acceptable conditions (see criteria reference below)

#### 2. First-time Pi setup (run once)

```bash
make install-service PI_HOST=pi@<your-pi-ip>
```

This command:
- Cross-compiles the binary with the `hardware` feature enabled
- SSH-copies the binary to `/usr/local/bin/skagit-flats` on the Pi
- Installs the systemd service (`skagit-flats.service`)
- Creates a dedicated `skagit-flats` system user
- Adds the user to the `spi` and `gpio` groups
- Copies `config.toml` and `destinations.toml` (without overwriting if they exist)
- Enables and starts the service

#### 3. Verify the service is running

```bash
ssh pi@<your-pi-ip> sudo systemctl status skagit-flats
```

Expected output includes `Active: active (running)`.

#### 4. Access the web UI

```bash
open http://<your-pi-ip>:8080
```

The web UI lets you:
- View the current display preview (PNG, pixel-accurate)
- Enable and disable individual data sources
- Add, edit, and delete destinations with go/no-go criteria
- See source status (last fetch time, errors, next scheduled fetch)

#### 5. Deploy updates

After the service is installed, deploy code or config changes with:

```bash
make deploy PI_HOST=pi@<your-pi-ip>
```

### First Successful Launch Checkpoints

1. **Cross-compilation succeeds** — `cargo build --release --target aarch64-unknown-linux-gnu --features hardware` exits 0
2. **Service starts** — `systemctl status skagit-flats` shows `active (running)`
3. **Display initializes** — E-ink panel shows the initial layout (may be blank/loading for up to 30 seconds on first boot while sources complete their first fetch)
4. **Health check passes** — `curl http://<pi-ip>:8080/health` returns `OK`
5. **Web UI loads** — `http://<pi-ip>:8080` shows configuration interface
6. **Sources populate** — Within 5 minutes, all five panels show live data (not `--`)

### First Useful Output

The first useful output is the display rendering live data for your location:
temperature and wind from your nearest NOAA station, river level for your configured
USGS gauge, and a go/no-go decision for at least one destination you've configured.

The minimum viable first-useful-state is:
- Weather panel shows a real temperature and sky condition
- River panel shows a real gauge reading
- At least one destination shows a clear Go or No-Go decision

### Criteria Reference

Go/no-go criteria fields and their semantics:

| Field | Type | Meaning |
|-------|------|---------|
| `min_temp_f` | float | Minimum acceptable temperature (°F) |
| `max_temp_f` | float | Maximum acceptable temperature (°F) |
| `max_precip_chance_pct` | float | Maximum precipitation probability (0–100) |
| `max_river_level_ft` | float | Maximum river gauge height (ft); varies by site |
| `max_river_flow_cfs` | float | Maximum streamflow (cfs); varies by site |
| `road_open_required` | bool | Whether the destination's roads must be open |

A destination shows **GO** when all configured criteria are met. A destination
shows **NO GO** when one or more criteria are violated, with the violated
criteria highlighted.

**Suggested starting thresholds for Skagit Valley:**

| Destination type | min_temp_f | max_temp_f | max_precip_chance_pct | max_river_level_ft |
|------------------|-----------|-----------|----------------------|-------------------|
| Car camping | 40 | 90 | 60 | 12 |
| Backpacking | 35 | 85 | 40 | — |
| Road cycling | 45 | 95 | 30 | — |
| Paddling | 50 | 90 | 50 | 10 |

### Pain Points — Pi Path

| Pain Point | Severity | Description |
|------------|----------|-------------|
| Cross-compilation toolchain setup is opaque | High | `make install-service` assumes the cross-compiler is installed and configured. No pre-flight check or helpful error message if it isn't. Users who haven't done aarch64 cross-compilation before will fail silently or see cryptic linker errors. **Required product support:** A `make check-deps` or `scripts/setup.sh` that verifies prerequisites and prints installation instructions per platform. |
| USGS site ID lookup is off-platform | High | Users must leave the project to find their USGS gauge ID on an external map. No in-product guidance on how to find it. **Required product support:** Add a comment in `config.sample.toml` with the lookup URL and an example of how to read the site ID from the URL. Add a setup-wizard step in the web UI that accepts a location and returns nearby gauge options. |
| NPS park code lookup is off-platform | Medium | Same issue as USGS. No in-product guidance. **Required product support:** Add a comment in `config.sample.toml` with the lookup URL. |
| Blank display on first boot | Medium | Sources need time for their first fetch. The display is blank or shows stale data for up to 30 seconds (weather, river) to 15 minutes (trail conditions). There's no "initializing" or "waiting for data" placeholder. **Required product support:** Render a startup state ("Loading data…") that transitions to the live layout when sources complete their first fetch. |
| SSH key vs. password auth ambiguity | Medium | `make install-service` passes `PI_HOST` directly to rsync/ssh. If the user hasn't set up SSH key authentication, they'll be prompted for a password multiple times during install. No documentation of which approach is assumed or how to set up keys. **Required product support:** Document the SSH key setup requirement in the README; optionally add a `make setup-ssh` target. |
| No indication of live vs. fixture mode in web UI | Medium | If `SKAGIT_FIXTURE_DATA=1` is set, the UI shows fixture data with no visual indication. A user who forgets to unset this will not know their display isn't showing live conditions. **Required product support:** Add a banner or badge in the web UI when fixture mode is active. |
| Display renders blank if SPI is not enabled | Medium | If the user forgets to enable SPI on the Pi, the daemon starts but the display doesn't initialize. The error is logged but not surfaced in the web UI. **Required product support:** Add a `/health/hardware` endpoint that checks SPI availability; surface hardware errors prominently in the web UI status panel. |
| Criteria threshold UX | Low | The go/no-go criteria fields are numeric and not self-explaining. A new user doesn't know what "reasonable" thresholds look like without context. **Required product support:** Add tooltip help text next to each field in the web UI. Add the criteria reference table from this document to the web UI. |
| No quickstart guide | Low | Documentation is spread across README, AGENTS.md, and `docs/`. There is no single "Getting Started" guide that walks a new operator from zero to first useful output. **Required product support:** Add `QUICKSTART.md` with the two deployment paths summarized as numbered checklists. |

---

## Minimum Viable Setup Flow

The simplest path to first useful output is Docker with fixture data:

1. `git clone <repo> && cd skagit-flats`
2. `docker compose up`
3. Open `http://localhost:8080/preview`

This takes under 5 minutes on a machine with Docker installed and requires no
configuration changes. The preview is not personalized (it shows Skagit Valley
fixture data) but demonstrates the full layout.

The minimum path to personalized, live-data output on a Pi:

1. Clone + install cross-compilation toolchain (~15 minutes one-time)
2. Copy and edit `config.toml` with your location and USGS site ID (~10 minutes)
3. Copy and edit `destinations.toml` with one destination and rough criteria (~5 minutes)
4. `make install-service PI_HOST=pi@<ip>` (~5 minutes)
5. Visit `http://<pi-ip>:8080` and watch sources populate (~5 minutes)

Total: ~40 minutes for someone who has SSH key auth set up and knows their USGS site ID.

---

## Required Product Support — Priority Summary

The following product changes are required to make the first-run experience
tractable for non-developer operators:

| Priority | Item | Impact |
|----------|------|--------|
| P0 | Startup placeholder on e-ink display | Users see a blank display on first boot with no explanation |
| P0 | Hardware error surface in web UI | SPI failures are silent; operators have no way to diagnose |
| P1 | `make check-deps` / setup script | Cross-compilation failure is opaque; gates the entire Pi path |
| P1 | In-config comments for USGS/NPS lookup | Off-platform lookup is the #1 friction point for config |
| P1 | Fixture mode indicator in web UI | Operators can't tell if they're seeing real or fake data |
| P2 | Setup wizard for location → USGS gauge | Removes the biggest manual step in `config.toml` authoring |
| P2 | Criteria tooltip help in web UI | Thresholds are opaque without context |
| P2 | `QUICKSTART.md` | Documentation is fragmented; a single entrypoint is needed |
| P3 | SSH key setup documentation | Minor friction, but affects first-time operators |

---

## What "Done" Looks Like (Operator Perspective)

A successful installation and first-run produces:

1. **Physical display** shows live weather, river gauge, ferry status, and a
   go/no-go decision for at least one configured destination — all updating
   automatically without intervention
2. **Web UI** at `http://<pi-ip>:8080` shows the current preview, source status
   (all green, last-fetched timestamps recent), and the destination list
3. **Service survives power cycle** — after `sudo reboot`, the display returns
   to showing live data within 2 minutes
4. **A new household member** can look at the display and understand what it
   shows without explanation (data labels and Go/No-Go decisions are legible)
