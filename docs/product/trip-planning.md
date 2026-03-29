# Trip Planning Workflow — Product Definition

## Summary

This document defines the first-version trip planning product model for Skagit Flats.
It supersedes treating the display as a passive status dashboard and defines the
workflow, information model, recommendation states, and UI boundary needed to
actually help a user decide whether to make a trip.

Scope: saved destinations only. Arbitrary one-off trips are deferred.

---

## Baseline Assessment: Current Display

The current display is a **status dashboard**, not a trip planner. It shows correct
data well — improved readability, meaningful hierarchy, good layout — but it stops
short of answering the user's actual question.

### What the current display does well

- Shows primary decision state (GO / NO GO) at hero size
- Lists NO GO blockers inline under the decision text
- Shows temperature, weather, river level, ferry departures, trail/road status
- Renders data at legible scale for viewing from across the room

### What is missing for trip planning

| Gap | Why it matters |
|-----|---------------|
| **No destination selection** | The display shows one fixed destination's evaluation; users can't switch to a different saved destination without editing config |
| **Binary decision only** | GO or NO GO — no CAUTION (marginal conditions) or UNKNOWN (missing data) |
| **Missing data treated as passing** | If weather data is unavailable, the system defaults to GO — this is wrong and misleading |
| **No signal freshness** | A 6-hour-old river reading is shown identically to a fresh one — the user has no way to judge if the data is stale |
| **No source relevance rules** | Baker Lake doesn't need ferry departure times; Skagit Flats Loop doesn't need mountain road status — but the display shows everything regardless |
| **No rationale distinction** | The current NO GO bullet list mixes hard blockers ("road closed") with soft factors ("slightly high precip") with no distinction |
| **No planning horizon** | The display shows "now" only; "this weekend" is not surfaced at all |

---

## Trip Planning Workflow (v1)

### Actor

A household member who wants to decide whether a planned outing should happen.
They are not sitting at a keyboard. They are standing across the room, or glancing
at the display while doing something else.

### Primary flow

1. **Select destination** — user picks a saved destination (default: last viewed, or most recently configured)
2. **System assembles relevant signals** — only the signals configured as relevant for this destination are evaluated
3. **System produces a recommendation** — GO, CAUTION, NO GO, or UNKNOWN
4. **Display shows the recommendation + rationale** — hero-size decision, supporting data, any blockers, freshness of inputs

### Selection mechanism

- **Display**: a destination name in the header indicates the active destination; navigation controls (buttons or web UI) cycle through saved destinations
- **Web UI**: destination selector with instant preview update
- **Default**: the first destination in `destinations.toml` is shown on startup

---

## Recommendation Model

The recommendation is one of four states:

### States

| State | Meaning | Display rendering |
|-------|---------|------------------|
| **GO** | All configured criteria pass; all required signals fresh | "GO" at hero size |
| **CAUTION** | No hard blockers; one or more signals in marginal range OR one relevant signal stale | "CAUTION" at hero size; marginal factors listed |
| **NO GO** | At least one hard criterion violated (road closed, river too high, etc.) | "NO GO" at hero size; blockers listed |
| **UNKNOWN** | Required signals are missing or too stale to evaluate | "?" at hero size; missing/stale signals listed |

### State resolution logic

Evaluate in this order:
1. If any **required** signal is missing or older than its staleness threshold → **UNKNOWN**
2. If any configured criterion is a **hard blocker** (road_open_required, max_river_level_ft exceeded, etc.) → **NO GO**
3. If any configured criterion is in a **caution range** (approaching threshold, marginal temp, moderate precip chance) → **CAUTION**
4. Otherwise → **GO**

UNKNOWN takes precedence over NO GO. If we don't have the data we can't even rule
out the trip — showing NO GO when we haven't heard from the river gauge is incorrect.

### Caution thresholds

Caution is triggered when a signal is within a configurable margin of its hard limit.
Example: if max_river_level_ft = 12.0 and caution_margin_pct = 20%, then
river level between 9.6 ft and 12.0 ft triggers CAUTION.

The margin is per-destination and has a sensible default (20%).

### Hard blockers vs. soft factors

In the NO GO and CAUTION states, the rationale display distinguishes:

- **Hard blockers** (rendered with filled square bullet, bold): Road closed, river above max
- **Soft factors** (rendered with outline bullet, normal weight): Temperature marginal, high precip chance

This distinction helps the user understand whether the trip is impossible vs. just
suboptimal.

---

## Information Model

### Per-destination configuration

Each saved destination requires:

| Field | Type | Purpose |
|-------|------|---------|
| `name` | string | Display label |
| `relevant_signals` | list | Which source signals are evaluated (e.g., `["weather", "river", "road"]`) |
| `criteria` | TripCriteria | Hard thresholds per signal |
| `caution_margin_pct` | float (default 20%) | How close to threshold triggers CAUTION |
| `signal_staleness_thresholds` | per-signal durations | How old is "too old" for each source |
| `planning_horizon` | enum (today, weekend) | v1: today only; weekend deferred |

`relevant_signals` is the key addition: Baker Lake doesn't evaluate ferry departures;
Skagit Flats Loop doesn't need SR-20 mountain pass status. Each destination declares
what it cares about. Only declared signals can block or warn for that destination.

### Data freshness

Each domain value gains a `fetched_at` timestamp. The evaluation layer compares
`fetched_at` to `now` using per-signal staleness thresholds.

Default staleness thresholds:

