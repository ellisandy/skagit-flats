use serde::{Deserialize, Serialize};

/// Current weather at a NOAA observation station.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherObservation {
    pub temperature_f: f32,
    pub wind_speed_mph: f32,
    pub wind_direction: String,
    pub sky_condition: String,
    /// Probability of precipitation, 0–100.
    #[serde(default)]
    pub precip_chance_pct: f32,
    /// Unix timestamp of the observation.
    pub observation_time: u64,
}

/// River gauge reading from a USGS NWIS site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiverGauge {
    pub site_id: String,
    pub site_name: String,
    pub water_level_ft: f32,
    pub streamflow_cfs: f32,
    /// Unix timestamp of the reading.
    pub timestamp: u64,
}

/// Ferry vessel status and upcoming departures for a WSDOT route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FerryStatus {
    pub route: String,
    pub vessel_name: String,
    /// Estimated departure times as Unix timestamps.
    pub estimated_departures: Vec<u64>,
}

/// Trail or campsite suitability from external sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailCondition {
    pub destination_name: String,
    pub suitability_summary: String,
    /// Unix timestamp of the last update.
    pub last_updated: u64,
}

/// Road closure or restriction status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadStatus {
    pub road_name: String,
    /// Human-readable status: "open", "closed", "restricted", etc.
    pub status: String,
    pub affected_segment: String,
}

/// Per-destination go/no-go thresholds, loaded from destinations.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TripCriteria {
    pub min_temp_f: Option<f32>,
    pub max_temp_f: Option<f32>,
    pub max_precip_chance_pct: Option<f32>,
    pub max_river_level_ft: Option<f32>,
    pub max_river_flow_cfs: Option<f32>,
    #[serde(default)]
    pub road_open_required: bool,
}

/// Result of evaluating a destination against current conditions.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "decision")]
pub enum TripDecision {
    Go,
    NoGo { reasons: Vec<String> },
}

/// All possible outputs from a data source.
#[derive(Debug, Clone)]
pub enum DataPoint {
    Weather(WeatherObservation),
    River(RiverGauge),
    Ferry(FerryStatus),
    Trail(TrailCondition),
    Road(RoadStatus),
}

/// Snapshot of the most recent value from every source.
#[derive(Debug, Clone, Default)]
pub struct DomainState {
    pub weather: Option<WeatherObservation>,
    pub river: Option<RiverGauge>,
    pub ferry: Option<FerryStatus>,
    pub trail: Option<TrailCondition>,
    pub road: Option<RoadStatus>,
}

