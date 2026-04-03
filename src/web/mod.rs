use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::app::SharedState;
use crate::config::{Destination, DestinationsConfig};
use crate::domain::{RelevantSignals, TripCriteria};
use crate::evaluation::{current_unix_secs, evaluate};
use crate::presentation::build_display_layout;
use crate::render::render_display;

/// Fixture RDB returned when SKAGIT_FIXTURE_DATA=1 for gauge search.
const FIXTURE_SITES_RDB: &str = include_str!("../sources/fixtures/usgs_sites.rdb");

/// Build the axum Router for the local web interface.
pub fn build_router(state: Arc<SharedState>) -> Router {
    Router::new()
        .route("/", get(handler_index))
        .route("/health", get(handler_health))
        .route("/health/hardware", get(handler_health_hardware))
        .route("/preview", get(handler_preview))
        .route("/sources", get(handler_sources))
        .route("/destinations", get(handler_list_destinations))
        .route("/destinations", post(handler_upsert_destination))
        .route("/destinations/:name", delete(handler_delete_destination))
        .route(
            "/sources/:name/enable",
            post(handler_enable_source),
        )
        .route(
            "/sources/:name/disable",
            post(handler_disable_source),
        )
        .route("/setup/gauges", get(handler_gauge_search))
        .with_state(state)
}

/// Query parameters for GET /setup/gauges.
#[derive(Debug, serde::Deserialize)]
struct GaugeSearchQuery {
    lat: f64,
    lon: f64,
    #[serde(default = "default_radius_km")]
    radius_km: f64,
}

fn default_radius_km() -> f64 {
    50.0
}

/// A single USGS gauge candidate returned by the gauge search endpoint.
#[derive(Debug, serde::Serialize)]
struct GaugeCandidate {
    site_id: String,
    site_name: String,
    lat: f64,
    lon: f64,
    distance_km: f64,
}

/// GET /setup/gauges?lat=<lat>&lon=<lon>&radius_km=<km>
///
/// Searches the USGS NWIS site service for stream gauges within the given
/// radius of the specified coordinates. Returns a JSON array of candidates
/// sorted by distance. In fixture mode (SKAGIT_FIXTURE_DATA=1) returns
/// canned results near Mount Vernon, WA.
async fn handler_gauge_search(
    Query(params): Query<GaugeSearchQuery>,
) -> impl IntoResponse {
    if !(-90.0..=90.0).contains(&params.lat) || !(-180.0..=180.0).contains(&params.lon) {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"invalid coordinates"}"#.to_string(),
        );
    }
    let radius_km = params.radius_km.clamp(1.0, 200.0);

    let use_fixtures = std::env::var("SKAGIT_FIXTURE_DATA")
        .map(|v| v == "1")
        .unwrap_or(false);

    let rdb_text = if use_fixtures {
        FIXTURE_SITES_RDB.to_string()
    } else {
        match fetch_usgs_sites(params.lat, params.lon, radius_km) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("gauge search fetch failed: {e}");
                let body = serde_json::json!({"error": e}).to_string();
                return (
                    StatusCode::BAD_GATEWAY,
                    [(header::CONTENT_TYPE, "application/json")],
                    body,
                );
            }
        }
    };

    let mut candidates = parse_usgs_sites_rdb(&rdb_text, params.lat, params.lon);
    candidates.retain(|c| c.distance_km <= radius_km);
    candidates.sort_by(|a, b| a.distance_km.partial_cmp(&b.distance_km).unwrap_or(std::cmp::Ordering::Equal));

    let body = serde_json::to_string(&candidates).unwrap_or_else(|_| "[]".to_string());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
}

/// Compute an approximate bounding box and call the USGS NWIS site service.
fn fetch_usgs_sites(lat: f64, lon: f64, radius_km: f64) -> Result<String, String> {
    // 1 degree latitude ≈ 111 km; longitude degrees shrink with cos(lat).
    let lat_deg = radius_km / 111.0;
    let lon_deg = radius_km / (111.0 * lat.to_radians().cos().abs().max(0.01));

    let min_lat = (lat - lat_deg).max(-90.0);
    let max_lat = (lat + lat_deg).min(90.0);
    let min_lon = (lon - lon_deg).max(-180.0);
    let max_lon = (lon + lon_deg).min(180.0);

    let url = format!(
        "https://waterservices.usgs.gov/nwis/site/?format=rdb&bBox={:.6},{:.6},{:.6},{:.6}&siteType=ST&hasDataTypeCd=iv",
        min_lon, min_lat, max_lon, max_lat
    );

    ureq::get(&url)
        .set("Accept", "text/plain")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

/// Parse USGS NWIS site service RDB format into gauge candidates.
///
/// RDB format: comment lines start with `#`, first non-comment line is
/// tab-separated column names, second non-comment line is column types
/// (skip it), remaining lines are tab-separated data rows.
fn parse_usgs_sites_rdb(rdb: &str, origin_lat: f64, origin_lon: f64) -> Vec<GaugeCandidate> {
    let mut candidates = Vec::new();
    let mut header_idx: Option<Vec<String>> = None;
    let mut skip_type_row = false;

    for line in rdb.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        // First non-comment line is the header row.
        if header_idx.is_none() {
            header_idx = Some(
                line.split('\t')
                    .map(|s| s.to_string())
                    .collect(),
            );
            skip_type_row = true;
            continue;
        }

        // Second non-comment line is the column type row — skip it.
        if skip_type_row {
            skip_type_row = false;
            continue;
        }

        let headers = header_idx.as_ref().unwrap();
        let fields: Vec<&str> = line.split('\t').collect();

        let get = |name: &str| -> Option<&str> {
            headers.iter().position(|h| h == name).and_then(|i| fields.get(i).copied())
        };

        let site_id = match get("site_no") {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => continue,
        };
        let site_name = match get("station_nm") {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => continue,
        };
        let lat: f64 = match get("dec_lat_va").and_then(|v| v.parse().ok()) {
            Some(v) => v,
            None => continue,
        };
        let lon: f64 = match get("dec_long_va").and_then(|v| v.parse().ok()) {
            Some(v) => v,
            None => continue,
        };

        let distance_km = haversine_km(origin_lat, origin_lon, lat, lon);
        candidates.push(GaugeCandidate {
            site_id,
            site_name,
            lat,
            lon,
            distance_km,
        });
    }

    candidates
}

/// Approximate great-circle distance in km between two lat/lon points.
fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

async fn handler_health() -> &'static str {
    "OK"
}

