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
                    log::error!(
                        "failed to initialize hardware display: {e}, falling back to NullDisplay"
                    );
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

    let refresh = Duration::from_secs(config.device.refresh_interval_secs);
    log::info!(
        "fetching {} every {}s ({}x{})",
        config.device.image_url,
        config.device.refresh_interval_secs,
        config.display.width,
        config.display.height,
    );

    // Last successfully-pushed frame. We skip pushing identical frames so the
    // panel doesn't trigger its full-refresh waveform (the visible black/white
    // flash) when nothing has changed upstream.
    let mut last_pushed: Option<Vec<u8>> = None;

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
                    match display.update(&buf, RefreshMode::Full) {
                        Ok(()) => last_pushed = Some(buf.pixels),
                        Err(e) => log::error!("display update failed: {e}"),
                    }
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
}
