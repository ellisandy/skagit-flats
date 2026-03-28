use crate::domain::{
    DomainState, FerryStatus, RiverGauge, RoadStatus, TrailCondition, TripDecision,
    WeatherObservation,
};

/// A rendered panel: a title and zero or more text rows.
///
/// Panels have no knowledge of pixels, fonts, or geometry — that is render's job.
#[derive(Debug, Clone)]
pub struct Panel {
    pub title: String,
    pub rows: Vec<String>,
}

impl Panel {
    pub fn new(title: impl Into<String>) -> Self {
        Panel {
            title: title.into(),
            rows: Vec::new(),
        }
    }

    pub fn with_row(mut self, row: impl Into<String>) -> Self {
        self.rows.push(row.into());
        self
    }
}

/// Format a WeatherObservation into a Panel.
pub fn format_weather(obs: &WeatherObservation) -> Panel {
    Panel::new("Weather")
        .with_row(format!("{:.0}°F  {}", obs.temperature_f, obs.sky_condition))
        .with_row(format!(
            "Wind {} at {:.0} mph",
            obs.wind_direction, obs.wind_speed_mph
        ))
}

/// Format a RiverGauge reading into a Panel.
pub fn format_river(gauge: &RiverGauge) -> Panel {
    Panel::new("Skagit River")
        .with_row(format!("{:.1} ft", gauge.water_level_ft))
        .with_row(format!("{:.0} cfs", gauge.streamflow_cfs))
}

/// Format FerryStatus into a Panel.
pub fn format_ferry(status: &FerryStatus) -> Panel {
    let mut panel = Panel::new(format!("Ferry — {}", status.route));
    panel.rows.push(status.vessel_name.clone());
    for ts in status.estimated_departures.iter().take(3) {
        panel.rows.push(format!("Departs {}", fmt_time(*ts)));
    }
    panel
}

/// Format TrailCondition into a Panel.
pub fn format_trail(cond: &TrailCondition) -> Panel {
    Panel::new(cond.destination_name.clone())
        .with_row(cond.suitability_summary.clone())
}

/// Format RoadStatus into a Panel.
pub fn format_road(road: &RoadStatus) -> Panel {
    Panel::new(road.road_name.clone())
        .with_row(format!("{} — {}", road.status.to_uppercase(), road.affected_segment))
}

/// Format a TripDecision into a Panel.
pub fn format_trip_decision(destination: &str, decision: &TripDecision) -> Panel {
    match decision {
        TripDecision::Go => Panel::new(destination).with_row("GO".to_string()),
        TripDecision::NoGo { reasons } => {
            let mut panel = Panel::new(destination).with_row("NO GO".to_string());
            for r in reasons {
                panel.rows.push(format!("• {r}"));
            }
            panel
        }
    }
}

/// Build all panels from the current domain state.
pub fn build_panels(state: &DomainState) -> Vec<Panel> {
    let mut panels = Vec::new();
    if let Some(w) = &state.weather {
        panels.push(format_weather(w));
    }
    if let Some(r) = &state.river {
        panels.push(format_river(r));
    }
    if let Some(f) = &state.ferry {
        panels.push(format_ferry(f));
    }
    if let Some(t) = &state.trail {
        panels.push(format_trail(t));
    }
    if let Some(r) = &state.road {
        panels.push(format_road(r));
    }
    panels
}

/// Format a Unix timestamp as HH:MM (local time, best effort).
fn fmt_time(ts: u64) -> String {
    // Minimal time formatting without a time library.
    let secs = ts % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WeatherObservation;

    #[test]
    fn weather_panel_has_expected_rows() {
        let obs = WeatherObservation {
            temperature_f: 52.0,
            wind_speed_mph: 10.0,
            wind_direction: "SW".to_string(),
            sky_condition: "Mostly Cloudy".to_string(),
            observation_time: 0,
        };
        let panel = format_weather(&obs);
        assert_eq!(panel.title, "Weather");
        assert_eq!(panel.rows.len(), 2);
        assert!(panel.rows[0].contains("52°F"));
        assert!(panel.rows[1].contains("SW"));
    }

    #[test]
    fn trip_decision_go_panel() {
        let panel = format_trip_decision("Test Dest", &TripDecision::Go);
        assert_eq!(panel.rows[0], "GO");
    }

    #[test]
    fn trip_decision_no_go_panel_lists_reasons() {
        let decision = TripDecision::NoGo {
            reasons: vec!["Too cold".to_string()],
        };
        let panel = format_trip_decision("Test Dest", &decision);
        assert_eq!(panel.rows[0], "NO GO");
        assert!(panel.rows[1].contains("Too cold"));
    }
}
