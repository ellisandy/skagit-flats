# Architecture Assessment: SkagitFlats vs. TRMNL Thin-Client Pattern

**Date:** 2026-04-04
**Convoy:** `hq-cv-tfjc5`
**Research beads:** `hq-h4f` (current arch), `hq-17c` (TRMNL review), `hq-n1n` (comparison), `hq-9mq` (target arch), `hq-3n1` (migration plan)

---

## A. Executive Summary

- **TRMNL is a pure thin client.** The device is ~370 lines of Go. It does: poll server → download pre-rendered image URL → exec hardware binary. Zero composition or rendering on device.
- **SkagitFlats is a thick client.** It owns the full rendering stack locally: 1,256 lines of pixel-level layout code, font rasterization, icon drawing, sparkline rendering, and composition orchestration.
- **The TRMNL API contract is `{image_url, refresh_rate}`.** The device never receives layout data or content. Just a URL.
- **SkagitFlats already has an HTTP server** (`web/mod.rs`, 1,819 lines). This is the natural seam for the thin-client pattern — the Pi could serve itself a rendered image.
- **`render/layout.rs` is the primary offender.** Hardcoded zone geometry (lines 232–259) embedded across 1,200+ lines. No abstraction over spacing, font sizes, or column splits.
- **Rendering orchestration is duplicated 3–4 times.** `build_display_layout()` → `render_display()` appears independently in `app/mod.rs` (×3) and `web/mod.rs` (×1). No shared abstraction.
- **The presentation layer (`presentation/mod.rs`) is clean.** The domain → display-ready-structure conversion is well-separated. The problem is below it, in `render/`.
- **The `display/waveshare.rs` seam is already correct.** Hardware is isolated behind a trait. This is the boundary TRMNL uses too (its `show_img` binary).
- **Thin-client is achievable incrementally.** The web server already re-renders on demand for `/preview`. This is the entire thin-client model — just expose it as the official image source.
- **A rewrite is not required.** The natural migration is: extract `DisplayUpdater` → expose `/api/display-image` → shrink app loop to `fetch-local-server → display`.

---

## B. Current Architecture Diagnosis

### Module health summary

| Module | Status | Notes |
|--------|--------|-------|
| `domain/` | Clean | Pure data, no rendering knowledge |
| `config/` | Clean | Pure config structs |
| `evaluation/` | Mild coupling | Staleness thresholds hardcoded |
| `presentation/` | Clean | Domain → display structure, no pixel knowledge |
| `render/mod.rs` | Clean | Thin delegation layer |
| `render/layout.rs` | **Tightly coupled** | 1,256 lines of hardcoded pixel geometry |
| `render/font.rs` | Clean | Font data and metrics, no logic |
| `display/` | Clean | Hardware abstraction behind trait |
| `display/waveshare.rs` | Clean | Hardware-specific, isolated |
| `app/` | **Significant coupling** | Main loop orchestrates presentation + rendering, refresh policy hardcoded |
| `web/` | **Significant coupling** | Duplicates rendering orchestration from app loop |

### Critical hotspots

**1. Hardcoded zone geometry** (`render/layout.rs:232–259`)
```rust
const HEADER_Y: u32 = 0;     const HEADER_H: u32 = 28;
const HERO_Y: u32 = 30;      const HERO_H: u32 = 202;
const DATA_Y: u32 = 234;     const DATA_H: u32 = 140;
const CONTEXT_Y: u32 = 376;  const CONTEXT_H: u32 = 102;
const HERO_LEFT_W: u32 = 490; const HERO_DIVIDER_X: u32 = 492;
```
These zone boundaries are embedded across 1,200+ lines. Changing any of them requires hunting down every downstream calculation. This is layout policy masquerading as implementation.

**2. Scattered rendering orchestration** (`app/mod.rs:235–289`)
The sequence `lock state → build_display_layout() → render_display() → display.update()` appears **three times** in `app/mod.rs` (main loop, hourly refresh, destinations watcher) and **once more** in `web/mod.rs`. No shared abstraction.

**3. Inline sub-view composition** (`render/layout.rs:472–526`)
Hero zone reasons list, river level+trend arrow, ferry times — all composed inline with magic numbers (6×6 bullet, 4×4 sparkline marker, 2px pen, 64×64 icon assumed). No modular sub-renderer interface.

**4. Refresh policy in the wrong layer** (`app/mod.rs:235`)
`if elapsed >= 3600s → RefreshMode::Full else RefreshMode::Partial` lives in the app loop. This is display policy and should live in the display abstraction or a `DisplayUpdater` service.

