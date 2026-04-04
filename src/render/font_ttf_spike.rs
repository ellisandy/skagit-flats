/// TrueType font rendering spike — sf-rb0
///
/// # Evaluation Summary
///
/// ## Crate options considered
///
/// | Crate       | Status       | Native C? | 1-bit mode | Cross-compile | Embedded font |
/// |-------------|-------------|-----------|------------|---------------|---------------|
/// | `freetype`  | maintained   | YES (FreeType lib) | FT_RENDER_MODE_MONO native | Hard — needs libfreetype for target | via include_bytes! |
/// | `rusttype`  | archived     | no        | threshold coverage | Easy | via include_bytes! |
/// | `ab_glyph`  | maintained   | no        | threshold coverage | Easy | via include_bytes! |
/// | `fontdue`   | maintained   | no        | threshold coverage | Easy | via include_bytes! |
///
/// ## Recommendation: `fontdue`
///
/// For this project's constraints (1-bit e-ink, aarch64 cross-compile, Pi Zero 2 W):
///
/// - **Pure Rust**: no C library, `cargo build --target aarch64-unknown-linux-gnu` works
///   without extra sysroot setup.
/// - **Coverage-based rasterizer**: returns `u8` coverage per pixel (0–255);
///   threshold at 128 for clean 1-bit output. At the sizes we use (56–98px glyphs)
///   aliasing artifacts are negligible — the strokes are many pixels wide.
/// - **Embedded font with `include_bytes!`**: font data lives in the binary, no
///   filesystem dependency. NotoSans-Bold is ~562 KB; the binary grows by that amount.
/// - **Render latency** (measured in tests below): well under 1 ms per string on
///   desktop; Pi Zero 2 W estimate ~5–20 ms for a full layout refresh. The display
///   refresh cycle (~300 ms partial, ~2 s full) dominates — rendering is negligible.
/// - **No hinting at large sizes**: TrueType hinting matters most below ~20px.
///   At Hero (≈98px) and Large (≈56px) outlines look crisp without hinting.
///   `fontdue` skips hinting entirely, which is fine for our sizes.
///
/// ## `freetype` trade-off
///
/// `FT_RENDER_MODE_MONO` gives native 1-bit output with full hinting. Quality is
/// marginally better at small sizes, but at ≥56px the difference is not visible
/// on an e-ink panel. The cost: linking `libfreetype` for `aarch64-unknown-linux-gnu`
/// requires a sysroot or pre-built cross-compiled library, which adds CI and Docker
/// complexity. Not worth it.
///
/// ## Embedded vs. filesystem font
///
/// `include_bytes!("../../assets/fonts/NotoSans-Bold.ttf")` embeds 562 KB into
/// the binary. The Pi Zero 2 W has 512 MB RAM; this is negligible. No path
/// configuration needed, no extra files to deploy. **Verdict: embed the font.**
///
/// ## Target pixel sizes
///
/// Current bitmap font uses integer upscaling of a 5×7 base glyph:
/// - Hero (14×): glyph 70×98 px
/// - Large (8×): glyph 40×56 px
///
/// `fontdue`'s `px` parameter sets the line height (em). Cap height is ~70% of em
/// for NotoSans-Bold, so:
/// - Hero: `px = 140.0` → cap height ≈ 98 px  (matches bitmap Hero)
/// - Large: `px = 80.0` → cap height ≈ 56 px  (matches bitmap Large)

#[cfg(test)]
mod tests {
    use fontdue::{Font, FontSettings};

    /// Embedded Noto Sans Bold — 562 KB, ships with the binary.
    static FONT_BYTES: &[u8] =
        include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");

    fn load_font() -> Font {
        Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("NotoSans-Bold.ttf should parse without error")
    }

    /// Rasterize a single character to a 1-bit bitmap.
    ///
    /// Returns `(width, height, pixels)` where `pixels[y * width + x] == true`
    /// means black. Coverage threshold is 128 (out of 255).
    fn rasterize_char(font: &Font, ch: char, px: f32) -> (usize, usize, Vec<bool>) {
        let (metrics, coverage) = font.rasterize(ch, px);
        let pixels: Vec<bool> = coverage.iter().map(|&c| c >= 128).collect();
        (metrics.width, metrics.height, pixels)
    }

