//! The design system of the manager window: spacing, radii, strokes, elevation, the type scale,
//! motion and a few layout helpers. Every size the shell, the cards and the footer use comes from
//! here, so all blocks sit on one gutter grid. Colours stay in `palette`.
//!
//! The shell is laid out by hand from the window rect (no egui panels), so every block is placed
//! in the same frame from known sizes and nothing depends on what the last frame measured.

use std::sync::Arc;

use egui::{Color32, FontId, Galley, Painter, Pos2, Rect, Response, Stroke, Ui, Widget};

use crate::palette as pal;

// Spacing scale (points).
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;

/// Left and right gutter of every block: header, tabs, toolbar, sidebar column, cards, footer.
pub const GUTTER: f32 = S4;

// Radii: one for controls (buttons, fields, switches' track ends), one for cards and panels.
pub const RADIUS_CONTROL: u8 = 6;
pub const RADIUS_PANEL: u8 = 10;

/// The one stroke width; borders and dividers are `pal::EDGE`.
pub const STROKE: f32 = 1.0;

pub fn border() -> Stroke {
    Stroke::new(STROKE, pal::EDGE)
}

// Control heights: buttons, the search box and the toolbar row share `CONTROL_H`.
pub const CONTROL_H: f32 = 28.0;
pub const CONTROL_SM_H: f32 = 24.0;

// Shell rows.
pub const HEADER_H: f32 = 60.0;
pub const TABS_H: f32 = 40.0;
pub const FOOTER_H: f32 = 36.0;
pub const SIDEBAR_W: f32 = 200.0;
/// Below this window width (points) the filters sit in a strip under the toolbar.
pub const SIDEBAR_MIN_WIDTH: f32 = 600.0;

// Type scale (points; the window zoom scales all of them).
pub const TYPE_TITLE: f32 = 22.0;
pub const TYPE_TAB: f32 = 15.0;
pub const TYPE_HEADING: f32 = 15.0;
pub const TYPE_CARD_TITLE: f32 = 14.0;
pub const TYPE_BODY: f32 = 13.0;
pub const TYPE_CAPTION: f32 = 11.0;

/// Height of the capital letters of the UI font as a share of its size (Ubuntu Light).
const CAP_SHARE: f32 = 0.70;

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

// Motion (seconds). Animations only ask for repaints while they run.
pub const MOTION_FAST: f32 = 0.12;
pub const MOTION: f32 = 0.15;

pub fn ease(t: f32) -> f32 {
    egui::emath::easing::cubic_out(t.clamp(0.0, 1.0))
}

/// Surfaces by elevation: 0 window, 1 panels (chrome, sidebar, footer), 2 cards, 3 hover.
/// A fractional level blends the two levels around it (hover lift animations).
pub fn elevation(level: f32) -> Color32 {
    let levels = [pal::SURFACE_0, pal::SURFACE_1, pal::SURFACE_2, pal::SURFACE_3];
    let l = level.clamp(0.0, 3.0);
    let i = (l.floor() as usize).min(2);
    lerp_color(levels[i], levels[i + 1], l - i as f32)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// Rounds a coordinate to a whole physical pixel, so edges of neighbouring blocks line up.
pub fn snap(ppp: f32, x: f32) -> f32 {
    (x * ppp).round() / ppp
}

pub fn snap_rect(ppp: f32, r: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(snap(ppp, r.left()), snap(ppp, r.top())), Pos2::new(snap(ppp, r.right()), snap(ppp, r.bottom())))
}

/// A text laid out once (no colour yet), as the measuring and the painting step share it.
pub fn galley(painter: &Painter, text: impl Into<String>, size: f32, wrap: f32, max_rows: usize) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::simple(text.into(), font(size), Color32::PLACEHOLDER, wrap);
    if max_rows > 0 {
        job.wrap.max_rows = max_rows;
    }
    painter.layout_job(job)
}

/// Height of the first line of `g` (0 for an empty galley).
fn first_row_height(g: &Galley) -> f32 {
    g.rows.first().map_or(g.size().y, |r| r.row.size.y)
}

/// Top-left for `g` so its first line is centred on `mid_y`.
pub fn mid_pos(g: &Galley, x: f32, mid_y: f32) -> Pos2 {
    Pos2::new(x, mid_y - first_row_height(g) / 2.0)
}

