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
  .preview-wrap img {{ width: 100%; height: auto; image-rendering: pixelated; border-radius: 4px; border: 1px solid #e0e0e0; display: block; }}
  .form-card {{ background: #fff; border-radius: 10px; padding: 1rem; box-shadow: 0 1px 3px rgba(0,0,0,.1); }}
  .form-field {{ margin-bottom: 0.75rem; }}
  .form-field label {{ display: block; font-size: 0.8rem; font-weight: 600; color: #555; margin-bottom: 0.3rem; }}
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
  .btn-refresh {{ background: #f4f4f4; color: #555; border-color: #ddd; min-height: 36px; font-size: 0.8rem; }}
  .btn-submit {{ background: #1e7e34; color: #fff; border-color: #155724; width: 100%; font-size: 1rem; }}
  #toast {{ position: fixed; bottom: 1.5rem; left: 50%; transform: translateX(-50%) translateY(6rem); background: #222; color: #fff; padding: 0.55rem 1.2rem; border-radius: 20px; font-size: 0.875rem; transition: transform .25s ease; pointer-events: none; white-space: nowrap; z-index: 100; box-shadow: 0 2px 8px rgba(0,0,0,.25); }}
  #toast.show {{ transform: translateX(-50%) translateY(0); }}
  #toast.error {{ background: #c0392b; }}
  @media (min-width: 480px) {{
    .form-grid {{ grid-template-columns: 1fr 1fr 1fr; }}
  }}
</style>
</head>
<body>
<header class="page-header"><h1>SKAGIT FLATS</h1></header>
<main>

<section>
  <div class="section-title">Destinations</div>
  <div class="dest-cards" id="dest-cards">
{dest_cards}
  </div>
</section>

<section>
  <div class="section-title">Live Preview</div>
  <div class="preview-wrap">
    <div class="preview-toolbar">
      <span class="preview-label" id="preview-label">refreshes every 60s</span>
      <button class="btn-refresh" onclick="refreshPreview()">Refresh</button>
    </div>
    <img id="preview" src="/preview" alt="Display preview">
  </div>
</section>

<section>
  <div class="section-title">Add / Update Destination</div>
  <div class="form-card">
    <form id="dest-form" onsubmit="return submitDestination(event)">
      <div class="form-field">
        <label for="dest-name">Name</label>
        <input type="text" id="dest-name" name="name" required placeholder="e.g. Skagit Loop">
      </div>
      <div class="form-grid">
        <div class="form-field">
          <label for="min-temp">Min Temp (&deg;F)</label>
          <input type="number" id="min-temp" step="any" placeholder="optional">
        </div>
        <div class="form-field">
          <label for="max-temp">Max Temp (&deg;F)</label>
          <input type="number" id="max-temp" step="any" placeholder="optional">
        </div>
        <div class="form-field">
          <label for="max-precip">Max Precip (%)</label>
          <input type="number" id="max-precip" step="any" placeholder="optional">
        </div>
        <div class="form-field">
          <label for="max-river">Max River (ft)</label>
          <input type="number" id="max-river" step="any" placeholder="optional">
        </div>
        <div class="form-field">
          <label for="max-flow">Max Flow (cfs)</label>
          <input type="number" id="max-flow" step="any" placeholder="optional">
        </div>
      </div>
      <div class="form-field-check">
        <input type="checkbox" id="road-req">
        <label for="road-req">Road open required</label>
      </div>
      <button type="submit" class="btn-submit">Save Destination</button>
    </form>
  </div>
</section>

<section>
  <div class="section-title">Sources</div>
  <div class="source-rows" id="source-rows">
{source_rows}
  </div>
</section>

</main>
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
        if (d.decision === 'Go' || (d.decision && d.decision.Go !== undefined)) {{
          state = 'go'; badge = 'GO'; cls = 'go';
        }} else if (d.decision && d.decision.Caution) {{
          state = 'caution'; badge = 'CAUTION'; cls = 'caution';
        }} else if (d.decision && d.decision.NoGo) {{
          state = 'nogo'; badge = 'NO GO'; cls = 'nogo';
        }}
        var msgs = [];
        if (d.decision && d.decision.Caution && d.decision.Caution.warnings) msgs = d.decision.Caution.warnings;
        else if (d.decision && d.decision.NoGo && d.decision.NoGo.reasons) msgs = d.decision.NoGo.reasons;
        else if (d.decision && d.decision.Unknown && d.decision.Unknown.missing) msgs = d.decision.Unknown.missing;
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
</script>
</body>
</html>"##,
        dest_cards = dest_cards,
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
        assert!(html.contains("SKAGIT FLATS"));
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
