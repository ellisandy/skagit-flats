# Planning View — Summary, Rationale, Blockers, and Freshness

**Bead:** sf-a25
**Status:** Design spec

---

## Purpose

The planning view is the primary answer to "should I go?" for a saved destination.
It must do two things at once:

1. **Communicate the decision immediately.** The user should see GO/CAUTION/NO GO/UNKNOWN
   without reading anything.

2. **Explain the decision completely.** Every criterion, every source reading, and every
   data age indicator must be reachable without navigating away.

These two goals require a deliberate split: a **decision summary** (hero) and
**supporting detail** (collapsible or secondary).

---

## Two Rendering Surfaces

The planning view is rendered in two contexts:

| Surface | Location | Purpose |
|---------|----------|---------|
| **E-ink hero zone** | y=30, h=202 in 800×480 display | Glanceable decision at 5 m |
| **Web destination detail** | `/destinations/:name` | Full rationale + freshness |

Both surfaces must reflect the same decision and the same reasons. The web view
adds per-criterion context and per-source freshness that the e-ink display cannot
fit.

---

## Data Model: PlanningView

Implementation must build a `PlanningView` struct from the evaluation output before
either rendering surface consumes it. This is the single source of truth for both
the e-ink layout and the web page.

```rust
/// Everything needed to render either the e-ink or web planning view.
pub struct PlanningView {
    /// Name of the destination.
    pub destination: String,
    /// The go/no-go decision and its immediate cause.
    pub summary: DecisionSummary,
    /// Per-criterion rationale rows (only for configured criteria).
    pub rationale: Vec<CriterionRow>,
    /// Raw source readings, independent of criteria.
    pub conditions: Vec<SourceReading>,
    /// Per-source freshness, independent of whether a criterion is configured.
    pub freshness: Vec<SourceFreshness>,
}

/// The hero-level summary: decision + single most important line.
pub struct DecisionSummary {
    pub state: TripDecision,
    /// The single line shown alongside the decision badge.
    /// - Go: the most constrained passing value ("River 9.4 ft · SR-20 open")
    /// - Caution: the reading closest to its limit ("River near limit (10.5 / 12 ft)")
    /// - NoGo: the first blocking reason ("River 14.5 ft exceeds 12.0 ft limit")
    /// - Unknown: the first missing source ("No road data")
    pub headline: String,
}

/// One row in the "Why This Decision" section.
pub struct CriterionRow {
    pub label: String,           // plain language ("temperature below")
    pub current_value: Option<String>, // formatted reading ("62°F"), None if missing
    pub limit: String,           // configured threshold ("min 45°F")
    pub status: CriterionStatus,
}

pub enum CriterionStatus {
    Passing,          // ✓ green
    NearMiss,         // ! amber (within caution margin)
    Blocked,          // ✗ red (threshold exceeded)
    DataMissing,      // ? grey (source absent or stale)
}

/// One row in "Conditions at a Glance" — raw reading, no criterion context.
pub struct SourceReading {
    pub source: SourceKind,
    pub summary: String,     // "62°F · Partly Cloudy · SW 8 mph"
}

/// Per-source freshness metadata.
pub struct SourceFreshness {
    pub source: SourceKind,
    pub age_label: String,      // "14:32" or "2h 15m ago"
    pub freshness: FreshnessLevel,
}

pub enum FreshnessLevel {
    Fresh,           // < 50% of staleness threshold
    Aging,           // 50–75% of threshold (normal — no indicator)
    Stale,           // > 75% of threshold — show amber indicator
    Expired,         // > 100% of threshold — drives UNKNOWN; show red indicator
}

pub enum SourceKind {
    Weather,
    River,
    Ferry,
    Trail,
    Road,
}
```

---

## E-Ink Hero Zone

The hero zone (y=30, h=202) renders the **decision summary only**.

### Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  Baker Lake                                                                   │
│                                                                               │
│              NO GO                                                            │
│                                                                               │
│  • River 14.5 ft exceeds 12.0 ft limit                                       │
│  • SR-20 is closed                                                            │
│                                                                               │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Left column (≈ 400 px wide):**
- Destination name: Small tier (18px), top-left
- Decision text: Hero tier (96px), vertically centered

