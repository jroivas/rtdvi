//! Render a raster image as terminal "half-block" art.
//!
//! Each text cell shows the Unicode upper-half block `▀`: its **foreground** is
//! the top pixel and its **background** the bottom pixel, so one text row packs
//! two image rows. With truecolor this gives a recognizable, fully portable
//! image — no terminal graphics protocol required. The output is ordinary
//! `ratatui` styled lines, so it drops straight into a render buffer.

use std::path::Path;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

const UPPER_HALF: &str = "▀";

/// Decode the image at `path` and render it as half-block lines, scaled to fit
/// within `max_cols` columns and `max_rows` text rows while preserving aspect.
/// Returns `None` when the file can't be read or decoded.
pub fn render_half_blocks(path: &Path, max_cols: usize, max_rows: usize) -> Option<Vec<Line<'static>>> {
    if max_cols == 0 || max_rows == 0 {
        return None;
    }
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }

    // One cell is 1px wide and (via the half-block) 2px tall, which already
    // matches a terminal cell being ~twice as tall as wide — so the on-screen
    // aspect is correct when rows = h*cols/(2*w). Fit width to `max_cols`, then
    // shrink the width too if the resulting height would exceed `max_rows`.
    let mut cols = (w as usize).min(max_cols).max(1);
    let mut rows = ((h as f64 * cols as f64) / (2.0 * w as f64)).round().max(1.0) as usize;
    if rows > max_rows {
        rows = max_rows;
        cols = ((2 * rows) as f64 * w as f64 / h as f64).round().max(1.0) as usize;
        cols = cols.min(max_cols);
    }
    let cols = cols as u32;
    let target_h = (rows * 2) as u32;

    let resized = image::imageops::resize(&img, cols, target_h, image::imageops::FilterType::Triangle);

    let cell_rows = rows;

    let mut lines = Vec::with_capacity(cell_rows);
    for cy in 0..cell_rows as u32 {
        let mut spans = Vec::with_capacity(cols as usize);
        for cx in 0..cols {
            let top = blend(resized.get_pixel(cx, cy * 2));
            let bot = blend(resized.get_pixel(cx, (cy * 2 + 1).min(target_h - 1)));
            spans.push(Span::styled(
                UPPER_HALF,
                Style::default()
                    .fg(Color::Rgb(top.0, top.1, top.2))
                    .bg(Color::Rgb(bot.0, bot.1, bot.2)),
            ));
        }
        lines.push(Line::from(spans));
    }
    Some(lines)
}

/// Flatten an RGBA pixel onto a black background — terminals have no per-cell
/// alpha, so transparent areas read as black.
fn blend(px: &image::Rgba<u8>) -> (u8, u8, u8) {
    let a = px.0[3] as u32;
    let c = |v: u8| ((v as u32 * a) / 255) as u8;
    (c(px.0[0]), c(px.0[1]), c(px.0[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_small_image_to_half_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("red.png");
        let mut img = image::RgbaImage::new(6, 6);
        for p in img.pixels_mut() {
            *p = image::Rgba([200, 30, 30, 255]);
        }
        img.save(&path).unwrap();

        let lines = render_half_blocks(&path, 6, 6).unwrap();
        assert!(!lines.is_empty());
        // Every cell is the upper-half block with both fg and bg set.
        let cell = &lines[0].spans[0];
        assert_eq!(cell.content.as_ref(), "▀");
        assert!(cell.style.fg.is_some() && cell.style.bg.is_some());
        // Width is capped to max_cols.
        assert!(lines[0].spans.len() <= 6);
    }

    #[test]
    fn missing_file_is_none() {
        assert!(render_half_blocks(std::path::Path::new("/no/such/file.png"), 8, 8).is_none());
    }

    #[test]
    fn preserves_aspect_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sq.png");
        // A square image: since a cell shows 2 vertical pixels, the rendered
        // height in cells is ~half the width in cells (so it reads square).
        image::RgbaImage::from_pixel(40, 40, image::Rgba([10, 20, 30, 255]))
            .save(&path)
            .unwrap();
        let lines = render_half_blocks(&path, 20, 100).unwrap();
        assert_eq!(lines[0].spans.len(), 20, "width fills the column budget");
        assert_eq!(lines.len(), 10, "square image → rows == cols / 2");
    }

    #[test]
    fn tall_image_shrinks_width_to_fit_row_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tall.png");
        // 20x200 (very tall). Capped at 5 rows → width scales down to keep aspect.
        image::RgbaImage::from_pixel(20, 200, image::Rgba([0, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let lines = render_half_blocks(&path, 80, 5).unwrap();
        assert_eq!(lines.len(), 5, "height respects max_rows");
        assert!(lines[0].spans.len() <= 2, "width shrank to preserve aspect, got {}", lines[0].spans.len());
    }
}
