use crate::presentation::Panel;
use crate::render::font::{self, CELL_HEIGHT, CELL_WIDTH, GLYPH_HEIGHT, GLYPH_WIDTH};
use crate::render::PixelBuffer;

/// Padding inside each panel border (pixels).
const PANEL_PADDING: u32 = 2;
/// Border thickness (pixels).
const BORDER_WIDTH: u32 = 1;
/// Height of the title bar (border + padding + glyph + padding + divider).
const TITLE_BAR_HEIGHT: u32 = BORDER_WIDTH + PANEL_PADDING + GLYPH_HEIGHT + PANEL_PADDING + BORDER_WIDTH;
/// Gap between panels (pixels).
const PANEL_GAP: u32 = 2;

/// A rectangle on the pixel buffer.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Compute a grid layout for `n` panels within the given dimensions.
fn compute_grid(n: usize, total_w: u32, total_h: u32) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }

    let cols = match n {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 2,
        5 | 6 => 3,
        _ => ((n as f32).sqrt().ceil() as usize).max(1),
    };
    let rows = n.div_ceil(cols);

    let cell_w = (total_w - PANEL_GAP * (cols as u32).saturating_sub(1)) / cols as u32;
    let cell_h = (total_h - PANEL_GAP * (rows as u32).saturating_sub(1)) / rows as u32;

    let mut rects = Vec::with_capacity(n);
    for i in 0..n {
        let row = i / cols;
        let col = i % cols;
        rects.push(Rect {
            x: col as u32 * (cell_w + PANEL_GAP),
            y: row as u32 * (cell_h + PANEL_GAP),
            w: cell_w,
            h: cell_h,
        });
    }
    rects
}

/// Draw a 1-pixel border rectangle.
fn draw_border(buf: &mut PixelBuffer, r: Rect) {
    // Top and bottom edges
    for x in r.x..r.x + r.w {
        buf.set_pixel(x, r.y, true);
        buf.set_pixel(x, r.y + r.h - 1, true);
    }
    // Left and right edges
    for y in r.y..r.y + r.h {
        buf.set_pixel(r.x, y, true);
        buf.set_pixel(r.x + r.w - 1, y, true);
    }
}

/// Fill a rectangular region with a solid color.
fn fill_rect(buf: &mut PixelBuffer, r: Rect, black: bool) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            buf.set_pixel(x, y, black);
        }
    }
}

/// Draw a single glyph at (x, y), with the given polarity.
/// If `invert` is true, draws white-on-black (assumes background already filled).
fn draw_glyph(buf: &mut PixelBuffer, x: u32, y: u32, ch: char, invert: bool) {
    let glyph = font::glyph(ch);
    for row in 0..GLYPH_HEIGHT {
        let bits = glyph[row as usize];
        for col in 0..GLYPH_WIDTH {
            let pixel_on = (bits >> (4 - col)) & 1 == 1;
            let draw_black = if invert { !pixel_on } else { pixel_on };
            buf.set_pixel(x + col, y + row, draw_black);
        }
    }
}

/// Draw a string starting at (x, y). Returns the number of characters drawn.
fn draw_text(
    buf: &mut PixelBuffer,
    x: u32,
    y: u32,
    text: &str,
    max_width: u32,
    invert: bool,
) -> usize {
    let max_chars = (max_width / CELL_WIDTH) as usize;
    let mut drawn = 0;
    for ch in text.chars().take(max_chars) {
        draw_glyph(buf, x + drawn as u32 * CELL_WIDTH, y, ch, invert);
        drawn += 1;
    }
    drawn
}

