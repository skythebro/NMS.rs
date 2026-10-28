//! Slot plan, preset JSON, and the numbers the UI files were built with.

use std::path::Path;

use serde_json::Value;

pub const COLUMNS: i32 = 10;
pub const SLOT: f32 = 90.0;
pub const MARGIN: f32 = 3.0;
pub const TECH_TOP: f32 = 735.0;
pub const TECH_ROWS_VANILLA: i32 = 3;
pub const GRID_BOTTOM: f32 = 1550.0;
pub const HEADER_GAP: f32 = 45.0;
pub const SLACK: f32 = 0.5;
pub const SCALE_RANGE: (f32, f32) = (0.5, 1.0);
pub const VANILLA_SCALE: f32 = 1.0;

pub const ELEMENT_DATA: usize = 0x48;
pub const DATA_ID: usize = 0x48;
pub const LAYOUT_H: usize = 0x24;
pub const LAYOUT_X: usize = 0x2C;
pub const LAYOUT_Y: usize = 0x30;
pub const LAYOUT_W: usize = 0x34;
pub const VANILLA_SLOT: i32 = 90;

pub const REMEMBER_TAB_FILTER: bool = true;
pub const REMEMBER_TAB_SCROLL: bool = true;
pub const SHRINK_CHEST_GRIDS: bool = true;
pub const SHRINK_FREIGHTER_GRIDS: bool = false;
pub const PROBE_PAGE_DIFF: bool = false;
pub const REMEMBER_FREIGHTER_TAB: bool = true;

/// Freighter Q/E selector on the inventory page (`SetSubTab`: `cmp edx,[rcx+0x196D0]`, build 183791;
/// 0x19670 before).
pub const FREIGHTER_SELECTOR: usize = 0x196D0;
pub const FREIGHTER_CORE_KIND: u64 = 7;
pub const FREIGHTER_FIRST: i32 = 3;
pub const FREIGHTER_LAST: i32 = 12;
pub const STORE_STRIDE: u64 = 0x248;
/// Busy flag (`page+0x14C84` on build 183791, 0x14C44 before) minus the screen id (`page+0x14488`).
pub const PAGE_BUSY_FROM_SCREEN: usize = 0x7FC;
pub const PAGE_OPENED_FROM_SELECTOR: usize = 0x80;
pub const CARGO_HEIGHTS: [i32; 2] = [5, 7];
pub const CHEST_SLOT: i32 = 74;
pub const CHEST_ROWS: i32 = 7;
pub const CHEST_WIDTH: f32 = 804.0;
pub const FREIGHTER_ROWS: i32 = 5;
pub const FREIGHTER_WIDTH: f32 = 1010.0;
pub const STORAGE_NAME: &str = "BLD_STORAGE_NAME";
pub const FILTER_OFFSET_LOW: i32 = 0x1000;
pub const FILTER_OFFSET_HIGH: i32 = 0x40000;

pub const PAGE_ELEMENT: usize = 0x14478;
pub const CHILD_MAP: usize = 0x150;
pub const IS_HIDDEN: usize = 0x61;
pub const SORT_BUTTONS: [&str; 4] = ["SORTBYVALUE", "SORTBYTYPE", "SORTBYNAME", "SORTBYCOLOUR"];
pub const STORE_NAME: usize = 0x104;
pub const STORE_NAME_LEN: usize = 0x100;
pub const STORE_STACK_GROUP: usize = 0xB8;
pub const PROBE_DUMP_DWORDS: usize = 32;
pub const PROBE_CHUNK: usize = 0x1000;
pub const PROBE_DIFF_CAP: usize = 200;
pub const PROBE_SCREEN_WINDOW: f64 = 0.05;
pub const MIN_ADDR: usize = nms_core::mem::MIN_ADDR;

pub const FILTER_FIELD_AT: usize = 0x0A;
pub const DIM_AT: usize = 0xD7;
pub const JOIN_AT: usize = 0xFF;
pub const PRODUCT_CATEGORY: u32 = 0x1E8;
pub const VANILLA_FILTERS: u8 = 5;
/// Side-button slots past the vanilla five: the 12 product categories (ids 5 to 16), then Foods
/// (id 17, `FILTER18`) and Custom (id 18, `FILTER19`).
pub const CATEGORIES: u8 = 14;
/// Index of Foods and Custom in `EXTRA_FILTER_NAMES` (and in the hide cave's `on` table).
pub const FOODS_INDEX: usize = 8;
pub const CUSTOM_INDEX: usize = 9;

