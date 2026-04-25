# skagit-flats — Quick Start

This is the thin e-ink client. It fetches a pre-rendered PNG from a companion
server and pushes it to a Waveshare 7.5" panel over SPI. There's no web UI,
no data sources, and no rendering on this side.

## Local smoke test (no hardware)

You need a server somewhere returning `image/png` at the configured URL. The
`cascades` repo serves one at `http://127.0.0.1:9090/image.png` by default.

```sh
cp config.sample.toml config.toml          # edit image_url if needed
cargo run --release -- --no-hardware --config config.toml
```

You should see one log line per refresh interval:

```
[INFO  skagit_flats::app] fetching http://127.0.0.1:9090/image.png every 60s (800x480)
[DEBUG skagit_flats::display] NullDisplay: update() called (no-op)
```

## Deploying to a Raspberry Pi

### Prerequisites

**Pi:**
- Raspberry Pi OS, SPI enabled (`sudo raspi-config` → Interface Options → SPI)
- SSH key access (run `make setup-ssh PI_HOST=pi@your-pi.local` once)

**Workstation:**
- `rustup target add aarch64-unknown-linux-gnu`
- `brew install zig && cargo install cargo-zigbuild`
- `rsync`, `ssh`

### First-time install

```sh
cp config.sample.toml config.toml           # edit image_url for your server
make install-service PI_HOST=pi@your-pi.local
make deploy          PI_HOST=pi@your-pi.local
```

`install-service` creates the `skagit-flats` system user (with `spi`/`gpio`
group access), installs the systemd unit, and enables it on boot.

`deploy` cross-compiles with `--features hardware`, rsyncs the binary +
`config.sample.toml` (without overwriting an existing `/etc/skagit-flats/config.toml`),
and restarts the service.

### Checking status

```sh
ssh pi@your-pi.local sudo systemctl status skagit-flats
ssh pi@your-pi.local sudo journalctl -u skagit-flats -f
```

The service auto-restarts on crash (`Restart=always`, `RestartSec=5`).

### Updating config

```sh
rsync config.toml pi@your-pi.local:/etc/skagit-flats/config.toml
ssh pi@your-pi.local sudo systemctl restart skagit-flats
```
