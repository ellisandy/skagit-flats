# ADR: cargo-zigbuild for aarch64 Cross-Compilation

**Status:** Accepted
**Date:** 2026-04-02
**Issue:** sf-719

## Context

skagit-flats targets a Raspberry Pi (aarch64-unknown-linux-gnu) for hardware
deployment. Cross-compiling from a macOS development machine requires a C
linker that can emit Linux ELF binaries for the aarch64 architecture. The
project evaluated two approaches before landing on cargo-zigbuild.

## Approaches Evaluated

### aarch64-elf-gcc (Homebrew, bare-metal)

`brew install aarch64-elf-gcc` installs a bare-metal cross-compiler targeting
`aarch64-elf`, not `aarch64-linux-gnu`. The difference is significant:

- **Bare-metal (`aarch64-elf`)** targets systems with no operating system. It
  produces ELF objects but links against no libc and has no awareness of Linux
  ABI conventions, dynamic linker paths (`/lib/ld-linux-aarch64.so.1`), or
  glibc symbol versioning.
- **Linux userspace (`aarch64-linux-gnu`)** targets Linux userspace programs.
  It knows the sysroot layout, glibc ABI, and produces binaries that the Linux
  kernel can load and execute.

When Cargo invokes `aarch64-elf-gcc` as the linker for
`aarch64-unknown-linux-gnu`, the resulting binary may fail to run on the Pi
with errors like "No such file or directory" (missing dynamic linker) or link
errors for glibc-versioned symbols. Crates that include C code (e.g., `ring`,
`openssl-sys`) are particularly sensitive to this mismatch.

**Verdict:** Not suitable for Linux userspace targets. Works as a workaround
for simple binaries but breaks on any crate with C FFI against glibc.

### aarch64-linux-gnu-gcc (proper GNU toolchain)

`gcc-aarch64-linux-gnu` is the correct toolchain: it targets Linux userspace
and includes a matching sysroot for glibc. It is the standard cross-compiler
on Debian/Ubuntu (`apt install gcc-aarch64-linux-gnu`).

On macOS via Homebrew, **this toolchain is not available.** Homebrew does not
package `gcc-aarch64-linux-gnu` for macOS because it requires a Linux sysroot
(header files, libc stubs) that Homebrew does not distribute. The bare-metal
`aarch64-elf-gcc` is offered instead as the closest available alternative, but
it does not fulfill the same role.

**Verdict:** Correct solution on Linux. Not available in stock Homebrew on
macOS, making it unsuitable as the primary cross-compilation path for a
macOS-first development workflow.

### cargo-zigbuild (zig as linker)

`cargo-zigbuild` is a Cargo wrapper that substitutes the Zig compiler as the
C linker. Zig ships a built-in, self-contained C cross-compiler based on LLVM
that can target any supported triple — including `aarch64-unknown-linux-gnu` —
without requiring a separate sysroot or system cross-compiler package.

Key properties:
- **Single Homebrew package:** `brew install zig` provides everything needed on
  macOS. No separate sysroot or linker package required.
- **Linux-aware linking:** Zig's built-in libc stubs cover glibc symbol
  versioning. You can optionally pin the glibc version (e.g.,
  `aarch64-unknown-linux-gnu.2.28`) to guarantee binary compatibility with
  older Pi OS images.
- **No `.cargo/config.toml` linker override needed:** cargo-zigbuild handles
  linker selection internally.
- **Cross-platform consistency:** The same `cargo zigbuild` command works on
  macOS and Linux without platform-specific instructions.

**Verdict:** Solves the problem cleanly on macOS. Consistent behavior across
platforms.

## Decision

Use **cargo-zigbuild** for cross-compiling skagit-flats to
`aarch64-unknown-linux-gnu`.

```sh
cargo zigbuild --release --target aarch64-unknown-linux-gnu --features hardware
```

Prerequisites (macOS):
```sh
brew install zig
cargo install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu
```

## Trade-offs

| | cargo-zigbuild | aarch64-linux-gnu-gcc |
|---|---|---|
| macOS Homebrew install | `brew install zig` | Not available |
| Linux install | `snap install zig` or distro package | `apt install gcc-aarch64-linux-gnu` |
| glibc compatibility | Via zig libc stubs (version-pinnable) | Full sysroot (native glibc) |
| Familiarity | Less common; zig toolchain is non-obvious | Standard GNU cross-compiler |
| Crate C FFI (ring, etc.) | Works | Works |
| CI support | Requires zig in CI environment | Standard apt package |

**Accepted costs:**

- **Zig dependency:** Contributors need to install zig, which is not a standard
  Rust toolchain component. The setup script (`scripts/setup.sh`) and README
  document this explicitly.
- **Less familiar toolchain:** Zig-as-linker is not the conventional approach.
  Errors from the zig linker may be harder to search for than GCC linker errors.
- **Zig version sensitivity:** Some versions of zig have had bugs in their libc
  stubs. Pinning to a stable zig release reduces but does not eliminate this
  risk.

## Consequences

- `make build-pi` uses `cargo zigbuild` and works on macOS without any
  `.cargo/config.toml` modifications.
- `scripts/setup.sh` checks for `zig` and `cargo-zigbuild` instead of a
  GNU cross-compiler.
- CI environments that need to build for aarch64 must install zig (available
  via snap or direct download on Ubuntu).
- The bare-metal `aarch64-elf-gcc` check is no longer part of the dependency
  verification; contributors who have it installed get no benefit for this
  target.
