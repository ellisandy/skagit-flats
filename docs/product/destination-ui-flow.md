# Destination Trip Planning — Control UI Flow

**Bead:** sf-b30
**Status:** Design spec

---

## Problem with the Current UI

The existing destinations table exposes raw threshold fields (Min Temp, Max Temp,
Max Precip %, Max River ft, Road Required) as direct form inputs. This is functional
but has two problems:

1. **No context.** A number like "12.0 ft" is meaningless without knowing the current
   river level (11.9 ft) or what 12.0 ft means for trail access.

2. **No flow.** The UI does not help a user answer the question they actually have:
   *"Should I go to Baker Lake this weekend?"* It only helps them answer: *"What
   thresholds have I configured?"*

The redesigned flow centers on the decision, not the thresholds.

---

## Decision States

The current `TripDecision` enum has two states: `Go` and `NoGo`. The control UI
requires four states to be useful:

| State | Meaning | When shown |
|-------|---------|------------|
| **GO** | All criteria met; all required data available | Criteria satisfied and data present |
| **CAUTION** | Criteria technically met, but one or more readings are within a warning margin of a limit | Any value within 15% of its configured limit |
| **NO GO** | One or more criteria exceeded | Any configured threshold exceeded |
| **UNKNOWN** | A required criterion cannot be evaluated (missing data) | Data source offline or not yet fetched |

### Caution Margin

A reading triggers CAUTION if it falls within 15% of the configured limit, but has
not yet exceeded it. Examples:

- River limit: 12.0 ft. River at 10.5 ft → 87.5% of limit → CAUTION.
- Min temp: 45°F. Current temp: 47°F → within 2°F of limit → CAUTION.

The 15% margin is a design choice. It should be configurable per criterion in a
future iteration, but the initial implementation uses a single global margin.

### UNKNOWN vs GO with Missing Data

The current evaluation returns `Go` when data is missing (e.g., no river reading
when a river-level criterion is configured). This is generous but misleading — the
user hasn't gotten a green light, they've gotten an "I have no idea."

For the redesigned UI:
- If a criterion requires data that is absent → the decision becomes `UNKNOWN`.
- If no criteria require the missing data → the decision is unaffected.