impl DomainState {
    /// Apply an incoming DataPoint, replacing the stored value for its type.
    pub fn apply(&mut self, point: DataPoint) {
        match point {
            DataPoint::Weather(v) => self.weather = Some(v),
            DataPoint::River(v) => self.river = Some(v),
            DataPoint::Ferry(v) => self.ferry = Some(v),
            DataPoint::Trail(v) => self.trail = Some(v),
            DataPoint::Road(v) => self.road = Some(v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_weather() -> WeatherObservation {
        WeatherObservation {
            temperature_f: 55.0,
            wind_speed_mph: 8.0,
            wind_direction: "NW".to_string(),
            sky_condition: "Clear".to_string(),
            precip_chance_pct: 10.0,
            observation_time: 1000,
        }
    }

    fn sample_river() -> RiverGauge {
        RiverGauge {
            site_id: "12200500".to_string(),
            site_name: "Skagit River".to_string(),
            water_level_ft: 8.5,
            streamflow_cfs: 5000.0,
            timestamp: 2000,
        }
    }

    fn sample_ferry() -> FerryStatus {
        FerryStatus {
            route: "Anacortes / San Juan Islands".to_string(),
            vessel_name: "MV Samish".to_string(),
            estimated_departures: vec![3000, 6000],
        }
    }

    fn sample_trail() -> TrailCondition {
        TrailCondition {
            destination_name: "Cascade Pass".to_string(),
            suitability_summary: "Snow above 5000ft".to_string(),
            last_updated: 4000,
        }
    }

    fn sample_road() -> RoadStatus {
        RoadStatus {
            road_name: "SR-20".to_string(),
            status: "closed".to_string(),
            affected_segment: "Newhalem to Rainy Pass".to_string(),
        }
    }

    #[test]
    fn default_domain_state_is_all_none() {
        let state = DomainState::default();
        assert!(state.weather.is_none());
        assert!(state.river.is_none());
        assert!(state.ferry.is_none());
        assert!(state.trail.is_none());
        assert!(state.road.is_none());
    }

    #[test]
    fn apply_weather_sets_weather() {
        let mut state = DomainState::default();
        state.apply(DataPoint::Weather(sample_weather()));
        assert!(state.weather.is_some());
        assert_eq!(state.weather.as_ref().unwrap().temperature_f, 55.0);
        assert!(state.river.is_none());
    }

    #[test]
    fn apply_river_sets_river() {
        let mut state = DomainState::default();
        state.apply(DataPoint::River(sample_river()));
        assert!(state.river.is_some());
        assert_eq!(state.river.as_ref().unwrap().site_id, "12200500");
    }

    #[test]
    fn apply_ferry_sets_ferry() {
        let mut state = DomainState::default();
        state.apply(DataPoint::Ferry(sample_ferry()));
        assert!(state.ferry.is_some());
        assert_eq!(state.ferry.as_ref().unwrap().vessel_name, "MV Samish");
    }

    #[test]
    fn apply_trail_sets_trail() {
        let mut state = DomainState::default();
        state.apply(DataPoint::Trail(sample_trail()));
        assert!(state.trail.is_some());
        assert_eq!(state.trail.as_ref().unwrap().destination_name, "Cascade Pass");
    }

    #[test]
    fn apply_road_sets_road() {
        let mut state = DomainState::default();
        state.apply(DataPoint::Road(sample_road()));
        assert!(state.road.is_some());
        assert_eq!(state.road.as_ref().unwrap().status, "closed");
    }

    #[test]
    fn apply_replaces_existing_value() {
        let mut state = DomainState::default();
        state.apply(DataPoint::Weather(sample_weather()));
        assert_eq!(state.weather.as_ref().unwrap().temperature_f, 55.0);

        let mut updated = sample_weather();
        updated.temperature_f = 72.0;
        state.apply(DataPoint::Weather(updated));
        assert_eq!(state.weather.as_ref().unwrap().temperature_f, 72.0);
    }

    #[test]
    fn apply_all_types_populates_full_state() {
        let mut state = DomainState::default();
        state.apply(DataPoint::Weather(sample_weather()));
        state.apply(DataPoint::River(sample_river()));
        state.apply(DataPoint::Ferry(sample_ferry()));
        state.apply(DataPoint::Trail(sample_trail()));
        state.apply(DataPoint::Road(sample_road()));

        assert!(state.weather.is_some());
        assert!(state.river.is_some());
        assert!(state.ferry.is_some());
        assert!(state.trail.is_some());
        assert!(state.road.is_some());
    }

    #[test]
    fn weather_observation_serialization_roundtrip() {
        let obs = sample_weather();
        let json = serde_json::to_string(&obs).unwrap();
        let parsed: WeatherObservation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.temperature_f, obs.temperature_f);
        assert_eq!(parsed.wind_direction, obs.wind_direction);
        assert_eq!(parsed.precip_chance_pct, obs.precip_chance_pct);
    }

    #[test]
    fn river_gauge_serialization_roundtrip() {
        let gauge = sample_river();
        let json = serde_json::to_string(&gauge).unwrap();
        let parsed: RiverGauge = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.site_id, gauge.site_id);
        assert_eq!(parsed.water_level_ft, gauge.water_level_ft);
    }

    #[test]
    fn ferry_status_serialization_roundtrip() {
        let status = sample_ferry();
        let json = serde_json::to_string(&status).unwrap();
        let parsed: FerryStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.vessel_name, status.vessel_name);
        assert_eq!(parsed.estimated_departures.len(), 2);
    }

    #[test]
    fn trip_decision_go_serialization() {
        let decision = TripDecision::Go;
        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("\"decision\":\"Go\""));
    }

    #[test]
    fn trip_decision_nogo_serialization() {
        let decision = TripDecision::NoGo {
            reasons: vec!["Too cold".to_string()],
        };
        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("\"decision\":\"NoGo\""));
        assert!(json.contains("Too cold"));
    }

    #[test]
    fn trip_criteria_default() {
        let criteria = TripCriteria::default();
        assert!(criteria.min_temp_f.is_none());
        assert!(criteria.max_temp_f.is_none());
        assert!(criteria.max_precip_chance_pct.is_none());
        assert!(criteria.max_river_level_ft.is_none());
        assert!(criteria.max_river_flow_cfs.is_none());
        assert!(!criteria.road_open_required);
    }

    #[test]
    fn precip_chance_defaults_to_zero() {
        let json = r#"{
            "temperature_f": 50.0,
            "wind_speed_mph": 5.0,
            "wind_direction": "N",
            "sky_condition": "Clear",
            "observation_time": 0
        }"#;
        let obs: WeatherObservation = serde_json::from_str(json).unwrap();
        assert_eq!(obs.precip_chance_pct, 0.0);
    }
}
