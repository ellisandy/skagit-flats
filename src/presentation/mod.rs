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
    let title = if gauge.site_name.is_empty() {
        "River Gauge".to_string()
    } else {
        gauge.site_name.clone()
    };
    Panel::new(title)
        .with_row(format!("{:.1} ft", gauge.water_level_ft))
        .with_row(format!("{:.0} cfs", gauge.streamflow_cfs))
        .with_row(fmt_time(gauge.timestamp))
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
    build_panels_with_destinations(state, &[])
}

/// Build all panels from the current domain state, including trip decision
/// panels for each configured destination.
pub fn build_panels_with_destinations(
    state: &DomainState,
    destinations: &[crate::config::Destination],
) -> Vec<Panel> {
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
    for dest in destinations {
        let decision = crate::evaluation::evaluate(dest, state);
        panels.push(format_trip_decision(&dest.name, &decision));
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
    use crate::config::Destination;
    use crate::domain::{
        FerryStatus, RiverGauge, RoadStatus, TrailCondition, TripCriteria, WeatherObservation,
    };

    fn sample_weather() -> WeatherObservation {
        WeatherObservation {
            temperature_f: 52.0,
            wind_speed_mph: 10.0,
            wind_direction: "SW".to_string(),
            sky_condition: "Mostly Cloudy".to_string(),
            precip_chance_pct: 0.0,
            observation_time: 0,
        }
    }

    fn sample_river() -> RiverGauge {
        RiverGauge {
            site_id: "12200500".to_string(),
            site_name: "Skagit River Near Mount Vernon, WA".to_string(),
            water_level_ft: 11.87,
            streamflow_cfs: 8750.0,
            timestamp: 46800, // 13:00 UTC
        }
    }

    fn sample_ferry() -> FerryStatus {
        FerryStatus {
            route: "Anacortes / San Juan Islands".to_string(),
            vessel_name: "MV Samish".to_string(),
            estimated_departures: vec![37800, 45000, 52200], // 10:30, 12:30, 14:30
        }
    }

    fn sample_trail() -> TrailCondition {
        TrailCondition {
            destination_name: "Cascade Pass Trail".to_string(),
            suitability_summary: "Snow above 5000ft".to_string(),
            last_updated: 0,
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
    fn weather_panel_has_expected_rows() {
        let panel = format_weather(&sample_weather());
        assert_eq!(panel.title, "Weather");
        assert_eq!(panel.rows.len(), 2);
        assert!(panel.rows[0].contains("52°F"));
        assert!(panel.rows[0].contains("Mostly Cloudy"));
        assert!(panel.rows[1].contains("SW"));
        assert!(panel.rows[1].contains("10 mph"));
    }

    #[test]
    fn river_panel_shows_level_and_flow() {
        let panel = format_river(&sample_river());
        assert!(panel.title.contains("Skagit River"));
        assert_eq!(panel.rows.len(), 3);
        assert!(panel.rows[0].contains("11.9 ft"));
        assert!(panel.rows[1].contains("8750 cfs"));
        // Third row is the formatted timestamp
        assert!(panel.rows[2].contains(":"));
    }

    #[test]
    fn river_panel_empty_name_uses_default_title() {
        let mut gauge = sample_river();
        gauge.site_name = String::new();
        let panel = format_river(&gauge);
        assert_eq!(panel.title, "River Gauge");
    }

    #[test]
    fn ferry_panel_shows_vessel_and_departures() {
        let panel = format_ferry(&sample_ferry());
        assert!(panel.title.contains("Ferry"));
        assert!(panel.title.contains("Anacortes"));
        assert_eq!(panel.rows[0], "MV Samish");
        // Should show up to 3 departure times
        assert!(panel.rows.len() >= 2);
        assert!(panel.rows[1].starts_with("Departs"));
    }

    #[test]
    fn ferry_panel_limits_to_three_departures() {
        let mut status = sample_ferry();
        status.estimated_departures = vec![1000, 2000, 3000, 4000, 5000];
        let panel = format_ferry(&status);
        // 1 vessel name row + 3 departure rows = 4 total
        assert_eq!(panel.rows.len(), 4);
    }

    #[test]
    fn trail_panel_shows_condition() {
        let panel = format_trail(&sample_trail());
        assert_eq!(panel.title, "Cascade Pass Trail");
        assert_eq!(panel.rows.len(), 1);
        assert!(panel.rows[0].contains("Snow above 5000ft"));
    }

    #[test]
    fn road_panel_shows_status_and_segment() {
        let panel = format_road(&sample_road());
        assert_eq!(panel.title, "SR-20");
        assert_eq!(panel.rows.len(), 1);
        assert!(panel.rows[0].contains("CLOSED"));
        assert!(panel.rows[0].contains("Newhalem to Rainy Pass"));
    }

    #[test]
    fn trip_decision_go_panel() {
        let panel = format_trip_decision("Test Dest", &TripDecision::Go);
        assert_eq!(panel.title, "Test Dest");
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

    #[test]
    fn trip_decision_nogo_multiple_reasons() {
        let decision = TripDecision::NoGo {
            reasons: vec![
                "Too cold".to_string(),
                "River too high".to_string(),
                "Road closed".to_string(),
            ],
        };
        let panel = format_trip_decision("Test", &decision);
        // 1 "NO GO" + 3 reason rows
        assert_eq!(panel.rows.len(), 4);
        assert!(panel.rows[1].starts_with('•'));
        assert!(panel.rows[2].starts_with('•'));
        assert!(panel.rows[3].starts_with('•'));
    }

    #[test]
    fn build_panels_empty_state() {
        let state = DomainState::default();
        let panels = build_panels(&state);
        assert!(panels.is_empty());
    }

    #[test]
    fn build_panels_weather_only() {
        let state = DomainState {
            weather: Some(sample_weather()),
            ..Default::default()
        };
        let panels = build_panels(&state);
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].title, "Weather");
    }

    #[test]
    fn build_panels_all_sources() {
        let state = DomainState {
            weather: Some(sample_weather()),
            river: Some(sample_river()),
            ferry: Some(sample_ferry()),
            trail: Some(sample_trail()),
            road: Some(sample_road()),
        };
        let panels = build_panels(&state);
        assert_eq!(panels.len(), 5);
    }

    #[test]
    fn build_panels_with_destinations_adds_trip_panels() {
        let state = DomainState {
            weather: Some(sample_weather()),
            ..Default::default()
        };
        let destinations = vec![
            Destination {
                name: "Skagit Loop".to_string(),
                criteria: TripCriteria {
                    min_temp_f: Some(40.0),
                    ..Default::default()
                },
            },
            Destination {
                name: "Baker Lake".to_string(),
                criteria: TripCriteria {
                    max_temp_f: Some(90.0),
                    ..Default::default()
                },
            },
        ];
        let panels = build_panels_with_destinations(&state, &destinations);
        // 1 weather panel + 2 destination panels
        assert_eq!(panels.len(), 3);
        assert_eq!(panels[1].title, "Skagit Loop");
        assert_eq!(panels[2].title, "Baker Lake");
    }

    #[test]
    fn panel_builder_pattern() {
        let panel = Panel::new("Title")
            .with_row("Row 1")
            .with_row("Row 2")
            .with_row("Row 3");
        assert_eq!(panel.title, "Title");
        assert_eq!(panel.rows.len(), 3);
    }

    #[test]
    fn fmt_time_formats_correctly() {
        // 13:00 UTC = 46800 seconds into the day
        assert_eq!(fmt_time(46800), "13:00");
        // 00:00
        assert_eq!(fmt_time(0), "00:00");
        // 23:59
        assert_eq!(fmt_time(86340), "23:59");
    }
}
