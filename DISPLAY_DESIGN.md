# SkagitFlats E-Ink Display Design Specification

**Display:** 800×480px, 1-bit monochrome (black/white only)
**Viewing distance:** 3–5 meters
**Refresh:** Waveshare 7.5" e-paper, full refresh ~2s, partial ~0.3s

---

## Design Goals

The current display is unreadable at distance: a 5×7 bitmap font (9px cell height)
produces characters roughly 0.5mm tall — invisible across a room. This spec replaces the
uniform grid with a hierarchy that communicates the single most important fact (GO/NO-GO)
at a glance from 5 meters, and supporting data at progressively closer reading distances.

**Principle:** Hierarchy over symmetry. Not all data is equal. Size = importance.

---

## Viewing Distance Font Size Requirements

Using the 1:200 legibility rule (letter height ≥ viewing distance / 200):

| Viewing Distance | Min Letter Height | Min Pixel Height (96ppi) |
|-----------------|-------------------|--------------------------|
| 3 m             | 15 mm             | 56 px                    |
| 4 m             | 20 mm             | 75 px                    |
| 5 m             | 25 mm             | 94 px                    |

**Minimum for critical data read at 5 m: 96 px tall glyphs (hero size)**
**Minimum for supporting data read at 3 m: 56 px tall glyphs (primary size)**

The current 5×7 font is insufficient for any glanceable data. All critical numbers
must use bitmap fonts sized to these specifications.

### Font Size Tiers

| Tier      | Pixel Height | Usage                                    |
|-----------|-------------|------------------------------------------|
| **Hero**  | 96 px       | GO / NO GO decision text                 |
| **Large** | 56 px       | Temperature, river level, next ferry time|
| **Medium**| 28 px       | Flow rate, wind, vessel name, additional departures |
| **Small** | 18 px       | Panel titles, trail/road status, reason bullets |
| **Micro** | 14 px       | Last-updated timestamp, site names       |

All fonts must be bitmap (pixel-aligned, zero antialiasing). Minimum stroke width: 2 px.
Thin 1-pixel strokes are unreliable on e-ink and must not be used in data glyphs.

Recommended bitmap font families: Terminus, ProggyClean, or a custom bold face.
The existing 5×7 font may remain for Micro tier only (non-critical metadata).

---

## Layout Zones

The 800×480 canvas is divided into four horizontal bands. Bands do NOT have equal height.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  HEADER  [800 × 28 px]                                                y=0    │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                               │
│  HERO ZONE  [800 × 202 px]                                            y=30   │
│                                                                               │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                               │
│  DATA ZONE  [800 × 140 px]                                            y=234  │
│                                                                               │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                               │
│  CONTEXT ZONE  [800 × 102 px]                                         y=376  │
│                                                                               │
└──────────────────────────────────────────────────────────────────────────────┘
```

Horizontal dividers: 2 px black lines at y=28, y=232, y=374.
Total: 28 + 2 + 202 + 2 + 140 + 2 + 102 + 2 = 480 px. ✓

---

## Zone 1: Header Strip (y=0, h=28)

Single horizontal bar across the full width.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ SKAGIT FLATS          SKAGIT RIVER @ MT VERNON          Updated 14:32        │
└──────────────────────────────────────────────────────────────────────────────┘
```

- Left: App name or destination name. Font: Small (18px). Left-aligned, x=16.
- Center: Primary river site name (optional, if space). Font: Micro (14px). Centered.
- Right: "Updated HH:MM". Font: Micro (14px). Right-aligned, x=784.
- Background: White. No border.

---

## Zone 2: Hero Zone (y=30, h=202)

Two columns, unequal width. Left column is the primary GO/NO-GO decision.

```
┌─────────────────────────────────────┬──────────────────────────────┐
│                                     │                               │
│         GO                          │  [cloud icon 64×64]   52°F   │
│                                     │                               │
│                                     │  Overcast                     │
│                                     │  SW 12 mph  PoP 30%          │
│                                     │                               │
└─────────────────────────────────────┴──────────────────────────────┘
  x=0, w=490                           x=494, w=306
```

### Column A: GO/NO-GO Decision (x=0, w=490)