    /// Render a string to a 1-bit bitmap.
    ///
    /// Characters are placed side by side on a shared baseline. Returns
    /// `(total_width, canvas_height, pixels)`. Pixels default to white (false).
    fn render_string_1bit(font: &Font, text: &str, px: f32) -> (usize, usize, Vec<bool>) {
        // Collect per-char rasterization results.
        struct GlyphData {
            width: usize,
            height: usize,
            xmin: i32,
            ymin: i32,
            advance: usize,
            coverage: Vec<u8>,
        }

        let glyphs: Vec<GlyphData> = text
            .chars()
            .map(|ch| {
                let (m, cov) = font.rasterize(ch, px);
                GlyphData {
                    width: m.width,
                    height: m.height,
                    xmin: m.xmin,
                    ymin: m.ymin,
                    advance: m.advance_width as usize,
                    coverage: cov,
                }
            })
            .collect();

        if glyphs.is_empty() {
            return (1, 1, vec![false]);
        }

        // Canvas height: tallest glyph top above baseline.
        // For each glyph: top_above_baseline = ymin + height
        let max_top: i32 = glyphs.iter().map(|g| g.ymin + g.height as i32).max().unwrap_or(1);
        // Max depth below baseline: most negative ymin (descenders).
        let max_depth: i32 = glyphs.iter().map(|g| (-g.ymin).max(0)).max().unwrap_or(0);
        let canvas_h = (max_top + max_depth).max(1) as usize;
        // Baseline row (from top of canvas).
        let baseline = max_top as usize;

        // Canvas width: sum of advances + 1px inter-char gap.
        let total_w: usize = glyphs.iter().map(|g| g.advance + 1).sum::<usize>()
            .saturating_sub(1)
            .max(1);

        let mut pixels = vec![false; total_w * canvas_h];

        let mut x_cursor: usize = 0;
        for g in &glyphs {
            // Glyph top row relative to canvas top.
            // baseline is where ymin=0 sits; glyph top is baseline - (ymin + height) up from baseline.
            // row 0 of glyph bitmap = top of glyph = baseline - (ymin + height) from canvas top.
            let glyph_top_row =
                (baseline as i32 - (g.ymin + g.height as i32)).max(0) as usize;

            for row in 0..g.height {
                for col in 0..g.width {
                    let src = row * g.width + col;
                    let dst_x = (x_cursor as i32 + g.xmin + col as i32).max(0) as usize;
                    let dst_y = glyph_top_row + row;
                    if dst_x < total_w && dst_y < canvas_h && g.coverage[src] >= 128 {
                        pixels[dst_y * total_w + dst_x] = true;
                    }
                }
            }
            x_cursor += g.advance + 1;
        }

        (total_w, canvas_h, pixels)
    }

