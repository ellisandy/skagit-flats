use crate::config::Config;
use crate::display::{DisplayDriver, NullDisplay, RefreshMode};
use crate::render::PixelBuffer;
use std::io::Read;
use std::thread;
use std::time::Duration;

/// Top-level runtime options parsed from CLI flags or environment variables.
#[derive(Debug, Clone)]
pub struct AppOptions {
    /// Disable the SPI display driver; use NullDisplay instead.
    pub no_hardware: bool,
    /// Path to config.toml.
    pub config_path: std::path::PathBuf,
}

impl Default for AppOptions {
    fn default() -> Self {
        AppOptions {
            no_hardware: std::env::var("SKAGIT_NO_HARDWARE").is_ok(),
            config_path: "config.toml".into(),
        }
    }
}

impl AppOptions {
    /// Parse CLI arguments into AppOptions.
    ///
    /// Supported flags:
    /// - `--no-hardware`: disable the SPI display driver
    /// - `--config <path>`: path to config.toml
    pub fn from_args(args: Vec<String>) -> Self {
        let mut opts = Self::default();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--no-hardware" => opts.no_hardware = true,
                "--config" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        opts.config_path = val.into();
                    }
                }
                _ => {}
            }
            i += 1;
        }
        opts
    }
}

/// Run the device display loop: fetch a pre-rendered PNG from the configured
/// URL and push it to the e-ink display, sleeping between refreshes.
pub fn run(opts: AppOptions, config: Config) {
    log::info!("skagit-flats device loop starting");

    let partial_enabled = config.device.partial_refresh;
    let cadence = config.device.partial_refresh_cadence;

    let mut display: Box<dyn DisplayDriver> = if opts.no_hardware {
        log::info!("no-hardware mode: using NullDisplay");
        Box::new(NullDisplay)
    } else {
        #[cfg(feature = "hardware")]
        {
            log::info!("hardware mode: initializing Waveshare SPI display");
            match crate::display::waveshare::WaveshareDisplay::new(partial_enabled) {
                Ok(d) => Box::new(d),
                Err(e) => {
                    log::error!(
                        "failed to initialize hardware display: {e}, falling back to NullDisplay"
                    );
                    Box::new(NullDisplay)
                }
            }
        }
        #[cfg(not(feature = "hardware"))]
        {
            let _ = partial_enabled;
            log::info!("hardware feature not enabled, using NullDisplay");
            Box::new(NullDisplay)
        }
    };

    let refresh = Duration::from_secs(config.device.refresh_interval_secs);
    log::info!(
        "fetching {} every {}s ({}x{})",
        config.device.image_url,
        config.device.refresh_interval_secs,
        config.display.width,
        config.display.height,
    );

    // Last successfully-pushed frame. We skip pushing identical frames so the
    // panel doesn't refresh at all when nothing has changed upstream.
    let mut last_pushed: Option<Vec<u8>> = None;
    // How many consecutive partial refreshes have happened since the last full
    // refresh. Reset to 0 on each full refresh; when it hits `cadence`, the
    // next push is forced to full to clear accumulated ghosting.
    let mut partial_count: u32 = 0;

    loop {
        match fetch_image(
            &config.device.image_url,
            config.display.width,
            config.display.height,
        ) {
            Ok(buf) => {
                if last_pushed.as_ref() == Some(&buf.pixels) {
                    log::debug!("image unchanged, skipping display update");
                } else {
                    let mode =
                        pick_refresh_mode(partial_enabled, last_pushed.is_none(), partial_count, cadence);
                    log::debug!(
                        "image changed, refreshing ({mode:?}, partial_count={partial_count}/{cadence})"
                    );
                    match display.update(&buf, mode) {
                        Ok(()) => {
                            last_pushed = Some(buf.pixels);
                            match mode {
                                RefreshMode::Full => partial_count = 0,
                                RefreshMode::Partial => partial_count += 1,
                            }
                        }
                        Err(e) => {
                            log::error!("display update failed ({mode:?}): {e}");
                            // Force a full refresh on the next attempt to
                            // recover from any stuck partial-mode state.
                            partial_count = cadence;
                        }
                    }
                }
            }
            Err(e) => log::error!("image fetch failed: {e}"),
        }
        thread::sleep(refresh);
    }
}

/// Decide which refresh mode to use for the next push.
///
/// Always picks `Full` when:
/// - partial mode is disabled in config,
/// - this is the first push since startup (panel state may be unknown), or
/// - the partial-refresh counter has hit the cadence (force a ghost-cleanup full).
///
/// Otherwise picks `Partial`.
fn pick_refresh_mode(
    partial_enabled: bool,
    is_first_push: bool,
    partial_count: u32,
    cadence: u32,
) -> RefreshMode {
    if !partial_enabled || is_first_push || partial_count >= cadence {
        RefreshMode::Full
    } else {
        RefreshMode::Partial
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
) -> Result<PixelBuffer, Box<dyn std::error::Error>> {
    let resp = ureq::get(url)
        .timeout(Duration::from_secs(10))
        .call()?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(5 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
    let img = image::load_from_memory(&bytes)?.into_luma8();
    if img.width() != width || img.height() != height {
        log::warn!(
            "image dimensions {}x{} do not match display {}x{}; pixels outside bounds will be cropped",
            img.width(), img.height(), width, height,
        );
    }
    let mut buf = PixelBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels() {
        buf.set_pixel(x, y, pixel.0[0] < 128);
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options() {
        let opts = AppOptions::default();
        assert_eq!(opts.config_path.to_str().unwrap(), "config.toml");
    }

    #[test]
    fn parse_cli_flags() {
        let args = vec![
            "skagit-flats".to_string(),
            "--no-hardware".to_string(),
            "--config".to_string(),
            "/tmp/config.toml".to_string(),
        ];
        let opts = AppOptions::from_args(args);
        assert!(opts.no_hardware);
        assert_eq!(opts.config_path.to_str().unwrap(), "/tmp/config.toml");
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

    #[test]
    fn pick_refresh_mode_uses_full_when_partial_disabled() {
        assert!(matches!(
            pick_refresh_mode(false, false, 0, 30),
            RefreshMode::Full
        ));
        // Even with mid-cadence count, disabled config wins.
        assert!(matches!(
            pick_refresh_mode(false, false, 15, 30),
            RefreshMode::Full
        ));
    }

    #[test]
    fn pick_refresh_mode_uses_full_on_first_push() {
        assert!(matches!(
            pick_refresh_mode(true, true, 0, 30),
            RefreshMode::Full
        ));
    }

    #[test]
    fn pick_refresh_mode_uses_partial_in_steady_state() {
        assert!(matches!(
            pick_refresh_mode(true, false, 0, 30),
            RefreshMode::Partial
        ));
        assert!(matches!(
            pick_refresh_mode(true, false, 29, 30),
            RefreshMode::Partial
        ));
    }

    #[test]
    fn pick_refresh_mode_forces_full_at_cadence_boundary() {
        assert!(matches!(
            pick_refresh_mode(true, false, 30, 30),
            RefreshMode::Full
        ));
        // After error recovery: count is bumped to/past cadence.
        assert!(matches!(
            pick_refresh_mode(true, false, 100, 30),
            RefreshMode::Full
        ));
    }
}