**GO state:**
- "GO" rendered at Hero size (96px), centered vertically within 202px column.
- Bold, centered horizontally in the 490px column.
- No border, white background.

**NO GO state:**
- "NO GO" at Hero size (96px), top-aligned within column (y_offset=20).
- Below the hero text: reason bullets at Small size (18px), left-aligned at x=24.
- Max 4 reasons visible. If more, show "...+N more" at Micro size.
- Bullet character: filled 6×6 square (not the current "•" text char).

**Divider between columns:** 2 px vertical black line at x=492, y=30 to y=232.

### Column B: Weather Panel (x=494, w=306)

Layout within column (internal padding 12px each side):
```
y offset within column:
  8 px   — top padding
  64 px  — weather icon (left-aligned at x=506)
           temperature (Large, 56px) right of icon, y-baseline aligned to icon center
  6 px   — gap
  22 px  — sky condition text (Small, 18px)
  6 px   — gap
  20 px  — "DIR SPD mph  PoP XX%" (Micro, 14px)
  remaining — padding
```

**Weather icon position:** x=506, y=38 (icon top), 64×64 px.
**Temperature position:** x=580, y=46. Font: Large (56px). Right-aligned to x=790.
**Sky condition:** x=506, y=114. Font: Small (18px).
**Wind + precipitation line:** x=506, y=142. Font: Micro (14px).

---

## Zone 3: Data Zone (y=234, h=140)

Two equal columns: River Gauge (left) and Ferry Status (right).

```
┌──────────────────────────────────────┬─────────────────────────────────────┐
│ SKAGIT R @ MOUNT VERNON              │ ANACORTES FERRY                      │
│                                      │                                      │
│  11.9 ft    ↑                        │  Next: 14:30                         │
│  8,750 cfs                           │  16:00    18:30                      │
│                                      │  MV Samish                           │
│  [sparkline: 24h river trend]        │                                      │
└──────────────────────────────────────┴─────────────────────────────────────┘
  x=0, w=396                             x=400, w=400
```

**Vertical divider:** 2 px at x=398, y=234 to y=374.

### Column A: River Gauge (x=0, w=396)

Internal layout (left padding 12px, right padding 12px):
```
y=238 — site name (Micro, 14px, truncated to fit)
y=256 — water level "XX.X ft" (Large, 56px) + trend arrow (Medium, 28px)
        level text left-aligned at x=12
        trend arrow: ↑ ↓ → at x=200, y=270 (Medium size)
y=318 — streamflow "X,XXX cfs" (Medium, 28px) at x=12
y=350 — sparkline: 380×22px, x=8, y=350
```

**Trend arrow glyphs:** Three states only:
- Rising (>0.2 ft/hr): ↑ (up arrow, 28px)
- Falling (>0.2 ft/hr): ↓ (down arrow, 28px)
- Stable: omitted (no icon displayed)

### Sparkline (River Gauge, 24h trend)

Position: x=8, y=350, w=380, h=22
Data: up to 24 hourly readings (or fewer if not available)
Rendering:
- Normalize all readings to the 22px height range (min to max of dataset).
- Connect data points with 2px polyline.
- Mark the current (rightmost) reading with a 4×4 filled square.
- If a flood threshold is configured, draw a 2px horizontal dashed line (4 on / 2 off).
- No axes, no labels, no ticks. The sparkline is supplementary visual context only.

### Column B: Ferry Status (x=400, w=400)

Internal layout (left padding 12px, right padding 12px):
```
y=238 — route name (Micro, 14px, truncated)
y=256 — "Next: HH:MM" (Large, 56px) at x=412
         — if no departures: "NO SERVICE" at same size
y=320 — second departure "HH:MM" (Medium, 28px) at x=412
y=354 — third departure "HH:MM" (Medium, 28px) at x=412 (if available)
y=358 — vessel name (Micro, 14px) right-aligned at x=792 (bottom of column)
```

Ferry departure times are shown as HH:MM in 24-hour format. If fewer than 3 departures
are available, remaining lines are blank. If zero departures, show "NO SERVICE" at
Large size, centered vertically.

---

## Zone 4: Context Zone (y=376, h=102)

