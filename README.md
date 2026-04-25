# skagit-flats

Thin e-ink display client for the Skagit Valley dashboard. Runs on a Raspberry Pi
with a Waveshare 7.5" v2 panel, fetches a pre-rendered PNG from a companion
server, and pushes it to the display over SPI.

The data fetching, layout, and rendering all happen upstream (see the
[`cascades`](../cascades) repo). This binary is the dumb end of the wire:
HTTP GET → decode PNG → SPI write → sleep → repeat.

---

## How it runs

```
fetch image_url → decode PNG → 1-bit pack → SPI write → sleep refresh_interval_secs → repeat
```

A failed fetch logs and retries on the next tick — the display keeps showing
the last successfully-pushed frame.

---

## Configuration

Copy `config.sample.toml` to `config.toml`:

```toml
[device]
image_url = "http://127.0.0.1:9090/image.png"
refresh_interval_secs = 60

[display]
width = 800
height = 480
```

The display dimensions must match the connected panel — the Waveshare driver
hard-fails on size mismatch.

---

## Running locally (no Pi)

```sh
cargo run --release -- --no-hardware --config config.toml
```

`--no-hardware` swaps the SPI driver for a no-op `NullDisplay`, so you can
verify the fetch loop works against a running upstream server without any
hardware attached.

---

## Building for the Pi

Cross-compile with the `hardware` feature enabled:

```sh
cargo zigbuild --release --target aarch64-unknown-linux-gnu --features hardware
```

Prereqs: `rustup target add aarch64-unknown-linux-gnu`, `brew install zig`,
`cargo install cargo-zigbuild`. See [`docs/decisions/cargo-zigbuild-aarch64.md`](docs/decisions/cargo-zigbuild-aarch64.md)
for why zigbuild over the alternatives.

---

## Deploying to the Pi

See [`QUICKSTART.md`](QUICKSTART.md) for the full first-time deploy flow.
Once configured:

```sh
make deploy PI_HOST=pi@skagit-flats.local
```

This cross-compiles, rsyncs the binary + sample config, and restarts the
systemd service.

---

## Hardware tests

Connected-panel smoke tests live in `tests/hardware_tests.rs` and are gated
behind both the `hardware` feature and `SKAGIT_HARDWARE_TESTS=1`:

```sh
SKAGIT_HARDWARE_TESTS=1 cargo test --features hardware --test hardware_tests
```