/// Distance from the galley top to the top of the capitals of its first line.
pub fn cap_top(g: &Galley, size: f32) -> f32 {
    let baseline = g.rows.first().and_then(|r| r.row.glyphs.first().map(|gl| r.pos.y + gl.pos.y));
    baseline.map_or(0.0, |b| (b - size * CAP_SHARE).max(0.0))
}

/// Paints `g` with its first line centred on `mid_y`; returns the painted rect.
pub fn text_mid(painter: &Painter, g: Arc<Galley>, x: f32, mid_y: f32, color: Color32) -> Rect {
    let pos = mid_pos(&g, x, mid_y);
    let rect = Rect::from_min_size(pos, g.size());
    painter.galley(pos, g, color);
    rect
}

/// Lays boxes of `widths` out in rows of `avail` width; returns (row, x offset) per box and the
/// number of rows. A box wider than the row gets a row of its own.
pub fn flow(widths: &[f32], avail: f32, gap: f32) -> (Vec<(usize, f32)>, usize) {
    let mut out = Vec::with_capacity(widths.len());
    let (mut row, mut x) = (0usize, 0.0_f32);
    for &w in widths {
        if x > 0.0 && x + w > avail + 0.01 {
            row += 1;
            x = 0.0;
        }
        out.push((row, x));
        x += w + gap;
    }
    let rows = if widths.is_empty() { 0 } else { row + 1 };
    (out, rows)
}

/// Height of `rows` rows of `row_h` with `gap` between them.
pub fn rows_height(rows: usize, row_h: f32, gap: f32) -> f32 {
    if rows == 0 {
        0.0
    } else {
        rows as f32 * row_h + (rows as f32 - 1.0) * gap
    }
}

/// Width a one-line button needs for `text` at the body size.
pub fn button_width(ui: &Ui, text: &str) -> f32 {
    let g = galley(ui.painter(), text, TYPE_BODY, f32::INFINITY, 0);
    (g.size().x + 2.0 * ui.spacing().button_padding.x).ceil()
}

/// Adds `widget` filling `rect` without moving the cursor of `ui` (manual layout). `salt` keeps
/// the widget id stable whatever else is drawn first.
pub fn place(ui: &mut Ui, salt: impl std::hash::Hash, rect: Rect, enabled: bool, widget: impl Widget) -> Response {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(rect)
            .layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)),
    );
    child.add_enabled(enabled, widget)
}

/// Horizontal hairline divider.
pub fn hline(painter: &Painter, left: f32, right: f32, y: f32) {
    painter.hline(left..=right, y, border());
}

/// Vertical hairline divider.
pub fn vline(painter: &Painter, x: f32, top: f32, bottom: f32) {
    painter.vline(x, top..=bottom, border());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_wraps_and_counts_rows() {
        let (pos, rows) = flow(&[50.0, 50.0, 50.0], 120.0, 8.0);
        assert_eq!(rows, 2);
        assert_eq!(pos, vec![(0, 0.0), (0, 58.0), (1, 0.0)]);
        assert_eq!(flow(&[], 100.0, 8.0).1, 0);
        // Too wide for any row: still placed, on a row of its own.
        assert_eq!(flow(&[300.0, 10.0], 100.0, 8.0).0, vec![(0, 0.0), (1, 0.0)]);
        assert_eq!(rows_height(2, 28.0, 8.0), 64.0);
    }

    #[test]
    fn elevation_steps_and_blends() {
        assert_eq!(elevation(0.0), pal::SURFACE_0);
        assert_eq!(elevation(2.0), pal::SURFACE_2);
        assert_eq!(elevation(3.0), pal::SURFACE_3);
        let mid = elevation(2.5);
        assert!(mid.r() >= pal::SURFACE_2.r() && mid.r() <= pal::SURFACE_3.r());
    }

    #[test]
    fn spacing_tokens_are_a_scale() {
        assert_eq!([S1, S2, S3, S4, S5], [4.0, 8.0, 12.0, 16.0, 24.0]);
        const { assert!(CONTROL_SM_H < CONTROL_H && FOOTER_H > CONTROL_H) };
    }
}
