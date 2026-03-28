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
    use crate::domain::{DomainState, TripCriteria, WeatherObservation};

    fn make_dest(min_temp: Option<f32>, max_temp: Option<f32>) -> Destination {
        Destination {
            name: "Test".to_string(),
            criteria: TripCriteria {
                min_temp_f: min_temp,
                max_temp_f: max_temp,
                max_precip_in: None,
                max_river_level_ft: None,
                road_open_required: false,
            },
        }
    }

    fn weather_state(temp: f32) -> DomainState {
        DomainState {
            weather: Some(WeatherObservation {
                temperature_f: temp,
                wind_speed_mph: 5.0,
                wind_direction: "N".to_string(),
                sky_condition: "Clear".to_string(),
                observation_time: 0,
            }),
            ..Default::default()
        }
    }

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
        assert!(matches!(evaluate(&dest, &state), TripDecision::NoGo { .. }));
    }

    #[test]
    fn no_go_when_too_hot() {
        let dest = make_dest(None, Some(80.0));
        let state = weather_state(85.0);
        assert!(matches!(evaluate(&dest, &state), TripDecision::NoGo { .. }));
    }

    #[test]
    fn go_when_no_weather_data() {
        // Missing data does not trigger a NoGo — the display shows stale data instead.
        let dest = make_dest(Some(50.0), None);
        let state = DomainState::default();
        assert!(matches!(evaluate(&dest, &state), TripDecision::Go));
    }
}