Two equal columns: Trail (left) and Road (right).

```
┌──────────────────────────────────────┬─────────────────────────────────────┐
│ CASCADE PASS TRAIL                   │ SR-20 NORTH CASCADES HWY             │
│                                      │                                      │
│ Snow above 5000 ft                   │ CLOSED                               │
│ Suitable for day hikes below snow    │ Newhalem to Rainy Pass               │
└──────────────────────────────────────┴─────────────────────────────────────┘
  x=0, w=396                             x=400, w=400
```

**Vertical divider:** 2 px at x=398, y=376 to y=480.

### Column A: Trail Condition (x=0, w=396)

```
y=380 — destination name (Small, 18px) at x=12
y=402 — suitability summary line 1 (Small, 18px) at x=12
y=424 — suitability summary line 2 (Small, 18px) at x=12 (if wrapped)
y=450 — last updated (Micro, 14px) at x=12
```

Text wraps at word boundaries to fit 372px (396 - 24px padding).
Max 2 lines of summary text. If text overflows, truncate with "…".

### Column B: Road Status (x=400, w=400)

```
y=380 — road name (Small, 18px) at x=412
y=402 — STATUS text (Medium, 28px, ALL CAPS) at x=412
         inverted (white text on black rectangle) if status is "CLOSED"
y=436 — affected segment (Small, 18px) at x=412
```

**Road status rendering by value:**

| Status     | Display Text | Rendering               |
|------------|-------------|-------------------------|
| `open`     | OPEN        | Normal text             |
| `closed`   | CLOSED      | Inverted (white on black box) |
| `restricted` | RESTRICT  | Normal text, 28px       |

The black-on-white inversion for CLOSED gives an immediate visual alert without color.

---

## Icon Specifications

All icons are 1-bit bitmap. No antialiasing. No gradients. Stroke weight ≥ 2 px.

### Weather Condition Icons (64×64 px for hero column, 32×32 for others)

Each icon must be clean at 32×32. Scale up 2× for 64×64 by pixel-doubling (not resampling).

| Sky Condition          | Icon Description                              |
|-----------------------|-----------------------------------------------|
| Clear / Sunny         | Filled circle (sun) + 8 radiating lines       |
| Partly Cloudy         | Half-circle sun partially behind cloud outline|
| Mostly Cloudy         | Solid cloud outline, no sun visible           |
| Overcast              | Two overlapping cloud outlines, filled        |
| Rain / Showers        | Cloud + 3–4 vertical line segments below      |
| Heavy Rain            | Cloud + dense vertical lines, thick stroke    |
| Drizzle               | Cloud + 3 dots below                         |
| Snow                  | Cloud + 3 asterisk (*) symbols below         |
| Thunderstorm          | Cloud + zigzag lightning bolt below          |
| Fog                   | 4 horizontal lines of decreasing length      |
| Wind                  | 3 curved horizontal lines (swoosh pattern)   |

**Icon rendering rules:**
- Sun circle: 12 px radius at 32×32, 8 rays 4px long at 45° intervals
- Cloud: rounded rectangle approximation using 2px corners, 2px stroke
- Rain lines: 2px wide, 6px tall, 4px spacing, 30° angle from vertical
- Lightning: 3-segment zigzag, 2px stroke, contained within lower 40% of icon

### Trend Arrow Glyphs (28×28 px)

Simple filled arrowhead shapes:
- **Up arrow (↑):** Triangle pointing up (base 16px wide, height 12px) + 4×8px stem
- **Down arrow (↓):** Mirror of up arrow
- All strokes 2px minimum; filled solid black

### Status Indicator (for future use)

16×16 filled circle: fully black = GO, fully white with border = NO GO.

---

## Typography Rules for 1-bit E-Ink

1. **No thin strokes.** Minimum 2px stroke weight for all rendered elements. 1px
   strokes may disappear or appear irregular on e-ink due to subpixel inconsistencies.

2. **No antialiasing.** All text and icons rendered as pure black or white pixels.
   Intermediate gray values must not appear in the final bitmap.

3. **Minimum body text: 18px tall (Small tier).** Anything smaller is not readable
   at the intended viewing distance and should be used only for truly supplementary data.

