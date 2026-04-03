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

    // BCM GPIO pin assignments (matching Waveshare HAT).
    const RST_PIN: u8 = 17;
    const DC_PIN: u8 = 25;
    const BUSY_PIN: u8 = 24;
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
        busy: rppal::gpio::InputPin,
    }

    impl WaveshareDisplay {
        pub fn new() -> Result<Self, DisplayError> {
            let gpio = Gpio::new().map_err(|e| DisplayError::Spi(format!("GPIO init: {e}")))?;

            let rst = gpio
                .get(RST_PIN)
                .map_err(|e| DisplayError::Spi(format!("RST pin {RST_PIN}: {e}")))?
                .into_output();
            let dc = gpio
                .get(DC_PIN)
                .map_err(|e| DisplayError::Spi(format!("DC pin {DC_PIN}: {e}")))?
                .into_output();
            let busy = gpio
                .get(BUSY_PIN)
                .map_err(|e| DisplayError::Spi(format!("BUSY pin {BUSY_PIN}: {e}")))?
                .into_input();

            let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, SPI_CLOCK_HZ, Mode::Mode0)
                .map_err(|e| DisplayError::Spi(format!("SPI init: {e}")))?;

            let mut display = WaveshareDisplay {
                spi,
                rst,
                dc,
                busy,
            };

            display.hw_reset()?;
            display.init_panel()?;

            log::info!(
                "WaveshareDisplay initialized ({}x{}, SPI @ {} Hz)",
                WIDTH,
                HEIGHT,
                SPI_CLOCK_HZ
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

        fn wait_busy(&self) -> Result<(), DisplayError> {
            let start = std::time::Instant::now();
            // BUSY pin is HIGH when the panel is busy, LOW when idle (Waveshare 7.5" v2).
            while self.busy.is_high() {
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
            // Power setting.
            self.send_command(0x01)?;
            self.send_data(&[0x07, 0x07, 0x3F, 0x3F])?;

            // Power on.
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

            // VCOM and data interval setting.
            self.send_command(0x50)?;
            self.send_data(&[0x10, 0x07])?;

            // TCON setting.
            self.send_command(0x60)?;
            self.send_data(&[0x22])?;

            Ok(())
        }

        fn display_frame(&mut self, buffer: &[u8]) -> Result<(), DisplayError> {
            // Start data transmission (DTM2 for new data).
            self.send_command(0x13)?;
            self.send_data(buffer)?;

            // Display refresh.
            self.send_command(0x12)?;
            thread::sleep(Duration::from_millis(100));
            self.wait_busy()?;

            Ok(())
        }

        fn power_off(&mut self) -> Result<(), DisplayError> {
            self.send_command(0x02)?;
            self.wait_busy()?;
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

            match mode {
                RefreshMode::Full => {
                    log::debug!("WaveshareDisplay: full refresh");
                    // Re-initialize to clear ghosting artifacts.
                    self.init_panel()?;
                    self.display_frame(&buffer.pixels)?;
                }
                RefreshMode::Partial => {
                    log::debug!("WaveshareDisplay: partial refresh");
                    // Partial refresh sends the same data but skips re-init.
                    // The panel uses its built-in partial update LUT.
                    self.display_frame(&buffer.pixels)?;
                }
            }

            Ok(())
        }

        fn clear(&mut self) -> Result<(), DisplayError> {
            log::debug!("WaveshareDisplay: clearing display");
            self.init_panel()?;
            let white = vec![0x00u8; (WIDTH * HEIGHT / 8) as usize];
            self.display_frame(&white)?;
            self.power_off()?;
            Ok(())
        }
    }

    impl Drop for WaveshareDisplay {
        fn drop(&mut self) {
            if let Err(e) = self.power_off() {
                log::warn!("failed to power off display on drop: {e}");
            }
            self.rst.set_low();
        }
    }
}

#[cfg(feature = "hardware")]
pub use driver::WaveshareDisplay;
