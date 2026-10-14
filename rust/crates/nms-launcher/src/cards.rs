//! Look and helpers of the Mods tab card grid: groups, filters, the column rule, the card text
//! measurement, the hex badge, the on/off switch and the two navigation styles (underlined tabs,
//! sidebar list). Everything here is pure or only paints; the Manager in `ui` owns the state and
//! the file work. Sizes come from `style`, colours from `palette`.

use std::sync::Arc;

use egui::{Color32, CornerRadius, Galley, Id, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

use crate::palette as pal;
use crate::style::{self as st, CONTROL_H, MOTION, MOTION_FAST, S1, S2, S3};
use crate::ui::ModRow;

/// Narrowest card (points) and the gap between cards (both directions).
pub const CARD_MIN_WIDTH: f32 = 185.0;
pub const CARD_GAP: f32 = S3;
/// Inner padding of a card, on every side.
pub const CARD_PAD: f32 = S3;
/// Box of the hex badge, and the gap between the badge and the title.
pub const BADGE: f32 = 36.0;
/// The hexagon's top point sits this far below the top of its box.
const BADGE_INSET: f32 = 1.0;
/// Lines of title and of description on a card (full text on hover).
pub const TITLE_ROWS: usize = 2;
pub const DESC_ROWS: usize = 3;
/// A column is only added once the grid is this much wider than the breakpoint (points).
pub const COLUMN_HYSTERESIS: f32 = 6.0;

/// Mod type shown as a heading in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Inventory,
    Crafting,
    Map,
    Interface,
    Other,
}

pub const GROUPS: [Group; 5] = [Group::Inventory, Group::Crafting, Group::Map, Group::Interface, Group::Other];

impl Group {
    pub fn key(self) -> &'static str {
        match self {
            Group::Inventory => "rs_group_inventory",
            Group::Crafting => "rs_group_crafting",
            Group::Map => "rs_group_map",
            Group::Interface => "rs_group_ui",
            Group::Other => "rs_group_other",
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Group::Inventory => pal::GROUP_INVENTORY,
            Group::Crafting => pal::GROUP_CRAFTING,
            Group::Map => pal::GROUP_MAP,
            Group::Interface => pal::GROUP_INTERFACE,
            Group::Other => pal::GROUP_OTHER,
        }
    }
}

/// Static mapping by mod folder name; anything unknown (other people's mods, new mods) is Other.
pub fn group_of(name: &str) -> Group {
    match name {
        "BiggerItemGrids" | "SmallerInventorySlots" | "QuickTransferAll" | "CompactQuickTransfer" => Group::Inventory,
        "CraftFromStorage" | "QuickRecharge" | "RefinerGuard" => Group::Crafting,
        "GalaxyMapFilters" => Group::Map,
        "CompactTooltips" | "HideEmptyHover" | "PickerTweaks" | "TradeUITweaks" | "NMSrsMenu" => Group::Interface,
        _ => Group::Other,
    }
}

/// Our mods that need the NMS.rs loader (nms-mods `lib.rs` list) plus the pause-menu row,
/// which opens the loader's Mods menu. Everything else is game files only (UI, MBIN, lua).
const LOADER_MODS: &[&str] = &[
    "SphereSizedMining", "HideEmptyHover", "BiggerItemGrids", "QuickRecharge", "TradeUITweaks",
    "CraftFromStorage", "PickerTweaks", "RefinerGuard", "ToolModeBinds", "QuickTransferAll",
    "SmallerInventorySlots", "GalaxyMapFilters", "NMSrsMenu",
];

