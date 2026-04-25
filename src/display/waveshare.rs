//! Waveshare 7.5 inch v2 e-ink display driver over SPI (via rppal).
//!
//! Only compiled when the `hardware` feature is enabled.
//! Requires running on a Raspberry Pi with SPI enabled.

#[cfg(feature = "hardware")]
mod driver {
    use crate::display::{DisplayDriver, DisplayError, RefreshMode};
    use crate::render::PixelBuffer;
    use rppal::gpio::{Gpio, OutputPin};
    use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
    use std::thread;
    use std::time::Duration;

    // Waveshare 7.5" v2 panel dimensions.
    const WIDTH: u32 = 800;
    const HEIGHT: u32 = 480;

    // BCM GPIO pin assignments (matching Waveshare HAT epdconfig.py).
    const RST_PIN: u8 = 17;
    const DC_PIN: u8 = 25;
    const BUSY_PIN: u8 = 24;
    /// PWR_PIN (GPIO 18) enables the 5V supply to the display module.
    /// Must be driven HIGH before any SPI communication; LOW on shutdown.
    const PWR_PIN: u8 = 18;
    // CS (GPIO 8 / CE0) is controlled by the SPI hardware via SlaveSelect::Ss0.
    // Do NOT claim it as a GPIO OutputPin — dual ownership corrupts CS signalling.

    // SPI clock speed: 4 MHz is safe for the Waveshare panel.
    const SPI_CLOCK_HZ: u32 = 4_000_000;

    // Maximum time to wait for the panel to become ready.
    const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

    pub struct WaveshareDisplay {
        spi: Spi,
        rst: OutputPin,
        dc: OutputPin,
        pwr: OutputPin,
        busy: rppal::gpio::InputPin,
        /// Enables the post-Sept-2023 panel revision's partial-refresh path
        /// (`update()` with `RefreshMode::Partial` hits `display_frame_partial`).
        partial_enabled: bool,
    }

    impl WaveshareDisplay {
        pub fn new(partial_enabled: bool) -> Result<Self, DisplayError> {
            let gpio = Gpio::new().map_err(|e| DisplayError::Spi(format!("GPIO init: {e}")))?;

            let rst = gpio
                .get(RST_PIN)
                .map_err(|e| DisplayError::Spi(format!("RST pin {RST_PIN}: {e}")))?
                .into_output();
            let dc = gpio
                .get(DC_PIN)
                .map_err(|e| DisplayError::Spi(format!("DC pin {DC_PIN}: {e}")))?
                .into_output();
            let mut pwr = gpio
                .get(PWR_PIN)
                .map_err(|e| DisplayError::Spi(format!("PWR pin {PWR_PIN}: {e}")))?
                .into_output();
            let busy = gpio
                .get(BUSY_PIN)
                .map_err(|e| DisplayError::Spi(format!("BUSY pin {BUSY_PIN}: {e}")))?
                .into_input_pullup();

            let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, SPI_CLOCK_HZ, Mode::Mode0)
                .map_err(|e| DisplayError::Spi(format!("SPI init: {e}")))?;

            // Enable 5V supply to display module before any SPI communication.
            pwr.set_high();
            thread::sleep(Duration::from_millis(20));

            let mut display = WaveshareDisplay {
                spi,
                rst,
                dc,
                pwr,
                busy,
                partial_enabled,
            };

            display.hw_reset()?;
            display.init_panel()?;
            if partial_enabled {
                display.init_partial_extensions()?;
            }

            log::info!(
                "WaveshareDisplay initialized ({}x{}, SPI @ {} Hz, partial_refresh={})",
                WIDTH,
                HEIGHT,
                SPI_CLOCK_HZ,
                partial_enabled,
            );
            Ok(display)
        }

        fn hw_reset(&mut self) -> Result<(), DisplayError> {
            self.rst.set_high();
            thread::sleep(Duration::from_millis(20));
            self.rst.set_low();
            thread::sleep(Duration::from_millis(2));
            self.rst.set_high();
            thread::sleep(Duration::from_millis(20));
            Ok(())
        }

        fn send_command(&mut self, cmd: u8) -> Result<(), DisplayError> {
            self.dc.set_low();
            self.spi
                .write(&[cmd])
                .map_err(|e| DisplayError::Spi(format!("SPI write cmd 0x{cmd:02X}: {e}")))?;
            Ok(())
        }

