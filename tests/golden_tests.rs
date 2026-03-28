use skagit_flats::presentation::Panel;
use skagit_flats::render;
use std::path::Path;

/// Fixed set of panels used for golden file testing.
fn test_panels() -> Vec<Panel> {
    vec![
        Panel::new("Weather")
            .with_row("52F  Mostly Cloudy")
            .with_row("Wind SW at 10 mph"),
        Panel::new("Skagit River")
            .with_row("12.3 ft")
            .with_row("4500 cfs"),
        Panel::new("Ferry -- Anacortes")
            .with_row("MV Samish")
            .with_row("Departs 10:30")
            .with_row("Departs 12:15"),
        Panel::new("Baker Lake Road")
            .with_row("OPEN -- MP 0-25"),
    ]
}

#[test]
fn golden_render_four_panels() {
    let panels = test_panels();
    let buf = render::render(&panels);
    let png_data = buf.to_png();

    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("four_panels.png");

    if std::env::var("GENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden_path, &png_data).expect("failed to write golden file");
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
        png_data, expected,
        "Render output does not match golden file. Run with GENERATE_GOLDEN=1 to regenerate."
    );
}

#[test]
fn golden_render_single_panel() {
    let panels = vec![
        Panel::new("Status")
            .with_row("All systems nominal")
            .with_row("Last update: 08:00"),
    ];
    let buf = render::render(&panels);
    let png_data = buf.to_png();

    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("single_panel.png");

    if std::env::var("GENERATE_GOLDEN").is_ok() {
        std::fs::write(&golden_path, &png_data).expect("failed to write golden file");
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
        png_data, expected,
        "Render output does not match golden file. Run with GENERATE_GOLDEN=1 to regenerate."
    );
}
