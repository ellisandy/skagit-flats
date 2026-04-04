use crate::config::{Config, DestinationsConfig};
use crate::display::{DisplayDriver, NullDisplay, RefreshMode};
use crate::domain::{DataPoint, DomainState};
use crate::evaluation::{current_unix_secs, evaluate};
use crate::presentation::build_display_layout;
use crate::render::{render_display, render_startup};
use crate::sources::noaa::NoaaSource;
use crate::sources::usgs::UsgsSource;
use crate::sources::wsdot::WsdotFerrySource;
use crate::sources::Source;
use crate::web;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

/// Shared state between the main loop and the web server.
pub struct SharedState {
    /// Current pixel buffer (rendered display image).
    pub pixel_buffer: RwLock<crate::render::PixelBuffer>,
    /// Source status information for the /sources endpoint.
    pub source_statuses: RwLock<Vec<SourceStatus>>,
    /// Current destinations configuration (editable via web UI).
    pub destinations_config: RwLock<DestinationsConfig>,
    /// Current domain state for trip evaluation.
    pub domain_state: RwLock<DomainState>,
    /// Path to destinations.toml for persistence.
    pub destinations_path: std::path::PathBuf,
    /// Display dimensions for re-rendering.
    pub display_width: u32,
    /// Display height for re-rendering.
    pub display_height: u32,
    /// Hardware initialization error, if any (None = hardware OK or no-hardware mode).
    pub hardware_error: RwLock<Option<String>>,
    /// Whether the app is running in fixture data mode.
    pub fixture_data: bool,
    /// Web UI authentication configuration (None = no auth required).
    pub auth: Option<crate::config::AuthConfig>,
    /// Active web UI sessions: token → creation time.
    pub sessions: RwLock<HashMap<String, Instant>>,
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
///
/// The `shared` parameter is the same Arc<SharedState> held by the web server,
/// allowing the main loop to update the pixel buffer and domain state visible
/// to web endpoints.
pub fn run(
    opts: AppOptions,
    config: Config,
    destinations: DestinationsConfig,
    shared: Arc<SharedState>,
) {
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
        #[cfg(feature = "hardware")]
        {
            log::info!("hardware mode: initializing Waveshare SPI display");
            match crate::display::waveshare::WaveshareDisplay::new() {
                Ok(d) => Box::new(d),
                Err(e) => {
                    let msg = format!("failed to initialize hardware display: {e}");
                    log::error!("{msg}");
                    log::warn!("falling back to NullDisplay");
                    *shared.hardware_error.write().expect("hardware_error lock poisoned") = Some(msg);
                    Box::new(NullDisplay)
                }
            }
        }
        #[cfg(not(feature = "hardware"))]
        {
            log::info!("hardware mode: built without 'hardware' feature, using NullDisplay");
            Box::new(NullDisplay)
        }
    };

    if opts.fixture_data {
        log::info!("fixture-data mode: sources will return static responses");
    }

    let (tx, rx) = mpsc::channel::<DataPoint>();

    // Spawn NOAA weather source thread.
    let noaa = NoaaSource::new(&config.location, config.sources.weather_interval_secs);
    spawn_source(noaa, tx.clone());

    // Spawn USGS river gauge source thread.
    let usgs_site_id = config
        .sources
        .river
        .as_ref()
        .map(|r| r.usgs_site_id.as_str())
        .unwrap_or("12200500");
    let usgs = UsgsSource::new(usgs_site_id, config.sources.river_interval_secs);
    spawn_source(usgs, tx.clone());

    // Spawn WSDOT ferries source thread.
    match WsdotFerrySource::new(config.sources.ferry.as_ref(), config.sources.ferry_interval_secs) {
        Ok(ferry) => spawn_source(ferry, tx.clone()),
        Err(e) => log::warn!("WSDOT ferries source disabled: {}", e),
    }

    // Drop the original sender so the channel closes when all source threads exit.
    drop(tx);

    // Spawn file watcher for destinations.toml.
    spawn_destinations_watcher(opts.destinations_path.clone(), Arc::clone(&shared));

    // Initial render: show startup placeholder while sources complete first fetch.
    let startup_buf = render_startup();
    if let Err(e) = display.update(&startup_buf, RefreshMode::Full) {
        log::error!("display update failed: {e}");
    }

    // Update shared pixel buffer with startup render.
    {
        let mut pb = shared.pixel_buffer.write().expect("pixel_buffer lock poisoned");
        *pb = startup_buf;
    }

    // Log destination decisions against current state.
    for dest in &destinations.destinations {
        let decision = evaluate(dest, &DomainState::default(), current_unix_secs());
        log::info!("destination '{}': {:?}", dest.name, decision);
    }

