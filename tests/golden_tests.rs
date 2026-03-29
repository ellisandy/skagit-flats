use skagit_flats::presentation::{
    ContextContent, DataContent, DisplayLayout, FerryContent, HeaderContent, HeroContent,
    HeroDecision, RiverContent, RoadContent, Sparkline, TrailContent, TrendArrow, WeatherContent,
    WeatherIcon,
};
use skagit_flats::render;
use std::path::Path;

/// Realistic DisplayLayout used for the primary golden render test.
fn test_layout() -> DisplayLayout {
    DisplayLayout {
        header: HeaderContent {
            app_name: "SKAGIT FLATS".to_string(),
            river_site: Some("Skagit River Near Mount Vernon".to_string()),
            last_updated: Some("14:32".to_string()),
        },
        hero: HeroContent {
            decision: HeroDecision::Go {
                destination: "Skagit Flats Loop".to_string(),
            },
            weather: Some(WeatherContent {
                icon: WeatherIcon::MostlyCloudy,
                temperature_f: 52.0,
                sky_condition: "Mostly Cloudy".to_string(),
                wind_dir: "SW".to_string(),
                wind_speed_mph: 10.0,
                precip_chance_pct: 20.0,
            }),
        },
        data: DataContent {
            river: Some(RiverContent {
                site_name: "Skagit River Near Mount Vernon".to_string(),
                level_ft: 11.9,
                flow_cfs: 8750.0,
                trend: TrendArrow::Rising,
                sparkline: Some(Sparkline {
                    values: vec![
                        9.8, 10.2, 10.5, 10.9, 11.1, 11.3, 11.5, 11.7, 11.9,
                    ],
                    threshold: Some(15.0),
                }),
            }),
            ferry: Some(FerryContent {
                route: "Anacortes / San Juan Islands".to_string(),
                vessel_name: "MV Samish".to_string(),
                departures: vec!["14:30".to_string(), "16:00".to_string(), "18:30".to_string()],
            }),
        },
        context: ContextContent {
            trail: Some(TrailContent {
                name: "Cascade Pass Trail".to_string(),
                condition: "Snow above 5000ft".to_string(),
            }),
            road: Some(RoadContent {
                name: "SR-20".to_string(),
                status: "open".to_string(),
                segment: "All sections passable".to_string(),
            }),
        },
    }
}

/// NO GO layout for testing the inverted/alert rendering path.
fn nogo_layout() -> DisplayLayout {
    DisplayLayout {
        header: HeaderContent {
            app_name: "SKAGIT FLATS".to_string(),
            river_site: None,
            last_updated: Some("08:00".to_string()),
        },
        hero: HeroContent {
            decision: HeroDecision::NoGo {
                destination: "North Cascades".to_string(),
                reasons: vec![
                    "River too high (11.9 ft > 10 ft limit)".to_string(),
                    "SR-20 closed at Newhalem".to_string(),
                ],
            },
            weather: None,
        },
        data: DataContent { river: None, ferry: None },
        context: ContextContent {
            trail: None,
            road: Some(RoadContent {
                name: "SR-20 North Cascades Hwy".to_string(),
                status: "closed".to_string(),
                segment: "Newhalem to Rainy Pass".to_string(),
            }),
        },
    }
}

fn write_or_compare(golden_name: &str, png_data: &[u8]) {
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(golden_name);

    if std::env::var("GENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden_path, png_data).expect("failed to write golden file");
        eprintln!("Golden file written to {:?}", golden_path);
        return;
    }

    let expected = std::fs::read(&golden_path).unwrap_or_else(|e| {
        panic!(
            "Golden file not found at {:?}: {}. Run with GENERATE_GOLDEN=1 to create it.",
            golden_path, e
        )
    });

    assert_eq!(
        png_data,
        expected.as_slice(),
        "Render output does not match {}. Run with GENERATE_GOLDEN=1 to regenerate.",
        golden_name
    );
}

#[test]
fn golden_render_full_layout() {
    let layout = test_layout();
    let buf = render::render_display(&layout);
    let png_data = buf.to_png();
    write_or_compare("four_panels.png", &png_data);
}

#[test]
fn golden_render_nogo_layout() {
    let layout = nogo_layout();
    let buf = render::render_display(&layout);
    let png_data = buf.to_png();
    write_or_compare("single_panel.png", &png_data);
}
