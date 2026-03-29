# Destination Signal Relevance

## Problem

Not every data signal matters for every destination. A lowland bike loop needs
river flood data; an island trip needs ferry schedules. Showing all signals
for every destination produces clutter and dilutes the signal-to-noise ratio
on a small e-ink display.

## Model

Each destination declares which signals are relevant to it. This controls two
things:

1. **Display filtering** — only relevant signals appear in the planning view.
   The display omits panels whose source is not relevant to any configured destination.
2. **Evaluation scope** — go/no-go criteria are only checked for relevant signals.
   A river level threshold on a ferry-only destination is never consulted.

## Signals

| Signal | Source | When to enable |
|--------|--------|----------------|
| `weather` | NOAA/NWS | Nearly always. Temperature, precipitation, wind. |
| `river` | USGS NWIS | Lowland or valley destinations at flood risk. |
| `ferry` | WSDOT Ferries | Island or ferry-dependent destinations. |
| `trail` | NPS/USFS/WTA | Hiking or camping destinations. |
| `road` | WSDOT/USFS | Destinations with seasonal or closure-prone road access. |

## Rules

**Globally relevant:**
- Weather is relevant for every destination. There is no case where temperature
  and precipitation are meaningless for a trip decision.

**Destination-specific (default on, configure explicitly):**
- River, ferry, trail, and road signals should be declared per-destination.
- When no destinations are configured, all signals appear (full dashboard mode).
- When destinations are configured, the display shows signals that are relevant
  to at least one destination. Signals relevant to no destination are suppressed.

**Evaluation is scoped:**
- If `river = false` for a destination, river criteria (`max_river_level_ft`,
  `max_river_flow_cfs`) are not evaluated, even if thresholds are configured.
  This prevents accidental blocking from irrelevant data.

## Examples

**Lowland bike loop** (Skagit River floodplain):
- River flooding makes roads impassable → `river = true`, `road = true`
- No ferry required, no mountain trail → `ferry = false`, `trail = false`

**Mountain campsite** (Baker Lake, accessed via SR-20):
- SR-20 closes seasonally → `road = true`
- Trail and campsite conditions affect suitability → `trail = true`
- River gauge at Mount Vernon is irrelevant → `river = false`
- No ferry → `ferry = false`

**Island destination** (Guemes Island):
- Ferry schedule is the primary access constraint → `ferry = true`
- No river flooding concern → `river = false`
- No mountain roads → `road = false`

## Configuration

Signals are declared in `destinations.toml` under `[destinations.signals]`.
All signals default to `true` if the section is omitted, preserving backward
compatibility for existing configurations.

```toml
[[destinations]]
name = "Baker Lake"

[destinations.signals]
weather = true
river = false
ferry = false
trail = true
road = true

[destinations.criteria]
min_temp_f = 40.0
road_open_required = true
```

See `destinations.sample.toml` for annotated examples covering lowland, mountain,
and island destination types.

## Display behavior

The e-ink display has a fixed four-zone layout:
- **Hero**: weather + go/no-go decision (weather always shown)
- **Data**: river (left) + ferry (right)
- **Context**: trail (left) + road (right)

Data and context zone slots are populated only if the signal is relevant to at
least one configured destination. A river slot with no relevant destination is
left empty; the renderer falls back to the next available content or leaves the
zone blank.

When no destinations are configured, all slots are filled from available source
data (full dashboard mode, useful for general ambient display without trip planning).