        fn send_data(&mut self, data: &[u8]) -> Result<(), DisplayError> {
            self.dc.set_high();
            // SPI transfer in chunks to avoid kernel buffer limits.
            for chunk in data.chunks(4096) {
                self.spi
                    .write(chunk)
                    .map_err(|e| DisplayError::Spi(format!("SPI write data: {e}")))?;
            }
            Ok(())
        }

        fn wait_busy(&mut self) -> Result<(), DisplayError> {
            let start = std::time::Instant::now();
            // BUSY is active-LOW: LOW = panel busy, HIGH = panel ready.
            // Poll the pin directly; do not send 0x71 (GET_STATUS) — doing so
            // in a tight loop resets the BUSY assertion on this panel revision.
            while self.busy.is_low() {
                if start.elapsed() > BUSY_TIMEOUT {
                    return Err(DisplayError::Spi(format!(
                        "panel busy timeout ({:?})",
                        BUSY_TIMEOUT
                    )));
                }
                thread::sleep(Duration::from_millis(10));
            }
            log::debug!("wait_busy: panel ready ({:?})", start.elapsed());
            Ok(())
        }

        fn init_panel(&mut self) -> Result<(), DisplayError> {
            // Booster soft start — required before power setting (matches official Waveshare driver).
            self.send_command(0x06)?;
            self.send_data(&[0x17, 0x17, 0x28, 0x17])?;

            // Power setting: VGH=20V, VGL=-20V, VDH=15V, VDL=-15V.
            self.send_command(0x01)?;
            self.send_data(&[0x07, 0x07, 0x28, 0x17])?;

            // Power on. Wait for panel to finish powering up before sending config.
            self.send_command(0x04)?;
            thread::sleep(Duration::from_millis(100));
            self.wait_busy()?;

            // Panel setting: LUT from OTP, black/white mode, scan-up, shift-right.
            self.send_command(0x00)?;
            self.send_data(&[0x1F])?;

            // Resolution setting: 800x480.
            self.send_command(0x61)?;
            self.send_data(&[
                (WIDTH >> 8) as u8,
                (WIDTH & 0xFF) as u8,
                (HEIGHT >> 8) as u8,
                (HEIGHT & 0xFF) as u8,
            ])?;

            // Dual SPI off.
            self.send_command(0x15)?;
            self.send_data(&[0x00])?;

            // VCOM and data interval setting (full-refresh value).
            self.send_command(0x50)?;
            self.send_data(&[0x10, 0x07])?;

            // TCON setting.
            self.send_command(0x60)?;
            self.send_data(&[0x22])?;

            Ok(())
        }

        /// Enable the post-Sept-2023 panel revision's fast/partial waveform paths.
        ///
        /// These two commands are not in the older Waveshare reference driver.
        /// Without them, partial-mode commands silently no-op even on the right
        /// silicon. With them, on a pre-Sept-2023 panel, behavior is undefined
        /// (the partial-refresh feature gate in config exists to keep these off
        /// for older panels). Ends with the panel powered off, matching the
        /// "ready state" expected by the per-refresh power-cycle pattern.
        fn init_partial_extensions(&mut self) -> Result<(), DisplayError> {
            self.send_command(0xE0)?;
            self.send_data(&[0x02])?;
            self.send_command(0xE5)?;
            self.send_data(&[0x5A])?;

            self.send_command(0x02)?;
            self.wait_busy()?;
            Ok(())
        }

        fn power_on(&mut self) -> Result<(), DisplayError> {
            self.send_command(0x04)?;
            thread::sleep(Duration::from_millis(200));
            self.wait_busy()?;
            Ok(())
        }

        fn power_off(&mut self) -> Result<(), DisplayError> {
            self.send_command(0x02)?;
            self.wait_busy()?;
            Ok(())
        }

