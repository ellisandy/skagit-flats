# ADR: Road Closures Data Source

**Status:** Accepted
**Date:** 2026-03-28
**Issue:** sf-81h

## Context

The skagit-flats display needs road closure and restriction data for roads
leading to configured destinations in the Skagit Valley. Key routes include
SR-20 (North Cascades Highway), I-5, SR-530, and SR-9. A research spike was
required to evaluate available APIs.

## Sources Evaluated

### WSDOT Traveler Information API — Highway Alerts

- **Endpoint:** `https://www.wsdot.wa.gov/Traffic/api/HighwayAlerts/HighwayAlertsREST.svc/GetAlertsAsJson?AccessCode=KEY`
- **Auth:** Free API key, registered at wsdot.wa.gov/traffic/api/. Passed as
  `AccessCode` query parameter.
- **Rate limit:** Not formally published; data updates every few minutes on
  WSDOT's side, so polling every 15–30 minutes is appropriate.
- **Data:** All active highway alerts statewide. Each alert includes
  `EventCategory` (Closure, Construction, Incident, Maintenance),
  `HeadlineDescription`, `ExtendedDescription`, road name, county, mileposts,
  and priority. Client-side filtering by `RoadName` or `County` is required.
- **Date format:** Legacy Microsoft `/Date(millis)/` — not ISO 8601.
- **Verdict:** Best available source for state highway closures. Official,
  documented, free, and covers all WSDOT-maintained routes in the valley.

### WSDOT Mountain Pass Conditions API

- **Endpoint:** `https://www.wsdot.wa.gov/Traffic/api/MountainPassConditions/MountainPassConditionsREST.svc/GetMountainPassConditionsAsJson?AccessCode=KEY`
- **Data:** Pass-specific conditions (temperature, weather, restrictions).
  Relevant for SR-20 seasonal closure status.
- **Verdict:** Useful supplement but Highway Alerts already covers SR-20
  closures. Deferred to a future enhancement.

### USFS Roads (Cascade River Road, Baker Lake Road)

- **No real-time API exists.** Forest road status is published on per-forest
  web pages only.
- The NPS Alerts API (already integrated in `trail_conditions.rs`) partially
  covers roads within North Cascades NP boundaries.
- **Verdict:** Not usable without scraping. Gap acknowledged.

### Skagit County Roads

- **No public API.** Closures posted to the county website and social media.
- **Verdict:** Not usable.

### WSDOT GeoPortal (ArcGIS)

- **Endpoint:** `https://data.wsdot.wa.gov/arcgis/rest/services/Shared/HighwayAlerts/MapServer/0/query`
- **Data:** Same highway alerts via ArcGIS REST, supports spatial bounding-box
  queries.
- **Verdict:** More complex integration for marginal benefit. The REST API is
  simpler and sufficient for named-route filtering.

## Decision

Implement a **road closures source** using the **WSDOT Highway Alerts API**.

- Fetch all alerts, filter client-side by a configured list of route numbers
  (e.g., `["020", "005", "530"]`).
- Filter to `EventCategory == "Closure"` by default (configurable).
- Map matching alerts to `RoadStatus` domain objects.
- If no closures match, report the first configured road as "OPEN" (no active
  closures).

### Rationale

- WSDOT Highway Alerts is the only API with real-time road closure data for
  state highways in the Skagit Valley.
- Free registration, no approval process, generous usage for a personal display.
- The response is straightforward JSON, and client-side filtering is trivial.
- USFS and county roads are a known gap with no API solution; NPS Alerts
  partially covers national park roads.

## Implementation

- `src/sources/road_closures.rs` implements `Source` for WSDOT Highway Alerts.
- Fixture mode (`SKAGIT_FIXTURE_DATA=1`) returns static JSON without network
  calls.
- The WSDOT access code is configurable via `config.toml` (`wsdot_access_code`)
  or `WSDOT_ACCESS_CODE` environment variable.
- Monitored routes are configurable via `config.toml` (`routes`), defaulting to
  `["020"]` (SR-20).
- Road name mapping (e.g., `"020"` → `"SR-20 North Cascades Hwy"`) is handled
  in the source for display purposes.

## Consequences

- The display will show road closure status for configured state highways.
- USFS and county roads remain uncovered until APIs become available or
  scraping is implemented.
- The WSDOT access code must be set for live mode. Fixture mode works without
  it.