    /// Print a 1-bit pixel grid as ASCII art to stdout (for visual inspection).
    fn print_bitmap(label: &str, width: usize, height: usize, pixels: &[bool]) {
        println!("\n=== {label} ({width}×{height}) ===");
        for row in 0..height {
            let line: String = (0..width)
                .map(|col| if pixels[row * width + col] { '█' } else { ' ' })
                .collect();
            println!("|{line}|");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Correctness tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn font_loads_from_embedded_bytes() {
        let font = load_font();
        assert!(font.lookup_glyph_index('G') != 0, "G not found");
        assert!(font.lookup_glyph_index('O') != 0, "O not found");
        assert!(font.lookup_glyph_index('7') != 0, "7 not found");
        assert!(font.lookup_glyph_index('2') != 0, "2 not found");
        assert!(font.lookup_glyph_index('\u{00B0}') != 0, "° not found");
        assert!(font.lookup_glyph_index('F') != 0, "F not found");
    }

    /// "GO" at Hero pixel size (px=140, cap height ≈ 98 px).
    #[test]
    fn render_go_hero_size() {
        let font = load_font();
        let px = 140.0_f32;

        let (g_w, g_h, g_px) = rasterize_char(&font, 'G', px);
        let (o_w, o_h, o_px) = rasterize_char(&font, 'O', px);

        println!("G @ Hero (px={px}): {g_w}×{g_h}, {} black px",
            g_px.iter().filter(|&&b| b).count());
        println!("O @ Hero (px={px}): {o_w}×{o_h}, {} black px",
            o_px.iter().filter(|&&b| b).count());

        // Cap height of NotoSans-Bold at px=140 should be 80–120 px.
        assert!(g_h >= 70 && g_h <= 130,
            "G height {g_h} out of range for px=140");
        assert!(o_h >= 70 && o_h <= 130,
            "O height {o_h} out of range for px=140");

        // Glyphs must have substantial ink.
        assert!(g_px.iter().filter(|&&b| b).count() > 200,
            "G too few black pixels");
        assert!(o_px.iter().filter(|&&b| b).count() > 200,
            "O too few black pixels");
    }

    /// "72°F" at Large pixel size (px=80, cap height ≈ 56 px).
    #[test]
    fn render_temperature_large_size() {
        let font = load_font();
        let px = 80.0_f32;

        for ch in ['7', '2', '\u{00B0}', 'F'] {
            let (w, h, px_data) = rasterize_char(&font, ch, px);
            let black = px_data.iter().filter(|&&b| b).count();
            println!("  '{ch}': {w}×{h} px, {black} black pixels");
            assert!(w > 0 && h > 0,
                "'{ch}' rasterized to zero dimensions at px={px}");
            assert!(black > 0,
                "'{ch}' has no black pixels at px={px}");
        }
    }

    /// String render: "GO" at Hero, visual dump.
    #[test]
    fn render_go_string_hero_visual() {
        let font = load_font();
        let px = 140.0_f32;
        let (w, h, pixels) = render_string_1bit(&font, "GO", px);
        print_bitmap(&format!("'GO' Hero px={px}"), w, h, &pixels);

        let black = pixels.iter().filter(|&&b| b).count();
        assert!(black > 500, "GO should have >500 black pixels, got {black}");
        assert!(w > 80, "GO string width {w} too narrow");
    }

    /// String render: "72°F" at Large, visual dump.
    #[test]
    fn render_temperature_string_large_visual() {
        let font = load_font();
        let px = 80.0_f32;
        let (w, h, pixels) = render_string_1bit(&font, "72\u{00B0}F", px);
        print_bitmap(&format!("'72°F' Large px={px}"), w, h, &pixels);

        let black = pixels.iter().filter(|&&b| b).count();
        assert!(black > 200, "72°F should have >200 black pixels, got {black}");
        assert!(w > 50, "72°F width {w} too narrow");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Performance / size notes
    // ─────────────────────────────────────────────────────────────────────────

    /// Measure per-glyph rasterize latency.
    ///
    /// Desktop baseline (for reference); real-world Pi target is 10–40× slower.
    /// Conclusion: render time is negligible compared to display refresh latency.
    #[test]
    fn rasterize_latency_estimate() {
        use std::time::Instant;

        let font = load_font();
        let n = 200u128;

        let start = Instant::now();
        for _ in 0..n {
            let _ = font.rasterize('G', 140.0);
            let _ = font.rasterize('O', 140.0);
        }
        let per_glyph_us = start.elapsed().as_micros() / (n * 2);

        println!("Desktop rasterize latency: ~{per_glyph_us} µs/glyph at px=140");
        println!("Pi Zero 2W estimate (×20): ~{} µs/glyph", per_glyph_us * 20);
        println!("Full layout (~20 glyphs) on Pi: ~{} ms",
            per_glyph_us * 20 * 20 / 1000);

        assert!(per_glyph_us < 5_000,
            "Rasterize too slow on CI: {per_glyph_us} µs");
    }

    /// Embedded font binary size note.
    #[test]
    fn embedded_font_size() {
        let kb = FONT_BYTES.len() / 1024;
        println!("NotoSans-Bold embedded: {kb} KB ({} bytes)", FONT_BYTES.len());
        // Sanity: should be 400 KB – 2 MB.
        assert!(FONT_BYTES.len() > 400 * 1024, "Font too small — wrong file?");
        assert!(FONT_BYTES.len() < 2 * 1024 * 1024, "Font unexpectedly large");
    }
}
