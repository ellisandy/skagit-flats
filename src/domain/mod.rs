use serde::{Deserialize, Serialize};

/// Current weather at a NOAA observation station.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherObservation {
    pub temperature_f: f32,
    pub wind_speed_mph: f32,
    pub wind_direction: String,
    pub sky_condition: String,
    /// Unix timestamp of the observation.
    pub observation_time: u64,
}

/// River gauge reading from a USGS NWIS site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiverGauge {
    pub site_id: String,
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
    pub max_precip_in: Option<f32>,
    pub max_river_level_ft: Option<f32>,
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