/// GET /health/hardware — returns hardware status as JSON.
///
/// Response: `{"ok": true}` when hardware is working or no-hardware mode,
///           `{"ok": false, "error": "..."}` when hardware initialization failed.
async fn handler_health_hardware(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    let hw_error = state.hardware_error.read().expect("hardware_error lock poisoned");
    let body = match &*hw_error {
        None => serde_json::json!({"ok": true}),
        Some(msg) => serde_json::json!({"ok": false, "error": msg}),
    };
    (
        if hw_error.is_none() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&body).unwrap_or_else(|_| r#"{"ok":false}"#.to_string()),
    )
}

async fn handler_preview(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    let buf = state.pixel_buffer.read().expect("pixel_buffer lock poisoned");
    let png_bytes = buf.to_png();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png")],
        png_bytes,
    )
}

async fn handler_sources(State(state): State<Arc<SharedState>>) -> impl IntoResponse {
    let statuses = state.source_statuses.read().expect("source_statuses lock poisoned");
    let json = serde_json::to_string(&*statuses).unwrap_or_else(|_| "[]".to_string());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
}

/// GET /destinations — list all destinations with their current TripDecision.
async fn handler_list_destinations(
    State(state): State<Arc<SharedState>>,
) -> impl IntoResponse {
    let dests = state
        .destinations_config
        .read()
        .expect("destinations_config lock poisoned");
    let domain = state
        .domain_state
        .read()
        .expect("domain_state lock poisoned");

    let now = current_unix_secs();
    let result: Vec<serde_json::Value> = dests
        .destinations
        .iter()
        .map(|d| {
            let decision = evaluate(d, &domain, now);
            serde_json::json!({
                "name": d.name,
                "criteria": d.criteria,
                "decision": decision,
            })
        })
        .collect();

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&result).unwrap_or_else(|_| "[]".to_string()),
    )
}

/// Request body for POST /destinations.
#[derive(Debug, serde::Deserialize)]
struct DestinationRequest {
    name: String,
    #[serde(default)]
    signals: RelevantSignals,
    criteria: TripCriteria,
}

/// POST /destinations — create or update a destination.
async fn handler_upsert_destination(
    State(state): State<Arc<SharedState>>,
    Json(req): Json<DestinationRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "destination name cannot be empty").into_response();
    }

    {
        let mut dests = state
            .destinations_config
            .write()
            .expect("destinations_config lock poisoned");

        if let Some(existing) = dests.destinations.iter_mut().find(|d| d.name == req.name) {
            existing.signals = req.signals;
            existing.criteria = req.criteria;
        } else {
            dests.destinations.push(Destination {
                name: req.name,
                signals: req.signals,
                criteria: req.criteria,
            });
        }

        if let Err(e) = save_destinations(&state.destinations_path, &dests) {
            log::error!("failed to save destinations.toml: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to save").into_response();
        }
    }

    re_render(&state);
    StatusCode::OK.into_response()
}

/// DELETE /destinations/:name — remove a destination.
async fn handler_delete_destination(
    State(state): State<Arc<SharedState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    {
        let mut dests = state
            .destinations_config
            .write()
            .expect("destinations_config lock poisoned");

        let before = dests.destinations.len();
        dests.destinations.retain(|d| d.name != name);

        if dests.destinations.len() == before {
            return (StatusCode::NOT_FOUND, "destination not found").into_response();
        }

        if let Err(e) = save_destinations(&state.destinations_path, &dests) {
            log::error!("failed to save destinations.toml: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "failed to save").into_response();
        }
    }

    re_render(&state);
    StatusCode::OK.into_response()
}

/// POST /sources/:name/enable — enable a source.
async fn handler_enable_source(
    State(state): State<Arc<SharedState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    set_source_enabled(&state, &name, true)
}

/// POST /sources/:name/disable — disable a source.
async fn handler_disable_source(
    State(state): State<Arc<SharedState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    set_source_enabled(&state, &name, false)
}

fn set_source_enabled(state: &SharedState, name: &str, enabled: bool) -> impl IntoResponse {
    let mut statuses = state
        .source_statuses
        .write()
        .expect("source_statuses lock poisoned");

    if let Some(src) = statuses.iter_mut().find(|s| s.name == name) {
        src.enabled = enabled;
        StatusCode::OK.into_response()
    } else {
        (StatusCode::NOT_FOUND, "source not found").into_response()
    }
}

/// Re-render the pixel buffer after a destinations change.
fn re_render(state: &SharedState) {
    let buf = {
        let dests = state
            .destinations_config
            .read()
            .expect("destinations_config lock poisoned");
        let domain = state
            .domain_state
            .read()
            .expect("domain_state lock poisoned");
        let layout = build_display_layout(&domain, &dests.destinations, current_unix_secs());
        render_display(&layout)
    };

    let mut pixel_buffer = state
        .pixel_buffer
        .write()
        .expect("pixel_buffer lock poisoned");
    *pixel_buffer = buf;
}

/// Serialize and write destinations config to disk.
fn save_destinations(
    path: &std::path::Path,
    config: &DestinationsConfig,
) -> Result<(), String> {
    let toml_str = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, toml_str).map_err(|e| e.to_string())
}