| Signal | Staleness threshold |
|--------|-------------------|
| Weather | 90 minutes |
| River gauge | 90 minutes |
| Ferry | 15 minutes |
| Trail conditions | 24 hours |
| Road status | 4 hours |

If a relevant signal exceeds its staleness threshold, the recommendation is UNKNOWN.
The display shows which signal(s) are stale, and when they were last updated.

### Rationale display

Every recommendation state (including GO) should surface:

- **Decision** — the state, at hero size
- **Rationale summary** — one-line explanation of why (e.g., "All conditions nominal" or "River at 11.4 ft — below 12.0 ft limit")
- **Blockers** (NO GO only) — the specific criteria violated
- **Caution factors** (CAUTION only) — the marginal signals
- **Unknown signals** (UNKNOWN only) — which signals are missing/stale and why
- **Data freshness** — "as of HH:MM" per signal, or a summary "all sources current"

---

## Display vs. Control UI Boundary

### E-ink display (passive, always-on)

**Shows:**
- Active destination name (in header)
- Recommendation state (hero: GO / CAUTION / NO GO / ?)
- Rationale / blockers (below hero)
- Supporting signal values (data zone: temperature, river level, departure time)
- Signal freshness indicators (context zone or header)
- Trail/road status for the active destination

**Does not show:**
- Destination selector (controlled via web UI or physical button)
- Criteria configuration
- Source status
- Historical trends beyond the 24h sparkline

The display is read-only output. All inputs happen via the web interface.

### Web UI (interactive, local network)

**Planning view:**
- Destination selector (cycle through saved destinations)
- Live preview of recommendation for selected destination
- Per-signal freshness status
- Recommendation state with full rationale (not truncated to display size)

**Configuration view:**
- Manage saved destinations (add, edit, remove)
- Set per-destination criteria (thresholds, relevant signals, staleness limits)
- Source management (enable/disable sources, refresh intervals)
- E-ink preview (current pixel buffer, exact match to physical display)

---

## v1 Planning Horizon

**In scope for v1:** current conditions only ("should I go today?").

The display and recommendation evaluate current sensor readings against current
criteria. The question answered is: "Given what the world looks like right now,
is this trip viable?"

**Deferred:** "This weekend" planning horizon. This requires forecast data beyond
current observation, which is an explicit non-goal of the system. It can be
revisited when/if forecast integration is scoped.

The `planning_horizon` field in destination config reserves the space. For v1 it
always defaults to `today` and the weekend option is not rendered.

---

## Open Product Questions (Resolved for v1)

| Question | v1 Decision |
|----------|------------|
| First planning horizon | Today/current only. Weekend deferred. |
| CAUTION state in v1? | Yes. Binary GO/NO GO loses important nuance. |
| Which signals are global vs. destination-specific? | All signals are destination-specific via `relevant_signals`. No global signals in v1. |
| Minimum trust explanation? | Recommendation + rationale summary + blockers + freshness. See above. |

---

## Current Code Gaps (Implementation Targets)

This section identifies the specific code changes needed to realize this design.
Each gap is a candidate for a follow-on implementation bead.

| Gap | Current state | Target state | Bead |
|-----|--------------|-------------|------|
| Recommendation states | `TripDecision::Go` / `NoGo { reasons }` | Add `Caution { factors }` and `Unknown { missing }` | sf-*: domain model |
| Freshness tracking | None — `DomainState` has no timestamps | Add `fetched_at: SystemTime` to each domain value wrapper | sf-*: domain freshness |
| UNKNOWN on missing data | Missing data → GO (silently passes) | Missing required signals → UNKNOWN | sf-*: evaluation |
| Destination relevance | All signals evaluated for every destination | `relevant_signals` list per destination | sf-*: config + evaluation |
| Caution thresholds | No marginal range — threshold is hard cutoff | `caution_margin_pct` per destination, evaluated in `evaluation::evaluate` | sf-*: caution model |
| Destination selection | One implicit destination; no switching | Active destination in SharedState; web UI + display header | sf-*: destination selection |
| Rationale display | NO GO bullets only | All states get rationale; hard/soft distinction in rendering | sf-*: rationale rendering |
| Freshness display | No freshness shown | Per-signal "as of HH:MM" in context zone | sf-*: freshness display |
| Display header | Shows app name + site name | Shows active destination name | sf-*: display header |

---

## Follow-on Bead Targets

These are the implementation beads that should be created from this definition:

1. **Extend TripDecision to CAUTION and UNKNOWN** — add new variants to the enum, update the evaluation module, update presentation to render all four states
2. **Add data freshness to DomainState** — wrap each optional value with a timestamp; add staleness evaluation to the evaluation layer
3. **Add destination-relevance rules** — add `relevant_signals` to DestinationConfig; skip irrelevant signals in evaluation
4. **Add caution margin configuration** — add `caution_margin_pct` per destination; implement marginal-range detection in evaluation
5. **Implement destination selection** — add active destination to SharedState; add navigation to web UI; update display header
6. **Update display for CAUTION and UNKNOWN states** — add CAUTION hero rendering (distinct from GO/NO GO); add UNKNOWN "?" state with missing-signal list
7. **Add rationale distinction in rendering** — distinguish hard blockers from soft factors in the NO GO and CAUTION bullet list
8. **Add per-signal freshness display** — add "as of HH:MM" per signal to context zone or data zone

These beads are ordered by dependency: beads 1–4 are domain/evaluation work and
can be done roughly in parallel; beads 5–8 are presentation/render work and depend
on the domain changes in beads 1–4.
