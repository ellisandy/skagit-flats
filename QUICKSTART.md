# Skagit Flats — Quick Start

Two deployment paths are available: **Docker** (any machine, no hardware) and
**Raspberry Pi** (production, drives e-ink display). Start with Docker to
verify configuration before deploying to hardware.

---

## Path A: Docker (no hardware required)

Use this to explore the web UI, tune your configuration, and preview the
display layout before touching a Pi.

### Prerequisites

- Docker and Docker Compose installed
- Git (to clone the repo)

### Steps

1. **Clone the repository**

   ```sh
   git clone <repo-url> skagit-flats
   cd skagit-flats
   ```

2. **Start the stack**

   ```sh
   docker compose up
   ```

   This builds the image and starts the daemon with fixture data and the SPI
   driver disabled. No API keys or hardware required.

3. **Open the web UI**

   Visit `http://localhost:8080` in your browser.

   You should see a live preview of the e-ink layout populated with fixture data.

4. **Configure for your location** *(optional at this stage)*

   ```sh
   cp config.sample.toml config.toml
   cp destinations.sample.toml destinations.toml
   ```

   Edit `config.toml` to set your coordinates, and edit `destinations.toml` to
   define your destinations and go/no-go criteria. Then restart with your config
   mounted:

   ```sh
   docker compose down
   # Edit docker-compose.yml to point volumes at your config.toml / destinations.toml
   docker compose up
   ```

   Or set `SKAGIT_FIXTURE_DATA=0` (or remove that env var) to use live API calls.

### First successful output checkpoint

- Web UI loads at `http://localhost:8080`
- Dashboard panels are populated (fixture or live data)
- Go/no-go panel shows a decision for at least one destination

### Where to go next

- Tune `config.toml` poll intervals and `destinations.toml` criteria
- Add or remove data sources (see `AGENTS.md` → Source trait)
- Proceed to Path B when ready to run on Pi hardware

---

## Path B: Raspberry Pi (e-ink display)

Use this to deploy the daemon to a Pi with a Waveshare 7.5" e-ink display
connected over SPI.

### Prerequisites

**On the Pi:**
- Raspberry Pi OS (64-bit recommended)
- SPI enabled: `sudo raspi-config` → Interface Options → SPI → Enable
- SSH access configured (key-based recommended)

**On your development machine:**
- Rust toolchain with the Pi cross-compilation target:
  ```sh
  rustup target add aarch64-unknown-linux-gnu
  ```
- `rsync` and `ssh` installed
- Cross-linker for aarch64 (e.g. `gcc-aarch64-linux-gnu` on Debian/Ubuntu)

### Steps

1. **Verify cross-compilation dependencies**

   ```sh
   make check-deps
   ```

2. **Copy and edit configuration files**

   ```sh
   cp config.sample.toml config.toml
   cp destinations.sample.toml destinations.toml
   ```

   Set your location in `config.toml`:

   ```toml
   [location]
   latitude  = 48.4232
   longitude = -122.3351
   name      = "Mount Vernon, WA"
   ```

   Edit `destinations.toml` to define your destinations and go/no-go thresholds.

3. **Install the systemd service** *(first time only)*

   ```sh
   make install-service PI_HOST=pi@your-pi.local
   ```

   This creates the `skagit-flats` system user, adds it to the `spi` and `gpio`
   groups, installs the systemd unit, and enables the service on boot.

4. **Deploy the binary**

   ```sh
   make deploy PI_HOST=pi@your-pi.local
   ```

   This cross-compiles with the `hardware` feature enabled, rsyncs the binary
   and config files to the Pi (without overwriting existing configs), and
   restarts the service.

5. **Verify the service is running**

   ```sh
   ssh pi@your-pi.local sudo systemctl status skagit-flats
   ```

   Expected output includes `Active: active (running)`.

### First successful output checkpoint

- `systemctl status skagit-flats` shows `active (running)`
- Display refreshes within ~30 seconds of startup (shows a startup screen while
  first data fetch completes)
- Web UI is reachable at `http://your-pi.local:8080` from the local network

### Checking logs

```sh
ssh pi@your-pi.local sudo journalctl -u skagit-flats -f
```

The service auto-restarts on crash (`Restart=always`, `RestartSec=5`).

### Deploying updates

After changing code or config:

```sh
make deploy PI_HOST=pi@your-pi.local
```

Config files are synced with `--ignore-existing` — your customized
`config.toml` and `destinations.toml` on the Pi are never overwritten.

To push a config change explicitly:

```sh
rsync config.toml pi@your-pi.local:/etc/skagit-flats/config.toml
ssh pi@your-pi.local sudo systemctl restart skagit-flats
```

### Where to go next

- `config.toml` — adjust poll intervals, display geometry, location
- `destinations.toml` — add destinations, tune go/no-go criteria per signal
- Web UI at `http://your-pi.local:8080` — configure sources and preview layout
- `AGENTS.md` — architecture reference and contribution guide
- `docs/product/overview.md` — goals, non-goals, success criteria
- `docs/architecture/overview.md` — component design and data flow
