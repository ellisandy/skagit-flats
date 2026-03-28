use crate::config::{Config, DestinationsConfig};
use crate::display::{DisplayDriver, NullDisplay, RefreshMode};
use crate::domain::{DataPoint, DomainState};
use crate::evaluation::evaluate;
use crate::presentation::build_panels;
use crate::render::render_panels;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Top-level runtime options parsed from CLI flags or environment variables.
#[derive(Debug, Clone)]
pub struct AppOptions {
    /// Disable the SPI display driver; use NullDisplay instead.
    pub no_hardware: bool,
    /// Path to config.toml.
    pub config_path: std::path::PathBuf,
    /// Path to destinations.toml.
    pub destinations_path: std::path::PathBuf,
}

impl Default for AppOptions {
    fn default() -> Self {
        AppOptions {
            no_hardware: std::env::var("SKAGIT_NO_HARDWARE").is_ok(),
            config_path: "config.toml".into(),
            destinations_path: "destinations.toml".into(),
        }
    }
}

/// Run the skagit-flats daemon until a shutdown signal is received.
///
/// This is a stub for Wave 1. Full scheduler, channel plumbing, web server
/// startup, and refresh coordination are implemented in later waves.
pub fn run(opts: AppOptions, config: Config, destinations: DestinationsConfig) {
    log::info!("skagit-flats starting");
    log::info!(
        "location: {} ({}, {})",
        config.location.name,
        config.location.latitude,
        config.location.longitude
    );

    let mut display: Box<dyn DisplayDriver> = if opts.no_hardware {
        log::info!("no-hardware mode: using NullDisplay");
        Box::new(NullDisplay)
    } else {
        log::info!("hardware mode: NullDisplay (SPI driver not yet implemented)");
        Box::new(NullDisplay)
    };

    // The sender is cloned and passed to source threads in later waves.
    // Dropping the original here means the channel closes when all source
    // threads exit, cleanly shutting down the main loop.
    let (tx, rx) = mpsc::channel::<DataPoint>();
    drop(tx);

    // Stub: initial render with empty state
    let state = DomainState::default();
    let panels = build_panels(&state);
    let buf = render_panels(&panels, config.display.width, config.display.height);
    if let Err(e) = display.update(&buf, RefreshMode::Full) {
        log::error!("display update failed: {e}");
    }

    // Log destination decisions against empty state
    for dest in &destinations.destinations {
        let decision = evaluate(dest, &state);
        log::info!("destination '{}': {:?}", dest.name, decision);
    }

    // Main loop — blocks until all source threads exit (channel closes).
    // Wave 1: no sources registered, so the loop exits immediately.
    log::info!("entering main loop (stub — no sources registered yet)");
    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(_point) => {
                // Future waves: apply point to state, re-render, push to display
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                log::debug!("heartbeat tick");
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("channel disconnected, shutting down");
                break;
            }
        }
    }
}

/// Start the axum web server on a dedicated thread.
///
/// Stub for Wave 1 — the server is not actually bound until Wave 2.
pub fn start_web_server(_config: &Config) -> thread::JoinHandle<()> {
    thread::spawn(|| {
        log::info!("web server thread started (stub — not yet bound)");
    })
}
