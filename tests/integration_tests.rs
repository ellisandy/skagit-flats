//! Integration tests that verify end-to-end behavior using fixture mode.
//!
//! These tests exercise the web API layer with a fully constructed SharedState,
//! simulating how the application operates with fixture data (no network calls).

use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use skagit_flats::app::{SharedState, SourceStatus};
use skagit_flats::config::{Destination, DestinationsConfig};
use skagit_flats::domain::{
    DomainState, FerryStatus, RiverGauge, RoadStatus, TrailCondition, TripCriteria,
    WeatherObservation,
};
use skagit_flats::web::build_router;

/// Build a SharedState pre-populated with fixture domain data, simulating
/// a running application that has received data from all sources.
fn populated_state() -> Arc<SharedState> {
    let state = DomainState {
        weather: Some(WeatherObservation {
            temperature_f: 52.0,
            wind_speed_mph: 10.0,
            wind_direction: "SW".to_string(),
            sky_condition: "Mostly Cloudy".to_string(),
            precip_chance_pct: 20.0,
            observation_time: 1711648800,
        }),
        river: Some(RiverGauge {
            site_id: "12200500".to_string(),
            site_name: "Skagit River Near Mount Vernon, WA".to_string(),
            water_level_ft: 11.87,
            streamflow_cfs: 8750.0,
            timestamp: 1711648800,
        }),
        ferry: Some(FerryStatus {
            route: "Anacortes → Friday Harbor".to_string(),
            vessel_name: "MV Samish".to_string(),
            estimated_departures: vec![1711652400, 1711659600, 1711666800],
        }),
        trail: Some(TrailCondition {
            destination_name: "Cascade Pass".to_string(),
            suitability_summary: "[Caution] Snow above 5000ft".to_string(),
            last_updated: 1711648800,
        }),
        road: Some(RoadStatus {
            road_name: "SR-20 North Cascades Hwy".to_string(),
            status: "Seasonal closure".to_string(),
            affected_segment: "MP 134 to MP 171".to_string(),
        }),
    };

    let destinations = DestinationsConfig {
        destinations: vec![
            Destination {
                name: "Skagit Flats Loop".to_string(),
                criteria: TripCriteria {
                    min_temp_f: Some(40.0),
                    max_temp_f: Some(90.0),
                    max_river_level_ft: Some(15.0),
                    road_open_required: false,
                    ..Default::default()
                },
            },
            Destination {
                name: "North Cascades".to_string(),
                criteria: TripCriteria {
                    min_temp_f: Some(45.0),
                    road_open_required: true,
                    ..Default::default()
                },
            },
        ],
    };

    let sources = vec![
        SourceStatus {
            name: "noaa-weather".to_string(),
            enabled: true,
            last_fetch: Some(1711648800),
            last_error: None,
            next_fetch: Some(1711649100),
        },
        SourceStatus {
            name: "usgs-river".to_string(),
            enabled: true,
            last_fetch: Some(1711648800),
            last_error: None,
            next_fetch: Some(1711649100),
        },
        SourceStatus {
            name: "wsdot-ferries".to_string(),
            enabled: true,
            last_fetch: Some(1711648800),
            last_error: None,
            next_fetch: Some(1711648860),
        },
        SourceStatus {
            name: "trail-conditions".to_string(),
            enabled: true,
            last_fetch: Some(1711648800),
            last_error: None,
            next_fetch: Some(1711649700),
        },
        SourceStatus {
            name: "road-closures".to_string(),
            enabled: true,
            last_fetch: Some(1711648800),
            last_error: None,
            next_fetch: Some(1711650600),
        },
    ];

    // Pre-render the pixel buffer with current state and destinations.
    let panels = skagit_flats::presentation::build_panels_with_destinations(
        &state,
        &destinations.destinations,
    );
    let pixel_buffer = skagit_flats::render::render_panels(&panels, 800, 480);

    Arc::new(SharedState {
        pixel_buffer: RwLock::new(pixel_buffer),
        source_statuses: RwLock::new(sources),
        destinations_config: RwLock::new(destinations),
        domain_state: RwLock::new(state),
        destinations_path: "/tmp/skagit-integration-test-destinations.toml".into(),
        display_width: 800,
        display_height: 480,
    })
}

