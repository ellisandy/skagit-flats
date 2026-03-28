# ADR: Trail and Campsite Conditions Data Source

**Status:** Accepted
**Date:** 2026-03-28
**Issue:** sf-czh

## Context

The skagit-flats display needs trail and campsite condition data for the Skagit
Valley / North Cascades area. There is no single unified API for this. A spike
was required to evaluate available programmatic sources.

## Sources Evaluated

### WTA (Washington Trails Association, wta.org)

- **No public API.** Trip reports and trail conditions are available only via the
  website. An internal mobile API exists but is undocumented and not intended for
  third-party use.
- **Scraping is not viable.** WTA is a nonprofit; their `robots.txt` restricts
  automated access. Using their data without a partnership agreement would be
  ethically and legally problematic.
- **Verdict:** Not usable without a data-sharing agreement.

### Recreation.gov — RIDB API (Official)

- **Endpoint:** `https://ridb.recreation.gov/api/v1/`
- **Auth:** Free API key (register at ridb.recreation.gov).
- **Rate limit:** 50 requests/minute.
- **Data:** Facility metadata (campgrounds, campsites, locations, amenities).
  Does NOT include real-time availability.
- **Verdict:** Useful for campground metadata but not conditions.

### Recreation.gov — Availability API (Undocumented)

- **Endpoint:** `https://www.recreation.gov/api/camps/availability/campground/{id}/month?start_date={date}`
- **Auth:** None required.
- **Data:** Per-campsite availability by date (Available/Reserved), campsite
  type, capacity, loop. JSON format.
- **Caveats:** Undocumented, no stability guarantee. Widely used by open-source
  projects. Should be called respectfully (low frequency, with backoff).
- **Verdict:** Best source for campsite availability. Acceptable risk for a
  personal display project with infrequent polling.

### USFS — Mt. Baker-Snoqualmie NF RSS Feeds

- **Feed URL:** `https://www.fs.usda.gov/rss/mbs`
- **Auth:** None.
- **Data:** Forest alerts, closures, and trail/road condition notices in
  standard RSS/XML format.
- **Verdict:** Good source for closures and alerts. Simple to parse.

### NPS API — National Park Service

- **Endpoint:** `https://developer.nps.gov/api/v1/alerts?parkCode=noca`
- **Auth:** Free API key (register at nps.gov/subjects/developer).
- **Rate limit:** 1,000 requests/hour.
- **Data:** Park alerts including trail closures, hazards, and condition changes.
  Categories: danger, caution, information, park-closure. JSON format.
- **Verdict:** Excellent for North Cascades NP conditions. Official, documented,
  stable API.

### Washington State Parks

- **No public API** for conditions. GIS-only data via geo.wa.gov.
- **Verdict:** Not usable.

## Decision

Implement a **composite trail conditions source** that fetches from two APIs:

1. **NPS Alerts API** — for North Cascades National Park trail/campsite
   conditions. This is the primary source for the Skagit Valley area.
2. **USFS RSS feed** — for Mt. Baker-Snoqualmie National Forest closures and
   alerts.

Recreation.gov campsite availability is deferred to a future source — it solves
a different problem (booking availability vs. conditions) and requires campground
IDs to be configured per destination.

WTA is excluded unless a partnership is established.

### Rationale

- NPS API is official, documented, free, and covers the most relevant area
  (North Cascades).
- USFS RSS is simple, stable, and covers the national forest land surrounding
  the Cascades.
- Both can be fetched without scraping and without API key issues (NPS key is
  free and generous).
- The two sources together cover the majority of trail destinations accessible
  from the Skagit Valley.

## Implementation

- `src/sources/trail_conditions.rs` implements `Source` for the NPS Alerts API.
- Fixture mode (`SKAGIT_FIXTURE_DATA=1`) returns static JSON instead of calling
  the live API.
- The NPS park code is configurable via `config.toml` (`trail_park_code`),
  defaulting to `"noca"` (North Cascades).
- The source maps NPS alerts to `TrailCondition` domain objects, using the alert
  title as `destination_name` and the alert description as
  `suitability_summary`.
- USFS RSS integration is a follow-up (separate bead) — the RSS parsing adds
  a dependency and the NPS source alone provides sufficient initial coverage.

## Consequences

- The display will show trail/campsite alerts from North Cascades NP.
- A future source can add USFS RSS, Recreation.gov availability, or WTA data
  (if a partnership is established).
- The NPS API key must be set in the environment (`NPS_API_KEY`) for live mode.
  Fixture mode works without it.
