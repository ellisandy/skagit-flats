//! Hardware integration tests — only run on a Raspberry Pi with a connected display.
//!
//! Gated behind the `hardware` feature AND the SKAGIT_HARDWARE_TESTS=1 env var.
//! Run with:
//!   SKAGIT_HARDWARE_TESTS=1 cargo test --features hardware --test hardware_tests

#![cfg(feature = "hardware")]

use skagit_flats::display::{DisplayDriver, RefreshMode};
use skagit_flats::render::PixelBuffer;

fn hardware_tests_enabled() -> bool {
    std::env::var("SKAGIT_HARDWARE_TESTS")
        .map(|v| v == "1")
        .unwrap_or(false)
}

#[test]
fn waveshare_init_and_clear() {
    if !hardware_tests_enabled() {
        eprintln!("skipping: SKAGIT_HARDWARE_TESTS not set");
        return;
    }

    let mut display = skagit_flats::display::waveshare::WaveshareDisplay::new()
        .expect("failed to initialize Waveshare display");
    display.clear().expect("failed to clear display");
}

#[test]
fn waveshare_full_refresh() {
    if !hardware_tests_enabled() {
        eprintln!("skipping: SKAGIT_HARDWARE_TESTS not set");
        return;
    }

    let mut display = skagit_flats::display::waveshare::WaveshareDisplay::new()
        .expect("failed to initialize Waveshare display");

    // Render a checkerboard pattern to visually verify the display works.
    let mut buf = PixelBuffer::new(800, 480);
    for y in 0..480 {
        for x in 0..800 {
            let black = ((x / 40) + (y / 40)) % 2 == 0;
            buf.set_pixel(x, y, black);
        }
    }

    display
        .update(&buf, RefreshMode::Full)
        .expect("full refresh failed");
}

#[test]
fn waveshare_partial_refresh() {
    if !hardware_tests_enabled() {
        eprintln!("skipping: SKAGIT_HARDWARE_TESTS not set");
        return;
    }

    let mut display = skagit_flats::display::waveshare::WaveshareDisplay::new()
        .expect("failed to initialize Waveshare display");

    let buf = PixelBuffer::new(800, 480);
    display
        .update(&buf, RefreshMode::Partial)
        .expect("partial refresh failed");
}

#[test]
fn waveshare_buffer_size_mismatch_rejected() {
    if !hardware_tests_enabled() {
        eprintln!("skipping: SKAGIT_HARDWARE_TESTS not set");
        return;
    }

    let mut display = skagit_flats::display::waveshare::WaveshareDisplay::new()
        .expect("failed to initialize Waveshare display");

    // Wrong dimensions should be rejected.
    let buf = PixelBuffer::new(640, 480);
    assert!(display.update(&buf, RefreshMode::Full).is_err());
}