/// Custom filter test at the head of the FILTER block. The first
/// 14 bytes stay untouched (QuickTransferAll's lock dim owns the first 5, both mods read the filter
/// field at +0x0A); the patch takes `test eax, eax; jne +0x23; cmp r14d, 1` and returns to the
/// `jbe` at +0x16 with the flags of that compare.
pub const CUSTOM_AT: usize = 0x0E;
pub const CUSTOM_BYTES: &[u8] = &[0x85, 0xC0, 0x75, 0x11, 0x41, 0x83, 0xFE, 0x01];
pub const CUSTOM_JBE_AT: usize = 0x16;
pub const CUSTOM_JBE: &[u8] = &[0x0F, 0x86];
pub const CUSTOM_KIND_AT: usize = 0x23;
pub const CUSTOM_HEAD: &[u8] = &[0x41, 0x83, 0xFE, 0x12];

pub const TIPS_AT: usize = 3;
pub const LOOP_TOP: usize = 0x16;
pub const ICON_SITES: [usize; 2] = [0x354, 0x41A];
pub const ICON_LEN: usize = 7;
pub const CLICK_AT: usize = 0x47A;
pub const SKIP_AT: usize = 0x45A;
pub const SKIP_BACK: usize = 0x476;
pub const SKIP_MISS: usize = 0x5AF;
pub const HIDE_AT: usize = 10;
pub const HIDE_FIND_AT: usize = 0x3D;
pub const HIDE_HASH_AT: usize = 0x5C;
pub const HIDE_CHILD_AT: usize = 0x6A;
pub const HIDE_CURSOR_AT: usize = 0x84;
pub const TIP_COPY_AT: usize = 0x5BB;
pub const STEP_AT: usize = 0x62D;
pub const COUNT_AT: usize = STEP_AT + 15;
pub const TIP_SIZE: usize = 0x20;
#[expect(dead_code)]
pub const PAD_A: u16 = 0x1000;
#[expect(dead_code)]
pub const MAX_CONTROLLERS: usize = 16;
pub const HOVER_STALE: f64 = 0.25;
pub const HOVER_NONE: u8 = 0xFF;

pub const DIM_BYTES: &[u8] = &[0x41, 0x83, 0xFE, 0x04, 0x75, 0x22];
pub const CAVE_HEAD: &[u8] = &[0x51, 0x41, 0x83, 0xFE, 0x05];
pub const ICON_BYTES: &[u8] = &[0x8B, 0x8C, 0x39];
pub const ICON_AFTER: &[u8] = &[0x89, 0x4C, 0x24, 0x20];
pub const CLICK_BYTES: &[u8] = &[0x41, 0x89, 0x86];
/// Click gates `cmp dword [r14+0x14A20], 2` and `cmp dword [r14+0x14C10], 2` (the second was
/// 0x14BF0 before build 183791).
pub const SKIP_BYTES: &[u8] = &[
    0x41, 0x83, 0xBE, 0x20, 0x4A, 0x01, 0x00, 0x02, 0x0F, 0x84, 0x47, 0x01, 0x00, 0x00, 0x41, 0x83,
    0xBE, 0x10, 0x4C, 0x01, 0x00, 0x02, 0x0F, 0x84, 0x39, 0x01, 0x00, 0x00,
];
pub const HIDE_BYTES: &[u8] = &[0x0F, 0x11, 0x44, 0x24, 0x70];
pub const TIP_COPY_BYTES: &[u8] = &[0x41, 0x0F, 0x10, 0x04, 0x24, 0x41, 0x0F, 0x10, 0x4C, 0x24, 0x10];
pub const STEP_BYTES: &[u8] = &[
    0x48, 0x83, 0x45, 0x88, 0x18, 0x41, 0xFF, 0xC5, 0x49, 0x83, 0xC4, 0x20, 0x41, 0x83, 0xFD, 0x05,
    0x0F, 0x8C,
];

pub const EXTRA_FILTER_NAMES: [&[u8]; 10] = [
    b"FILTER6",
    b"FILTER8",
    b"FILTER9",
    b"FILTER11",
    b"FILTER14",
    b"FILTER15",
    b"FILTER16",
    b"FILTER17",
    b"FILTER18",
    b"FILTER19",
];