4. **Bold weight only below 28px.** Regular-weight fonts at small sizes lose stroke mass
   and become illegible. Use bold/heavy variants for all text below Large tier.

5. **Inter-element spacing: 6px minimum.** Elements touching or with 1–2px gaps
   visually merge on e-ink. Maintain at least 6px clear space between distinct data items.

6. **Panel padding: 12px.** All text starts 12px from any border or divider line.

7. **Uppercase status words.** "CLOSED", "OPEN", "GO", "NO GO" are uppercase for
   instant visual recognition from distance.

---

## Before/After Panel Descriptions

### Weather: Before vs After

**Before (current):**
```
┌──────────┐
│ Weather  │  ← 5x7 font title
├──────────┤
│52°F  Mos │  ← "Mostly Cloudy" truncated
│tly Cloudy│  ← word wrap
│Wind SW at│  ← "10 mph" truncated
│ 10 mph   │
└──────────┘
```
~6 lines of tiny text. Unreadable at 3m.

**After:**
```
┌────────────────────────────────────────────────────────────────────────────┐
│                   [64×64 overcast icon]           52°F                    │
│                                                                            │
│                   Overcast                                                 │
│                   SW 12 mph   PoP 30%                                     │
└────────────────────────────────────────────────────────────────────────────┘
```
Temperature in 56px is readable at 4m. Icon is recognizable at 5m.

---

### River Gauge: Before vs After

**Before:**
```
┌──────────────┐
│ Skagit River │  ← truncated to panel width
├──────────────┤
│11.9 ft       │
│8750 cfs      │
│13:00         │
└──────────────┘
```

**After:**
```
┌─────────────────────────────────────────────────────────────────────┐
│ SKAGIT R @ MOUNT VERNON                                             │
│                                                                     │
│  11.9 ft  ↑                                                         │
│  8,750 cfs                                                          │
│  ▁▂▃▄▃▂▃▄▅▆▅▄▃▄▅▆▇▇▆▅▄▃▂▁  ← 24h sparkline                       │
└─────────────────────────────────────────────────────────────────────┘
```
Level "11.9 ft" at 56px is readable at 4m. Rising arrow signals urgency.
Sparkline gives trend context without numbers.

---

### Ferry Status: Before vs After

**Before:**
```
┌───────────────────┐
│ Ferry — Anacortes │
├───────────────────┤
│ MV Samish         │
│ Departs 10:30     │
│ Departs 12:30     │
│ Departs 14:30     │
└───────────────────┘
```

**After:**
```
┌─────────────────────────────────────────────────────────────────────┐
│ ANACORTES FERRY                                                     │
│                                                                     │
│  Next: 14:30                                                        │
│  16:00    18:30                                                     │
│                                                          MV Samish  │
└─────────────────────────────────────────────────────────────────────┘
```
"Next: 14:30" at 56px answers the primary question at distance.
Subsequent departures in 28px for close reading.

---

### Trip Decision: Before vs After

**Before:**
```
┌──────────┐
│Skagit Lp │
├──────────┤
│GO        │
└──────────┘
```
or
```
┌──────────┐
│Baker Lake│
├──────────┤
│NO GO     │
│• Too cold│
│• Rd clsd │
└──────────┘
```

**After (GO state, full hero):**
```
┌──────────────────────────────────────────────────────────┐
│                                                          │
│                                                          │
│                         GO                              │
│                                                          │
│                                                          │
└──────────────────────────────────────────────────────────┘
```
"GO" at 96px is readable at 5m.

**After (NO GO state):**
```
┌──────────────────────────────────────────────────────────┐
│  NO GO                                                   │
│                                                          │
│  ■ River too high (11.9 ft > 10 ft limit)               │
│  ■ SR-20 closed at Newhalem                              │
└──────────────────────────────────────────────────────────┘
```
"NO GO" at 96px, reasons at 18px (readable at 1m for detail).

---

### Trail / Road: Before vs After

