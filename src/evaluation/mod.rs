use crate::config::Destination;
use crate::domain::{DomainState, TripDecision};

/// Evaluate a destination's go/no-go criteria against the current domain state.
///
/// This is pure logic — no I/O, no rendering. A `Go` result means all
/// configured criteria are satisfied. A `NoGo` result includes the list of
/// blocking reasons for display.
pub fn evaluate(destination: &Destination, state: &DomainState) -> TripDecision {
    let criteria = &destination.criteria;
    let mut reasons: Vec<String> = Vec::new();

    if let Some(weather) = &state.weather {
        if let Some(min) = criteria.min_temp_f {
            if weather.temperature_f < min {
                reasons.push(format!(
                    "Temperature {:.0}°F below minimum {:.0}°F",
                    weather.temperature_f, min
                ));
            }
        }
        if let Some(max) = criteria.max_temp_f {
            if weather.temperature_f > max {
                reasons.push(format!(
                    "Temperature {:.0}°F above maximum {:.0}°F",
                    weather.temperature_f, max
                ));
            }
        }
        if let Some(max_precip) = criteria.max_precip_chance_pct {
            if weather.precip_chance_pct > max_precip {
                reasons.push(format!(
                    "Precip chance {:.0}% exceeds limit {:.0}%",
                    weather.precip_chance_pct, max_precip
                ));
            }
        }
    }

    if let Some(river) = &state.river {
        if let Some(max_level) = criteria.max_river_level_ft {
            if river.water_level_ft > max_level {
                reasons.push(format!(
                    "River level {:.1}ft above limit {:.1}ft",
                    river.water_level_ft, max_level
                ));
            }
        }
        if let Some(max_flow) = criteria.max_river_flow_cfs {
            if river.streamflow_cfs > max_flow {
                reasons.push(format!(
                    "River flow {:.0}cfs exceeds limit {:.0}cfs",
                    river.streamflow_cfs, max_flow
                ));
            }
        }
    }

    if criteria.road_open_required {
        if let Some(road) = &state.road {
            if road.status != "open" {
                reasons.push(format!("{} is {}", road.road_name, road.status));
            }
        }
    }

    if reasons.is_empty() {
        TripDecision::Go
    } else {
        TripDecision::NoGo { reasons }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Destination;
    use crate::domain::{
        DomainState, RiverGauge, RoadStatus, TripCriteria, WeatherObservation,
    };

    fn default_criteria() -> TripCriteria {
        TripCriteria {
            min_temp_f: None,
            max_temp_f: None,
            max_precip_chance_pct: None,
            max_river_level_ft: None,
            max_river_flow_cfs: None,
            road_open_required: false,
        }
    }

    fn make_dest_with(criteria: TripCriteria) -> Destination {
        Destination {
            name: "Test".to_string(),
            criteria,
        }
    }

    fn make_dest(min_temp: Option<f32>, max_temp: Option<f32>) -> Destination {
        make_dest_with(TripCriteria {
            min_temp_f: min_temp,
            max_temp_f: max_temp,
            ..default_criteria()
        })
    }

    fn weather_obs(temp: f32) -> WeatherObservation {
        WeatherObservation {
            temperature_f: temp,
            wind_speed_mph: 5.0,
            wind_direction: "N".to_string(),
            sky_condition: "Clear".to_string(),
            precip_chance_pct: 0.0,
            observation_time: 0,
        }
    }

    fn weather_state(temp: f32) -> DomainState {
        DomainState {
            weather: Some(weather_obs(temp)),
            ..Default::default()
        }
    }

    fn river_gauge(level: f32, flow: f32) -> RiverGauge {
        RiverGauge {
            site_id: "12200500".to_string(),
            water_level_ft: level,
            streamflow_cfs: flow,
            timestamp: 0,
        }
    }

    fn road_status(status: &str) -> RoadStatus {
        RoadStatus {
            road_name: "SR-20".to_string(),
            status: status.to_string(),
            affected_segment: "Newhalem to Rainy Pass".to_string(),
        }
    }

    // --- Temperature criteria ---

    #[test]
    fn go_when_all_criteria_met() {
        let dest = make_dest(Some(40.0), Some(90.0));
        let state = weather_state(65.0);
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    #[test]
    fn no_go_when_too_cold() {
        let dest = make_dest(Some(50.0), None);
        let state = weather_state(40.0);
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("below minimum"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn no_go_when_too_hot() {
        let dest = make_dest(None, Some(80.0));
        let state = weather_state(85.0);
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("above maximum"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn go_at_exact_min_temp_boundary() {
        let dest = make_dest(Some(50.0), None);
        let state = weather_state(50.0);
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    #[test]
    fn go_at_exact_max_temp_boundary() {
        let dest = make_dest(None, Some(80.0));
        let state = weather_state(80.0);
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    // --- Precipitation criteria ---

    #[test]
    fn no_go_when_precip_too_high() {
        let dest = make_dest_with(TripCriteria {
            max_precip_chance_pct: Some(30.0),
            ..default_criteria()
        });
        let mut state = DomainState::default();
        let mut obs = weather_obs(65.0);
        obs.precip_chance_pct = 80.0;
        state.weather = Some(obs);
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("Precip chance"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn go_at_exact_precip_boundary() {
        let dest = make_dest_with(TripCriteria {
            max_precip_chance_pct: Some(50.0),
            ..default_criteria()
        });
        let mut state = DomainState::default();
        let mut obs = weather_obs(65.0);
        obs.precip_chance_pct = 50.0;
        state.weather = Some(obs);
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    // --- River level criteria ---

    #[test]
    fn no_go_when_river_too_high() {
        let dest = make_dest_with(TripCriteria {
            max_river_level_ft: Some(12.0),
            ..default_criteria()
        });
        let state = DomainState {
            river: Some(river_gauge(14.5, 5000.0)),
            ..Default::default()
        };
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("River level"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn go_at_exact_river_level_boundary() {
        let dest = make_dest_with(TripCriteria {
            max_river_level_ft: Some(12.0),
            ..default_criteria()
        });
        let state = DomainState {
            river: Some(river_gauge(12.0, 5000.0)),
            ..Default::default()
        };
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    // --- River flow criteria ---

    #[test]
    fn no_go_when_river_flow_too_high() {
        let dest = make_dest_with(TripCriteria {
            max_river_flow_cfs: Some(10000.0),
            ..default_criteria()
        });
        let state = DomainState {
            river: Some(river_gauge(8.0, 15000.0)),
            ..Default::default()
        };
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("River flow"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn go_at_exact_river_flow_boundary() {
        let dest = make_dest_with(TripCriteria {
            max_river_flow_cfs: Some(10000.0),
            ..default_criteria()
        });
        let state = DomainState {
            river: Some(river_gauge(8.0, 10000.0)),
            ..Default::default()
        };
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    // --- Road criteria ---

    #[test]
    fn no_go_when_road_closed() {
        let dest = make_dest_with(TripCriteria {
            road_open_required: true,
            ..default_criteria()
        });
        let state = DomainState {
            road: Some(road_status("closed")),
            ..Default::default()
        };
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("SR-20"));
                assert!(reasons[0].contains("closed"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn go_when_road_open() {
        let dest = make_dest_with(TripCriteria {
            road_open_required: true,
            ..default_criteria()
        });
        let state = DomainState {
            road: Some(road_status("open")),
            ..Default::default()
        };
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    #[test]
    fn go_when_road_not_required() {
        let dest = make_dest_with(TripCriteria {
            road_open_required: false,
            ..default_criteria()
        });
        let state = DomainState {
            road: Some(road_status("closed")),
            ..Default::default()
        };
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    // --- Missing data ---

    #[test]
    fn go_when_no_weather_data() {
        let dest = make_dest(Some(50.0), None);
        let state = DomainState::default();
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    #[test]
    fn go_when_no_river_data() {
        let dest = make_dest_with(TripCriteria {
            max_river_level_ft: Some(12.0),
            max_river_flow_cfs: Some(10000.0),
            ..default_criteria()
        });
        let state = DomainState::default();
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    #[test]
    fn go_when_no_road_data_but_required() {
        let dest = make_dest_with(TripCriteria {
            road_open_required: true,
            ..default_criteria()
        });
        let state = DomainState::default();
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }

    // --- Multiple blocking reasons ---

    #[test]
    fn multiple_reasons_when_several_criteria_fail() {
        let dest = make_dest_with(TripCriteria {
            min_temp_f: Some(50.0),
            max_river_level_ft: Some(12.0),
            road_open_required: true,
            ..default_criteria()
        });
        let state = DomainState {
            weather: Some(weather_obs(30.0)),
            river: Some(river_gauge(15.0, 5000.0)),
            road: Some(road_status("closed")),
            ..Default::default()
        };
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                assert_eq!(reasons.len(), 3);
                assert!(reasons[0].contains("Temperature"));
                assert!(reasons[1].contains("River level"));
                assert!(reasons[2].contains("SR-20"));
            }
            _ => panic!("expected NoGo"),
        }
    }

    #[test]
    fn all_criteria_fail_simultaneously() {
        let mut obs = weather_obs(30.0);
        obs.precip_chance_pct = 90.0;
        let dest = make_dest_with(TripCriteria {
            min_temp_f: Some(50.0),
            max_temp_f: Some(80.0),
            max_precip_chance_pct: Some(20.0),
            max_river_level_ft: Some(10.0),
            max_river_flow_cfs: Some(5000.0),
            road_open_required: true,
        });
        let state = DomainState {
            weather: Some(obs),
            river: Some(river_gauge(15.0, 20000.0)),
            road: Some(road_status("restricted")),
            ..Default::default()
        };
        match evaluate(&dest, &state) {
            TripDecision::NoGo { reasons } => {
                // temp below min, precip too high, river level, river flow, road
                assert_eq!(reasons.len(), 5);
            }
            _ => panic!("expected NoGo"),
        }
    }

    // --- No criteria configured ---

    #[test]
    fn go_when_no_criteria_configured() {
        let dest = make_dest_with(default_criteria());
        let state = DomainState {
            weather: Some(weather_obs(100.0)),
            river: Some(river_gauge(50.0, 100000.0)),
            road: Some(road_status("closed")),
            ..Default::default()
        };
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }
}