/// Render a single panel into its allocated rectangle.
fn render_panel(buf: &mut PixelBuffer, panel: &Panel, rect: Rect) {
    if rect.w < CELL_WIDTH + 2 * BORDER_WIDTH || rect.h < TITLE_BAR_HEIGHT + CELL_HEIGHT {
        return; // too small to render anything meaningful
    }

    // Draw border
    draw_border(buf, rect);

    // Title bar: filled black region inside the border at the top
    let title_rect = Rect {
        x: rect.x + BORDER_WIDTH,
        y: rect.y + BORDER_WIDTH,
        w: rect.w - 2 * BORDER_WIDTH,
        h: TITLE_BAR_HEIGHT - BORDER_WIDTH, // don't double-count the top border
    };
    fill_rect(buf, title_rect, true);

    // Render title text (white on black)
    let text_x = rect.x + BORDER_WIDTH + PANEL_PADDING;
    let text_y = rect.y + BORDER_WIDTH + PANEL_PADDING;
    let text_max_w = rect.w.saturating_sub(2 * (BORDER_WIDTH + PANEL_PADDING));
    draw_text(buf, text_x, text_y, &panel.title, text_max_w, true);

    // Horizontal divider below title bar
    let divider_y = rect.y + TITLE_BAR_HEIGHT;
    for x in rect.x + BORDER_WIDTH..rect.x + rect.w - BORDER_WIDTH {
        buf.set_pixel(x, divider_y, true);
    }

    // Body area
    let body_x = rect.x + BORDER_WIDTH + PANEL_PADDING;
    let body_y = divider_y + 1 + PANEL_PADDING;
    let body_w = rect.w.saturating_sub(2 * (BORDER_WIDTH + PANEL_PADDING));
    let body_h = rect
        .h
        .saturating_sub(TITLE_BAR_HEIGHT + 1 + PANEL_PADDING + BORDER_WIDTH);

    let chars_per_line = (body_w / CELL_WIDTH) as usize;
    if chars_per_line == 0 {
        return;
    }

    let max_lines = (body_h / CELL_HEIGHT) as usize;
    let mut line_idx = 0;

    for row_text in &panel.rows {
        if line_idx >= max_lines {
            break;
        }
        // Word-wrap the row text
        let wrapped = wrap_text(row_text, chars_per_line);
        for line in wrapped {
            if line_idx >= max_lines {
                break;
            }
            let ly = body_y + line_idx as u32 * CELL_HEIGHT;
            draw_text(buf, body_x, ly, &line, body_w, false);
            line_idx += 1;
        }
    }
}

/// Word-wrap text to fit within `max_chars` columns.
/// Falls back to character-level wrapping for long words.
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if word.len() > max_chars {
            // Long word: flush current line, then character-break
            if !current_line.is_empty() {
                lines.push(current_line);
                current_line = String::new();
            }
            let mut remaining = word;
            while remaining.len() > max_chars {
                let (chunk, rest) = remaining.split_at(max_chars);
                lines.push(chunk.to_string());
                remaining = rest;
            }
            if !remaining.is_empty() {
                current_line = remaining.to_string();
            }
        } else if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= max_chars {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Lay out and render all panels into the pixel buffer.
pub fn layout_and_render(panels: &[Panel], buf: &mut PixelBuffer) {
    let grid = compute_grid(panels.len(), buf.width, buf.height);
    for (panel, rect) in panels.iter().zip(grid.iter()) {
        render_panel(buf, panel, *rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_single_panel_fills_screen() {
        let rects = compute_grid(1, 800, 480);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[0].y, 0);
        assert_eq!(rects[0].w, 800);
        assert_eq!(rects[0].h, 480);
    }

    #[test]
    fn grid_two_panels_side_by_side() {
        let rects = compute_grid(2, 800, 480);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].x, 0);
        assert!(rects[1].x > 0);
    }

    #[test]
    fn grid_four_panels_2x2() {
        let rects = compute_grid(4, 800, 480);
        assert_eq!(rects.len(), 4);
        // 2 columns, 2 rows
        assert_eq!(rects[0].x, rects[2].x); // col 0
        assert_eq!(rects[1].x, rects[3].x); // col 1
    }

    #[test]
    fn wrap_text_basic() {
        let lines = wrap_text("hello world", 20);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn wrap_text_breaks_at_word_boundary() {
        let lines = wrap_text("hello world foo", 11);
        assert_eq!(lines, vec!["hello world", "foo"]);
    }

    #[test]
    fn wrap_text_long_word() {
        let lines = wrap_text("abcdefghij", 5);
        assert_eq!(lines, vec!["abcde", "fghij"]);
    }
}
