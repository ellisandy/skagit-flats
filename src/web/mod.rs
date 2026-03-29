use std::sync::Arc;

use axum::extract::{Path, State};
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

/// Build the axum Router for the local web interface.
pub fn build_router(state: Arc<SharedState>) -> Router {
    Router::new()
        .route("/", get(handler_index))
        .route("/health", get(handler_health))
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
        .with_state(state)
}

async fn handler_health() -> &'static str {
    "OK"
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

    let now = current_unix_secs();
    let mut dest_rows = String::new();
    for d in &dests.destinations {
        let decision = evaluate(d, &domain, now);
        let (badge, badge_class) = match &decision {
            crate::domain::TripDecision::Go => ("GO", "go"),
            crate::domain::TripDecision::Caution { .. } => ("CAUTION", "caution"),
            crate::domain::TripDecision::NoGo { .. } => ("NO GO", "nogo"),
            crate::domain::TripDecision::Unknown { .. } => ("UNKNOWN", "unknown"),
        };
        let reasons = match &decision {
            crate::domain::TripDecision::Go => String::new(),
            crate::domain::TripDecision::Caution { warnings } => warnings.join("; "),
            crate::domain::TripDecision::NoGo { reasons } => reasons.join("; "),
            crate::domain::TripDecision::Unknown { missing } => missing.join("; "),
        };
        let c = &d.criteria;
        dest_rows.push_str(&format!(
            r#"<tr>
  <td>{name}</td>
  <td class="{badge_class}">{badge}</td>
  <td>{reasons}</td>
  <td>{min_temp}</td>
  <td>{max_temp}</td>
  <td>{max_precip}</td>
  <td>{max_river}</td>
  <td>{road}</td>
  <td><form method="post" action="/destinations/{name_enc}" style="display:inline">
    <input type="hidden" name="_method" value="DELETE">
    <button type="submit" class="btn-delete" onclick="return deleteDestination('{name_js}')">Delete</button>
  </form></td>
</tr>"#,
            name = html_escape(&d.name),
            badge_class = badge_class,
            badge = badge,
            reasons = html_escape(&reasons),
            min_temp = c.min_temp_f.map(|v| format!("{v:.0}")).unwrap_or_default(),
            max_temp = c.max_temp_f.map(|v| format!("{v:.0}")).unwrap_or_default(),
            max_precip = c.max_precip_chance_pct.map(|v| format!("{v:.0}")).unwrap_or_default(),
            max_river = c.max_river_level_ft.map(|v| format!("{v:.1}")).unwrap_or_default(),
            road = if c.road_open_required { "Yes" } else { "No" },
            name_enc = urlencoding(&d.name),
            name_js = html_escape(&d.name),
        ));
    }

    let mut source_rows = String::new();
    for s in sources.iter() {
        let status_text = if s.enabled { "Enabled" } else { "Disabled" };
        let toggle_action = if s.enabled { "disable" } else { "enable" };
        let toggle_label = if s.enabled { "Disable" } else { "Enable" };
        source_rows.push_str(&format!(
            r#"<tr>
  <td>{name}</td>
  <td>{status}</td>
  <td>{last_err}</td>
  <td><button onclick="toggleSource('{name_js}', '{action}')" class="btn-toggle">{label}</button></td>
</tr>"#,
            name = html_escape(&s.name),
            status = status_text,
            last_err = html_escape(&s.last_error.clone().unwrap_or_default()),
            name_js = html_escape(&s.name),
            action = toggle_action,
            label = toggle_label,
        ));
    }

    Html(format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Skagit Flats Dashboard</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: system-ui, -apple-system, sans-serif; background: #f5f5f5; color: #333; padding: 1rem; }}
  h1 {{ margin-bottom: 0.5rem; }}
  h2 {{ margin: 1.5rem 0 0.5rem; border-bottom: 2px solid #333; padding-bottom: 0.25rem; }}
  .preview-container {{ text-align: center; margin: 1rem 0; background: #fff; padding: 1rem; border: 1px solid #ccc; border-radius: 4px; }}
  .preview-container img {{ max-width: 100%; height: auto; image-rendering: pixelated; border: 1px solid #999; }}
  table {{ width: 100%; border-collapse: collapse; background: #fff; margin-bottom: 1rem; }}
  th, td {{ padding: 0.5rem; text-align: left; border: 1px solid #ddd; }}
  th {{ background: #e8e8e8; font-weight: 600; }}
  .go {{ color: #fff; background: #2d7d2d; font-weight: bold; text-align: center; padding: 0.25rem 0.5rem; border-radius: 3px; }}
  .nogo {{ color: #fff; background: #c0392b; font-weight: bold; text-align: center; padding: 0.25rem 0.5rem; border-radius: 3px; }}
  .form-section {{ background: #fff; padding: 1rem; border: 1px solid #ccc; border-radius: 4px; margin-bottom: 1rem; }}
  .form-row {{ display: flex; flex-wrap: wrap; gap: 0.75rem; margin-bottom: 0.5rem; align-items: end; }}
  .form-row label {{ display: block; font-size: 0.85rem; margin-bottom: 0.2rem; }}
  .form-row input {{ padding: 0.35rem 0.5rem; border: 1px solid #ccc; border-radius: 3px; width: 120px; }}
  .form-row input[type="text"] {{ width: 200px; }}
  .form-row input[type="checkbox"] {{ width: auto; }}
  button {{ padding: 0.4rem 1rem; border: 1px solid #666; border-radius: 3px; cursor: pointer; background: #e8e8e8; }}
  button:hover {{ background: #d0d0d0; }}
  .btn-delete {{ background: #e74c3c; color: #fff; border-color: #c0392b; }}
  .btn-delete:hover {{ background: #c0392b; }}
  .btn-toggle {{ background: #3498db; color: #fff; border-color: #2980b9; }}
  .btn-toggle:hover {{ background: #2980b9; }}
  .btn-submit {{ background: #27ae60; color: #fff; border-color: #219a52; }}
  .btn-submit:hover {{ background: #219a52; }}
</style>
</head>
<body>
<h1>Skagit Flats Dashboard</h1>

<h2>Live Preview</h2>
<div class="preview-container">
  <img id="preview" src="/preview" alt="Display preview" width="800" height="480">
</div>

<h2>Destinations</h2>
<table>
<thead>
<tr>
  <th>Name</th><th>Status</th><th>Reasons</th>
  <th>Min Temp (F)</th><th>Max Temp (F)</th><th>Max Precip (%)</th>
  <th>Max River (ft)</th><th>Road Required</th><th>Actions</th>
</tr>
</thead>
<tbody>
{dest_rows}
</tbody>
</table>

<h2>Add / Update Destination</h2>
<div class="form-section">
<form id="dest-form" onsubmit="return submitDestination(event)">
  <div class="form-row">
    <div><label for="dest-name">Name</label>
    <input type="text" id="dest-name" name="name" required></div>
  </div>
  <div class="form-row">
    <div><label for="min-temp">Min Temp (F)</label>
    <input type="number" id="min-temp" step="any"></div>
    <div><label for="max-temp">Max Temp (F)</label>
    <input type="number" id="max-temp" step="any"></div>
    <div><label for="max-precip">Max Precip (%)</label>
    <input type="number" id="max-precip" step="any"></div>
    <div><label for="max-river">Max River (ft)</label>
    <input type="number" id="max-river" step="any"></div>
    <div><label for="max-flow">Max Flow (cfs)</label>
    <input type="number" id="max-flow" step="any"></div>
    <div><label for="road-req">Road Open Required</label>
    <input type="checkbox" id="road-req"></div>
  </div>
  <div class="form-row">
    <button type="submit" class="btn-submit">Save Destination</button>
  </div>
</form>
</div>

<h2>Sources</h2>
<table>
<thead>
<tr><th>Name</th><th>Status</th><th>Last Error</th><th>Actions</th></tr>
</thead>
<tbody>
{source_rows}
</tbody>
</table>

<script>
// Auto-refresh preview every 5 seconds
setInterval(function() {{
  var img = document.getElementById('preview');
  img.src = '/preview?' + Date.now();
}}, 5000);

function submitDestination(e) {{
  e.preventDefault();
  var name = document.getElementById('dest-name').value.trim();
  if (!name) return false;

  var criteria = {{}};
  var minTemp = document.getElementById('min-temp').value;
  var maxTemp = document.getElementById('max-temp').value;
  var maxPrecip = document.getElementById('max-precip').value;
  var maxRiver = document.getElementById('max-river').value;
  var maxFlow = document.getElementById('max-flow').value;
  var roadReq = document.getElementById('road-req').checked;

  if (minTemp !== '') criteria.min_temp_f = parseFloat(minTemp);
  if (maxTemp !== '') criteria.max_temp_f = parseFloat(maxTemp);
  if (maxPrecip !== '') criteria.max_precip_chance_pct = parseFloat(maxPrecip);
  if (maxRiver !== '') criteria.max_river_level_ft = parseFloat(maxRiver);
  if (maxFlow !== '') criteria.max_river_flow_cfs = parseFloat(maxFlow);
  criteria.road_open_required = roadReq;

  fetch('/destinations', {{
    method: 'POST',
    headers: {{ 'Content-Type': 'application/json' }},
    body: JSON.stringify({{ name: name, criteria: criteria }})
  }}).then(function(r) {{
    if (r.ok) location.reload();
    else r.text().then(function(t) {{ alert('Error: ' + t); }});
  }});
  return false;
}}

function deleteDestination(name) {{
  if (!confirm('Delete destination "' + name + '"?')) return false;
  fetch('/destinations/' + encodeURIComponent(name), {{ method: 'DELETE' }})
    .then(function(r) {{
      if (r.ok) location.reload();
      else r.text().then(function(t) {{ alert('Error: ' + t); }});
    }});
  return false;
}}

function toggleSource(name, action) {{
  fetch('/sources/' + encodeURIComponent(name) + '/' + action, {{ method: 'POST' }})
    .then(function(r) {{
      if (r.ok) location.reload();
      else r.text().then(function(t) {{ alert('Error: ' + t); }});
    }});
}}
</script>
</body>
</html>"##,
        dest_rows = dest_rows,
        source_rows = source_rows,
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
        assert!(html.contains("Skagit Flats Dashboard"));
        assert!(html.contains("/preview"));
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
}
