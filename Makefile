# Makefile for skagit-flats
#
# Pi deployment variables — override via environment or command line:
#   make deploy PI_HOST=pi@192.168.1.50

PI_HOST  ?= pi@skagit-flats.local
PI_BIN   ?= /usr/local/bin/skagit-flats
TARGET   ?= aarch64-unknown-linux-gnu

.PHONY: build build-pi deploy install-service clean

# Build for the host (no hardware feature).
build:
	cargo build --release

# Cross-compile for Raspberry Pi with SPI hardware support.
build-pi:
	cargo build --release --target $(TARGET) --features hardware

# Deploy binary and config to the Pi, then restart the service.
deploy: build-pi
	rsync -avz target/$(TARGET)/release/skagit-flats $(PI_HOST):$(PI_BIN)
	rsync -avz config.sample.toml $(PI_HOST):/etc/skagit-flats/config.toml --ignore-existing
	rsync -avz destinations.sample.toml $(PI_HOST):/etc/skagit-flats/destinations.toml --ignore-existing
	ssh $(PI_HOST) sudo systemctl restart skagit-flats

# Install the systemd service on the Pi (run once during initial setup).
install-service:
	rsync -avz deploy/skagit-flats.service $(PI_HOST):/tmp/skagit-flats.service
	ssh $(PI_HOST) 'sudo mv /tmp/skagit-flats.service /etc/systemd/system/ && \
		sudo useradd -r -s /usr/sbin/nologin skagit-flats 2>/dev/null || true && \
		sudo usermod -aG spi,gpio skagit-flats && \
		sudo mkdir -p /etc/skagit-flats && \
		sudo chown skagit-flats:skagit-flats /etc/skagit-flats && \
		sudo systemctl daemon-reload && \
		sudo systemctl enable skagit-flats'

clean:
	cargo clean