// --- Health endpoint ---

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = build_router(populated_state());
    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 100).await.unwrap();
    assert_eq!(&body[..], b"OK");
}

// --- Preview endpoint ---

#[tokio::test]
async fn preview_returns_valid_png_with_populated_state() {
    let app = build_router(populated_state());
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
    // PNG should be non-trivial (has rendered panels)
    assert!(body.len() > 100, "PNG too small: {} bytes", body.len());
}

// --- Sources endpoint ---

#[tokio::test]
async fn sources_returns_all_configured_sources() {
    let app = build_router(populated_state());
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
    assert_eq!(parsed.len(), 5);

    let names: Vec<&str> = parsed.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"noaa-weather"));
    assert!(names.contains(&"usgs-river"));
    assert!(names.contains(&"wsdot-ferries"));
    assert!(names.contains(&"trail-conditions"));
    assert!(names.contains(&"road-closures"));
}

// --- Destinations endpoint ---

#[tokio::test]
async fn destinations_returns_decisions_for_all_destinations() {
    let app = build_router(populated_state());
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
    assert_eq!(parsed.len(), 2);

    // Skagit Flats Loop: should be GO (52F > 40F min, river 11.87 < 15.0)
    assert_eq!(parsed[0]["name"], "Skagit Flats Loop");
    assert_eq!(parsed[0]["decision"]["decision"], "Go");

    // North Cascades: should be NO GO (road is not "open")
    assert_eq!(parsed[1]["name"], "North Cascades");
    assert_eq!(parsed[1]["decision"]["decision"], "NoGo");
}

// --- Destination CRUD ---