**Right column (≈ 380 px wide):**
- Reason bullets at Small tier (18px), one per blocking reason or missing source
- For GO: key conditions at Small tier ("River 9.4 ft · 62°F · SR-20 open")
- For CAUTION: warning lines at Small tier
- Maximum 3 lines; overflow is truncated with "…"

**Source freshness on e-ink:** Not shown per-source. The header strip already shows
the global last-updated time. Stale sources that caused UNKNOWN or CAUTION are named
in the reason bullets (e.g., "River data stale — last 6h 12m ago").

### No-destination fallback

When no destinations are configured, the hero zone shows "ALL SYSTEMS" at Large tier
(56px) with a summary of live conditions. This is the ambient dashboard mode; no
planning view logic applies.

---

## Web Destination Detail

URL: `/destinations/:name`

The detail page is the full planning view. It has four sections, in this order:

### Section 1 — Decision Banner (Summary)

Full-width colored block. One line of text.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                                                                               │
│                              NO GO                                            │
│                                                                               │
└──────────────────────────────────────────────────────────────────────────────┘
```

Background and text color from the decision badge table:

| State   | Background | Text    |
|---------|-----------|---------|
| GO      | #2d7d2d   | #ffffff |
| CAUTION | #c8860a   | #000000 |
| NO GO   | #c0392b   | #ffffff |
| UNKNOWN | #888888   | #ffffff |

The headline from `DecisionSummary.headline` appears below the decision text in
smaller type (14px), same color as the text.

---

### Section 2 — Decision Rationale

Heading: "WHY THIS DECISION"

One row per configured criterion. Rows are sorted: blocked first, then near-miss,
then passing, then missing.

```
WHY THIS DECISION
┌──────────────────────────────────────────────────────────────────────────────┐
│  ✗  River level      14.5 ft    (max 12.0 ft)                      [red]    │
│  ✗  Road access      SR-20 closed                                  [red]    │
│  ✓  Temperature      62°F       (min 45°F)                         [green]  │
│  ✓  Precipitation    15%        (max 40%)                          [green]  │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Row format:** `[icon]  [criterion label]  [current value]  ([configured limit])`

| Icon | Meaning | Color |
|------|---------|-------|
| ✓    | Passing | green |
| !    | Near-miss (CAUTION) | amber |
| ✗    | Blocked | red |
| ?    | Data missing | grey, italic |

For missing data rows, current_value is replaced by "— data unavailable".

For near-miss rows, append a fraction of the limit: `10.5 ft  (88% of 12.0 ft limit)`.

**Criteria not configured** are not shown at all. This section shows only what the
user has declared relevant. If a destination has no criteria, the section reads:
"No go/no-go criteria configured. This destination is always GO."

---

### Section 3 — Conditions at a Glance (Supporting Detail)

Heading: "CONDITIONS AT A GLANCE"

Raw readings from each active source, independent of criteria. One line per source.
Sources with no data are omitted.

```
CONDITIONS AT A GLANCE
┌──────────────────────────────────────────────────────────────────────────────┐
│  Weather   62°F · Partly Cloudy · SW 8 mph                                  │
│  River     14.5 ft · 8,900 cfs · Rising ↑                                   │
│  Road      SR-20 — closed (milepost 134–158)                                 │
└──────────────────────────────────────────────────────────────────────────────┘
```

This section is unconditionally visible. It lets the user see readings they did
not configure a threshold for (e.g., trail conditions, wind) that may influence
a trip decision even without a hard limit.

---

### Section 4 — Source Freshness

Heading: "DATA FRESHNESS"

One row per source that is relevant to this destination (per `RelevantSignals`).
Shows the age of each reading and a freshness indicator.

