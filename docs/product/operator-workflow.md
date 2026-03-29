# Skagit Flats — Ongoing Operator Workflow

This document describes how to operate Skagit Flats after initial installation.
It covers routine maintenance, source health monitoring, destination management,
and how to interpret and trust the planning output over time.

The goal is to make the system operable without log-diving or implementation
knowledge. Everything described here is achievable through the web interface or
straightforward config file edits.

---

## Mental Model

Skagit Flats does three things on an ongoing basis:

1. **Fetches** data from public APIs on independent schedules
2. **Evaluates** each configured destination against its go/no-go criteria
3. **Renders** the result to the e-ink display and web preview

Your ongoing role is to keep the inputs accurate (source configuration, destination
criteria) and recognize when the output should be trusted or questioned.

---

## The Web Interface

Open `http://<pi-hostname>:8080` from any device on your local network.

The web interface is the primary operator surface. It provides:

- **Live preview** — the same pixel buffer the e-ink display shows, updated in
  real time as data arrives
- **Source health dashboard** — last-fetched time, success/failure status, and
  most recent value for each source
- **Destination editor** — add, remove, and modify destinations and their
  go/no-go criteria
- **Configuration overview** — read-only view of active settings

You do not need SSH or log access for routine operation.

---

## Reviewing Destination Settings

### What destinations are

A destination is a named location (a trail, a campsite, a day-trip route) with
a set of go/no-go criteria: temperature range, river level limit, precipitation
threshold, road access requirement. The system evaluates each destination against
current source data and renders a GO or NO GO decision.

### Checking current destinations

In the web interface, go to **Destinations**. Each destination shows:

- Its name
- Its current decision (GO / NO GO)
- Which criteria are met or failed
- The source data values driving the evaluation

This answers "why did it say NO GO?" without reading any logs.

### Adding or modifying a destination

Edit `destinations.toml` directly or use the **Destinations** editor in the web
interface. Changes to destinations take effect immediately (the daemon watches
this file and reloads without restart).

**When to review destinations:**
- Before a trip — confirm the criteria still match your actual thresholds
- After a season change — temperature bounds that work in summer may not fit
  early spring
- After adding a new source — if you add road closure monitoring for a new route,
  add the corresponding destination

### Destination criteria reference

| Criterion | What it means | Example |
|-----------|---------------|---------|
| `min_temp_f` | Minimum acceptable temperature | `45.0` |
| `max_temp_f` | Maximum acceptable temperature | `85.0` |
| `max_precip_chance_pct` | Maximum tolerable precipitation probability | `40.0` |
| `max_river_level_ft` | River gauge level above which NO GO triggers | `12.0` |
| `max_river_flow_cfs` | Streamflow limit | `15000.0` |
| `road_open_required` | If true, any road closure on configured routes triggers NO GO | `true` |

Missing criteria are not evaluated — a destination with only `max_river_level_ft`
set ignores temperature entirely.

### Calibrating criteria over time

The criteria are your thresholds, not facts. If the system says GO but the
destination was actually miserable, tighten the relevant criterion. If it
repeatedly says NO GO for trips that turned out fine, loosen it.

The display's NO GO reasons panel shows exactly which criterion was violated,
making it straightforward to identify which value to adjust.

---

## Monitoring Source Freshness and Failures

### The source health dashboard

Go to **Sources** in the web interface. Each source shows:

| Field | Meaning |
|-------|---------|
| **Last fetched** | When the source last successfully retrieved data |
| **Status** | OK / Failing / Stale |
| **Value** | The most recent data point |
| **Next refresh** | When the next fetch is scheduled |

**OK** — the source is fetching successfully and data is fresh.

**Stale** — the source fetched successfully, but the last successful fetch was
longer ago than expected. This usually means network intermittency or an API
outage that has persisted beyond one retry cycle.

**Failing** — the source has been returning errors for multiple consecutive
fetch attempts. The last successful data is shown, with its age.

### What stale data means for the display

Skagit Flats shows stale data rather than blanking the panel. The header
timestamp ("Updated HH:MM") reflects the time of the last successful render.
If a source is stale, its panel values are from the last good fetch, and the
source health dashboard will show the age.

A panel displaying stale data is still useful — river levels and ferry schedules
change slowly enough that several-hour-old data is often still actionable —
but you should know whether to trust it.

### Handling a failing source

**Step 1: Check the web interface.** The source status shows the failure reason:
network timeout, HTTP error code, unexpected API response format. Most failures
are temporary (API down, DNS issue, network blip) and resolve without action.

**Step 2: Verify the external API manually.** Visit the API endpoint in a browser
(URLs are in the architecture overview). If the API itself is returning errors,
the problem is upstream and will resolve when the provider fixes it.

**Step 3: Check the source configuration.** A source that worked previously but
is now failing may have an invalid site ID, route number, or API key. Open
`config.toml` and verify the relevant settings (e.g., `usgs_site_id`,
`sources.road.routes`, API keys via environment variables).

