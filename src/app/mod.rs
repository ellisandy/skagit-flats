use crate::config::{Config, DestinationsConfig};
use crate::display::{DisplayDriver, NullDisplay, RefreshMode};
use crate::domain::DomainState;
use crate::web;
use std::collections::HashMap;
use std::io::Read;
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

/// Run the device display loop: fetch a pre-rendered PNG from the configured
/// URL and push it to the e-ink display, sleeping between refreshes.
///
/// This is a thin client loop (~30 lines). All rendering happens on the server.
/// The API contract: GET `config.device.image_url` returns `image/png` directly.
pub fn run(opts: AppOptions, config: Config) {
    log::info!("skagit-flats device loop starting");

    let device = config.device.unwrap_or_else(|| {
        eprintln!("error: config.toml must have a [device] section with image_url");
        std::process::exit(1);
    });

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
                    log::error!("failed to initialize hardware display: {e}, falling back to NullDisplay");
                    Box::new(NullDisplay)
                }
            }
        }
        #[cfg(not(feature = "hardware"))]
        {
            log::info!("hardware feature not enabled, using NullDisplay");
            Box::new(NullDisplay)
        }
    };

    let refresh = Duration::from_secs(device.refresh_interval_secs);
    log::info!(
        "fetching {} every {}s ({}x{})",
        device.image_url, device.refresh_interval_secs,
        config.display.width, config.display.height,
    );

    loop {
        match fetch_image(&device.image_url, config.display.width, config.display.height) {
            Ok(buf) => {
                if let Err(e) = display.update(&buf, RefreshMode::Full) {
                    log::error!("display update failed: {e}");
                }
            }
            Err(e) => log::error!("image fetch failed: {e}"),
        }
        thread::sleep(refresh);
    }
}

/// Fetch a PNG from `url`, decode it, and pack into a 1-bit PixelBuffer.
///
/// Pixels with luma < 128 are black; >= 128 are white. Image pixels outside
/// the buffer dimensions are silently ignored (PixelBuffer bounds-checks).
fn fetch_image(
    url: &str,
    width: u32,
    height: u32,
) -> Result<crate::render::PixelBuffer, Box<dyn std::error::Error>> {
    let resp = ureq::get(url).call()?;
    let mut bytes = Vec::new();
    resp.into_reader().take(5 * 1024 * 1024).read_to_end(&mut bytes)?;
    let img = image::load_from_memory(&bytes)?.into_luma8();
    let mut buf = crate::render::PixelBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels() {
        buf.set_pixel(x, y, pixel.0[0] < 128);
    }
    Ok(buf)
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