**Before:**
```
┌──────────────┐       ┌──────────────┐
│ Cascade Pass │       │ SR-20        │
├──────────────┤       ├──────────────┤
│ Snow above   │       │ CLOSED —     │
│ 5000ft       │       │ Newhalem to  │
└──────────────┘       │ Rainy Pass   │
                       └──────────────┘
```

**After:**
```
┌──────────────────────────┬─────────────────────────────┐
│ CASCADE PASS TRAIL       │ SR-20 NORTH CASCADES HWY    │
│                          │                             │
│ Snow above 5000 ft       │ ██████ CLOSED ██████        │
│ Day hikes OK below snow  │ Newhalem to Rainy Pass      │
└──────────────────────────┴─────────────────────────────┘
```
Road closed gets inverted background for immediate visual alert.

---

## Refresh Strategy

- **Full refresh (every 60 min):** Clears ghosting. Required for clean display. ~2s blank flash is acceptable.
- **Partial refresh (every 5–10 min):** Updates changed panels only. ~0.3s, no full clear.

For partial refresh, track which zones changed between renders and only redraw
those regions. The sparkline and ferry times change most frequently; GO/NO-GO
changes less often but should always trigger immediate full refresh when it flips.

---

## Implementation Notes for Rust Developer

### New Types Required

```rust
/// Font size tiers used by the new renderer.
pub enum FontSize {
    Hero,    // 96px tall glyphs
    Large,   // 56px tall glyphs
    Medium,  // 28px tall glyphs
    Small,   // 18px tall glyphs
    Micro,   // 14px tall glyphs (existing 5x7 scaled up, or new font)
}

/// A bitmap icon indexed by weather condition.
pub enum WeatherIcon {
    Clear, PartlyCloudy, MostlyCloudy, Overcast,
    Rain, HeavyRain, Drizzle, Snow, Thunderstorm, Fog, Wind,
}

/// Sparkline data: normalized time-series for river gauge.
pub struct Sparkline {
    pub values: Vec<f32>,   // raw readings, oldest first
    pub threshold: Option<f32>, // flood threshold line if configured
}
```

### Pixel Budget Per Zone (summary)

| Zone    | y range    | h px | Primary content                    |
|---------|------------|------|------------------------------------|
| Header  | 0–28       | 28   | App name, last-updated             |
| Hero    | 30–232     | 202  | GO/NO-GO (56%) + Weather (44%)     |
| Data    | 234–374    | 140  | River gauge + Ferry status         |
| Context | 376–480    | 104  | Trail + Road                       |

### Character Budget (approx chars at 56px font, assume 32px wide glyph)

A 400px-wide column at Large (56px) holds approximately 400/32 = **12 characters**.
Design content strings to fit. The temperature "52°F" (4 chars) fits easily.
River level "11.9 ft" (7 chars) fits easily. Avoid wrapping at hero sizes.

### Bitmap Font Sources

Recommended pre-existing bitmap fonts to embed:
- **Terminus Bold** — widely available, clean at all sizes, open source
- **Spleen** — designed specifically for small displays, clean at 12×24, 16×32
- **Cozette** — 6×13 for micro text, scaling not needed

For hero (96px) and large (56px), custom bold numeric-only fonts are acceptable.
A numeric-only subset (digits 0–9, colon, period, slash, degree symbol, space) covers
all hero-tier content and can be embedded as a small static table.

---

## Open Questions (for next implementation bead)

1. **Font rendering engine:** Embed pre-rasterized bitmap font tables (like the existing
   5×7 approach) or add a vector font renderer (higher quality, larger binary)?
   Recommendation: bitmap tables at each required size. Fast, deterministic, no deps.

2. **Multiple destinations:** If multiple GO/NO-GO destinations are configured, the hero
   zone can only show one at a time. Options: (a) always show the "most critical" (first
   NO GO if any, else GO), (b) cycle through destinations on each refresh.
   Recommendation: show worst-case: any NO GO takes priority; if all GO, show "ALL GO".

3. **Missing data:** If a data source fails, show "---" in the value field at the
   appropriate font size. Do not hide the panel; keep layout stable.

4. **Dynamic layout:** The spec above assumes all 6 data types are always present. If
   trail and road data are absent, the context zone could be collapsed and the data zone
   expanded. Implementation of this is optional.