/// True when the mod works from its game files alone, without the NMS.rs loader.
pub fn game_files_only(name: &str) -> bool {
    !LOADER_MODS.contains(&name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Live,
    Parked,
    Update,
    GameFiles,
}

pub const FILTERS: [Filter; 5] = [Filter::All, Filter::Live, Filter::Parked, Filter::Update, Filter::GameFiles];

impl Filter {
    pub fn key(self) -> &'static str {
        match self {
            Filter::All => "rs_filter_all",
            Filter::Live => "rs_filter_live",
            Filter::Parked => "rs_filter_parked",
            Filter::Update => "rs_filter_update",
            Filter::GameFiles => "rs_filter_game_files",
        }
    }

    /// Parked also holds mods that are not installed yet: both are "off".
    pub fn matches(self, row: &ModRow) -> bool {
        match self {
            Filter::All => true,
            Filter::Live => row.state == "live",
            Filter::Parked => row.state != "live",
            Filter::Update => crate::ui::has_update(row),
            Filter::GameFiles => game_files_only(&row.name),
        }
    }
}

pub fn count(rows: &[ModRow], filter: Filter) -> usize {
    rows.iter().filter(|row| filter.matches(row)).count()
}

/// Case-insensitive match of the search box against the mod name and its description.
pub fn matches_search(name: &str, description: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || name.to_lowercase().contains(&q) || description.to_lowercase().contains(&q)
}

/// Cards per row for a grid of `width` points: 3, 2 or 1.
pub fn columns_for(width: f32) -> usize {
    let fits = |n: f32| width >= n * CARD_MIN_WIDTH + (n - 1.0) * CARD_GAP;
    if fits(3.0) {
        3
    } else if fits(2.0) {
        2
    } else {
        1
    }
}

/// `columns_for` with hysteresis: fewer columns as soon as they no longer fit, more columns only
/// once the width is `COLUMN_HYSTERESIS` past the breakpoint. A window edge wobbling on the
/// breakpoint while resizing keeps the grid as it is instead of flipping it every frame.
pub fn columns_stable(width: f32, prev: Option<usize>) -> usize {
    let plain = columns_for(width);
    match prev {
        Some(p) if plain > p => columns_for(width - COLUMN_HYSTERESIS).max(p).min(plain),
        _ => plain,
    }
}

/// Left and right edge of each of `cols` columns sharing `width` points from `left`, on whole
/// physical pixels; the last column ends exactly at the right edge.
pub fn grid_columns(left: f32, width: f32, cols: usize, ppp: f32) -> Vec<(f32, f32)> {
    let cols = cols.max(1);
    let w = ((width - CARD_GAP * (cols as f32 - 1.0)) / cols as f32).max(40.0);
    (0..cols)
        .map(|i| {
            let l = left + i as f32 * (w + CARD_GAP);
            (st::snap(ppp, l), st::snap(ppp, l + w))
        })
        .collect()
}

/// Capital letters of a CamelCase name ("QuickTransferAll" is "QT"), at most two; the first two
/// characters when there are fewer than two capitals.
pub fn initials(name: &str) -> String {
    let caps: String = name.chars().filter(|c| c.is_ascii_uppercase()).take(2).collect();
    if caps.chars().count() >= 2 {
        caps
    } else {
        name.chars().take(2).collect::<String>().to_uppercase()
    }
}

