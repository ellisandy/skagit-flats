use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

/// Top-level runtime configuration loaded from config.toml.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub device: DeviceConfig,
    pub display: DisplayConfig,
}

/// Device display loop configuration.
///
/// `run()` fetches a pre-rendered PNG from `image_url`, decodes it, and pushes
/// it to the hardware display. The contract: GET `image_url` returns `image/png`.
#[derive(Debug, Deserialize, Clone)]
pub struct DeviceConfig {
    /// URL of the pre-rendered display image served by the upstream renderer.
    pub image_url: String,
    /// How often to fetch and refresh the display, in seconds.
    #[serde(default = "default_refresh_secs")]
    pub refresh_interval_secs: u64,
}

fn default_refresh_secs() -> u64 {
    60
}

/// Display panel dimensions. Must match the connected hardware.
#[derive(Debug, Deserialize, Clone)]
pub struct DisplayConfig {
    /// Display width in pixels (800 for the Waveshare 7.5").
    pub width: u32,
    /// Display height in pixels (480 for the Waveshare 7.5").
    pub height: u32,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse '{path}': {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

/// Load and parse config.toml. Fails fast on any error.
pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })?;
    toml::from_str(&contents).map_err(|e| ConfigError::Parse {
        path: path.to_string_lossy().into_owned(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_valid_config() {
        let toml = r#"
[device]
image_url = "http://127.0.0.1:9090/image.png"
refresh_interval_secs = 30

[display]
width = 800
height = 480
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(toml.as_bytes()).unwrap();
        let cfg = load_config(f.path()).expect("should parse");
        assert_eq!(cfg.device.image_url, "http://127.0.0.1:9090/image.png");
        assert_eq!(cfg.device.refresh_interval_secs, 30);
        assert_eq!(cfg.display.width, 800);
        assert_eq!(cfg.display.height, 480);
    }

    #[test]
    fn refresh_interval_defaults_to_60() {
        let toml = r#"
[device]
image_url = "http://example.com/image.png"

[display]
width = 800
height = 480
"#;
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(toml.as_bytes()).unwrap();
        let cfg = load_config(f.path()).expect("should parse");
        assert_eq!(cfg.device.refresh_interval_secs, 60);
    }

    #[test]
    fn missing_file_returns_read_error() {
        let result = load_config(Path::new("/nonexistent/config.toml"));
        assert!(matches!(result, Err(ConfigError::Read { .. })));
    }

    #[test]
    fn invalid_toml_returns_parse_error() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"not valid !!!").unwrap();
        assert!(matches!(load_config(f.path()), Err(ConfigError::Parse { .. })));
    }
}