/// Game localisation ids, one per button slot past the vanilla five (id 5 first). The last two are
/// Foods ("Cooking") and Custom ("Custom"), both translated by the game.
pub const BUTTON_TIPS: [&str; 14] = [
    "TRADE_FILTER_PROD",
    "UI_INV_FILTER_CONSUMABLES_TIP",
    "TRADE_FILTER_COMM",
    "UI_GUIDE_HEADING_CURIO_CATA",
    "TRADE_FILTER_BASE",
    "UI_PORTAL_CAT_WONDER_TREASURE",
    "TRADE_FILTER_EMOTE",
    "TRADE_FILTER_CUSTOM",
    "FOOD_GENERIC_VEG_NAME_L",
    "MSN_FISH_NOUN_8",
    "ITEMGEN_WATER_NOUN_14",
    "UI_SALVAGE_CRATE",
    "UI_PORTAL_CAT_MAT_COOK",
    "PC_SETTINGS_CUSTOM",
];

pub fn extra_filter_ids() -> [u8; 10] {
    let mut ids = [0u8; 10];
    let mut i = 0;
    while i < EXTRA_FILTER_NAMES.len() {
        let name = EXTRA_FILTER_NAMES[i];
        if let Some(digits) = name.get(6..) {
            if let Ok(text) = std::str::from_utf8(digits) {
                if let Ok(n) = text.parse::<u8>() {
                    ids[i] = n.saturating_sub(1);
                }
            }
        }
        i = i.wrapping_add(1);
    }
    ids
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub scale: f32,
    pub slot: f32,
    pub panel_w: f32,
    pub tech_rows: i32,
    pub cargo_rows: i32,
}

fn fmt_g(value: f32) -> String {
    if (value - value.round()).abs() < 1e-6 {
        format!("{}", value.round() as i32)
    } else {
        format!("{value}")
    }
}

impl Plan {
    pub fn describe(&self) -> String {
        format!(
            "slots {} ({:.0}%), tech {} rows, cargo {} rows",
            fmt_g(self.slot),
            self.scale * 100.0,
            self.tech_rows,
            self.cargo_rows
        )
    }
}

fn cargo_rows_for(tech_rows: i32, pitch: f32) -> i32 {
    let cargo_y = TECH_TOP + tech_rows as f32 * pitch + 2.0 + HEADER_GAP;
    ((GRID_BOTTOM - cargo_y) / pitch + 1e-6) as i32
}

pub fn plan_for(scale: f32) -> Plan {
    let slot = (SLOT * scale * 100.0).round() / 100.0;
    let pitch = slot + 2.0 * MARGIN;
    let mut best_total = i32::MIN;
    let mut options: Vec<(i32, i32)> = Vec::new();
    let mut t = TECH_ROWS_VANILLA;
    while t < 9 {
        let rows = cargo_rows_for(t, pitch);
        options.push((t, rows));
        let total = t.saturating_add(rows);
        if total > best_total {
            best_total = total;
        }
        t = t.saturating_add(1);
    }
    let ideal = best_total as f32 * TECH_ROWS_VANILLA as f32 / (TECH_ROWS_VANILLA as f32 + 5.0);
    let mut tech_rows = TECH_ROWS_VANILLA;
    let mut best_dist = f32::MAX;
    for (t, rows) in &options {
        if t.saturating_add(*rows) != best_total {
            continue;
        }
        let dist = (*t as f32 - ideal).abs();
        if dist < best_dist {
            best_dist = dist;
            tech_rows = *t;
        }
    }
    Plan {
        scale,
        slot,
        panel_w: pitch * (COLUMNS as f32 + SLACK),
        tech_rows,
        cargo_rows: cargo_rows_for(tech_rows, pitch),
    }
}

pub fn chest_panel_width(plan: &Plan) -> f32 {
    let slot = (CHEST_SLOT as f32 * plan.slot / SLOT).round() as i32;
    let pitch = slot as f32 + 2.0 * MARGIN;
    pitch * (COLUMNS as f32 + SLACK)
}

pub fn scaled_cells(vanilla_slot: i32, vanilla_rows: i32, plan: &Plan) -> Option<(i32, i32)> {
    let ratio = plan.slot / VANILLA_SLOT as f32;
    if ratio.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let slot = (vanilla_slot as f32 * ratio).round() as i32;
    let rows = (vanilla_rows as f32 / ratio + 1e-4) as i32;
    if slot < 1 || rows < 1 {
        return None;
    }
    Some((slot, rows))
}

pub fn vanilla_kind(render_h: i32) -> Option<&'static str> {
    match render_h {
        5 => Some("cargo"),
        3 => Some("tech"),
        _ => None,
    }
}

pub fn is_cargo_height(render_h: i32) -> bool {
    CARGO_HEIGHTS.contains(&render_h)
}