```
DATA FRESHNESS
┌──────────────────────────────────────────────────────────────────────────────┐
│  Weather   Updated 14:32  (32m ago)                          ● Fresh        │
│  River     Updated 07:45  (6h 47m ago)                       ● Expired      │
│  Road      Updated yesterday                                 ○ Stale        │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Age label format:**
- Same day, < 1 hour: "Updated HH:MM  (Nm ago)"
- Same day, ≥ 1 hour: "Updated HH:MM  (Nh Nm ago)"
- Previous day: "Updated yesterday"
- Older: "Updated N days ago"

**Freshness indicator:**

| Level    | Dot   | Description |
|----------|-------|-------------|
| Fresh    | green | < 50% of staleness threshold consumed |
| Aging    | none  | 50–75% of threshold — normal, no indicator |
| Stale    | amber | 75–100% of threshold — approaching expiry |
| Expired  | red   | > 100% of threshold — this drove UNKNOWN |

Thresholds: Weather 3h, River 6h, Road 24h, Trail 48h (from trip-recommendation-model.md).

Sources with no data show "No data received" with no indicator.

---

## Distinguishing Hero from Supporting Detail

The planning view always separates decision from evidence:

| Layer | Content | Section |
|-------|---------|---------|
| **Hero** | Decision state + single headline | Decision Banner |
| **Rationale** | Per-criterion result (did each pass?) | Why This Decision |
| **Evidence** | Raw source readings (no pass/fail) | Conditions at a Glance |
| **Metadata** | Source age indicators | Data Freshness |

Implementation rules:
- The **evaluation layer** (`evaluation/mod.rs`) produces `TripDecision` with its reasons/warnings/missing vectors.
- The **presentation layer** converts `TripDecision` + `DomainState` + `Destination` into `PlanningView`.
- The **web layer** renders `PlanningView` to HTML; the render layer reads `PlanningView.summary` for the e-ink hero.
- No decision logic lives in the web or render layers. They format, not evaluate.

---

## Stale and Missing Data — Explicit Handling Rules

### Missing data
A source reading is absent (no entry in `DomainState`).

- If a criterion depends on it → `CriterionRow.status = DataMissing`, `current_value = None`.
- The criterion's absence drives the `Unknown` decision (see trip-recommendation-model.md).
- Rationale row shows: `?  River level  — data unavailable  (max 12.0 ft)` in grey italic.
- Freshness row shows: `River   No data received`

### Stale data
A reading exists but its timestamp is beyond the staleness threshold.

- Treated as missing for evaluation purposes: same `DataMissing` status.
- The freshness indicator shows `Expired` (red).
- The age label quantifies the overage: "Updated 07:45  (6h 47m ago)" when threshold is 6h.
- Reason bullet: "River data stale (6h 47m — limit 6h)"

### Aging data (CAUTION trigger)
A reading is between 75% and 100% of its staleness threshold.

- Criterion still evaluates normally (data is present).
- `CriterionRow.status` reflects the criterion result, not the age.
- A separate CAUTION warning is added: "Weather data aging (2h 18m — limit 3h)"
- Freshness indicator shows `Stale` (amber).

### Unconfigured criteria and missing data
If a signal is relevant (`RelevantSignals.river = true`) but no criterion is configured
for it (`TripCriteria.max_river_level_ft = None`), missing river data does NOT affect the
decision. The freshness row still shows "No river data" as informational.

---

## Headline Generation Rules

`DecisionSummary.headline` is the single line shown in the banner and on the e-ink hero.

| State   | Rule | Example |
|---------|------|---------|
| Go      | Key passing conditions, comma-separated (most constrained first) | "River 9.4 ft · 62°F · SR-20 open" |
| Caution | The reading closest to its limit | "River near limit (10.5 ft / 12.0 ft max)" |
| NoGo    | First blocking reason from `reasons[0]` | "River 14.5 ft exceeds 12.0 ft limit" |
| Unknown | First missing source from `missing[0]` | "No road data" |

For GO, if no criteria are configured, headline is empty. The banner shows only "GO".
For multiple NO GO reasons, remaining reasons appear as bullets below the headline
in the banner (web) and as separate reason lines in the e-ink hero right column.

---

## Out of Scope

- Per-source retry or refresh controls (operator workflow handles source health)
- Forecast-based decisions (system works with current readings only)
- Push notifications when decision changes
- Multi-destination comparison view