/// "SmallerInventorySlots" as "Smaller Inventory Slots" and "TradeUITweaks" as "Trade UI Tweaks",
/// so a card name can wrap at word breaks. Names that already have spaces stay as they are.
pub fn pretty_name(name: &str) -> String {
    if name.contains(' ') {
        return name.to_string();
    }
    if let Some(rest) = name.strip_prefix("NMSrs") {
        return format!("NMS.rs {}", pretty_name(rest)).trim_end().to_string();
    }
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            let lower_to_upper = c.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit());
            let acronym_end = c.is_ascii_uppercase() && prev.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase());
            if lower_to_upper || acronym_end {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

/// Title and description of one card, laid out for its width before anything is painted. The
/// row height comes from these in the same frame, so a card never waits a frame for its size.
pub struct CardText {
    pub title: Arc<Galley>,
    pub desc: Arc<Galley>,
    /// Top of the title galley below the content top (its capitals start at the hexagon's top).
    pub title_top: f32,
    /// Height of the badge-and-title row.
    pub head_h: f32,
}

impl CardText {
    /// At most `DESC_ROWS` lines of description with an ellipsis (full text on hover).
    pub fn new(painter: &Painter, title: &str, desc: &str, card_w: f32) -> Self {
        let inner = (card_w - 2.0 * CARD_PAD).max(40.0);
        let title_w = (inner - BADGE - S3).max(40.0);
        let title = st::galley(painter, title, st::TYPE_CARD_TITLE, title_w, TITLE_ROWS);
        let desc = st::galley(painter, desc, st::TYPE_BODY, inner, DESC_ROWS);
        let title_top = BADGE_INSET - st::cap_top(&title, st::TYPE_CARD_TITLE);
        let head_h = BADGE.max(title_top + title.size().y);
        Self { title, desc, title_top, head_h }
    }

    /// Badge row, gap, description: the part above the footer row.
    pub fn body_h(&self) -> f32 {
        self.head_h + S2 + self.desc.size().y
    }

    /// Outer height of a card: body, gap, footer row (state, switch), padding.
    pub fn closed_height(&self) -> f32 {
        CARD_PAD + self.body_h() + S3 + CONTROL_H + CARD_PAD
    }
}

fn hex_points(center: Pos2, r: f32) -> Vec<Pos2> {
    (0..6)
        .map(|i| {
            let a = (-90.0 + 60.0 * i as f32).to_radians();
            center + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Pointy-top hexagon with an initials label, the mod "icon", filling the `BADGE` box at `min`.
pub fn paint_badge(painter: &Painter, min: Pos2, name: &str, color: Color32) {
    let r = BADGE / 2.0 - BADGE_INSET;
    let center = min + Vec2::splat(BADGE / 2.0);
    painter.add(Shape::convex_polygon(hex_points(center, r), color, Stroke::new(1.5_f32, pal::PRIMARY_LIGHT)));
    painter.add(Shape::convex_polygon(hex_points(center, r - 4.0), color.gamma_multiply(0.55), Stroke::NONE));
    painter.text(center, egui::Align2::CENTER_CENTER, initials(name), st::font(r * 0.78), pal::TEXT_STRONG);
}

pub fn paint_dot(painter: &Painter, center: Pos2, r: f32, color: Color32) {
    painter.circle_filled(center, r + 1.0, pal::SHADOW);
    painter.circle_filled(center, r, color);
}

/// A rectangle filled top to bottom with two colours.
pub fn gradient_shape(rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

fn pointer_cursor(ui: &Ui, response: &Response) {
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// Small dismiss cross (footer) filling `rect`. Drawn with two lines, so it needs no font glyph.
pub fn close_button(ui: &mut Ui, id: Id, rect: Rect) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let t = ui.ctx().animate_bool_with_time(id.with("hover"), response.hovered(), MOTION_FAST);
    let painter = ui.painter();
    if t > 0.0 {
        painter.rect_filled(rect, CornerRadius::same(st::RADIUS_CONTROL), pal::SURFACE_RAISED.gamma_multiply(t));
    }
    let color = st::lerp_color(pal::TEXT_MUTED, pal::TEXT_STRONG, t);
    let d = rect.width() * 0.2;
    let c = rect.center();
    let stroke = Stroke::new(1.8_f32, color);
    painter.line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
    painter.line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
    pointer_cursor(ui, &response);
    response
}

/// Size of the on/off switch.
pub const SWITCH: Vec2 = Vec2::new(36.0, 20.0);

/// On/off switch (installed or parked) filling `rect`. Click toggles; `enabled` false greys it
/// out. The knob slides and the track colour blends while `on` changes.
pub fn switch(ui: &mut Ui, id: Id, rect: Rect, on: bool, enabled: bool) -> Response {
    let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let t = st::ease(ui.ctx().animate_bool_with_time(id.with("knob"), on, MOTION));
    let hover = ui.ctx().animate_bool_with_time(id.with("hover"), response.hovered() && enabled, MOTION_FAST);
    let fade = if enabled { 1.0 } else { 0.45 };
    let track = st::lerp_color(pal::TRACK_OFF, pal::PRIMARY, t);
    let track = st::lerp_color(track, pal::PRIMARY_LIGHT, 0.18 * hover);
    let radius = CornerRadius::same((rect.height() / 2.0) as u8);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, track.gamma_multiply(fade));
    painter.rect_stroke(rect, radius, Stroke::new(st::STROKE, pal::SHADOW), egui::StrokeKind::Inside);
    let knob_r = rect.height() / 2.0 - 3.0;
    let x0 = rect.left() + rect.height() / 2.0;
    let x1 = rect.right() - rect.height() / 2.0;
    painter.circle_filled(Pos2::new(x0 + (x1 - x0) * t, rect.center().y), knob_r, pal::SWITCH_KNOB.gamma_multiply(fade));
    if enabled {
        pointer_cursor(ui, &response);
    }
    response
}

/// One entry of a navigation list: its label and, when not selected, an idle text colour
/// (None: muted). Update available is amber while something waits.
pub struct NavEntry {
    pub label: String,
    pub idle: Option<Color32>,
}

fn entry_color(selected: bool, idle: Option<Color32>, hover: f32) -> Color32 {
    if selected {
        pal::PRIMARY_LIGHT
    } else if let Some(c) = idle {
        c
    } else {
        st::lerp_color(pal::TEXT_MUTED, pal::TEXT, hover)
    }
}

/// Text size, gap between entries and row height of an `underline_nav`.
#[derive(Clone, Copy)]
pub struct NavStyle {
    pub size: f32,
    pub gap: f32,
    pub row_h: f32,
}

/// Height `underline_nav` needs for `entries` in `width`.
pub fn underline_nav_height(painter: &Painter, entries: &[NavEntry], nav: NavStyle, width: f32) -> f32 {
    let widths: Vec<f32> = entries.iter().map(|e| st::galley(painter, &e.label, nav.size, f32::INFINITY, 0).size().x).collect();
    st::rows_height(st::flow(&widths, width, nav.gap).1, nav.row_h, 0.0)
}

/// Text entries in a row (wrapping into more rows of `row_h` when `rect` is narrow) with one
/// accent underline under the selected one, on the bottom edge of its row. The underline slides
/// to a newly selected entry. Tabs and the narrow filter strip. The first entry's text starts
/// exactly at `rect.left()`.
pub fn underline_nav(ui: &mut Ui, salt: &str, rect: Rect, entries: &[NavEntry], selected: usize, nav: NavStyle) -> Option<usize> {
    let NavStyle { size, gap, row_h } = nav;
    let painter = ui.painter().clone();
    let galleys: Vec<Arc<Galley>> = entries.iter().map(|e| st::galley(&painter, &e.label, size, f32::INFINITY, 0)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x).collect();
    let (pos, _) = st::flow(&widths, rect.width(), gap);
    let base = ui.id().with(salt);
    let mut clicked = None;
    let mut mark = None;
    for (i, (((row, x), g), entry)) in pos.iter().zip(galleys).zip(entries).enumerate() {
        let top = rect.top() + *row as f32 * row_h;
        let text_rect = Rect::from_min_size(Pos2::new(rect.left() + x, top), Vec2::new(g.size().x, row_h));
        let id = base.with(i);
        let response = ui.interact(text_rect.expand2(Vec2::new(gap / 2.0 - 1.0, 0.0)), id, Sense::click());
        let hover = ui.ctx().animate_bool_with_time(id.with("hover"), response.hovered(), MOTION_FAST);
        st::text_mid(&painter, g, text_rect.left(), text_rect.center().y, entry_color(i == selected, entry.idle, hover));
        if i == selected {
            mark = Some(text_rect);
        }
        if response.clicked() {
            clicked = Some(i);
        }
        pointer_cursor(ui, &response);
    }
    if let Some(m) = mark {
        let ctx = ui.ctx();
        let x0 = ctx.animate_value_with_time(base.with("x0"), m.left(), MOTION);
        let x1 = ctx.animate_value_with_time(base.with("x1"), m.right(), MOTION);
        let y = ctx.animate_value_with_time(base.with("y"), m.bottom(), MOTION);
        let bar = Rect::from_min_max(Pos2::new(x0, y - 2.0), Pos2::new(x1, y));
        painter.rect_filled(bar, CornerRadius::same(1), pal::PRIMARY_LIGHT);
    }
    clicked
}

/// Inner text padding of sidebar entries and of the search box, so their texts line up.
pub const FIELD_PAD_X: f32 = S3;
/// Gap between sidebar entries.
const LIST_GAP: f32 = S1;

fn list_item_height(g: &Galley) -> f32 {
    (g.size().y + 2.0 * S2).max(CONTROL_H + S1)
}

/// Vertical list of entries filling `x` from `top` (the sidebar filters). The selected one has a
/// raised background and an accent bar on its left; both slide to a newly selected entry. Long
/// labels wrap. Returns the clicked entry.
pub fn side_list(ui: &mut Ui, salt: &str, x: egui::Rangef, top: f32, entries: &[NavEntry], selected: usize, size: f32) -> Option<usize> {
    let painter = ui.painter().clone();
    let wrap = (x.span() - 2.0 * FIELD_PAD_X).max(40.0);
    let base = ui.id().with(salt);
    let mut y = top;
    let mut rects = Vec::with_capacity(entries.len());
    let galleys: Vec<Arc<Galley>> = entries.iter().map(|e| st::galley(&painter, &e.label, size, wrap, 0)).collect();
    for g in &galleys {
        let h = list_item_height(g);
        rects.push(Rect::from_x_y_ranges(x, y..=y + h));
        y += h + LIST_GAP;
    }
    // The selection first, so the entry text is drawn over it.
    if let Some(sel) = rects.get(selected) {
        let ctx = ui.ctx();
        let t = ctx.animate_value_with_time(base.with("sel_top"), sel.top(), MOTION);
        let h = ctx.animate_value_with_time(base.with("sel_h"), sel.height(), MOTION);
        let r = Rect::from_x_y_ranges(x, t..=t + h);
        painter.rect_filled(r, CornerRadius::same(st::RADIUS_CONTROL), pal::SURFACE_3);
        let bar = Rect::from_min_max(r.left_top() + Vec2::new(0.0, S2), Pos2::new(r.left() + 3.0, r.bottom() - S2));
        painter.rect_filled(bar, CornerRadius::same(1), pal::PRIMARY_LIGHT);
    }
    let mut clicked = None;
    for (i, ((rect, g), entry)) in rects.iter().zip(galleys).zip(entries).enumerate() {
        let id = base.with(i);
        let response = ui.interact(*rect, id, Sense::click());
        let hover = ui.ctx().animate_bool_with_time(id.with("hover"), response.hovered() && i != selected, MOTION_FAST);
        if hover > 0.0 {
            painter.rect_filled(*rect, CornerRadius::same(st::RADIUS_CONTROL), pal::SURFACE_2.gamma_multiply(hover));
        }
        let pos = Pos2::new(rect.left() + FIELD_PAD_X, rect.center().y - g.size().y / 2.0);
        painter.galley(pos, g, entry_color(i == selected, entry.idle, hover));
        if response.clicked() {
            clicked = Some(i);
        }
        pointer_cursor(ui, &response);
    }
    clicked
}

/// Dark NMS look: blue primary, red accent, rounded widgets, the type scale and the motion time
/// of `style`. Every colour comes from `palette`.
pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = pal::SURFACE_0;
    visuals.window_fill = pal::SURFACE_1;
    visuals.extreme_bg_color = pal::SURFACE_SUNKEN;
    visuals.faint_bg_color = pal::SURFACE_STRIP;
    visuals.hyperlink_color = pal::PRIMARY_LIGHT;
    visuals.warn_fg_color = pal::WARN;
    visuals.error_fg_color = pal::DANGER;
    visuals.selection.bg_fill = pal::PRIMARY_DARK;
    visuals.selection.stroke = Stroke::new(st::STROKE, pal::PRIMARY_LIGHT);
    visuals.text_cursor.stroke = Stroke::new(2.0_f32, pal::PRIMARY_LIGHT);
    let radius = CornerRadius::same(st::RADIUS_CONTROL);
    let border = st::border();
    visuals.widgets.noninteractive.corner_radius = radius;
    visuals.widgets.noninteractive.bg_stroke = border;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(st::STROKE, pal::TEXT);
    visuals.widgets.inactive.corner_radius = radius;
    visuals.widgets.inactive.weak_bg_fill = pal::SURFACE_RAISED;
    visuals.widgets.inactive.bg_fill = pal::SURFACE_RAISED;
    visuals.widgets.inactive.bg_stroke = border;
    visuals.widgets.inactive.fg_stroke = Stroke::new(st::STROKE, pal::TEXT);
    visuals.widgets.hovered.corner_radius = radius;
    visuals.widgets.hovered.weak_bg_fill = pal::PRIMARY_DARK;
    visuals.widgets.hovered.bg_fill = pal::PRIMARY_DARK;
    visuals.widgets.hovered.bg_stroke = Stroke::new(st::STROKE, pal::PRIMARY);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, pal::TEXT_STRONG);
    // No grow-on-hover: a button keeps its exact rect, so rows stay aligned.
    visuals.widgets.hovered.expansion = 0.0;
    visuals.widgets.active.corner_radius = radius;
    visuals.widgets.active.weak_bg_fill = pal::PRIMARY;
    visuals.widgets.active.bg_fill = pal::PRIMARY;
    visuals.widgets.active.bg_stroke = Stroke::new(st::STROKE, pal::PRIMARY_LIGHT);
    visuals.widgets.active.fg_stroke = Stroke::new(2.0_f32, pal::TEXT_STRONG);
    visuals.widgets.active.expansion = 0.0;
    visuals.widgets.open.corner_radius = radius;
    visuals.widgets.open.weak_bg_fill = pal::SURFACE_RAISED;
    visuals.widgets.open.bg_fill = pal::SURFACE_RAISED;
    visuals.window_corner_radius = CornerRadius::same(st::RADIUS_PANEL);
    visuals.window_stroke = border;
    visuals.menu_corner_radius = CornerRadius::same(st::RADIUS_PANEL);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Small, FontId::new(st::TYPE_CAPTION, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(st::TYPE_BODY, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(st::TYPE_BODY, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(st::TYPE_HEADING + 2.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(st::TYPE_BODY - 1.0, FontFamily::Monospace)),
        ]
        .into();
        style.animation_time = MOTION;
        style.spacing.item_spacing = Vec2::new(S2, 6.0);
        style.spacing.button_padding = Vec2::new(S3, S1);
        style.spacing.interact_size.y = st::CONTROL_SM_H;
        // Floating scroll bar that takes no width: the content (and the cards' right edge) stays
        // on the gutter whether or not the page scrolls, and the bar sits inside the gutter.
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 8.0,
            floating_width: 4.0,
            bar_inner_margin: S1,
            bar_outer_margin: S1,
            dormant_handle_opacity: 0.5,
            dormant_background_opacity: 0.0,
            active_handle_opacity: 0.8,
            interact_handle_opacity: 1.0,
            ..egui::style::ScrollStyle::floating()
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, state: &str, outdated: bool) -> ModRow {
        ModRow {
            name: name.into(),
            state: state.into(),
            outdated,
        }
    }

    #[test]
    fn every_our_mod_has_a_group_and_unknown_is_other() {
        for name in crate::mods::OUR_MODS {
            // No mod of ours should fall into Other by accident, except the ones listed there.
            let group = group_of(name);
            if group == Group::Other {
                assert!(["SphereSizedMining", "ToolModeBinds"].contains(name), "{name}");
            }
        }
        assert_eq!(group_of("Some Other Mod 1.2"), Group::Other);
        assert_eq!(group_of("GalaxyMapFilters"), Group::Map);
    }

    #[test]
    fn filters_and_counts() {
        let rows = [
            row("A", "live", false),
            row("B", "live", true),
            row("C", "parked", true),
            row("D", "missing", false),
        ];
        assert_eq!(count(&rows, Filter::All), 4);
        assert_eq!(count(&rows, Filter::Live), 2);
        assert_eq!(count(&rows, Filter::Parked), 2);
        assert_eq!(count(&rows, Filter::Update), 2);
    }

    #[test]
    fn game_files_only_mods() {
        let ours: Vec<&str> = crate::mods::OUR_MODS.iter().copied().filter(|m| game_files_only(m)).collect();
        assert_eq!(ours, ["CompactTooltips", "CompactQuickTransfer"]);
        assert!(!game_files_only("NMSrsMenu"));
        assert!(game_files_only("Some Other Mod 1.2"));
    }

    #[test]
    fn search_matches_name_or_description() {
        assert!(matches_search("QuickRecharge", "Charge menu", ""));
        assert!(matches_search("QuickRecharge", "Charge menu", "  quick "));
        assert!(matches_search("QuickRecharge", "Charge menu", "MENU"));
        assert!(!matches_search("QuickRecharge", "Charge menu", "map"));
    }

    #[test]
    fn columns_follow_width() {
        let three = 3.0 * CARD_MIN_WIDTH + 2.0 * CARD_GAP;
        let two = 2.0 * CARD_MIN_WIDTH + CARD_GAP;
        assert_eq!(columns_for(900.0), 3);
        assert_eq!(columns_for(three), 3);
        assert_eq!(columns_for(three - 1.0), 2);
        assert_eq!(columns_for(two), 2);
        assert_eq!(columns_for(two - 1.0), 1);
        let cols = grid_columns(10.0, three, 3, 1.3);
        assert!((cols[2].1 - (10.0 + three)).abs() < 1.0, "{cols:?}");
        assert!(cols.windows(2).all(|w| w[1].0 - w[0].1 >= CARD_GAP - 1.0), "{cols:?}");
    }

    #[test]
    fn columns_have_hysteresis() {
        let three = 3.0 * CARD_MIN_WIDTH + 2.0 * CARD_GAP;
        // Growing past the breakpoint: stays at 2 until the hysteresis is used up.
        assert_eq!(columns_stable(three + 1.0, Some(2)), 2);
        assert_eq!(columns_stable(three + COLUMN_HYSTERESIS, Some(2)), 3);
        // Shrinking: drops as soon as 3 no longer fit (never overflows).
        assert_eq!(columns_stable(three - 1.0, Some(3)), 2);
        assert_eq!(columns_stable(three + 1.0, Some(3)), 3);
        assert_eq!(columns_stable(three + 1.0, None), 3);
        // A big jump still lands on the plain answer.
        assert_eq!(columns_stable(2000.0, Some(1)), 3);
    }

    #[test]
    fn pretty_names_break_at_words() {
        assert_eq!(pretty_name("SmallerInventorySlots"), "Smaller Inventory Slots");
        assert_eq!(pretty_name("TradeUITweaks"), "Trade UI Tweaks");
        assert_eq!(pretty_name("NMSrsMenu"), "NMS.rs Menu");
        assert_eq!(pretty_name("Better Frigate View 2.3"), "Better Frigate View 2.3");
        assert_eq!(pretty_name("Quick"), "Quick");
    }

    #[test]
    fn initials_of_names() {
        assert_eq!(initials("QuickTransferAll"), "QT");
        assert_eq!(initials("BiggerItemGrids"), "BI");
        assert_eq!(initials("Instant"), "IN");
        assert_eq!(initials("x"), "X");
    }
}