#[tokio::test]
async fn post_destination_then_get_includes_it() {
    let state = populated_state();

    // POST a new destination
    let app = build_router(Arc::clone(&state));
    let body = serde_json::json!({
        "name": "Baker Lake",
        "criteria": {
            "min_temp_f": 50.0,
            "max_river_level_ft": 10.0,
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

    // Verify it was added to state
    let dests = state.destinations_config.read().unwrap();
    assert_eq!(dests.destinations.len(), 3);
    assert!(dests.destinations.iter().any(|d| d.name == "Baker Lake"));
}

#[tokio::test]
async fn post_destination_updates_existing() {
    let state = populated_state();

    // Update existing "Skagit Flats Loop" with new criteria
    let app = build_router(Arc::clone(&state));
    let body = serde_json::json!({
        "name": "Skagit Flats Loop",
        "criteria": {
            "min_temp_f": 50.0,
            "max_temp_f": 80.0,
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

    // Should still have 2, not 3 (update, not create)
    let dests = state.destinations_config.read().unwrap();
    assert_eq!(dests.destinations.len(), 2);
    let updated = dests.destinations.iter().find(|d| d.name == "Skagit Flats Loop").unwrap();
    assert_eq!(updated.criteria.min_temp_f, Some(50.0));
    assert!(updated.criteria.road_open_required);
}

#[tokio::test]
async fn post_destination_empty_name_returns_400() {
    let app = build_router(populated_state());
    let body = serde_json::json!({
        "name": "  ",
        "criteria": {}
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
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_existing_destination() {
    let state = populated_state();
    let app = build_router(Arc::clone(&state));
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/destinations/Skagit%20Flats%20Loop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let dests = state.destinations_config.read().unwrap();
    assert_eq!(dests.destinations.len(), 1);
    assert_eq!(dests.destinations[0].name, "North Cascades");
}

#[tokio::test]
async fn delete_nonexistent_destination_returns_404() {
    let app = build_router(populated_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/destinations/DoesNotExist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- Source enable/disable ---

#[tokio::test]
async fn disable_and_reenable_source() {
    let state = populated_state();

    // Disable weather source
    let app = build_router(Arc::clone(&state));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sources/noaa-weather/disable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    {
        let statuses = state.source_statuses.read().unwrap();
        let weather = statuses.iter().find(|s| s.name == "noaa-weather").unwrap();
        assert!(!weather.enabled);
    }

    // Re-enable it
    let app = build_router(Arc::clone(&state));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sources/noaa-weather/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let statuses = state.source_statuses.read().unwrap();
    let weather = statuses.iter().find(|s| s.name == "noaa-weather").unwrap();
    assert!(weather.enabled);
}

// --- Index page ---

#[tokio::test]
async fn index_page_contains_all_sections() {
    let app = build_router(populated_state());
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap();
    let html = String::from_utf8_lossy(&body);

    // Must contain key sections
    assert!(html.contains("Skagit Flats Dashboard"), "missing title");
    assert!(html.contains("/preview"), "missing preview image");
    assert!(html.contains("Destinations"), "missing destinations section");
    assert!(html.contains("Sources"), "missing sources section");
    assert!(html.contains("Skagit Flats Loop"), "missing destination name");
    assert!(html.contains("noaa-weather"), "missing source name");
    // Check for GO/NO GO badges
    assert!(html.contains("GO"), "missing decision badge");
}

// --- Pixel buffer re-render after destination change ---

#[tokio::test]
async fn pixel_buffer_updates_after_destination_change() {
    let state = populated_state();

    // Capture initial pixel buffer
    let initial_pixels = {
        let buf = state.pixel_buffer.read().unwrap();
        buf.pixels.clone()
    };

    // Add a new destination (triggers re-render)
    let app = build_router(Arc::clone(&state));
    let body = serde_json::json!({
        "name": "New Dest",
        "criteria": {"min_temp_f": 60.0}
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

    // Pixel buffer should have changed (new panel added)
    let updated_pixels = {
        let buf = state.pixel_buffer.read().unwrap();
        buf.pixels.clone()
    };
    assert_ne!(initial_pixels, updated_pixels, "pixel buffer should change after adding destination");
}

// --- End-to-end render pipeline ---

#[test]
fn full_render_pipeline_fixture_data() {
    // Exercise the complete data flow: domain → presentation → render
    let state = DomainState {
        weather: Some(WeatherObservation {
            temperature_f: 55.0,
            wind_speed_mph: 8.0,
            wind_direction: "NW".to_string(),
            sky_condition: "Partly Cloudy".to_string(),
            precip_chance_pct: 15.0,
            observation_time: 1711648800,
        }),
        river: Some(RiverGauge {
            site_id: "12200500".to_string(),
            site_name: "Skagit River".to_string(),
            water_level_ft: 8.5,
            streamflow_cfs: 5000.0,
            timestamp: 1711648800,
        }),
        ferry: Some(FerryStatus {
            route: "Anacortes / SJI".to_string(),
            vessel_name: "MV Samish".to_string(),
            estimated_departures: vec![37800, 45000],
        }),
        trail: None,
        road: Some(RoadStatus {
            road_name: "SR-20".to_string(),
            status: "open".to_string(),
            affected_segment: String::new(),
        }),
    };

    let destinations = vec![Destination {
        name: "Test Trip".to_string(),
        criteria: TripCriteria {
            min_temp_f: Some(40.0),
            max_river_level_ft: Some(15.0),
            road_open_required: true,
            ..Default::default()
        },
    }];

    let panels = skagit_flats::presentation::build_panels_with_destinations(&state, &destinations);
    // Should have: weather, river, ferry, road, trip_decision = 5 panels
    assert_eq!(panels.len(), 5);

    let buf = skagit_flats::render::render_panels(&panels, 800, 480);
    assert_eq!(buf.width, 800);
    assert_eq!(buf.height, 480);
    // Should have some black pixels (rendered content)
    assert!(buf.pixels.iter().any(|&b| b != 0));

    // PNG should be valid
    let png = buf.to_png();
    assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
}
