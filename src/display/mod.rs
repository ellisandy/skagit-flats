#[cfg(feature = "hardware")]
pub mod waveshare;

use crate::render::PixelBuffer;
use thiserror::Error;

/// How much of the display to refresh.
#[derive(Debug, Clone, Copy)]
pub enum RefreshMode {
    /// Full refresh: clears ghosting, ~2 seconds. Run hourly.
    Full,
    /// Partial refresh: repaints a region, ~0.3 seconds.
    Partial,
}

#[derive(Debug, Error)]
pub enum DisplayError {
    #[error("SPI error: {0}")]
    Spi(String),
    #[error("display not available: {0}")]
    Unavailable(String),
}

/// Abstraction over the physical e-ink display.
///
/// Both the real SPI driver and the no-op stub implement this trait so that
/// app code is hardware-agnostic.
pub trait DisplayDriver: Send {
    /// Push a PixelBuffer to the display using the given refresh mode.
    fn update(&mut self, buffer: &PixelBuffer, mode: RefreshMode) -> Result<(), DisplayError>;

    /// Clear the display to all-white.
    fn clear(&mut self) -> Result<(), DisplayError>;
}

/// A no-op display driver used when `--no-hardware` is set or in Docker.
///
/// All operations succeed immediately without touching any hardware.
pub struct NullDisplay;

impl DisplayDriver for NullDisplay {
    fn update(&mut self, _buffer: &PixelBuffer, _mode: RefreshMode) -> Result<(), DisplayError> {
        log::debug!("NullDisplay: update() called (no-op)");
        Ok(())
    }

    fn clear(&mut self) -> Result<(), DisplayError> {
        log::debug!("NullDisplay: clear() called (no-op)");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_display_update_succeeds() {
        let mut d = NullDisplay;
        let buf = PixelBuffer::new(800, 480);
        assert!(d.update(&buf, RefreshMode::Full).is_ok());
        assert!(d.update(&buf, RefreshMode::Partial).is_ok());
    }

    #[test]
    fn null_display_clear_succeeds() {
        let mut d = NullDisplay;
        assert!(d.clear().is_ok());
    }
}