        /// Full-waveform refresh. ~2s, visible black/white flash, clears ghosting.
        ///
        /// When `partial_enabled`, wraps the SPI sequence in a power-cycle so the
        /// panel ends in the same "off" state that partial refresh expects. When
        /// `partial_enabled` is false, the panel stays powered on (legacy
        /// behavior — no surprises for users who explicitly chose to disable
        /// partial mode, e.g. on older panel revisions).
        fn display_frame_full(&mut self, buffer: &[u8]) -> Result<(), DisplayError> {
            if self.partial_enabled {
                self.power_on()?;
                // Reset VCOM/data interval to the full-refresh value in case a
                // prior partial refresh changed it.
                self.send_command(0x50)?;
                self.send_data(&[0x10, 0x07])?;
            }

            // DTM1 (0x10): old frame = bitwise inverse of new image (for waveform calculation).
            self.send_command(0x10)?;
            let inverted: Vec<u8> = buffer.iter().map(|b| !b).collect();
            self.send_data(&inverted)?;

            // DTM2 (0x13): new frame data.
            self.send_command(0x13)?;
            self.send_data(buffer)?;

            // Display refresh.
            self.send_command(0x12)?;
            thread::sleep(Duration::from_millis(100));
            self.wait_busy()?;

            if self.partial_enabled {
                self.power_off()?;
            }
            Ok(())
        }

        /// Partial-waveform refresh. ~0.4s, no visible flash, accumulates
        /// ghosting after many cycles (caller must periodically force a full
        /// refresh to clear it — see `app::run`).
        ///
        /// Mirrors the SPI sequence from ESPHome's `WaveshareEPaper7P5InV2P`
        /// (esphome PR #7751, validated on a 2024-manufactured panel). The
        /// partial window covers the entire panel, so the full buffer is sent —
        /// no diff/bounding-box logic. The panel powers down after refresh.
        fn display_frame_partial(&mut self, buffer: &[u8]) -> Result<(), DisplayError> {
            self.power_on()?;

            // VCOM and data interval setting (partial-mode value, differs from full).
            self.send_command(0x50)?;
            self.send_data(&[0xA9, 0x07])?;

            // Activate partial waveform path.
            self.send_command(0xE5)?;
            self.send_data(&[0x6E])?;

            // Enter partial mode (no data bytes).
            self.send_command(0x91)?;

            // Partial window: full panel.
            self.send_command(0x90)?;
            self.send_data(&[
                0x00, 0x00,
                ((WIDTH - 1) >> 8) as u8, ((WIDTH - 1) & 0xFF) as u8,
                0x00, 0x00,
                ((HEIGHT - 1) >> 8) as u8, ((HEIGHT - 1) & 0xFF) as u8,
                0x01,
            ])?;

            // DTM2 (0x13): new frame data.
            self.send_command(0x13)?;
            self.send_data(buffer)?;

            // Display refresh.
            self.send_command(0x12)?;
            thread::sleep(Duration::from_millis(100));
            self.wait_busy()?;

            self.power_off()?;
            Ok(())
        }
    }

    impl DisplayDriver for WaveshareDisplay {
        fn update(
            &mut self,
            buffer: &PixelBuffer,
            mode: RefreshMode,
        ) -> Result<(), DisplayError> {
            if buffer.width != WIDTH || buffer.height != HEIGHT {
                return Err(DisplayError::Spi(format!(
                    "buffer size {}x{} does not match panel {}x{}",
                    buffer.width, buffer.height, WIDTH, HEIGHT
                )));
            }

            log::debug!("WaveshareDisplay: update ({mode:?})");
            match mode {
                RefreshMode::Full => self.display_frame_full(&buffer.pixels)?,
                RefreshMode::Partial => {
                    if !self.partial_enabled {
                        return Err(DisplayError::Spi(
                            "partial refresh requested but partial_refresh is disabled in config"
                                .to_string(),
                        ));
                    }
                    self.display_frame_partial(&buffer.pixels)?;
                }
            }

            Ok(())
        }

        fn clear(&mut self) -> Result<(), DisplayError> {
            log::debug!("WaveshareDisplay: clearing display");
            self.init_panel()?;
            let white = vec![0x00u8; (WIDTH * HEIGHT / 8) as usize];
            self.display_frame_full(&white)?;
            if !self.partial_enabled {
                // display_frame_full handles power-off when partial_enabled.
                self.power_off()?;
            }
            Ok(())
        }
    }

    impl Drop for WaveshareDisplay {
        fn drop(&mut self) {
            if let Err(e) = self.power_off() {
                log::warn!("failed to power off display on drop: {e}");
            }
            self.rst.set_low();
            self.dc.set_low();
            self.pwr.set_low();
        }
    }
}

#[cfg(feature = "hardware")]
pub use driver::WaveshareDisplay;