    // Main loop — blocks until all source threads exit (channel closes).
    // Hourly full refresh clears e-ink ghosting; data updates use partial refresh.
    log::info!("entering main loop");
    let full_refresh_interval = Duration::from_secs(3600);
    let mut last_full_refresh = std::time::Instant::now();

    loop {
        // Check if an hourly full refresh is due.
        let needs_full = last_full_refresh.elapsed() >= full_refresh_interval;
        if needs_full {
            log::info!("hourly full refresh to clear ghosting");
            let buf = {
                let domain = shared.domain_state.read().expect("domain_state lock poisoned");
                let dests = shared.destinations_config.read().expect("destinations_config lock poisoned");
                let layout = build_display_layout(&domain, &dests.destinations, current_unix_secs());
                render_display(&layout)
            };

            if let Err(e) = display.update(&buf, RefreshMode::Full) {
                log::error!("full refresh failed: {e}");
            }
            let mut pb = shared.pixel_buffer.write().expect("pixel_buffer lock poisoned");
            *pb = buf;
            last_full_refresh = std::time::Instant::now();
        }

        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(point) => {
                // Update shared domain state.
                {
                    let mut ds = shared.domain_state.write().expect("domain_state lock poisoned");
                    ds.apply(point);
                }

                // Re-render with current destinations.
                let buf = {
                    let domain = shared.domain_state.read().expect("domain_state lock poisoned");
                    let dests = shared.destinations_config.read().expect("destinations_config lock poisoned");
                    let layout = build_display_layout(&domain, &dests.destinations, current_unix_secs());
                    render_display(&layout)
                };

                // Use partial refresh for data updates (fast, ~0.3s).
                if let Err(e) = display.update(&buf, RefreshMode::Partial) {
                    log::error!("partial refresh failed: {e}");
                }

                // Update shared pixel buffer.
                let mut pb = shared.pixel_buffer.write().expect("pixel_buffer lock poisoned");
                *pb = buf;
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

/// Watch destinations.toml for changes and reload when modified.
///
/// Uses a simple polling approach (checks mtime every 2 seconds) to avoid
/// adding a file-watcher dependency like `notify`. Sufficient for a local
/// config file that changes infrequently.
fn spawn_destinations_watcher(path: std::path::PathBuf, shared: Arc<SharedState>) {
    thread::Builder::new()
        .name("destinations-watcher".to_string())
        .spawn(move || {
            let mut last_modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok();

            loop {
                thread::sleep(Duration::from_secs(2));

                let current_modified = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok();

                if current_modified != last_modified {
                    last_modified = current_modified;
                    log::info!("destinations.toml changed, reloading");

                    match crate::config::load_destinations(&path) {
                        Ok(new_config) => {
                            let mut dests = shared
                                .destinations_config
                                .write()
                                .expect("destinations_config lock poisoned");
                            *dests = new_config;
                            drop(dests);

                            // Re-render with updated destinations.
                            let buf = {
                                let domain = shared
                                    .domain_state
                                    .read()
                                    .expect("domain_state lock poisoned");
                                let dests = shared
                                    .destinations_config
                                    .read()
                                    .expect("destinations_config lock poisoned");
                                let layout = build_display_layout(&domain, &dests.destinations, current_unix_secs());
                                render_display(&layout)
                            };

                            let mut pb = shared
                                .pixel_buffer
                                .write()
                                .expect("pixel_buffer lock poisoned");
                            *pb = buf;

                            log::info!("destinations reloaded and display re-rendered");
                        }
                        Err(e) => {
                            log::warn!("failed to reload destinations.toml: {e}");
                        }
                    }
                }
            }
        })
        .expect("failed to spawn destinations watcher thread");
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
pub fn start_web_server(
    config: &Config,
    opts: &AppOptions,
    destinations: &DestinationsConfig,
) -> (thread::JoinHandle<()>, Arc<SharedState>) {
    let initial_buf = crate::render::PixelBuffer::new(config.display.width, config.display.height);

    let shared = Arc::new(SharedState {
        pixel_buffer: RwLock::new(initial_buf),
        source_statuses: RwLock::new(Vec::new()),
        destinations_config: RwLock::new(destinations.clone()),
        domain_state: RwLock::new(DomainState::default()),
        destinations_path: opts.destinations_path.clone(),
        display_width: config.display.width,
        display_height: config.display.height,
        hardware_error: RwLock::new(None),
        fixture_data: opts.fixture_data,
        auth: config.auth.clone(),
        sessions: RwLock::new(HashMap::new()),
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