**5. Missing line wrapping** (`render/layout.rs:472–526`)
Long reasons/warnings are clipped silently. No word-wrap fallback in hero left rendering.

**6. `RelevantSignals` in wrong layer** (`domain/mod.rs`, used in `presentation/`)
Signal filtering logic (which zones to show) is a presentation concern but lives in domain.

---

## C. Relevant TRMNL Architectural Patterns

**Pattern 1: The `{image_url, refresh_rate}` contract**
```go
type TerminalResponse struct {
    ImageURL    string `json:"image_url"`
    RefreshRate int    `json:"refresh_rate"`
}
```
Nothing else. No layout data, no plugin state, no compositing instructions. The server is the final rendering authority.

**Pattern 2: Device as glue, not renderer**
Device role in TRMNL: auth credential injection → `GET /api/display` → `GET image_url` → write file → exec `show_img`. The Go binary is 370 lines total, of which the core logic is under 150 lines.

**Pattern 3: Hardware isolation via native binary**
`show_img` is a compiled C++ binary. The Go binary execs it as a subprocess with `file=`, `invert=`, `mode=` args. This is exactly what `display/waveshare.rs` achieves in SkagitFlats — but TRMNL keeps it more aggressively isolated behind an exec boundary.

**Pattern 4: Refresh rate authority on server**
Server controls cadence via `refresh_rate` in the response. Device is not autonomous about when it wants new content.

**Pattern 5: One device-side display policy**
`(frames & 3) == 0 → mode=fast` — every 4th frame triggers a full EPD refresh to clear ghosting. This is hardcoded on device because the server has no knowledge of display state.

---

## D. Direct Comparison

| Concern | TRMNL | SkagitFlats | Verdict |
|--------|-------|-------------|---------|
| Rendering/composition | Server-side (zero device) | `render/layout.rs`, 1,256 lines | Over-owned |
| Layout policy | Server-side | Hardcoded constants in `layout.rs` | Over-owned |
| Font rendering | Server-side | `render/font.rs`, bitmap glyphs | Over-owned |
| Icon drawing | Server-side | 8+ icon functions in `layout.rs` | Over-owned |
| Sparklines | Server-side | `render_sparkline()` in `layout.rs` | Over-owned |
| Refresh rate scheduling | Server-owned (`refresh_rate`) | Hardcoded 3600s in app loop | Over-owned |
| Refresh mode (full/partial) | `show_img` args | App loop timer logic | Slightly over-owned |
| Hardware control | `show_img` binary | `display/waveshare.rs` (trait) | Equivalent |
| Data fetching | Server-side (device has no sources) | On-device (NOAA, USGS, WSDOT) | Correct — unavoidable for edge |
| Evaluation/decision logic | Server-side | `evaluation/mod.rs`, on-device | Design choice — user-private |
| API contract | `{image_url, refresh_rate}` | N/A (no client/server split today) | Missing abstraction |
| Orchestration (main loop) | 150 lines Go | Scattered across `app/` + `web/` | Disorganized |

**Key divergence:** TRMNL draws a sharp line between "what to show" (server) and "show it" (device). SkagitFlats collapses both into the device.

**The structural insight:** SkagitFlats is its own server. `web/mod.rs` already serves the device's own data and re-renders on demand. The thin-client model is one endpoint away.

---

## E. Recommended Target Architecture

### Principles

1. **The web server is the rendering authority.** `web/mod.rs` produces the canonical rendered image. The display loop fetches from it, like any TRMNL device fetches from its server.
2. **The display loop owns only hardware concerns.** It polls, downloads, and pushes pixels. Nothing else.
3. **Layout policy is declarative, not hardcoded.** Zone geometry lives in a `LayoutSpec` struct, not magic constants.
4. **No rendering orchestration in `app/`.** A `DisplayUpdater` service owns the build→render→update sequence.
5. **Sub-views are modular.** `HeroRenderer`, `DataRenderer`, `ContextRenderer` — each owns its zone.

### Responsibility map (target)

| Responsibility | Where it lives |
|---|---|
| Data fetching (NOAA, USGS, WSDOT) | On-device source threads (unchanged) |
| Evaluation/decision logic | `evaluation/` on-device (unchanged — user-private) |
| Layout specification | `LayoutSpec` in `render/` (declarative, testable) |
| Zone rendering | Sub-renderers: `HeroRenderer`, `DataRenderer`, etc. |
| Font/icon rendering | `render/` (unchanged, but better-abstracted) |
| Full image rendering | `web/` → `/api/display-image` endpoint |
| Display loop | Thin: poll localhost → download image → push hardware |
| Refresh rate | `web/` serves `refresh_rate` alongside image URL |
| Hardware control | `display/waveshare.rs` (unchanged) |