**Step 4: Restart the daemon.** `sudo systemctl restart skagit-flats`. This
re-establishes connections and triggers an immediate fetch for all sources.

**Step 5: Check system logs.** If the above steps don't resolve it:
```sh
journalctl -u skagit-flats -n 50
```
Look for repeated error messages from the failing source. These will include the
error type and (where available) the HTTP response body.

### Sources that require credentials

The road closure (WSDOT) and trail (NPS) sources require free API keys. If
these sources are failing with 401 or 403 errors:
- Verify the key is set in the environment (`WSDOT_ACCESS_CODE`, `NPS_API_KEY`)
  or uncommented in `config.toml`
- Keys are not rotated automatically; if a key expires, register a new one at
  the provider's developer portal

---

## Understanding Why Recommendations Changed

### The two things that change a GO/NO-GO decision

1. **Source data changed** — the river came up, the road closed, temperature dropped
2. **Destination criteria changed** — you edited `destinations.toml`

The web interface's Destinations view shows the current evaluation with the
values that drove it. To understand a change, compare the current displayed
values against your criteria:

```
NO GO — Baker Lake
  ■ River too high: 11.9 ft (limit: 10.0 ft)
  ■ SR-20 closed at Newhalem
```

This tells you exactly what changed. You can cross-check against the river
sparkline (24-hour trend) on the display or the source health dashboard to
confirm the data is fresh and from the expected source.

### If the decision seems wrong

**Check data freshness first.** A GO decision during active flooding means either
the criteria are wrong, or the river source is stale and still showing last week's
level. The source health dashboard distinguishes these cases.

**Verify the criteria.** Open `destinations.toml` (or the Destinations editor)
and confirm the thresholds match your intent. A `max_river_level_ft` set too high
will produce GO when the river is actually high.

**Verify source configuration.** Confirm the source is monitoring the correct
site (e.g., the correct USGS gauge, the correct ferry route, the correct road
segments).

---

## Maintaining Confidence in the Planning Output

### Trust signals

You can trust the display when:
- The source health dashboard shows all sources OK
- The "Updated" timestamp in the display header is recent (within the configured
  refresh interval)
- The displayed values match what you can independently verify (e.g., river level
  at waterdata.usgs.gov)

### Signals to investigate

Investigate when:
- The source dashboard shows Failing or Stale for any source driving a destination
  you care about
- The "Updated" timestamp is hours old
- A GO/NO-GO decision flipped unexpectedly (confirm it's real data, not a stale cache)
- A source was previously working and stopped without an obvious external cause

### Routine health check (weekly, ~2 minutes)

Open the web interface and glance at the Sources view:

1. Are all sources showing OK status?
2. Are last-fetched times recent (within a few multiples of the source's interval)?
3. Do the displayed values look plausible for current conditions?

If yes to all three, no action needed.

### Before a trip (day before)

1. Open the web interface
2. Confirm the relevant destination shows GO or NO GO with the expected reasoning
3. Verify source health for each data type driving that destination (weather, river, road)
4. Spot-check one or two values against public sources if conditions are borderline

If sources are stale, restart the daemon and wait one full refresh cycle before
relying on the output.

### After a long network outage

Power cycles and extended outages (router down, ISP issue) will leave all sources
in a stale or failing state. On recovery:

1. The daemon reconnects automatically on the next fetch cycle
2. If recovery takes more than a few minutes, restart the daemon:
   `sudo systemctl restart skagit-flats`
3. Verify the source health dashboard shows OK before relying on planning output

The daemon is designed to restart cleanly from `systemd` on power cycle. If the
display is blank or showing stale data after a reboot, verify the service started:
`sudo systemctl status skagit-flats`

---

## Configuration Reference

### config.toml (hardware and source settings)

Edit this file by hand. Requires a daemon restart to take effect.

Key settings operators adjust:

| Setting | When to change |
|---------|---------------|
| `sources.weather_interval_secs` | If NOAA rate-limits you or you want more frequent updates |
| `sources.river.usgs_site_id` | If you want to monitor a different gauge |
| `sources.road.routes` | If monitoring new or different roads |
| `location.latitude/longitude` | If you relocate or want a different weather station |

### destinations.toml (trip planning)

Edited by the web UI or by hand. Reloads without restart.

Keep this file as the source of truth for your trip criteria. The web interface
writes back to this file, so manual edits and UI edits are equivalent.

---

## Summary: When to Act

| Symptom | Likely Cause | Action |
|---------|-------------|--------|
| Source shows Failing | API down or misconfigured key | Wait (API outage) or fix config |
| Source shows Stale | Network intermittency | Restart daemon; check network |
| Display not updating | Service stopped or display driver issue | `systemctl status skagit-flats` |
| GO when conditions seem bad | Stale data or wrong criteria | Check source freshness; verify thresholds |
| NO GO when conditions seem fine | Criteria too conservative | Adjust `destinations.toml` |
| Decision flipped unexpectedly | Real data change or stale cache flipping | Cross-check source values |
