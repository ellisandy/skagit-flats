# Trip Planning Recommendation Model

## Overview

The trip planning feature evaluates user-configured destinations against current
conditions and renders a clear, glanceable recommendation on the e-ink display.
This document defines the four recommendation states, their user-facing meanings,
explanation requirements, and the rules that govern state selection.

---

## Recommendation States

### GO

**Meaning:** All configured criteria are satisfied. Data is present and fresh.

**When displayed:** Every threshold the user configured is met, and the source
data used for the evaluation is within its freshness window.

**Minimum explanation:** None required beyond the label. No qualifying text is
needed when everything is fine.

**Display example:**
```
Cascade Pass   GO
```

---

### CAUTION

**Meaning:** No criteria are breached, but one or more conditions are close to
the configured limits, or source data is approaching (but not yet at) its
staleness threshold. Worth reviewing before committing to the trip.

**When displayed:** At least one of:
- A numerical criterion is met but within the near-miss margin (see below)
- Source data is between 75% and 100% of its staleness window

**Minimum explanation:** At least one warning line identifying which condition is
marginal and by how much.

**Display example:**
```
Cascade Pass   CAUTION
• Temp 52°F — 2° above minimum
```

---

### NO GO

**Meaning:** One or more configured criteria are exceeded. The trip conditions
fall outside the user's stated limits.

**When displayed:** At least one threshold is breached. This takes precedence
over missing data: if a hard blocker is confirmed (e.g., road is closed), NO GO
is returned even if other data sources are missing.

**Minimum explanation:** One line per blocking reason, naming the specific
criterion and the current value vs. the limit.

**Display example:**
```
Cascade Pass   NO GO
• River 14.5ft above limit 12.0ft
• SR-20 is closed
```

---

### UNKNOWN

**Meaning:** The system cannot make a confident recommendation. Required data
is absent or too stale to trust. No hard blockers have been confirmed.

**When displayed:** No hard blocker was found, AND at least one data source
needed to evaluate a configured criterion is either (a) not yet received, or
(b) older than its staleness threshold.

**Minimum explanation:** One line per missing or stale source.

**Display example:**
```
Cascade Pass   UNKNOWN
• No weather data
• River data stale (>6h)
```

---

## Priority Rules

When multiple conditions apply simultaneously, states are selected in this order:

1. **NO GO** — if any hard criterion is exceeded (regardless of missing data)
2. **UNKNOWN** — if no blocker confirmed but required data is missing or stale
3. **CAUTION** — if all criteria met but one or more are near-miss or data is
   approaching staleness
4. **GO** — all criteria met, all required data present and fresh

Rationale for NO GO taking priority over UNKNOWN: if we can confirm a blocker
(e.g., road closed), we tell the user that directly. An UNKNOWN alongside a
confirmed blocker adds no information and dilutes the actionable signal.

---

## Staleness Thresholds

Data older than these limits makes the recommendation UNKNOWN for any criteria
that depend on that source:

| Source  | Threshold | Rationale |
|---------|-----------|-----------|
| Weather | 3 hours   | NOAA observations update hourly; 3h allows two missed fetches |
| River   | 6 hours   | USGS gauges update every 15–30 min; 6h allows network outages |
| Road    | 24 hours  | Road closures change infrequently; one missed day is acceptable |
| Trail   | 48 hours  | Trail conditions change on the scale of days |

These thresholds are system-level constants (not per-destination). If users need
finer control, that can be added as a future `destinations.toml` field.

---

## Near-Miss Margins (CAUTION Triggers)

A criterion is a near-miss — sufficient to trigger CAUTION — when it is met
but within the following margins of the configured threshold:

| Criterion          | Margin                    |
|--------------------|---------------------------|
| Minimum temperature | Within 5°F above min     |
| Maximum temperature | Within 5°F below max     |
| Precipitation chance | Within 10 percentage points below max |
| River level        | Within 10% of max level  |
| River flow         | Within 10% of max flow   |
| Road status        | Binary — no near-miss; road is open or not |

**Example:** `min_temp_f = 50`, current temp = 52°F → CAUTION because 52 is within 5°F of 50.

---

## Missing vs. Unconfigured Data

A distinction exists between **unconfigured** and **missing** criteria:

- **Unconfigured** (`None` in `TripCriteria`): the user has not set a threshold
  for that dimension. Missing source data for an unconfigured criterion does NOT
  trigger UNKNOWN.
- **Missing** or **stale**: source data is absent or expired, AND the user HAS
  configured a criterion that depends on it. This triggers UNKNOWN.

This means a destination with only a temperature threshold configured will not
go UNKNOWN due to missing river data — only due to missing weather data.

---

## Explanation Requirements

Every non-GO state requires at least one explanation line:

| State   | Explanation content |
|---------|---------------------|
| CAUTION | Which criterion is near-miss, current value, and how close to threshold |
| NO GO   | Which criterion failed, current value, and the configured limit |
| UNKNOWN | Which source is missing or stale; if stale, how old |

Explanations are rendered as bullet points in the trip panel on both the e-ink
display and the web preview.

---

## Relationship to Evaluation Layer

All recommendation logic lives in `src/evaluation/mod.rs`. The presentation
layer (`src/presentation/mod.rs`) formats the result but never re-derives the
state. The display and web layers consume the formatted `Panel` struct. No
trip-decision logic belongs outside of `evaluation`.
