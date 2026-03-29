#!/usr/bin/env bash
# setup.sh — Check and optionally install cross-compilation prerequisites
# for building skagit-flats targeting aarch64-unknown-linux-gnu (Raspberry Pi).
#
# Usage:
#   ./scripts/setup.sh          # Check only, print instructions for missing deps
#   ./scripts/setup.sh --install # Attempt to install missing deps automatically

set -euo pipefail

TARGET="aarch64-unknown-linux-gnu"
ERRORS=0
WARNINGS=0

# Colors (disabled if not a terminal)
if [ -t 1 ]; then
    RED='\033[0;31m'
    YELLOW='\033[1;33m'
    GREEN='\033[0;32m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    RED='' YELLOW='' GREEN='' BOLD='' RESET=''
fi

ok()   { printf "${GREEN}[OK]${RESET}    %s\n" "$*"; }
fail() { printf "${RED}[MISSING]${RESET} %s\n" "$*"; ERRORS=$((ERRORS + 1)); }
warn() { printf "${YELLOW}[WARN]${RESET}   %s\n" "$*"; WARNINGS=$((WARNINGS + 1)); }
info() { printf "        %s\n" "$*"; }

detect_os() {
    if [[ "$(uname)" == "Darwin" ]]; then
        echo "macos"
    elif [[ -f /etc/debian_version ]]; then
        echo "debian"
    elif [[ -f /etc/fedora-release ]]; then
        echo "fedora"
    elif [[ -f /etc/arch-release ]]; then
        echo "arch"
    else
        echo "unknown"
    fi
}

OS="$(detect_os)"

printf "\n${BOLD}Cross-compilation prerequisite check${RESET}\n"
printf "Target: %s\n\n" "$TARGET"

# ── rustup ────────────────────────────────────────────────────────────────────
if command -v rustup &>/dev/null; then
    ok "rustup ($(rustup --version 2>&1 | head -1))"
else
    fail "rustup not found"
    info "Install from https://rustup.rs:"
    info "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

# ── Rust aarch64 target ───────────────────────────────────────────────────────
if command -v rustup &>/dev/null; then
    if rustup target list --installed 2>/dev/null | grep -q "^${TARGET}$"; then
        ok "rustup target ${TARGET}"
    else
        fail "rustup target ${TARGET} not installed"
        info "Run: rustup target add ${TARGET}"
    fi
fi

# ── aarch64 C cross-compiler ──────────────────────────────────────────────────
if command -v aarch64-linux-gnu-gcc &>/dev/null; then
    ok "aarch64-linux-gnu-gcc ($(aarch64-linux-gnu-gcc --version 2>&1 | head -1))"
else
    fail "aarch64-linux-gnu-gcc not found"
    case "$OS" in
        macos)
            info "Install via Homebrew:"
            info "  brew install aarch64-elf-gcc"
            info ""
            info "Then add to ~/.cargo/config.toml:"
            info "  [target.aarch64-unknown-linux-gnu]"
            info "  linker = \"aarch64-elf-gcc\""
            ;;
        debian)
            info "Install via apt:"
            info "  sudo apt-get install gcc-aarch64-linux-gnu"
            ;;
        fedora)
            info "Install via dnf:"
            info "  sudo dnf install gcc-aarch64-linux-gnu"
            ;;
        arch)
            info "Install via pacman:"
            info "  sudo pacman -S aarch64-linux-gnu-gcc"
            ;;
        *)
            info "Install the aarch64-linux-gnu cross-compiler for your platform."
            info "Search your package manager for: gcc-aarch64-linux-gnu"
            ;;
    esac
fi

# ── Cargo cross-compilation config ───────────────────────────────────────────
CARGO_CONFIG="${CARGO_HOME:-$HOME/.cargo}/config.toml"
if [ -f "$CARGO_CONFIG" ] && grep -q "aarch64-unknown-linux-gnu" "$CARGO_CONFIG" 2>/dev/null; then
    ok "Cargo linker config for ${TARGET}"
elif [ -f ".cargo/config.toml" ] && grep -q "aarch64-unknown-linux-gnu" ".cargo/config.toml" 2>/dev/null; then
    ok "Cargo linker config for ${TARGET} (.cargo/config.toml)"
else
    warn "No Cargo linker config found for ${TARGET}"
    info "Add to ~/.cargo/config.toml or .cargo/config.toml:"
    info "  [target.aarch64-unknown-linux-gnu]"
    case "$OS" in
        macos)
            info "  linker = \"aarch64-elf-gcc\""
            ;;
        *)
            info "  linker = \"aarch64-linux-gnu-gcc\""
            ;;
    esac
fi

# ── rsync ─────────────────────────────────────────────────────────────────────
if command -v rsync &>/dev/null; then
    ok "rsync ($(rsync --version 2>&1 | head -1 | awk '{print $3}'))"
else
    fail "rsync not found"
    case "$OS" in
        macos)   info "Install: brew install rsync" ;;
        debian)  info "Install: sudo apt-get install rsync" ;;
        fedora)  info "Install: sudo dnf install rsync" ;;
        arch)    info "Install: sudo pacman -S rsync" ;;
        *)       info "Install rsync via your package manager." ;;
    esac
fi

# ── ssh ───────────────────────────────────────────────────────────────────────
if command -v ssh &>/dev/null; then
    ok "ssh ($(ssh -V 2>&1 | head -1))"
else
    fail "ssh not found"
    case "$OS" in
        macos)   info "ssh is pre-installed on macOS; check your system." ;;
        debian)  info "Install: sudo apt-get install openssh-client" ;;
        fedora)  info "Install: sudo dnf install openssh-clients" ;;
        arch)    info "Install: sudo pacman -S openssh" ;;
        *)       info "Install openssh-client via your package manager." ;;
    esac
fi

# ── Summary ──────────────────────────────────────────────────────────────────
printf "\n"
if [ "$ERRORS" -eq 0 ] && [ "$WARNINGS" -eq 0 ]; then
    printf "${GREEN}${BOLD}All prerequisites satisfied.${RESET} Ready to cross-compile.\n\n"
    exit 0
elif [ "$ERRORS" -eq 0 ]; then
    printf "${YELLOW}${BOLD}%d warning(s).${RESET} Cross-compilation may work, but review the warnings above.\n\n" "$WARNINGS"
    exit 0
else
    printf "${RED}${BOLD}%d missing prerequisite(s)${RESET}" "$ERRORS"
    if [ "$WARNINGS" -gt 0 ]; then
        printf " and ${YELLOW}${BOLD}%d warning(s)${RESET}" "$WARNINGS"
    fi
    printf ". Follow the instructions above, then re-run:\n\n"
    printf "  make check-deps\n\n"
    exit 1
fi