pub fn preset_from_value(data: &Value) -> Result<(f32, bool), String> {
    if !data.is_object() {
        return Err("smaller_inventory_slots_preset.json is not a settings object".into());
    }
    let scale = data
        .get("scale")
        .and_then(Value::as_f64)
        .ok_or_else(|| "smaller_inventory_slots_preset.json has no scale".to_string())?;
    let scale = (scale as f32 * 100.0).round() / 100.0;
    let (low, high) = SCALE_RANGE;
    if !(low <= scale && scale <= high) {
        return Err(format!(
            "smaller_inventory_slots_preset.json has scale {scale}, which is not between {low} and {high}"
        ));
    }
    let filters = data.get("filters") != Some(&Value::Bool(false));
    Ok((scale, filters))
}

pub fn load_preset_file(dir: &Path) -> Result<(f32, bool), String> {
    let page = dir.join("UI").join("INVENTORYPAGE.MBIN");
    if !page.is_file() {
        return Err("UI\\INVENTORYPAGE.MBIN is missing: extract the whole mod folder".into());
    }
    let text = std::fs::read_to_string(dir.join("smaller_inventory_slots_preset.json")).map_err(|e| e.to_string())?;
    let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    preset_from_value(&data)
}

pub fn preset_plan_from_file(dir: &Path) -> Result<Option<Plan>, String> {
    let (scale, _) = load_preset_file(dir)?;
    if scale == VANILLA_SCALE {
        Ok(None)
    } else {
        Ok(Some(plan_for(scale)))
    }
}

pub fn preset_filters_from_file(dir: &Path) -> Result<bool, String> {
    let text = std::fs::read_to_string(dir.join("smaller_inventory_slots_preset.json")).map_err(|e| e.to_string())?;
    let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(preset_from_value(&data)?.1)
}

pub fn sane_y(value: f32) -> bool {
    value.is_finite() && value.abs() <= 1.0e6
}

pub fn fixed_f32(value: f32) -> String {
    format!("{value:.3}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_matches_readme_sizes() {
        let p70 = plan_for(0.7);
        assert_eq!(p70.tech_rows, 4);
        assert_eq!(p70.cargo_rows, 7);
        assert!((p70.slot - 63.0).abs() < f32::EPSILON);
        let p80 = plan_for(0.8);
        assert_eq!(p80.tech_rows, 3);
        assert_eq!(p80.cargo_rows, 6);
        let p100 = plan_for(1.0);
        assert_eq!(p100.tech_rows, 3);
        assert_eq!(p100.cargo_rows, 5);
    }

    #[test]
    fn chest_width_and_scaled_cells() {
        let plan = plan_for(0.7);
        let w = chest_panel_width(&plan);
        assert!((w - 609.0).abs() < 0.01);
        assert_eq!(scaled_cells(90, 5, &plan), Some((63, 7)));
        assert_eq!(scaled_cells(74, 7, &plan), Some((52, 10)));
        assert_eq!(scaled_cells(90, 3, &plan), Some((63, 4)));
    }

    #[test]
    fn preset_parsing_and_filter_ids() {
        let (scale, filters) = preset_from_value(&json!({"scale": 0.7})).unwrap();
        assert!((scale - 0.7).abs() < f32::EPSILON);
        assert!(filters);
        let (_, filters) = preset_from_value(&json!({"scale": 1.0, "filters": false})).unwrap();
        assert!(!filters);
        assert!(preset_from_value(&json!({"scale": 1.1})).is_err());
        assert_eq!(extra_filter_ids(), [5, 7, 8, 10, 13, 14, 15, 16, 17, 18]);
        assert_eq!(VANILLA_FILTERS.wrapping_add(CATEGORIES), 19);
    }

    #[test]
    fn foods_and_custom_slots() {
        use crate::common::custom_filter::{CUSTOM_ID, FOODS_ID};
        let ids = extra_filter_ids();
        assert_eq!(i32::from(ids[FOODS_INDEX]), FOODS_ID);
        assert_eq!(i32::from(ids[CUSTOM_INDEX]), CUSTOM_ID);
        assert_eq!(BUTTON_TIPS.len(), CATEGORIES as usize);
        assert_eq!(i32::from(VANILLA_FILTERS) + BUTTON_TIPS.len() as i32 - 1, CUSTOM_ID);
        assert_eq!(CUSTOM_HEAD[3] as i32, CUSTOM_ID);
        for tip in BUTTON_TIPS {
            assert!(tip.len() < TIP_SIZE && tip.is_ascii());
        }
    }
}
