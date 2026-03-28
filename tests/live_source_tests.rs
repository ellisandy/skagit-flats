//! Live source integration tests — verify real API endpoints respond correctly.
//!
//! These tests make actual network calls to external APIs (NOAA, USGS, WSDOT).
//! They are gated behind the SKAGIT_LIVE_TESTS=1 environment variable so CI
//! does not depend on external API availability.
//!
//! Run with:
//!   SKAGIT_LIVE_TESTS=1 cargo test --test live_source_tests
//!
//! Some sources require API keys:
//!   - WSDOT ferries/roads: set WSDOT_ACCESS_CODE
//!   - NPS trail conditions: set NPS_API_KEY

use skagit_flats::domain::DataPoint;
use skagit_flats::sources::Source;
use std::time::Duration;

fn live_tests_enabled() -> bool {
    std::env::var("SKAGIT_LIVE_TESTS")
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[test]
fn noaa_live_fetch() {
    if !live_tests_enabled() {
        eprintln!("skipping: SKAGIT_LIVE_TESTS not set");
        return;
    }

    let location = skagit_flats::config::LocationConfig {
        latitude: 48.4232,
        longitude: -122.3351,
        name: "Mount Vernon, WA".to_string(),
    };
    let source = skagit_flats::sources::noaa::NoaaSource::new(&location, 300);

    assert_eq!(source.name(), "noaa-weather");
    assert_eq!(source.refresh_interval(), Duration::from_secs(300));

    let result = source.fetch();
    match result {
        Ok(DataPoint::Weather(obs)) => {
            // Temperature should be in a plausible range (-40F to 130F)
            assert!(
                (-40.0..=130.0).contains(&obs.temperature_f),
                "temperature out of range: {}",
                obs.temperature_f
            );
            // Wind speed should be non-negative
            assert!(obs.wind_speed_mph >= 0.0);
            // Wind direction should be a compass abbreviation or "Calm"
            assert!(!obs.wind_direction.is_empty());
            // Sky condition should be non-empty
            assert!(!obs.sky_condition.is_empty());
            // Observation time should be recent (within last 24 hours)
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            assert!(
                obs.observation_time > now - 86400,
                "observation too old: {}",
                obs.observation_time
            );

            eprintln!(
                "NOAA live: {:.1}°F, {} at {:.0} mph, {}",
                obs.temperature_f, obs.wind_direction, obs.wind_speed_mph, obs.sky_condition
            );
        }
        Ok(other) => panic!("expected Weather, got {:?}", other),
        Err(e) => {
            eprintln!("NOAA fetch failed (may be API issue): {e}");
            // Don't panic — the API might be temporarily down
        }
    }
}

#[test]
fn usgs_live_fetch() {
    if !live_tests_enabled() {
        eprintln!("skipping: SKAGIT_LIVE_TESTS not set");
        return;
    }

    let source = skagit_flats::sources::usgs::UsgsSource::new("12200500", 300);

    assert_eq!(source.name(), "usgs-river");

    let result = source.fetch();
    match result {
        Ok(DataPoint::River(gauge)) => {
            assert_eq!(gauge.site_id, "12200500");
            assert!(!gauge.site_name.is_empty());
            // Water level should be in a plausible range (0-50 ft for Skagit)
            assert!(
                (0.0..=50.0).contains(&gauge.water_level_ft),
                "water level out of range: {}",
                gauge.water_level_ft
            );
            // Streamflow should be non-negative
            assert!(gauge.streamflow_cfs >= 0.0);
            // Timestamp should be recent
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            assert!(
                gauge.timestamp > now - 86400,
                "gauge reading too old: {}",
                gauge.timestamp
            );

            eprintln!(
                "USGS live: {} — {:.1} ft, {:.0} cfs",
                gauge.site_name, gauge.water_level_ft, gauge.streamflow_cfs
            );
        }
        Ok(other) => panic!("expected River, got {:?}", other),
        Err(e) => {
            eprintln!("USGS fetch failed (may be API issue): {e}");
        }
    }
}

#[test]
fn wsdot_ferries_live_fetch() {
    if !live_tests_enabled() {
        eprintln!("skipping: SKAGIT_LIVE_TESTS not set");
        return;
    }

    if std::env::var("WSDOT_ACCESS_CODE").is_err() {
        eprintln!("skipping: WSDOT_ACCESS_CODE not set");
        return;
    }

    let source = match skagit_flats::sources::wsdot::WsdotFerrySource::new(None, 60) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("WSDOT ferry source init failed: {e}");
            return;
        }
    };

    assert_eq!(source.name(), "wsdot-ferries");

    let result = source.fetch();
    match result {
        Ok(DataPoint::Ferry(status)) => {
            assert!(!status.route.is_empty());
            assert!(!status.vessel_name.is_empty());

            eprintln!(
                "WSDOT live: {} — {} ({} departures)",
                status.route,
                status.vessel_name,
                status.estimated_departures.len()
            );
        }
        Ok(other) => panic!("expected Ferry, got {:?}", other),
        Err(e) => {
            eprintln!("WSDOT ferry fetch failed (may be API issue): {e}");
        }
    }
}

#[test]
fn nps_trail_conditions_live_fetch() {
    if !live_tests_enabled() {
        eprintln!("skipping: SKAGIT_LIVE_TESTS not set");
        return;
    }

    if std::env::var("NPS_API_KEY").is_err() {
        eprintln!("skipping: NPS_API_KEY not set");
        return;
    }

    let source =
        match skagit_flats::sources::trail_conditions::TrailConditionsSource::new(None, 900) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("NPS trail source init failed: {e}");
                return;
            }
        };

    assert_eq!(source.name(), "trail-conditions");

    let result = source.fetch();
    match result {
        Ok(DataPoint::Trail(cond)) => {
            assert!(!cond.destination_name.is_empty());
            assert!(!cond.suitability_summary.is_empty());
            assert!(cond.last_updated > 0);

            eprintln!(
                "NPS live: {} — {}",
                cond.destination_name, cond.suitability_summary
            );
        }
        Ok(other) => panic!("expected Trail, got {:?}", other),
        Err(e) => {
            eprintln!("NPS trail fetch failed (may be API issue): {e}");
        }
    }
}

#[test]
fn wsdot_road_closures_live_fetch() {
    if !live_tests_enabled() {
        eprintln!("skipping: SKAGIT_LIVE_TESTS not set");
        return;
    }

    if std::env::var("WSDOT_ACCESS_CODE").is_err() {
        eprintln!("skipping: WSDOT_ACCESS_CODE not set");
        return;
    }

    let source =
        match skagit_flats::sources::road_closures::RoadClosuresSource::new(None, 1800) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("WSDOT road source init failed: {e}");
                return;
            }
        };

    assert_eq!(source.name(), "road-closures");

    let result = source.fetch();
    match result {
        Ok(DataPoint::Road(status)) => {
            assert!(!status.road_name.is_empty());
            assert!(!status.status.is_empty());

            eprintln!(
                "WSDOT road live: {} — {} ({})",
                status.road_name, status.status, status.affected_segment
            );
        }
        Ok(other) => panic!("expected Road, got {:?}", other),
        Err(e) => {
            eprintln!("WSDOT road fetch failed (may be API issue): {e}");
        }
    }
}