/// GET / — serve the configuration UI as plain HTML.
async fn handler_index(State(state): State<Arc<SharedState>>) -> Html<String> {
    let dests = state
        .destinations_config
        .read()
        .expect("destinations_config lock poisoned");
    let domain = state
        .domain_state
        .read()
        .expect("domain_state lock poisoned");
    let sources = state
        .source_statuses
        .read()
        .expect("source_statuses lock poisoned");
    let hw_error = state
        .hardware_error
        .read()
        .expect("hardware_error lock poisoned")
        .clone();

    let hw_error_banner = match &hw_error {
        None => String::new(),
        Some(msg) => format!(
            r#"<div class="hw-error-banner"><span class="hw-icon">&#9888;</span><div class="hw-msg"><strong>Hardware Error</strong>{}</div></div>"#,
            html_escape(msg)
        ),
    };

    let fixture_data = state.fixture_data;
    let now = current_unix_secs();
    let mut dest_cards = String::new();
    for d in &dests.destinations {
        let decision = evaluate(d, &domain, now);
        let (badge, badge_class) = match &decision {
            crate::domain::TripDecision::Go => ("GO", "go"),
            crate::domain::TripDecision::Caution { .. } => ("CAUTION", "caution"),
            crate::domain::TripDecision::NoGo { .. } => ("NO GO", "nogo"),
            crate::domain::TripDecision::Unknown { .. } => ("UNKNOWN", "unknown"),
        };
        let messages: Vec<&str> = match &decision {
            crate::domain::TripDecision::Go => vec![],
            crate::domain::TripDecision::Caution { warnings } => {
                warnings.iter().map(|s| s.as_str()).collect()
            }
            crate::domain::TripDecision::NoGo { reasons } => {
                reasons.iter().map(|s| s.as_str()).collect()
            }
            crate::domain::TripDecision::Unknown { missing } => {
                missing.iter().map(|s| s.as_str()).collect()
            }
        };
        let reasons_html: String = if messages.is_empty() {
            String::new()
        } else {
            let items: String = messages
                .iter()
                .map(|r| format!("<li>{}</li>", html_escape(r)))
                .collect();
            format!("<ul class=\"reasons\">{items}</ul>")
        };
        let c = &d.criteria;
        let mut criteria_parts: Vec<String> = Vec::new();
        if let Some(v) = c.min_temp_f { criteria_parts.push(format!("min {v:.0}\u{b0}F")); }
        if let Some(v) = c.max_temp_f { criteria_parts.push(format!("max {v:.0}\u{b0}F")); }
        if let Some(v) = c.max_precip_chance_pct { criteria_parts.push(format!("precip \u{2264}{v:.0}%")); }
        if let Some(v) = c.max_river_level_ft { criteria_parts.push(format!("river \u{2264}{v:.1}ft")); }
        if let Some(v) = c.max_river_flow_cfs { criteria_parts.push(format!("flow \u{2264}{v:.0}cfs")); }
        if c.road_open_required { criteria_parts.push("road open".to_string()); }
        let criteria_summary = if criteria_parts.is_empty() {
            String::new()
        } else {
            format!("<p class=\"criteria-summary\">{}</p>", html_escape(&criteria_parts.join(" \u{b7} ")))
        };
        dest_cards.push_str(&format!(
            r#"<div class="dest-card" id="card-{name_id}">
  <div class="card-header">
    <span class="dest-name">{name}</span>
    <span class="badge {badge_class}">{badge}</span>
  </div>
  {reasons_html}
  {criteria_summary}
  <div class="card-actions">
    <button class="btn-delete" onclick="deleteDestination('{name_js}')">Delete</button>
  </div>
</div>"#,
            name_id = urlencoding(&d.name),
            name = html_escape(&d.name),
            badge_class = badge_class,
            badge = badge,
            reasons_html = reasons_html,
            criteria_summary = criteria_summary,
            name_js = html_escape(&d.name),
        ));
    }

    let mut source_rows = String::new();
    for s in sources.iter() {
        let status_text = if s.enabled { "Enabled" } else { "Disabled" };
        let toggle_action = if s.enabled { "disable" } else { "enable" };
        let toggle_label = if s.enabled { "Disable" } else { "Enable" };
        let err_html = if let Some(ref e) = s.last_error {
            format!("<span class=\"src-error\">{}</span>", html_escape(e))
        } else {
            String::new()
        };
        source_rows.push_str(&format!(
            r#"<div class="source-row" id="src-{name_id}">
  <div class="src-info">
    <span class="src-name">{name}</span>
    <span class="src-status {status_class}">{status}</span>
    {err_html}
  </div>
  <button class="btn-toggle" onclick="toggleSource('{name_js}', '{action}')">{label}</button>
</div>"#,
            name_id = urlencoding(&s.name),
            name = html_escape(&s.name),
            status_class = if s.enabled { "enabled" } else { "disabled" },
            status = status_text,
            err_html = err_html,
            name_js = html_escape(&s.name),
            action = toggle_action,
            label = toggle_label,
        ));
    }

    let fixture_banner = if fixture_data {
        r#"<div class="fixture-banner">&#9888; FIXTURE DATA MODE — not showing live conditions</div>"#
    } else {
        ""
    };

    Html(format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Skagit Flats</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: system-ui, -apple-system, sans-serif; background: #f0f0f0; color: #222; }}
  .page-header {{ background: #1a1a2e; color: #fff; padding: 0.75rem 1rem; }}
  .page-header h1 {{ font-size: 1.1rem; font-weight: 600; letter-spacing: 0.06em; }}
  main {{ padding: 0.75rem; max-width: 700px; margin: 0 auto; }}
  section {{ margin-bottom: 1.25rem; }}
  .section-title {{ font-size: 0.72rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em; color: #888; margin-bottom: 0.5rem; padding-left: 0.1rem; }}
  .dest-cards {{ display: flex; flex-direction: column; gap: 0.6rem; }}
  .dest-card {{ background: #fff; border-radius: 10px; padding: 0.9rem 1rem; box-shadow: 0 1px 3px rgba(0,0,0,.1); }}
  .dest-card.loading {{ opacity: 0.5; pointer-events: none; }}
  .card-header {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.15rem; }}
  .dest-name {{ font-size: 1.05rem; font-weight: 600; }}
  .badge {{ font-size: 0.75rem; font-weight: 700; padding: 0.2rem 0.6rem; border-radius: 20px; letter-spacing: 0.04em; white-space: nowrap; }}
  .badge.go {{ background: #1e7e34; color: #fff; }}
  .badge.nogo {{ background: #c0392b; color: #fff; }}
  .badge.caution {{ background: #d68910; color: #fff; }}
  .badge.unknown {{ background: #888; color: #fff; }}
  .reasons {{ margin: 0.35rem 0 0.1rem 0; padding-left: 1.2rem; font-size: 0.875rem; color: #666; }}
  .reasons li {{ margin-bottom: 0.1rem; }}
  .criteria-summary {{ font-size: 0.78rem; color: #aaa; margin-top: 0.35rem; }}
  .card-actions {{ margin-top: 0.65rem; display: flex; justify-content: flex-end; }}
  .preview-wrap {{ background: #fff; border-radius: 10px; padding: 0.75rem; box-shadow: 0 1px 3px rgba(0,0,0,.1); }}
  .preview-toolbar {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.5rem; }}
  .preview-label {{ font-size: 0.78rem; color: #aaa; }}
  .preview-toolbar-btns {{ display: flex; gap: 0.4rem; }}
  .preview-wrap img {{ width: 100%; height: auto; image-rendering: pixelated; border-radius: 4px; border: 1px solid #e0e0e0; display: block; cursor: zoom-in; }}
  .preview-overlay {{ display: none; position: fixed; inset: 0; background: rgba(0,0,0,.85); z-index: 200; align-items: center; justify-content: center; }}
  .preview-overlay.open {{ display: flex; }}
  .preview-overlay img {{ max-width: 96vw; max-height: 90vh; image-rendering: pixelated; border-radius: 4px; }}
  .btn-close-overlay {{ position: fixed; top: 1rem; right: 1rem; background: rgba(255,255,255,.15); color: #fff; border: 1.5px solid rgba(255,255,255,.3); border-radius: 6px; font-size: 1.2rem; min-height: 44px; min-width: 44px; cursor: pointer; z-index: 201; line-height: 1; }}
  .form-card {{ background: #fff; border-radius: 10px; padding: 1rem; box-shadow: 0 1px 3px rgba(0,0,0,.1); }}
  .form-field {{ margin-bottom: 0.75rem; }}
  .form-field label {{ display: flex; align-items: center; gap: 0.25rem; font-size: 0.8rem; font-weight: 600; color: #555; margin-bottom: 0.3rem; }}
  .tip {{ display: inline-flex; align-items: center; justify-content: center; width: 15px; height: 15px; border-radius: 50%; background: #ccc; color: #fff; font-size: 0.6rem; font-weight: 700; cursor: default; position: relative; flex-shrink: 0; }}
  .tip::after {{ content: attr(data-tip); position: absolute; bottom: calc(100% + 6px); left: 0; transform: none; background: #1a1a2e; color: #fff; padding: 0.5rem 0.65rem; border-radius: 6px; font-size: 0.72rem; font-weight: 400; white-space: pre-line; width: 230px; pointer-events: none; opacity: 0; transition: opacity .15s; z-index: 20; line-height: 1.45; box-shadow: 0 2px 8px rgba(0,0,0,.3); }}
  .tip:hover::after, .tip:focus::after {{ opacity: 1; }}
  .tip:focus {{ outline: 2px solid #3498db; }}
  .thresholds-ref {{ margin-top: 0.25rem; margin-bottom: 0.75rem; }}
  .thresholds-ref summary {{ font-size: 0.75rem; color: #888; cursor: pointer; padding: 0.35rem 0; min-height: 44px; display: flex; align-items: center; user-select: none; }}
  .thresholds-ref summary:hover {{ color: #555; }}
  .thresholds-ref table {{ width: 100%; border-collapse: collapse; font-size: 0.72rem; margin-top: 0.4rem; }}
  .thresholds-ref th {{ background: #f4f4f4; font-weight: 600; color: #555; padding: 0.3rem 0.4rem; text-align: left; border-bottom: 1.5px solid #ddd; }}
  .thresholds-ref td {{ padding: 0.3rem 0.4rem; border-bottom: 1px solid #eee; color: #444; }}
  .thresholds-ref tr:last-child td {{ border-bottom: none; }}
  .form-field input[type="text"],
  .form-field input[type="number"] {{ width: 100%; padding: 0.55rem 0.75rem; border: 1.5px solid #ddd; border-radius: 6px; font-size: 1rem; min-height: 44px; background: #fafafa; }}
  .form-field input:focus {{ outline: 2px solid #3498db; border-color: transparent; background: #fff; }}
  .form-grid {{ display: grid; grid-template-columns: 1fr 1fr; gap: 0.6rem; }}
  .form-field-check {{ display: flex; align-items: center; gap: 0.5rem; min-height: 44px; margin-bottom: 0.75rem; }}
  .form-field-check input {{ width: 20px; height: 20px; cursor: pointer; accent-color: #1e7e34; }}
  .form-field-check label {{ font-size: 0.9rem; font-weight: 500; color: #333; margin: 0; cursor: pointer; }}
  .source-rows {{ display: flex; flex-direction: column; gap: 0.5rem; }}
  .source-row {{ background: #fff; border-radius: 8px; padding: 0.75rem 1rem; box-shadow: 0 1px 2px rgba(0,0,0,.08); display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }}
  .src-info {{ display: flex; flex-direction: column; gap: 0.15rem; flex: 1; min-width: 0; }}
  .src-name {{ font-size: 0.9rem; font-weight: 600; }}
  .src-status.enabled {{ font-size: 0.75rem; color: #1e7e34; font-weight: 600; }}
  .src-status.disabled {{ font-size: 0.75rem; color: #aaa; }}
  .src-error {{ font-size: 0.75rem; color: #c0392b; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }}
  button {{ min-height: 44px; min-width: 44px; padding: 0 1rem; border-radius: 6px; border: 1.5px solid transparent; cursor: pointer; font-size: 0.875rem; font-weight: 600; transition: opacity .15s; }}
  button:active {{ opacity: 0.7; }}
  .btn-delete {{ background: #fff0f0; color: #c0392b; border-color: #f5c6cb; }}
  .btn-toggle {{ background: #eaf4fd; color: #2471a3; border-color: #b8d6ee; }}
  .btn-refresh {{ background: #f4f4f4; color: #555; border-color: #ddd; font-size: 0.8rem; }}
  .btn-submit {{ background: #1e7e34; color: #fff; border-color: #155724; width: 100%; font-size: 1rem; }}
  #toast {{ position: fixed; bottom: 1.5rem; left: 50%; transform: translateX(-50%) translateY(6rem); background: #222; color: #fff; padding: 0.55rem 1.2rem; border-radius: 20px; font-size: 0.875rem; transition: transform .25s ease; pointer-events: none; white-space: nowrap; z-index: 100; box-shadow: 0 2px 8px rgba(0,0,0,.25); }}
  #toast.show {{ transform: translateX(-50%) translateY(0); }}
  #toast.error {{ background: #c0392b; }}
  .hw-error-banner {{ background: #c0392b; color: #fff; padding: 0.75rem 1rem; display: flex; align-items: flex-start; gap: 0.6rem; }}
  .hw-error-banner .hw-icon {{ font-size: 1.1rem; flex-shrink: 0; line-height: 1.4; }}
  .hw-error-banner .hw-msg {{ font-size: 0.875rem; line-height: 1.4; }}
  .hw-error-banner strong {{ display: block; font-size: 0.95rem; margin-bottom: 0.2rem; }}
  .fixture-banner {{ background: #7b341e; color: #fefce8; padding: 0.45rem 1rem; font-size: 0.8rem; font-weight: 700; text-align: center; letter-spacing: 0.05em; }}
  .gauge-results {{ margin-top: 0.75rem; display: flex; flex-direction: column; gap: 0.4rem; }}
  .gauge-item {{ background: #f8f8f8; border-radius: 8px; padding: 0.65rem 0.9rem; display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; border: 1.5px solid #e5e5e5; }}
  .gauge-info {{ display: flex; flex-direction: column; gap: 0.1rem; flex: 1; min-width: 0; }}
  .gauge-name {{ font-size: 0.88rem; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
  .gauge-meta {{ font-size: 0.75rem; color: #999; }}
  .gauge-id {{ font-family: monospace; font-size: 0.85rem; font-weight: 700; color: #2471a3; white-space: nowrap; }}
  .btn-copy {{ background: #eaf4fd; color: #2471a3; border-color: #b8d6ee; font-size: 0.8rem; white-space: nowrap; }}
  #gauge-search-status {{ font-size: 0.82rem; color: #888; margin-top: 0.5rem; min-height: 1.2em; }}
  .config-hint {{ background: #fffbe6; border: 1.5px solid #f0d060; border-radius: 8px; padding: 0.7rem 0.9rem; font-size: 0.82rem; color: #555; margin-top: 0.75rem; display: none; }}
  .config-hint code {{ background: #f4f0d8; border-radius: 3px; padding: 0.1rem 0.35rem; font-size: 0.85rem; }}
  @media (min-width: 480px) {{
    .form-grid {{ grid-template-columns: 1fr 1fr 1fr; }}
  }}
  .section-header {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 0.5rem; }}
  .section-header .section-title {{ margin-bottom: 0; }}
  .btn-add-dest {{ background: #1e7e34; color: #fff; border-color: #155724; min-height: 36px; font-size: 0.85rem; padding: 0 0.9rem; }}
  .optional-label {{ font-size: 0.7rem; font-weight: 400; color: #aaa; margin-left: 0.3rem; }}
  .modal-backdrop {{ display: none; position: fixed; inset: 0; background: rgba(0,0,0,.5); z-index: 200; align-items: flex-end; }}
  .modal-backdrop.open {{ display: flex; }}
  .modal-sheet {{ background: #fff; border-radius: 16px 16px 0 0; padding: 1.25rem 1rem 2rem; width: 100%; max-height: 90vh; overflow-y: auto; transform: translateY(100%); transition: transform .3s ease; }}
  .modal-backdrop.open .modal-sheet {{ transform: translateY(0); }}
  .modal-header {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 1rem; padding-bottom: 0.75rem; border-bottom: 1px solid #eee; }}
  .modal-title {{ font-size: 1.05rem; font-weight: 700; }}
  .btn-modal-close {{ background: #f0f0f0; color: #666; border-color: #ddd; min-height: 36px; min-width: 36px; padding: 0; border-radius: 50%; font-size: 1.2rem; line-height: 1; }}
  @media (min-width: 480px) {{
    .modal-backdrop {{ align-items: center; justify-content: center; }}
    .modal-sheet {{ border-radius: 12px; max-width: 480px; width: 92%; margin: 0; }}
  }}
</style>
</head>
<body>
<header class="page-header"><h1>SKAGIT FLATS</h1></header>
{hw_error_banner}
{fixture_banner}
<main>

<section>
  <div class="section-header">
    <div class="section-title">Destinations</div>
    <button class="btn-add-dest" onclick="openDestModal()">+ Add</button>
  </div>
  <div class="dest-cards" id="dest-cards">
{dest_cards}
  </div>
</section>

<section>
  <div class="section-title">Live Preview</div>
  <div class="preview-wrap">
    <div class="preview-toolbar">
      <span class="preview-label" id="preview-label">refreshes every 60s</span>
      <div class="preview-toolbar-btns">
        <button class="btn-refresh" onclick="refreshPreview()">Refresh</button>
        <button class="btn-refresh" onclick="openPreviewFullscreen()" aria-label="Expand preview">&#x26F6;</button>
      </div>
    </div>
    <img id="preview" src="/preview" alt="Display preview" onclick="openPreviewFullscreen()">
  </div>
</section>

<div class="preview-overlay" id="preview-overlay" onclick="closePreviewFullscreen()">
  <button class="btn-close-overlay" onclick="closePreviewFullscreen()" aria-label="Close">&#x2715;</button>
  <img id="preview-full" src="" alt="Display preview fullscreen">
</div>

<section>
  <div class="section-title">Sources</div>
  <div class="source-rows" id="source-rows">
{source_rows}
  </div>
</section>

<section>
  <div class="section-title">Gauge Finder</div>
  <div class="form-card">
    <p style="font-size:.83rem;color:#666;margin-bottom:.75rem">Find nearby USGS stream gauges to use as your river data source. Enter coordinates, select a gauge, then copy the site ID into your <code style="background:#f4f4f4;border-radius:3px;padding:.1rem .3rem">config.toml</code> under <code style="background:#f4f4f4;border-radius:3px;padding:.1rem .3rem">[sources.river] usgs_site_id</code>.</p>
    <div class="form-grid">
      <div class="form-field">
        <label for="gauge-lat">Latitude</label>
        <input type="number" id="gauge-lat" step="any" placeholder="e.g. 48.42">
      </div>
      <div class="form-field">
        <label for="gauge-lon">Longitude</label>
        <input type="number" id="gauge-lon" step="any" placeholder="e.g. -122.34">
      </div>
      <div class="form-field">
        <label for="gauge-radius">Radius (km)</label>
        <input type="number" id="gauge-radius" step="any" value="50" min="1" max="200">
      </div>
    </div>
    <button type="button" class="btn-submit" onclick="searchGauges()" id="gauge-search-btn">Find Gauges</button>
    <div id="gauge-search-status"></div>
    <div class="gauge-results" id="gauge-results"></div>
    <div class="config-hint" id="gauge-config-hint">
      Add this to your <strong>config.toml</strong> under <code>[sources.river]</code>:<br>
      <code id="gauge-config-snippet"></code>
    </div>
  </div>
</section>

</main>

<div class="modal-backdrop" id="dest-modal" onclick="handleModalBackdropClick(event)" role="dialog" aria-modal="true" aria-label="Add Destination">
  <div class="modal-sheet">
    <div class="modal-header">
      <span class="modal-title">Add Destination</span>
      <button class="btn-modal-close" onclick="closeDestModal()" aria-label="Close">&times;</button>
    </div>
    <form id="dest-form" onsubmit="return submitDestination(event)">
      <div class="form-field">
        <label for="dest-name">Name</label>
        <input type="text" id="dest-name" name="name" required placeholder="e.g. Skagit Loop" autocomplete="off">
      </div>
      <div class="form-field">
        <label for="min-temp">Min Temp (&deg;F)<span class="optional-label">optional</span><span class="tip" tabindex="0" data-tip="Minimum acceptable temperature.&#10;NO-GO if temp drops below this.&#10;&#10;Suggested starting values:&#10;Camping: 40°F&#10;Backpacking: 35°F&#10;Cycling: 45°F&#10;Paddling: 50°F">?</span></label>
        <input type="number" id="min-temp" step="any" placeholder="e.g. 40">
      </div>
      <div class="form-field">
        <label for="max-temp">Max Temp (&deg;F)<span class="optional-label">optional</span><span class="tip" tabindex="0" data-tip="Maximum acceptable temperature.&#10;NO-GO if temp rises above this.&#10;&#10;Suggested starting values:&#10;Camping: 90°F&#10;Backpacking: 85°F&#10;Cycling: 95°F&#10;Paddling: 90°F">?</span></label>
        <input type="number" id="max-temp" step="any" placeholder="e.g. 90">
      </div>
      <div class="form-field">
        <label for="max-precip">Max Precip (%)<span class="optional-label">optional</span><span class="tip" tabindex="0" data-tip="Maximum precipitation probability (0–100%).&#10;NO-GO if rain chance exceeds this.&#10;&#10;Suggested starting values:&#10;Camping: 60%&#10;Backpacking: 40%&#10;Cycling: 30%&#10;Paddling: 50%">?</span></label>
        <input type="number" id="max-precip" step="any" placeholder="e.g. 60">
      </div>
      <div class="form-field">
        <label for="max-river">Max River (ft)<span class="optional-label">optional</span><span class="tip" tabindex="0" data-tip="Maximum river gauge height in feet.&#10;NO-GO if water level exceeds this.&#10;Varies by gauge site.&#10;&#10;Suggested for Skagit Valley:&#10;Camping/paddling: 10–12 ft">?</span></label>
        <input type="number" id="max-river" step="any" placeholder="e.g. 12">
      </div>
      <div class="form-field">
        <label for="max-flow">Max Flow (cfs)<span class="optional-label">optional</span><span class="tip" tabindex="0" data-tip="Maximum streamflow in cubic feet per second.&#10;NO-GO if flow exceeds this.&#10;Varies by gauge site — check USGS&#10;historical data for safe thresholds.">?</span></label>
        <input type="number" id="max-flow" step="any" placeholder="e.g. 5000">
      </div>
      <details class="thresholds-ref">
        <summary>Suggested thresholds by activity type</summary>
        <table>
          <thead>
            <tr><th>Activity</th><th>Min Temp</th><th>Max Temp</th><th>Max Precip</th><th>Max River</th></tr>
          </thead>
          <tbody>
            <tr><td>Car camping</td><td>40&deg;F</td><td>90&deg;F</td><td>60%</td><td>12 ft</td></tr>
            <tr><td>Backpacking</td><td>35&deg;F</td><td>85&deg;F</td><td>40%</td><td>&mdash;</td></tr>
            <tr><td>Road cycling</td><td>45&deg;F</td><td>95&deg;F</td><td>30%</td><td>&mdash;</td></tr>
            <tr><td>Paddling</td><td>50&deg;F</td><td>90&deg;F</td><td>50%</td><td>10 ft</td></tr>
          </tbody>
        </table>
      </details>
      <div class="form-field-check">
        <input type="checkbox" id="road-req">
        <label for="road-req">Road open required<span class="tip" tabindex="0" data-tip="NO-GO if the destination&#39;s road is reported closed (e.g. seasonal gate or weather closure).">?</span></label>
      </div>
      <button type="submit" class="btn-submit">Save Destination</button>
    </form>
  </div>
</div>

<div id="toast"></div>

<script>
setInterval(function() {{ refreshPreview(); }}, 60000);

function refreshPreview() {{
  var img = document.getElementById('preview');
  var label = document.getElementById('preview-label');
  img.src = '/preview?' + Date.now();
  var t = new Date();
  label.textContent = 'refreshed ' + t.toLocaleTimeString([], {{hour:'2-digit',minute:'2-digit'}});
}}

function openPreviewFullscreen() {{
  var src = document.getElementById('preview').src;
  document.getElementById('preview-full').src = src;
  document.getElementById('preview-overlay').classList.add('open');
  document.addEventListener('keydown', _overlayKeyHandler);
}}

function closePreviewFullscreen() {{
  document.getElementById('preview-overlay').classList.remove('open');
  document.removeEventListener('keydown', _overlayKeyHandler);
}}

function _overlayKeyHandler(e) {{
  if (e.key === 'Escape') closePreviewFullscreen();
}}

function showToast(msg, isError) {{
  var el = document.getElementById('toast');
  el.textContent = msg;
  el.className = (isError ? 'error' : '') + ' show';
  clearTimeout(el._t);
  el._t = setTimeout(function() {{ el.className = isError ? 'error' : ''; }}, 2500);
}}

function refreshDestinations() {{
  fetch('/destinations')
    .then(function(r) {{ return r.json(); }})
    .then(function(data) {{
      var container = document.getElementById('dest-cards');
      if (!container) return;
      var html = '';
      data.forEach(function(d) {{
        var state = 'unknown', badge = 'UNKNOWN', cls = 'unknown';
        var dec = d.decision && d.decision.decision;
        if (dec === 'Go') {{
          state = 'go'; badge = 'GO'; cls = 'go';
        }} else if (dec === 'Caution') {{
          state = 'caution'; badge = 'CAUTION'; cls = 'caution';
        }} else if (dec === 'NoGo') {{
          state = 'nogo'; badge = 'NO GO'; cls = 'nogo';
        }}
        var msgs = [];
        if (d.decision && d.decision.warnings) msgs = d.decision.warnings;
        else if (d.decision && d.decision.reasons) msgs = d.decision.reasons;
        else if (d.decision && d.decision.missing) msgs = d.decision.missing;
        var reasonsHtml = msgs.length ? '<ul class="reasons">' + msgs.map(function(r) {{ return '<li>' + escHtml(r) + '</li>'; }}).join('') + '</ul>' : '';
        var criteria = [];
        var c = d.criteria || {{}};
        if (c.min_temp_f != null) criteria.push('min ' + c.min_temp_f.toFixed(0) + '\u00b0F');
        if (c.max_temp_f != null) criteria.push('max ' + c.max_temp_f.toFixed(0) + '\u00b0F');
        if (c.max_precip_chance_pct != null) criteria.push('precip \u2264' + c.max_precip_chance_pct.toFixed(0) + '%');
        if (c.max_river_level_ft != null) criteria.push('river \u2264' + c.max_river_level_ft.toFixed(1) + 'ft');
        if (c.max_river_flow_cfs != null) criteria.push('flow \u2264' + c.max_river_flow_cfs.toFixed(0) + 'cfs');
        if (c.road_open_required) criteria.push('road open');
        var critHtml = criteria.length ? '<p class="criteria-summary">' + escHtml(criteria.join(' \u00b7 ')) + '</p>' : '';
        var nameId = encodeURIComponent(d.name);
        html += '<div class="dest-card" id="card-' + nameId + '">'
          + '<div class="card-header"><span class="dest-name">' + escHtml(d.name) + '</span>'
          + '<span class="badge ' + cls + '">' + badge + '</span></div>'
          + reasonsHtml + critHtml
          + '<div class="card-actions"><button class="btn-delete" onclick="deleteDestination(\'' + escJs(d.name) + '\')">Delete</button></div>'
          + '</div>';
      }});
      container.innerHTML = html || '<p style="color:#aaa;font-size:.9rem;padding:.5rem .1rem">No destinations yet.</p>';
    }});
}}

function openDestModal() {{
  var modal = document.getElementById('dest-modal');
  modal.classList.add('open');
  document.body.style.overflow = 'hidden';
  setTimeout(function() {{
    var nameField = document.getElementById('dest-name');
    if (nameField) nameField.focus();
  }}, 50);
}}

function closeDestModal() {{
  var modal = document.getElementById('dest-modal');
  modal.classList.remove('open');
  document.body.style.overflow = '';
}}

function handleModalBackdropClick(e) {{
  if (e.target === document.getElementById('dest-modal')) closeDestModal();
}}

document.addEventListener('keydown', function(e) {{
  if (e.key === 'Escape') closeDestModal();
}});

function submitDestination(e) {{
  e.preventDefault();
  var name = document.getElementById('dest-name').value.trim();
  if (!name) return false;
  var criteria = {{}};
  var f = function(id) {{ return document.getElementById(id).value; }};
  if (f('min-temp') !== '') criteria.min_temp_f = parseFloat(f('min-temp'));
  if (f('max-temp') !== '') criteria.max_temp_f = parseFloat(f('max-temp'));
  if (f('max-precip') !== '') criteria.max_precip_chance_pct = parseFloat(f('max-precip'));
  if (f('max-river') !== '') criteria.max_river_level_ft = parseFloat(f('max-river'));
  if (f('max-flow') !== '') criteria.max_river_flow_cfs = parseFloat(f('max-flow'));
  criteria.road_open_required = document.getElementById('road-req').checked;
  var btn = e.target.querySelector('button[type="submit"]');
  btn.disabled = true; btn.textContent = 'Saving\u2026';
  fetch('/destinations', {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{name: name, criteria: criteria}})
  }}).then(function(r) {{
    btn.disabled = false; btn.textContent = 'Save Destination';
    if (r.ok) {{
      showToast('\u201c' + name + '\u201d saved');
      e.target.reset();
      closeDestModal();
      refreshDestinations();
      refreshPreview();
    }} else {{ r.text().then(function(t) {{ showToast('Error: ' + t, true); }}); }}
  }}).catch(function() {{ btn.disabled = false; btn.textContent = 'Save Destination'; showToast('Network error', true); }});
  return false;
}}

function deleteDestination(name) {{
  if (!confirm('Delete \u201c' + name + '\u201d?')) return;
  var card = document.getElementById('card-' + encodeURIComponent(name));
  if (card) card.classList.add('loading');
  fetch('/destinations/' + encodeURIComponent(name), {{method: 'DELETE'}})
    .then(function(r) {{
      if (r.ok) {{
        showToast('\u201c' + name + '\u201d deleted');
        if (card) card.remove();
        refreshPreview();
      }} else {{
        if (card) card.classList.remove('loading');
        r.text().then(function(t) {{ showToast('Error: ' + t, true); }});
      }}
    }}).catch(function() {{ if (card) card.classList.remove('loading'); showToast('Network error', true); }});
}}

function toggleSource(name, action) {{
  var row = document.getElementById('src-' + encodeURIComponent(name));
  var btn = row && row.querySelector('button');
  if (btn) btn.disabled = true;
  fetch('/sources/' + encodeURIComponent(name) + '/' + action, {{method: 'POST'}})
    .then(function(r) {{
      if (r.ok) {{
        var on = action === 'enable';
        showToast(name + (on ? ' enabled' : ' disabled'));
        if (row) {{
          var st = row.querySelector('.src-status');
          if (st) {{ st.textContent = on ? 'Enabled' : 'Disabled'; st.className = 'src-status ' + (on ? 'enabled' : 'disabled'); }}
          if (btn) {{ btn.disabled = false; btn.textContent = on ? 'Disable' : 'Enable'; btn.onclick = function() {{ toggleSource(name, on ? 'disable' : 'enable'); }}; }}
        }}
      }} else {{
        if (btn) btn.disabled = false;
        r.text().then(function(t) {{ showToast('Error: ' + t, true); }});
      }}
    }}).catch(function() {{ if (btn) btn.disabled = false; showToast('Network error', true); }});
}}

function escHtml(s) {{
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}}
function escJs(s) {{
  return String(s).replace(/\\/g,'\\\\').replace(/'/g,"\\'");
}}

function searchGauges() {{
  var lat = document.getElementById('gauge-lat').value.trim();
  var lon = document.getElementById('gauge-lon').value.trim();
  var radius = document.getElementById('gauge-radius').value.trim() || '50';
  if (!lat || !lon) {{ showToast('Enter latitude and longitude', true); return; }}
  var btn = document.getElementById('gauge-search-btn');
  var status = document.getElementById('gauge-search-status');
  var results = document.getElementById('gauge-results');
  var hint = document.getElementById('gauge-config-hint');
  btn.disabled = true;
  btn.textContent = 'Searching\u2026';
  status.textContent = '';
  results.innerHTML = '';
  hint.style.display = 'none';
  var url = '/setup/gauges?lat=' + encodeURIComponent(lat) + '&lon=' + encodeURIComponent(lon) + '&radius_km=' + encodeURIComponent(radius);
  fetch(url)
    .then(function(r) {{ return r.json(); }})
    .then(function(data) {{
      btn.disabled = false;
      btn.textContent = 'Find Gauges';
      if (data.error) {{ status.textContent = 'Error: ' + data.error; return; }}
      if (!data.length) {{ status.textContent = 'No stream gauges found within ' + radius + ' km.'; return; }}
      status.textContent = data.length + ' gauge' + (data.length === 1 ? '' : 's') + ' found.';
      var html = '';
      data.forEach(function(g) {{
        var dist = g.distance_km < 10 ? g.distance_km.toFixed(1) : Math.round(g.distance_km);
        html += '<div class="gauge-item">'
          + '<div class="gauge-info">'
          + '<span class="gauge-name">' + escHtml(g.site_name) + '</span>'
          + '<span class="gauge-meta">' + dist + ' km away \u00b7 ' + g.lat.toFixed(4) + ', ' + g.lon.toFixed(4) + '</span>'
          + '</div>'
          + '<span class="gauge-id">' + escHtml(g.site_id) + '</span>'
          + '<button class="btn-copy" onclick="selectGauge(\'' + escJs(g.site_id) + '\', \'' + escJs(g.site_name) + '\')">Select</button>'
          + '</div>';
      }});
      results.innerHTML = html;
    }})
    .catch(function(e) {{
      btn.disabled = false;
      btn.textContent = 'Find Gauges';
      status.textContent = 'Network error: ' + e;
    }});
}}

function selectGauge(siteId, siteName) {{
  var snippet = 'usgs_site_id = "' + siteId + '"';
  document.getElementById('gauge-config-snippet').textContent = snippet;
  var hint = document.getElementById('gauge-config-hint');
  hint.style.display = 'block';
  if (navigator.clipboard) {{
    navigator.clipboard.writeText(snippet).then(function() {{
      showToast('Copied: ' + snippet);
    }}).catch(function() {{
      showToast('Selected: ' + siteId + ' (' + siteName + ')');
    }});
  }} else {{
    showToast('Selected: ' + siteId + ' (' + siteName + ')');
  }}
}}
</script>
</body>
</html>"##,
        fixture_banner = fixture_banner,
        dest_cards = dest_cards,
        source_rows = source_rows,
        hw_error_banner = hw_error_banner,
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use crate::app::{SharedState, SourceStatus};
    use crate::config::DestinationsConfig;
    use crate::domain::DomainState;
    use crate::render::PixelBuffer;
    use std::sync::RwLock;
    use tower::ServiceExt;

    fn test_state() -> Arc<SharedState> {
        Arc::new(SharedState {
            pixel_buffer: RwLock::new(PixelBuffer::new(800, 480)),
            source_statuses: RwLock::new(vec![SourceStatus {
                name: "weather".to_string(),
                enabled: true,
                last_fetch: Some(1000),
                last_error: None,
                next_fetch: Some(1300),
            }]),
            destinations_config: RwLock::new(DestinationsConfig::default()),
            domain_state: RwLock::new(DomainState::default()),
            destinations_path: "/tmp/skagit-test-destinations.toml".into(),
            display_width: 800,
            display_height: 480,
            hardware_error: RwLock::new(None),
            fixture_data: false,
        })
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_returns_png() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/preview").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "image/png"
        );
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        // PNG magic bytes
        assert_eq!(&body[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[tokio::test]
    async fn sources_returns_json() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/sources").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "weather");
    }

    #[tokio::test]
    async fn index_returns_html() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("SKAGIT FLATS"));
        assert!(html.contains("/preview"));
    }

    fn test_state_fixture() -> Arc<SharedState> {
        Arc::new(SharedState {
            pixel_buffer: RwLock::new(PixelBuffer::new(800, 480)),
            source_statuses: RwLock::new(vec![]),
            destinations_config: RwLock::new(DestinationsConfig::default()),
            domain_state: RwLock::new(DomainState::default()),
            destinations_path: "/tmp/skagit-test-destinations.toml".into(),
            display_width: 800,
            display_height: 480,
            hardware_error: RwLock::new(None),
            fixture_data: true,
        })
    }

    #[tokio::test]
    async fn index_shows_fixture_banner_when_fixture_mode() {
        let app = build_router(test_state_fixture());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("fixture-banner"), "fixture banner element should be present");
        assert!(html.contains("FIXTURE DATA MODE"), "fixture mode text should be present");
    }

    #[tokio::test]
    async fn index_no_fixture_banner_in_normal_mode() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(!html.contains("FIXTURE DATA MODE"), "fixture banner should not appear in normal mode");
    }

    #[tokio::test]
    async fn destinations_returns_empty_json() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/destinations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(parsed.is_empty());
    }

    #[tokio::test]
    async fn post_destination_creates_entry() {
        let state = test_state();
        let app = build_router(Arc::clone(&state));
        let body = serde_json::json!({
            "name": "Test Loop",
            "criteria": {
                "min_temp_f": 45.0,
                "road_open_required": true
            }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/destinations")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let dests = state.destinations_config.read().unwrap();
        assert_eq!(dests.destinations.len(), 1);
        assert_eq!(dests.destinations[0].name, "Test Loop");
    }

    #[tokio::test]
    async fn delete_nonexistent_destination_returns_404() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/destinations/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn enable_disable_source() {
        let state = test_state();
        let app = build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sources/weather/disable")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let statuses = state.source_statuses.read().unwrap();
        assert!(!statuses[0].enabled);
    }

    #[tokio::test]
    async fn toggle_nonexistent_source_returns_404() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sources/bogus/enable")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn hardware_health_ok_when_no_error() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health/hardware")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["ok"], true);
    }

    #[tokio::test]
    async fn hardware_health_error_when_hw_failed() {
        let state = test_state();
        *state.hardware_error.write().unwrap() =
            Some("SPI error: /dev/spidev0.0 not found".to_string());
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health/hardware")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].as_str().unwrap().contains("SPI"));
    }

    #[tokio::test]
    async fn index_shows_hw_error_banner_when_hw_failed() {
        let state = test_state();
        *state.hardware_error.write().unwrap() =
            Some("SPI error: /dev/spidev0.0 not found".to_string());
        let app = build_router(state);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("hw-error-banner"));
        assert!(html.contains("Hardware Error"));
        assert!(html.contains("SPI error"));
    }

    #[tokio::test]
    async fn index_no_hw_error_banner_when_hw_ok() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(!html.contains("Hardware Error"));
        assert!(!html.contains(r#"<div class="hw-error-banner"#));
    }

    #[tokio::test]
    async fn gauge_search_fixture_mode_returns_candidates() {
        std::env::set_var("SKAGIT_FIXTURE_DATA", "1");
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/setup/gauges?lat=48.42&lon=-122.34&radius_km=100")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let body = axum::body::to_bytes(resp.into_body(), 100_000).await.unwrap();
        let candidates: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(!candidates.is_empty(), "should return at least one gauge");
        let first = &candidates[0];
        assert!(first.get("site_id").is_some());
        assert!(first.get("site_name").is_some());
        assert!(first.get("distance_km").is_some());
        if candidates.len() > 1 {
            let d0 = candidates[0]["distance_km"].as_f64().unwrap();
            let d1 = candidates[1]["distance_km"].as_f64().unwrap();
            assert!(d0 <= d1, "results should be sorted by distance");
        }
        std::env::remove_var("SKAGIT_FIXTURE_DATA");
    }

    #[tokio::test]
    async fn gauge_search_invalid_coords_returns_400() {
        let app = build_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/setup/gauges?lat=999&lon=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn parse_usgs_sites_rdb_fixture() {
        let candidates = parse_usgs_sites_rdb(FIXTURE_SITES_RDB, 48.42, -122.34);
        assert!(!candidates.is_empty(), "fixture should yield candidates");
        let skagit = candidates.iter().find(|c| c.site_id == "12200500");
        assert!(skagit.is_some(), "should find Skagit River gauge");
        let s = skagit.unwrap();
        assert!(s.distance_km < 10.0, "Skagit River gauge should be <10 km away");
    }

    #[test]
    fn haversine_km_known_distance() {
        let d = haversine_km(47.6062, -122.3321, 45.5051, -122.6750);
        assert!((d - 235.0).abs() < 10.0, "Seattle-Portland distance ~235 km, got {d:.1}");
    }
}