### What stays local

- Source threads + NOAA/USGS/WSDOT polling
- Evaluation and trip decision logic
- Display hardware driver
- Config and destinations

### What moves to "server" (within the same Pi process)

- Rendering orchestration (absorbed by web layer)
- Layout policy (now served declaratively)
- Image generation (`/api/display-image` → returns PNG)

### What becomes modular

- Zone renderers become composable sub-renderers
- `LayoutSpec` parameterizes geometry (no more hardcoded constants)
- `DisplayUpdater` is a shared service, not inline code

---

## F. Migration Plan

### Stage 1: Extract `DisplayUpdater` (no behavior change)

**Effort:** Small. **Risk:** Low. **Validation:** Tests still pass, display unchanged.

Extract the `build_display_layout() → render_display() → display.update()` sequence into a `DisplayUpdater::update(domain_state, destinations, mode)` method. Replace all 4 call sites (3 in `app/`, 1 in `web/`) with this call. Zero functional change.

**Seam:** `app/mod.rs:263` and all `build_display_layout` call sites.

### Stage 2: Introduce `LayoutSpec` (no behavior change)

**Effort:** Medium. **Risk:** Low. **Validation:** Golden image tests pass byte-for-byte.

Pull the zone geometry constants (lines 232–259) into a `LayoutSpec` struct with sensible defaults. `layout_and_render_display()` takes `&LayoutSpec`. Verify with golden tests that output is identical.

**Seam:** `render/layout.rs:232–259`.

### Stage 3: Extract sub-view renderers

**Effort:** Medium. **Risk:** Medium (golden test regression risk). **Validation:** Golden tests + visual spot-check.

Extract `render_hero_left/right`, `render_data_left/right`, `render_context_left/right` into `HeroRenderer`, `DataRenderer`, `ContextRenderer` structs. Each takes a sub-layout and a pixel sub-buffer. Parameterize magic numbers.

Fix the known bug: add word-wrap for hero reasons text.

**Seam:** `render/layout.rs:402–877` (zone render functions).

### Stage 4: Add `/api/display-image` endpoint

**Effort:** Small. **Risk:** Very low. **Validation:** Endpoint returns a valid PNG; display loop unchanged.

Add a handler to `web/mod.rs` that calls `DisplayUpdater::render_to_png()` and returns `image/png`. Also return `X-Refresh-Rate` header.

**Seam:** `web/mod.rs` — add route alongside existing `/preview`.

### Stage 5: Refactor display loop to thin client

**Effort:** Small. **Risk:** Medium (hardware loop change). **Validation:** Device displays correctly; HTTP roundtrip adds <100ms.

Replace the main display loop's direct render call with:
```
GET localhost:PORT/api/display-image → write to temp file → display.update(buf, mode)
```

The loop shrinks to ~30 lines. All rendering concern is gone from `app/`.

**Seam:** `app/mod.rs:235–289` (main loop).

### Stage 6: Move `RelevantSignals` to presentation

**Effort:** Tiny. **Risk:** Very low.

Move `RelevantSignals` from `domain/mod.rs` to `presentation/mod.rs`. Update the 2–3 call sites in `build_display_layout()`.

---

## G. Final Recommendation

### Recommended direction

Adopt the TRMNL thin-client pattern, self-hosted. The Pi already runs both the data daemon and the web server. Make the web server the rendering authority. Make the display loop a client of the local server. The boundary is `GET localhost/api/display-image` → `{image_bytes, refresh_rate}`.

This is not a rewrite. It is a responsibility rebalancing. The rendering code stays in Rust, on the same device. What changes is that rendering is orchestrated through `web/`, not through the app loop. The display loop becomes as thin as TRMNL's Go binary.

### Top 3 refactor opportunities

1. **Extract `DisplayUpdater`** — eliminates 3–4 duplicated orchestration call sites in `app/` and `web/`. Highest leverage per line changed. Should happen first because it unlocks everything else.

2. **Add `/api/display-image` endpoint** — the thin-client seam. Enables the display loop to become thin. Lets you iterate on rendering without touching hardware code. Also enables future TRMNL-compatible devices to display SkagitFlats content.

3. **Introduce `LayoutSpec` and extract sub-renderers** — eliminates the hardcoded-constants problem in `render/layout.rs`. Makes layout design changes safe. Without this, every UI redesign is a 1,200-line archaeology expedition.
