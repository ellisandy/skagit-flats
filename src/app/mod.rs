use crate::config::{Config, DestinationsConfig};
use crate::display::{DisplayDriver, NullDisplay, RefreshMode};
use crate::domain::{DataPoint, DomainState};
use crate::evaluation::evaluate;
use crate::presentation::build_panels;
use crate::render::render_panels;
use crate::sources::noaa::NoaaSource;
use crate::sources::Source;
use crate::web;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

/// Shared state between the main loop and the web server.
pub struct SharedState {
    /// Current pixel buffer (rendered display image).
    pub pixel_buffer: RwLock<crate::render::PixelBuffer>,
    /// Source status information for the /sources endpoint.
    pub source_statuses: RwLock<Vec<SourceStatus>>,
}

/// Status of a single data source, exposed via GET /sources.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub enabled: bool,
    pub last_fetch: Option<u64>,
    pub last_error: Option<String>,
    pub next_fetch: Option<u64>,
}

/// Top-level runtime options parsed from CLI flags or environment variables.
#[derive(Debug, Clone)]
pub struct AppOptions {
    /// Disable the SPI display driver; use NullDisplay instead.
    pub no_hardware: bool,
    /// Use static fixture data instead of live API calls.
    pub fixture_data: bool,
    /// Path to config.toml.
    pub config_path: std::path::PathBuf,
    /// Path to destinations.toml.
    pub destinations_path: std::path::PathBuf,
    /// Web server listen port.
    pub port: u16,
}

impl Default for AppOptions {
    fn default() -> Self {
        AppOptions {
            no_hardware: std::env::var("SKAGIT_NO_HARDWARE").is_ok(),
            fixture_data: std::env::var("SKAGIT_FIXTURE_DATA")
                .map(|v| v == "1")
                .unwrap_or(false),
            config_path: "config.toml".into(),
            destinations_path: "destinations.toml".into(),
            port: std::env::var("SKAGIT_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
        }
    }
}

impl AppOptions {
    /// Parse CLI arguments into AppOptions.
    ///
    /// Supported flags:
    /// - `--no-hardware`: disable SPI display driver
    /// - `--fixture-data`: use static fixture responses
    /// - `--config <path>`: path to config.toml
    /// - `--destinations <path>`: path to destinations.toml
    /// - `--port <port>`: web server port
    ///
    /// Environment variables (SKAGIT_NO_HARDWARE, SKAGIT_FIXTURE_DATA, SKAGIT_PORT)
    /// are used as defaults; CLI flags override them.
    pub fn from_args(args: Vec<String>) -> Self {
        let mut opts = Self::default();
        let mut i = 1; // skip argv[0]
        while i < args.len() {
            match args[i].as_str() {
                "--no-hardware" => opts.no_hardware = true,
                "--fixture-data" => opts.fixture_data = true,
                "--config" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        opts.config_path = val.into();
                    }
                }
                "--destinations" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        opts.destinations_path = val.into();
                    }
                }
                "--port" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        if let Ok(p) = val.parse() {
                            opts.port = p;
                        }
                    }
                }
                _ => {
                    // Unknown flag — ignore for forward compatibility.
                }
            }
            i += 1;
        }
        opts
    }
}

/// Run the skagit-flats daemon until a shutdown signal is received.
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

    if opts.fixture_data {
        log::info!("fixture-data mode: sources will return static responses");
    }

    let (tx, rx) = mpsc::channel::<DataPoint>();

    // Spawn NOAA weather source thread.
    let noaa = NoaaSource::new(&config.location, config.sources.weather_interval_secs);
    spawn_source(noaa, tx.clone());

    // Drop the original sender so the channel closes when all source threads exit.
    drop(tx);

    // Initial render with empty state.
    let mut state = DomainState::default();
    let panels = build_panels(&state);
    let buf = render_panels(&panels, config.display.width, config.display.height);
    if let Err(e) = display.update(&buf, RefreshMode::Full) {
        log::error!("display update failed: {e}");
    }

    // Log destination decisions against empty state.
    for dest in &destinations.destinations {
        let decision = evaluate(dest, &state);
        log::info!("destination '{}': {:?}", dest.name, decision);
    }

    // Main loop — blocks until all source threads exit (channel closes).
    log::info!("entering main loop");
    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(point) => {
                state.apply(point);
                let panels = build_panels(&state);
                let buf = render_panels(&panels, config.display.width, config.display.height);
                if let Err(e) = display.update(&buf, RefreshMode::Full) {
                    log::error!("display update failed: {e}");
                }
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

/// Spawn a source on a dedicated thread. The source fetches data in a loop at
/// its configured refresh interval, sending results over the channel.
fn spawn_source(source: impl Source + 'static, tx: mpsc::Sender<DataPoint>) {
    let name = source.name().to_string();
    let interval = source.refresh_interval();

    thread::Builder::new()
        .name(format!("source-{}", name))
        .spawn(move || {
            log::info!("source '{}' started (interval: {:?})", name, interval);
            loop {
                match source.fetch() {
                    Ok(point) => {
                        log::debug!("source '{}' fetched successfully", name);
                        if tx.send(point).is_err() {
                            log::info!("source '{}' channel closed, exiting", name);
                            break;
                        }
                    }
                    Err(e) => {
                        log::warn!("source '{}' fetch failed: {}", name, e);
                    }
                }
                thread::sleep(interval);
            }
        })
        .expect("failed to spawn source thread");
}

/// Start the axum web server on a dedicated thread with its own tokio runtime.
///
/// Returns the JoinHandle and the SharedState (so the main loop can update it).
pub fn start_web_server(config: &Config, opts: &AppOptions) -> (thread::JoinHandle<()>, Arc<SharedState>) {
    let initial_buf = crate::render::PixelBuffer::new(config.display.width, config.display.height);

    let shared = Arc::new(SharedState {
        pixel_buffer: RwLock::new(initial_buf),
        source_statuses: RwLock::new(Vec::new()),
    });

    let state = Arc::clone(&shared);
    let port = opts.port;

    let handle = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for web server");

        rt.block_on(async move {
            let app = web::build_router(state);
            let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
            log::info!("web server listening on http://{addr}");
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("failed to bind web server");
            axum::serve(listener, app)
                .await
                .expect("web server error");
        });
    });

    (handle, shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_from_env() {
        // Ensure default construction works without panicking.
        let opts = AppOptions::default();
        assert_eq!(opts.config_path.to_str().unwrap(), "config.toml");
        assert_eq!(opts.destinations_path.to_str().unwrap(), "destinations.toml");
    }

    #[test]
    fn parse_cli_flags() {
        let args = vec![
            "skagit-flats".to_string(),
            "--no-hardware".to_string(),
            "--fixture-data".to_string(),
            "--config".to_string(),
            "/tmp/config.toml".to_string(),
            "--port".to_string(),
            "9090".to_string(),
        ];
        let opts = AppOptions::from_args(args);
        assert!(opts.no_hardware);
        assert!(opts.fixture_data);
        assert_eq!(opts.config_path.to_str().unwrap(), "/tmp/config.toml");
        assert_eq!(opts.port, 9090);
    }

    #[test]
    fn unknown_flags_ignored() {
        let args = vec![
            "skagit-flats".to_string(),
            "--future-flag".to_string(),
        ];
        let opts = AppOptions::from_args(args);
        assert!(!opts.no_hardware);
    }
}