This requires changing the evaluation logic. The domain type change is specified
in the [Implementation Notes](#implementation-notes) section below.

---

## UI Flow

### View 1 — Destination List

Entry point: the main dashboard page, "Destinations" section.

```
┌────────────────────────────────────────────────────────────┐
│  TRIP PLANNER                                              │
├────────────────────────────────────────────────────────────┤
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Baker Lake                          [  GO  ]        │  │
│  │  River 9.4 ft · 62°F · SR-20 open   ▶              │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Cascade Pass                        [ NO GO ]       │  │
│  │  River 11.9 ft exceeds 10.0 ft limit ▶              │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Diablo Lake                         [ CAUTION ]     │  │
│  │  River near limit (10.5 ft / 12 ft)  ▶              │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Thornton Lakes                      [ UNKNOWN ]     │  │
│  │  Road data unavailable               ▶              │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  [ + Add destination ]                                     │
└────────────────────────────────────────────────────────────┘
```

**Card content:**

- **Name** — left-aligned, prominent
- **Decision badge** — right-aligned; color-coded (green / amber / red / grey)
- **One-line summary** — the most important signal for the decision:
  - GO: key passing conditions ("River 9.4 ft · 62°F · SR-20 open")
  - NO GO: first blocking reason in plain language
  - CAUTION: the closest-to-limit reading
  - UNKNOWN: which data source is missing

Clicking anywhere on a card navigates to the Destination Detail view.

**Sort order:** NO GO first, then UNKNOWN, then CAUTION, then GO. Within each
group, alphabetical. (Worst-case destinations surface before the user has to scroll.)

---

### View 2 — Destination Detail

Accessed by clicking a destination card. URL: `/destinations/:name`

```
┌────────────────────────────────────────────────────────────┐
│  ← Destinations       BAKER LAKE                          │
├────────────────────────────────────────────────────────────┤
│                                                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                                                      │  │
│  │                    GO                                │  │
│  │                                                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  WHY THIS DECISION                                         │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  ✓  Temperature      62°F      (min 45°F)            │  │
│  │  ✓  Precipitation    15%       (max 40%)             │  │
│  │  ✓  River level      9.4 ft    (max 12.0 ft)         │  │
│  │  ✓  Road             SR-20 open                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  CONDITIONS AT A GLANCE                                    │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Weather   62°F · Partly Cloudy · SW 8 mph           │  │
│  │  River     9.4 ft · 4,200 cfs · Stable               │  │
│  │  Road      SR-20 open                                │  │
│  │  Updated   14:32                                     │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  [ Edit trip criteria ]      [ Delete destination ]        │
└────────────────────────────────────────────────────────────┘
```

**Decision banner:**

- Large, full-width. Text and background color match the state:
  - GO → dark green background, white "GO"
  - CAUTION → amber background, dark "CAUTION"
  - NO GO → dark red background, white "NO GO"
  - UNKNOWN → grey background, dark "UNKNOWN"

**Why This Decision:**

Each configured criterion is listed as a row:
- Check icon (✓) or block icon (✗) or warning icon (!) or unknown icon (?)
- Criterion name in plain language (not field names)
- Current reading alongside the configured limit
- For NO GO rows: text is red
- For CAUTION rows: text is amber
- For UNKNOWN rows: show "— data unavailable" instead of a reading

If a data type has no configured criterion, it does not appear here. This section
shows only what the user has said they care about.

**Conditions at a Glance:**

Full current readings from each active source, independent of criteria. This is
informational — it lets the user see conditions they didn't set a limit for. Only
data sources with live readings are shown; omit sources with no data.

**Buttons:**

- "Edit trip criteria" → navigates to Edit Criteria view
- "Delete destination" → confirmation dialog, then returns to list

---

### View 3 — Edit Trip Criteria

Accessed from "Edit trip criteria" on the detail view. URL: `/destinations/:name/edit`

```
┌────────────────────────────────────────────────────────────┐
│  ← Baker Lake        EDIT TRIP CRITERIA                   │
├────────────────────────────────────────────────────────────┤
│                                                            │
│  WEATHER                                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Don't go if temperature below  [ 45 ] °F            │  │
│  │  Current: 62°F  ✓ passing                            │  │
│  │                                                      │  │
│  │  Don't go if temperature above  [    ] °F            │  │
│  │  (not set — click to add)                            │  │
│  │                                                      │  │
│  │  Don't go if chance of rain above  [ 40 ] %          │  │
│  │  Current: 15%  ✓ passing                             │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  RIVER                                                     │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Don't go if river above  [ 12.0 ] ft                │  │
│  │  Current: 9.4 ft  ✓ passing (78% of limit)           │  │
│  │                                                      │  │
│  │  Don't go if flow above  [      ] cfs                │  │
│  │  (not set — click to add)                            │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  ROAD ACCESS                                               │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  [✓] Require road to be open                         │  │
│  │  Current: SR-20 open  ✓ passing                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  PREVIEW WITH THESE SETTINGS                               │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Today's conditions would be:   GO                   │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                            │
│  [ Save ]          [ Cancel ]                              │
└────────────────────────────────────────────────────────────┘
```

**Design principles:**

1. **Plain-language labels.** Each input is phrased as a complete sentence:
   "Don't go if temperature below __ °F" not "Min Temp (F)". The threshold is
   part of the sentence, not a standalone field.

2. **Live current reading alongside the input.** Every criterion shows the current
   value and whether it's passing or failing *right now*. This lets the user
   understand what their threshold means in practice.

3. **Progress toward limit for borderline values.** For numeric thresholds, show
   the percentage of the limit consumed: "9.4 ft (78% of 12.0 ft limit)". This
   is the signal that drives CAUTION on the detail view.

4. **Unset criteria are not clutter.** Criteria that are not configured appear as
   a single grayed-out "not set — click to add" affordance. They don't take up
   form space until the user decides to set them.

5. **Instant impact preview.** Below the form, a live preview shows what the
   decision would be with the *current input values* (not yet saved). This
   updates as the user types. No need to save to see the effect.

6. **Save applies to destinations.toml.** There is no "apply without saving" —
   the preview uses the form state, but the decision on the display only changes
   on Save.

---

### View 4 — Add Destination

Accessed from "+ Add destination" on the list view. URL: `/destinations/new`

Same layout as Edit Trip Criteria, but:
- Top bar shows "NEW DESTINATION" instead of "EDIT TRIP CRITERIA"
- A name field appears at the top
- All criteria start as unset
- Preview shows "No criteria set — destination will always be GO"
- Save → adds to `destinations.toml` and returns to list

---

## Decision Badge Color Coding

| State | Background | Text | Hex |
|-------|-----------|------|-----|
| GO | #2d7d2d | #ffffff | — |
| CAUTION | #c8860a | #000000 | — |
| NO GO | #c0392b | #ffffff | — |
| UNKNOWN | #888888 | #ffffff | — |

Use these consistently across all views: list cards, detail banner, preview
widget.

---

## Signal Display — Plain-Language Mapping

The raw field names from `TripCriteria` must be mapped to user-facing labels:

| Field | Display label | Unit | Format |
|-------|-------------|------|--------|
| `min_temp_f` | "temperature below" | °F | integer |
| `max_temp_f` | "temperature above" | °F | integer |
| `max_precip_chance_pct` | "chance of rain above" | % | integer |
| `max_river_level_ft` | "river above" | ft | 1 decimal |
| `max_river_flow_cfs` | "river flow above" | cfs | comma-separated integer |
| `road_open_required` | "road must be open" | — | checkbox |

The sentence structure is: "Don't go if [label] [value] [unit]."

---

## What the Evaluation Layer Needs to Change

To support CAUTION and UNKNOWN states, `TripDecision` must gain two new variants:

```rust
pub enum TripDecision {
    Go,
    Caution { warnings: Vec<String> },  // new
    NoGo { reasons: Vec<String> },
    Unknown { missing: Vec<String> },   // new
}
```

Priority when combining multiple criteria results:
1. Any criterion in NO GO state → whole decision is `NoGo` (list all blocking reasons)
2. Any required criterion with missing data → whole decision is `Unknown` (list missing sources)
3. Any criterion in CAUTION range → whole decision is `Caution` (list borderline readings)
4. All criteria passing and data present → `Go`

The `evaluate()` function in `src/evaluation/mod.rs` needs updating to implement
this logic, with a CAUTION_MARGIN constant (default 0.15 = 15%).

The web API response for `GET /destinations` already serializes `TripDecision`
via `serde(tag = "decision")`, so the JSON response will automatically reflect
new variants:

```json
{ "decision": "Caution", "warnings": ["River at 10.5 ft — 88% of 12.0 ft limit"] }
{ "decision": "Unknown", "missing": ["road data unavailable"] }
```

---

## Routing Changes Required

The current web server has no destination-detail route. New routes needed:

| Route | Method | Description |
|-------|--------|-------------|
| `GET /destinations/:name` | — | Destination detail view (HTML) |
| `GET /destinations/:name/edit` | — | Edit criteria form (HTML) |
| `GET /destinations/new` | — | Add destination form (HTML) |
| `PUT /destinations/:name` | JSON | Update existing destination criteria |

The existing `POST /destinations` handles both create and update; splitting into
`POST /destinations` (create) and `PUT /destinations/:name` (update) is cleaner
but can be done as a follow-up. The current handler is sufficient for the MVP.

---

## E-Ink Display Integration

The redesigned four-state decision directly maps to the e-ink hero panel:

| Decision | E-ink display |
|----------|--------------|
| GO | "GO" at 96px |
| CAUTION | "CAUTION" at 96px (or slightly smaller if it doesn't fit) |
| NO GO | "NO GO" at 96px + reasons at 18px |
| UNKNOWN | "?" at 96px + missing source name at 18px |

CAUTION and UNKNOWN need updated rendering logic in `src/render/` and
`src/presentation/`. These are tracked as implementation follow-ons from this
design (separate beads).

---

## Out of Scope for This Design

- Forecast-based decisions (the system works with current readings only)
- Per-destination caution margins (global 15% margin is sufficient for v1)
- Multiple concurrent active destinations on the e-ink display (deferred — see
  DISPLAY_DESIGN.md open question 2)
- Trail condition criteria (no structured API yet; suitability text is display-only)
- Notification or alerting when a destination flips from GO to NO GO
