//! Port of `inventory.py`: smaller tech/cargo slots and private UI copies.

use super::filter_bar::{self, SIDE_BAR};
use super::grid::py_g;
use super::{transform, Job};
use crate::tools::Tools;
use nms_amumss::exml::{Element, Mxml};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;

pub const PRIVATE: &str = "UI/BIGGERITEMGRIDS/INVENTORY";
pub const SWAP_PAGE: &str = "UI/SWAPPAGE.MBIN";
pub const SWAP_PAK: &str = "NMSARC.UI.pak";
pub const COLUMNS: i32 = 10;
pub const SLOT: f64 = 90.0;
pub const MARGIN: f64 = 3.0;
pub const GRID_LEFT: f64 = 1092.0;
pub const TECH_TOP: f64 = 735.0;
pub const TECH_ROWS_VANILLA: i32 = 3;
pub const GRID_BOTTOM: f64 = 1550.0;
pub const HEADER_GAP: f64 = 45.0;
pub const HEADER_ABOVE_CARGO: f64 = 40.0;
pub const SLACK: f64 = 0.5;
pub const SCROLLBAR_ROOM: f64 = 54.0;
pub const SCALE_RANGE: (f64, f64) = (0.5, 1.0);
pub const DEFAULT_SCALE: f64 = 1.0;
pub const ROWS_IN_PANEL: i32 = 12;
pub const COMPONENTS: &str = "UI/COMPONENTS/INVENTORY";
pub const TECH_LARGE_FILE: &str = "SQU_INV_BOXTECH_LARGE";
pub const CHEST_SLOT: f64 = 74.0;
pub const CHEST_ROWS: i32 = 7;
pub const CHEST_PANEL: f64 = 804.0;
pub const CHEST_ROOT_W: f64 = 855.0;
pub const CHEST_ROOT_PAD: f64 = 4.0;

const VANILLA_MSG: &str =
    "Inventory slots at 100%: inventory layout mod removed, the game uses its own.";

const BOXES: &[(&str, &str)] = &[
    ("BOXREGULAR", "SQU_INV_BOXREGULAR"),
    ("BOXTECH", "SQU_INV_BOXTECH"),
];
const SLOTS: &[(&str, &str)] = &[
    ("SLOT", "SQU_INV_SLOT_LARGE"),
    ("SLOTFAKE", "SQU_SLOTFAKELARGE"),
];
const CHEST_SLOTS: &[(&str, &str)] = &[
    ("SLOT_SMALL", "SQU_INV_SLOT_SMALL"),
    ("SLOTFAKE_SMALL", "SQU_SLOTFAKESMALL"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub scale: f64,
    pub slot: f64,
    pub pitch: f64,
    pub panel_w: f64,
    pub panel_h: f64,
    pub box_w: f64,
    pub box_x: f64,
    pub tech_rows: i32,
    pub tech_h: f64,
    pub cargo_rows: i32,
    pub cargo_h: f64,
    pub cargo_y: f64,
}

impl Plan {
    pub fn vanilla(&self) -> bool {
        self.scale == 1.0
    }

    pub fn describe(&self) -> String {
        format!(
            "slots {} ({:.0}%), tech {} rows, cargo {} rows",
            py_g(self.slot),
            self.scale * 100.0,
            self.tech_rows,
            self.cargo_rows
        )
    }
}

fn py_round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn scale_from(value: f64) -> Result<f64, String> {
    let scale = py_round2(value);
    let (low, high) = SCALE_RANGE;
    if !(low <= scale && scale <= high) {
        return Err(format!(
            "inventory_scale must be between {low} and {high}"
        ));
    }
    Ok(scale)
}

pub fn plan_for(scale: f64) -> Plan {
    let slot = py_round2(SLOT * scale);
    let pitch = slot + 2.0 * MARGIN;
    let panel_w = pitch * (COLUMNS as f64 + SLACK);
    let box_w = pitch * COLUMNS as f64 + SCROLLBAR_ROOM;

    let cargo_rows_for = |tech_rows: i32| -> (f64, i32) {
        let cargo_y = TECH_TOP + tech_rows as f64 * pitch + 2.0 + HEADER_GAP;
        (
            cargo_y,
            ((GRID_BOTTOM - cargo_y) / pitch + 1e-6).trunc() as i32,
        )
    };

    let tech_rows = if scale == 1.0 {
        TECH_ROWS_VANILLA
    } else {
        let options: Vec<(i32, f64, i32)> = (TECH_ROWS_VANILLA..9)
            .map(|t| {
                let (y, rows) = cargo_rows_for(t);
                (t, y, rows)
            })
            .collect();
        let best_total = options
            .iter()
            .map(|(t, _, rows)| t + rows)
            .max()
            .unwrap_or(0);
        let ideal = best_total as f64 * TECH_ROWS_VANILLA as f64
            / (TECH_ROWS_VANILLA as f64 + 5.0);
        options
            .iter()
            .filter(|(t, _, rows)| t + rows == best_total)
            .min_by(|a, b| {
                let ka = (a.0 as f64 - ideal).abs();
                let kb = (b.0 as f64 - ideal).abs();
                ka.partial_cmp(&kb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            })
            .map(|(t, _, _)| *t)
            .unwrap_or(TECH_ROWS_VANILLA)
    };
    let (cargo_y, cargo_rows) = cargo_rows_for(tech_rows);
    Plan {
        scale,
        slot,
        pitch,
        panel_w,
        panel_h: pitch * ROWS_IN_PANEL as f64,
        box_w,
        box_x: GRID_LEFT + box_w,
        tech_rows,
        tech_h: tech_rows as f64 * pitch + 2.0,
        cargo_rows,
        cargo_h: cargo_rows as f64 * pitch,
        cargo_y,
    }
}

pub fn chest_metrics(plan: &Plan) -> (f64, f64, f64, i32, f64, f64, f64) {
    let slot = (CHEST_SLOT * plan.slot / SLOT).round() as i32 as f64;
    let pitch = slot + 2.0 * MARGIN;
    let panel_w = pitch * (COLUMNS as f64 + SLACK);
    let rows = (CHEST_ROWS as f64 / (plan.slot / SLOT) + 1e-4).trunc() as i32;
    let window_h = rows as f64 * pitch;
    let root_w = panel_w + (CHEST_ROOT_W - CHEST_PANEL);
    (slot, pitch, panel_w, rows, window_h, root_w, window_h + CHEST_ROOT_PAD)
}

pub fn build_slots(
    scale: f64,
    filters: bool,
    mod_dir: &Path,
    pcbanks: &Path,
    tools: &Tools,
    work: &Path,
) -> Result<String, String> {
    if scale == 1.0 && !filters {
        if mod_dir.exists() {
            fs::remove_dir_all(mod_dir).map_err(|e| e.to_string())?;
        }
        return Ok(VANILLA_MSG.to_string());
    }
    for tool in [&tools.hgpaktool, &tools.mbincompiler] {
        if !tool.is_file() {
            return Err(format!("{} is missing.", tool.display()));
        }
    }
    let plan = plan_for(scale);
    let what = format!(
        "Inventory {}{}",
        plan.describe(),
        if filters { ", filter buttons" } else { "" }
    );
    let pak = pcbanks.join("NMSARC.Precache.pak");
    let sort_buttons: Rc<RefCell<Vec<Element>>> = Rc::new(RefCell::new(Vec::new()));

    let mut jobs = vec![Job {
        source: "UI/INVENTORYPAGE.MBIN".into(),
        target: "UI/INVENTORYPAGE.MBIN".into(),
        apply: {
            let plan = plan;
            Box::new(move |doc: &mut Mxml| page(doc.root_mut(), &plan, filters))
        },
    }];
    if !plan.vanilla() || filters {
        let sort_buttons = sort_buttons.clone();
        jobs.push(Job {
            source: SIDE_BAR.into(),
            target: SIDE_BAR.into(),
            apply: Box::new(move |doc: &mut Mxml| {
                let copied = filter_bar::copy_sort_buttons(doc.root())?;
                sort_buttons.borrow_mut().extend(copied);
                if filters {
                    filter_bar::side_bar(doc.root_mut())?;
                }
                Ok(())
            }),
        });
    }
    if !plan.vanilla() {
        jobs.push(box_job(
            &format!("{COMPONENTS}/SQU_INV_BOXREGULAR.MBIN"),
            &private("BOXREGULAR"),
            plan,
            false,
        ));
        jobs.push(box_job(
            &format!("{COMPONENTS}/SQU_INV_BOXTECH.MBIN"),
            &private("BOXTECH"),
            plan,
            true,
        ));
        jobs.push(Job {
            source: format!("{COMPONENTS}/{TECH_LARGE_FILE}.MBIN"),
            target: private("BOXTECH_LARGE"),
            apply: {
                let plan = plan;
                Box::new(move |doc: &mut Mxml| tech_large_transform(doc.root_mut(), &plan))
            },
        });
        jobs.push(slot_job(
            &format!("{COMPONENTS}/SQU_INV_SLOT_LARGE.MBIN"),
            &private("SLOT"),
            plan.slot,
        ));
        jobs.push(slot_job(
            &format!("{COMPONENTS}/SQU_SLOTFAKELARGE.MBIN"),
            &private("SLOTFAKE"),
            plan.slot,
        ));
        jobs.extend(chest_jobs(plan));
    }

    if work.exists() {
        fs::remove_dir_all(work).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut built = transform(&work.join("inventory"), jobs, &pak, tools)?;
        if !plan.vanilla() {
            let buttons = sort_buttons.borrow().clone();
            let swap_jobs = vec![Job {
                source: SWAP_PAGE.into(),
                target: SWAP_PAGE.into(),
                apply: Box::new(move |doc: &mut Mxml| {
                    if buttons.len() != 4 {
                        return Err(format!(
                            "the sort buttons were not read from the side bar, found {}",
                            buttons.len()
                        ));
                    }
                    filter_bar::add_locker_sort(doc.root_mut(), buttons)
                }),
            }];
            let more = transform(
                &work.join("swap"),
                swap_jobs,
                &pcbanks.join(SWAP_PAK),
                tools,
            )?;
            built.extend(more);
        }
        if !filters {
            built.remove(SIDE_BAR);
        }
        if mod_dir.exists() {
            fs::remove_dir_all(mod_dir).map_err(|e| e.to_string())?;
        }
        for (target, path) in &built {
            let out = mod_dir.join(target);
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(path, &out).map_err(|e| e.to_string())?;
        }
        Ok(format!("{what} built: {}", mod_dir.display()))
    })();
    let _ = fs::remove_dir_all(work);
    result
}

fn private(name: &str) -> String {
    format!("{PRIVATE}/{name}.MBIN")
}

fn box_job(source: &str, target: &str, plan: Plan, tech: bool) -> Job {
    Job {
        source: source.into(),
        target: target.into(),
        apply: Box::new(move |doc: &mut Mxml| box_transform(doc.root_mut(), &plan, tech)),
    }
}

fn slot_job(source: &str, target: &str, slot: f64) -> Job {
    Job {
        source: source.into(),
        target: target.into(),
        apply: Box::new(move |doc: &mut Mxml| slot_transform(doc.root_mut(), slot)),
    }
}

fn chest_jobs(plan: Plan) -> Vec<Job> {
    let (slot, _pitch, _panel_w, _rows, _window_h, root_w, root_h) = chest_metrics(&plan);
    vec![
        Job {
            source: format!("{COMPONENTS}/SQU_INV_BOXCOMPARE.MBIN"),
            target: private("BOXCOMPARE"),
            apply: {
                let plan = plan;
                Box::new(move |doc: &mut Mxml| chest_transform(doc.root_mut(), &plan))
            },
        },
        slot_job(
            &format!("{COMPONENTS}/SQU_INV_SLOT_SMALL.MBIN"),
            &private("SLOT_SMALL"),
            slot,
        ),
        slot_job(
            &format!("{COMPONENTS}/SQU_SLOTFAKESMALL.MBIN"),
            &private("SLOTFAKE_SMALL"),
            slot,
        ),
        Job {
            source: "UI/ROCKETLOCKERPAGE.MBIN".into(),
            target: "UI/ROCKETLOCKERPAGE.MBIN".into(),
            apply: Box::new(move |doc: &mut Mxml| {
                rocket_page_transform(doc.root_mut(), root_w, root_h)
            }),
        },
    ]
}

/// Remove the top-level `PICKING` layer (build 183791 added it: the "Transfer to other
/// Inventories" bar). On a page built here the game shows that layer all the time, and it hides
/// the whole side bar (filters, sort, Deposit, Lock). Tested in game on 183877: the same page
/// without it shows the side bar and the transfer still works. The stock files never had it.
/// Returns how many layers were removed (0 on older builds).
fn drop_picking_layer(root: &mut Element) -> usize {
    let Some(children) = xml::prop_mut(root, "Children") else {
        return 0;
    };
    let before = xml::el_count(children);
    children
        .children
        .retain(|node| node.as_element().map_or(true, |entry| xml::element_id(entry) != Some("PICKING")));
    before - xml::el_count(children)
}

fn page(root: &mut Element, plan: &Plan, filters: bool) -> Result<(), String> {
    drop_picking_layer(root);
    if !plan.vanilla() {
        page_transform(root, plan)?;
        if set_headers(root, plan)? != 2 {
            return Err(
                "the inventory page does not have its CARGOHEADER twice: the game changed its layout"
                    .into(),
            );
        }
    }
    if filters {
        filter_bar::widen_page(root)?;
    }
    Ok(())
}

fn page_transform(root: &mut Element, plan: &Plan) -> Result<(), String> {
    let mapping: HashMap<String, String> = BOXES
        .iter()
        .map(|(name, file)| {
            (
                format!("{COMPONENTS}/{file}.MBIN").to_ascii_uppercase(),
                private(name),
            )
        })
        .collect();
    let changed = repoint(root, &mapping);
    if changed != 4 {
        return Err(format!(
            "expected to find the tech and cargo boxes twice on the inventory page, found {changed}"
        ));
    }
    let mut placed = 0;
    walk_mut(root, &mut |entry| {
        if attr(entry, "value") != Some("GcNGuiLayerData") {
            return Ok(());
        }
        let Some(id) = element_id(entry).map(str::to_string) else {
            return Ok(());
        };
        let (y, h) = match id.as_str() {
            "SQU_INV_TECH" => (TECH_TOP, plan.tech_h),
            "SQU_INV_REGULAR" => (plan.cargo_y, plan.cargo_h),
            _ => return Ok(()),
        };
        set_box(entry, Some(plan.box_x), Some(y), plan.box_w, h)?;
        placed += 1;
        Ok(())
    })?;
    if placed != 4 {
        return Err(format!(
            "expected the tech and cargo box entries twice on the inventory page, found {placed}"
        ));
    }
    let large = HashMap::from([(
        format!("{COMPONENTS}/{TECH_LARGE_FILE}.MBIN").to_ascii_uppercase(),
        private("BOXTECH_LARGE"),
    )]);
    let changed = repoint(root, &large);
    if changed != 2 {
        return Err(format!(
            "expected the large tech box twice on the inventory page, found {changed}"
        ));
    }
    let mut placed = 0;
    walk_mut(root, &mut |entry| {
        if attr(entry, "value") == Some("GcNGuiLayerData") && element_id(entry) == Some("SQU_TECH_BIG")
        {
            set_box(
                entry,
                Some(plan.box_x),
                Some(TECH_TOP),
                plan.box_w,
                plan.cargo_h,
            )?;
            placed += 1;
        }
        Ok(())
    })?;
    if placed != 2 {
        return Err(format!(
            "expected the large tech box entry twice on the inventory page, found {placed}"
        ));
    }
    Ok(())
}

fn set_headers(root: &mut Element, plan: &Plan) -> Result<i32, String> {
    let y = plan.cargo_y - HEADER_ABOVE_CARGO;
    let paths = find_id_props(root, "CARGOHEADER");
    let count = paths.len() as i32;
    for path in paths {
        if path.is_empty() {
            return Err("CARGOHEADER ID has no parent".into());
        }
        let parent = get_mut(root, &path[..path.len() - 1]).ok_or("CARGOHEADER parent")?;
        let layout = prop_mut(parent, "Layout").ok_or("CARGOHEADER has no Layout")?;
        set_named(layout, "Position Y", &fmt_f(y))?;
    }
    Ok(count)
}

fn slot_transform(root: &mut Element, slot: f64) -> Result<(), String> {
    set_box(root, None, None, slot, slot)
}

fn box_transform(root: &mut Element, plan: &Plan, tech: bool) -> Result<(), String> {
    let height = if tech { plan.tech_h } else { plan.cargo_h };
    let own = if tech { "BOXTECH" } else { "BOXREGULAR" };
    let y = if tech { TECH_TOP } else { plan.cargo_y };
    let own_file = if tech {
        "SQU_INV_BOXTECH"
    } else {
        "SQU_INV_BOXREGULAR"
    };
    rewrite_box(
        root,
        Some(plan.box_x),
        Some(y),
        plan.box_w,
        height,
        plan.pitch * COLUMNS as f64,
        height,
        plan.panel_w,
        plan.panel_h,
        plan.slot,
        &file_map(SLOTS, own, own_file),
        None,
    )
}

fn tech_large_transform(root: &mut Element, plan: &Plan) -> Result<(), String> {
    rewrite_box(
        root,
        Some(plan.box_x),
        Some(TECH_TOP),
        plan.box_w,
        plan.cargo_h,
        plan.pitch * COLUMNS as f64,
        plan.cargo_h,
        plan.panel_w,
        plan.panel_h,
        plan.slot,
        &file_map(SLOTS, "BOXTECH_LARGE", TECH_LARGE_FILE),
        None,
    )
}

fn chest_transform(root: &mut Element, plan: &Plan) -> Result<(), String> {
    let (slot, pitch, panel_w, _rows, window_h, root_w, root_h) = chest_metrics(plan);
    rewrite_box(
        root,
        None,
        None,
        root_w,
        root_h,
        pitch * COLUMNS as f64,
        window_h,
        panel_w,
        window_h,
        slot,
        &file_map(CHEST_SLOTS, "BOXCOMPARE", "SQU_INV_BOXCOMPARE"),
        Some(root_w),
    )
}

fn rocket_page_transform(root: &mut Element, root_w: f64, root_h: f64) -> Result<(), String> {
    let target = private("BOXCOMPARE");
    let mapping = HashMap::from([(
        format!("{COMPONENTS}/SQU_INV_BOXCOMPARE.MBIN").to_ascii_uppercase(),
        target.clone(),
    )]);
    let changed = repoint(root, &mapping);
    if changed != 1 {
        return Err(format!(
            "expected the chest box once on the rocket locker page, found {changed}"
        ));
    }
    let target_u = target.to_ascii_uppercase();
    let mut placed = 0;
    walk_mut(root, &mut |entry| {
        if attr(entry, "value") != Some("GcNGuiLayerData") || element_id(entry) != Some("INVENTORY_BOX")
        {
            return Ok(());
        }
        let Some(filename) = prop(data_of(entry), "DataFilename") else {
            return Ok(());
        };
        let Some(value) = attr(filename, "value") else {
            return Ok(());
        };
        if value.to_ascii_uppercase() != target_u {
            return Ok(());
        }
        set_box(entry, None, None, root_w, root_h)?;
        placed += 1;
        Ok(())
    })?;
    if placed != 1 {
        return Err(format!(
            "expected the chest box entry once on the rocket locker page, found {placed}"
        ));
    }
    Ok(())
}

fn file_map(slots: &[(&str, &str)], own: &str, own_file: &str) -> HashMap<String, String> {
    let mut mapping = HashMap::new();
    for (name, file) in slots {
        mapping.insert(
            format!("{COMPONENTS}/{file}.MBIN").to_ascii_uppercase(),
            private(name),
        );
    }
    mapping.insert(
        format!("{COMPONENTS}/{own_file}.MBIN").to_ascii_uppercase(),
        private(own),
    );
    mapping
}

fn rewrite_box(
    root: &mut Element,
    x: Option<f64>,
    y: Option<f64>,
    box_w: f64,
    root_h: f64,
    outline_w: f64,
    outline_h: f64,
    panel_w: f64,
    panel_h: f64,
    slot: f64,
    slot_map: &HashMap<String, String>,
    scrollbar_x: Option<f64>,
) -> Result<(), String> {
    set_box(root, x, y, box_w, root_h)?;
    if let Some(sx) = scrollbar_x {
        place_scrollbar(root, sx)?;
    }
    let mut found_outline = false;
    let mut found_inv = false;
    let n = {
        let kids = prop(data_of(root), "Children").ok_or("inventory box has no Children")?;
        xml::el_count(kids)
    };
    for i in 0..n {
        let child = {
            let data = xml::data_of_mut(root);
            let kids = xml::prop_mut(data, "Children").ok_or("inventory box has no Children")?;
            xml::nth_el_mut(kids, i).ok_or("broken box child")?
        };
        let ident = xml::element_id(child).map(str::to_string);
        if ident.as_deref() == Some("OUTLINEBOX") {
            xml::set_box(child, None, None, outline_w, outline_h)?;
            found_outline = true;
        } else if ident.as_deref() == Some("INVENTORY") {
            xml::set_box(child, None, None, panel_w, panel_h)?;
            found_inv = true;
            rewrite_inventory_panel(child, panel_w, panel_h, slot)?;
        }
    }
    let mut missing = Vec::new();
    if !found_outline {
        missing.push("OUTLINEBOX");
    }
    if !found_inv {
        missing.push("INVENTORY");
    }
    if !missing.is_empty() {
        return Err(format!(
            "the inventory box has no {}: the game changed its layout",
            missing.join(", ")
        ));
    }
    let pitch = outline_w / COLUMNS as f64;
    if !(pitch * COLUMNS as f64 <= panel_w && panel_w < pitch * (COLUMNS as f64 + 1.0)) {
        return Err(format!(
            "panel width {} does not wrap after {COLUMNS} columns of pitch {}",
            py_g(panel_w),
            py_g(pitch)
        ));
    }
    if repoint(root, slot_map) < 10 {
        return Err("the inventory box does not embed its slot files as expected".into());
    }
    Ok(())
}

fn rewrite_inventory_panel(
    child: &mut Element,
    panel_w: f64,
    panel_h: f64,
    slot: f64,
) -> Result<(), String> {
    let n = {
        let kids = xml::prop(xml::data_of(child), "Children").ok_or("INVENTORY has no Children")?;
        xml::el_count(kids)
    };
    for g in 0..n {
        let grand = {
            let data = xml::data_of_mut(child);
            let kids = xml::prop_mut(data, "Children").ok_or("INVENTORY has no Children")?;
            xml::nth_el_mut(kids, g).ok_or("broken INVENTORY child")?
        };
        if xml::element_id(grand) == Some("OUTLINE") {
            xml::set_box(grand, None, None, panel_w, panel_h)?;
            let inner_n = {
                let kids = xml::prop(xml::data_of(grand), "Children").ok_or("OUTLINE has no Children")?;
                xml::el_count(kids)
            };
            for inn in 0..inner_n {
                let inner = {
                    let data = xml::data_of_mut(grand);
                    let kids = xml::prop_mut(data, "Children").ok_or("OUTLINE has no Children")?;
                    xml::nth_el_mut(kids, inn).ok_or("broken OUTLINE child")?
                };
                xml::set_box(inner, None, None, panel_w, panel_h)?;
                let fake_n = xml::prop(xml::data_of(inner), "Children")
                    .map(xml::el_count)
                    .unwrap_or(0);
                for f in 0..fake_n {
                    let fake = {
                        let data = xml::data_of_mut(inner);
                        let kids = xml::prop_mut(data, "Children").ok_or("inner has no Children")?;
                        xml::nth_el_mut(kids, f).ok_or("broken fake slot")?
                    };
                    xml::set_box(fake, None, None, slot, slot)?;
                }
            }
        } else {
            xml::set_box(grand, None, None, slot, slot)?;
        }
    }
    Ok(())
}

fn place_scrollbar(root: &mut Element, x: f64) -> Result<(), String> {
    let n = {
        let kids = xml::prop(xml::data_of(root), "Children").ok_or("box has no Children")?;
        xml::el_count(kids)
    };
    let mut moved = 0;
    for i in 0..n {
        let is_bar = {
            let kids = xml::prop(xml::data_of(root), "Children").ok_or("box has no Children")?;
            let child = xml::nth_el(kids, i).ok_or("broken box child")?;
            xml::subtree_has_id(child, "SCROLLBAR")
        };
        if is_bar {
            let child = {
                let data = xml::data_of_mut(root);
                let kids = xml::prop_mut(data, "Children").ok_or("box has no Children")?;
                xml::nth_el_mut(kids, i).ok_or("broken box child")?
            };
            let lay = xml::layout_mut(child)?;
            xml::set_named(lay, "Position X", &xml::fmt_f(x))?;
            moved += 1;
        }
    }
    if moved != 1 {
        return Err(format!(
            "expected one scrollbar at the box edge, found {moved}"
        ));
    }
    Ok(())
}

fn repoint(root: &mut Element, mapping: &HashMap<String, String>) -> i32 {
    let mut changed = 0;
    walk_mut(root, &mut |el| {
        if xml::attr(el, "name") == Some("DataFilename") {
            let key = xml::attr(el, "value").unwrap_or("").to_ascii_uppercase();
            if let Some(new) = mapping.get(&key) {
                xml::set_value(el, new.clone());
                changed += 1;
            }
        }
        Ok(())
    })
    .ok();
    changed
}

fn walk_mut(
    el: &mut Element,
    f: &mut impl FnMut(&mut Element) -> Result<(), String>,
) -> Result<(), String> {
    f(el)?;
    let n = xml::el_count(el);
    for i in 0..n {
        let child = xml::nth_el_mut(el, i).ok_or("broken walk")?;
        walk_mut(child, f)?;
    }
    Ok(())
}

fn find_id_props(root: &Element, id: &str) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    fn rec(el: &Element, path: &[usize], id: &str, out: &mut Vec<Vec<usize>>) {
        if xml::attr(el, "name") == Some("ID") && xml::attr(el, "value") == Some(id) {
            out.push(path.to_vec());
        }
        for (i, child) in xml::element_children(el).enumerate() {
            let mut p = path.to_vec();
            p.push(i);
            rec(child, &p, id, out);
        }
    }
    rec(root, &[], id, &mut out);
    out
}

fn get_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let mut cur = root;
    for &i in path {
        cur = xml::nth_el_mut(cur, i)?;
    }
    Some(cur)
}

pub(super) mod xml {
    use nms_amumss::exml::{Element, XMLNode};

    pub fn fmt_f(value: f64) -> String {
        format!("{value:.6}")
    }

    pub fn attr<'a>(el: &'a Element, key: &str) -> Option<&'a str> {
        el.attributes.get(key).map(String::as_str)
    }

    pub fn set_value(el: &mut Element, value: String) {
        el.attributes.insert("value".into(), value);
    }

    pub fn prop<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
        element_children(el).find(|c| attr(c, "name") == Some(name))
    }

    pub fn prop_mut<'a>(el: &'a mut Element, name: &str) -> Option<&'a mut Element> {
        el.children
            .iter_mut()
            .filter_map(|n| n.as_mut_element())
            .find(|c| attr(c, "name") == Some(name))
    }

    pub fn element_children(el: &Element) -> impl Iterator<Item = &Element> {
        el.children.iter().filter_map(|n| n.as_element())
    }

    pub fn nth_el(parent: &Element, i: usize) -> Option<&Element> {
        parent
            .children
            .iter()
            .filter_map(|n| n.as_element())
            .nth(i)
    }

    pub fn nth_el_mut(parent: &mut Element, i: usize) -> Option<&mut Element> {
        parent
            .children
            .iter_mut()
            .filter_map(|n| n.as_mut_element())
            .nth(i)
    }

    pub fn el_count(parent: &Element) -> usize {
        parent
            .children
            .iter()
            .filter(|n| n.as_element().is_some())
            .count()
    }

    pub fn data_of(entry: &Element) -> &Element {
        if let Some(first) = nth_el(entry, 0) {
            if attr(first, "name").unwrap_or("").starts_with("GcNGui") {
                return first;
            }
        }
        entry
    }

    pub fn data_of_mut(entry: &mut Element) -> &mut Element {
        let unwrap = nth_el(entry, 0)
            .map(|first| attr(first, "name").unwrap_or("").starts_with("GcNGui"))
            .unwrap_or(false);
        if unwrap {
            nth_el_mut(entry, 0).unwrap()
        } else {
            entry
        }
    }

    pub fn element_id(entry: &Element) -> Option<&str> {
        let element = prop(data_of(entry), "Element Data")?;
        let ident = prop(element, "ID")?;
        attr(ident, "value")
    }

    pub fn layout_of(entry: &Element) -> Result<&Element, String> {
        let element = prop(data_of(entry), "Element Data").ok_or("no Element Data")?;
        prop(element, "Layout").ok_or_else(|| "no Layout".to_string())
    }

    pub fn layout_mut(entry: &mut Element) -> Result<&mut Element, String> {
        let data = data_of_mut(entry);
        let element = prop_mut(data, "Element Data").ok_or("no Element Data")?;
        prop_mut(element, "Layout").ok_or_else(|| "no Layout".to_string())
    }

    pub fn set_named(layout: &mut Element, name: &str, value: &str) -> Result<(), String> {
        let field = prop_mut(layout, name).ok_or_else(|| format!("no {name}"))?;
        set_value(field, value.to_string());
        Ok(())
    }

    pub fn set_box(
        entry: &mut Element,
        x: Option<f64>,
        y: Option<f64>,
        w: f64,
        h: f64,
    ) -> Result<(), String> {
        let layout = layout_mut(entry)?;
        if let Some(x) = x {
            set_named(layout, "Position X", &fmt_f(x))?;
        }
        if let Some(y) = y {
            set_named(layout, "Position Y", &fmt_f(y))?;
        }
        set_named(layout, "Width", &fmt_f(w))?;
        set_named(layout, "Height", &fmt_f(h))?;
        Ok(())
    }

    pub fn get_f(entry: &Element, name: &str) -> Result<f64, String> {
        let layout = layout_of(entry)?;
        let field = prop(layout, name).ok_or_else(|| format!("no {name}"))?;
        let v = attr(field, "value").ok_or("missing value")?;
        v.parse().map_err(|_| format!("not a float: {v}"))
    }

    pub fn set_layout_field(entry: &mut Element, name: &str, value: f64) -> Result<(), String> {
        let layout = layout_mut(entry)?;
        set_named(layout, name, &fmt_f(value))
    }

    pub fn is_percent(entry: &Element) -> Result<bool, String> {
        let layout = layout_of(entry)?;
        let field = prop(layout, "AnchorPercent").ok_or("no AnchorPercent")?;
        Ok(attr(field, "value") == Some("true"))
    }

    pub fn horizontal(entry: &Element) -> Option<&str> {
        let layout = layout_of(entry).ok()?;
        let align = prop(layout, "Align")?;
        let side = prop(align, "Horizontal")?;
        attr(side, "value")
    }

    pub fn children_of(entry: &Element) -> Vec<&Element> {
        match prop(data_of(entry), "Children") {
            Some(c) => element_children(c).collect(),
            None => Vec::new(),
        }
    }

    pub fn child_count(entry: &Element) -> usize {
        prop(data_of(entry), "Children")
            .map(el_count)
            .unwrap_or(0)
    }

    pub fn child_mut(entry: &mut Element, i: usize) -> Option<&mut Element> {
        let data = data_of_mut(entry);
        let kids = prop_mut(data, "Children")?;
        nth_el_mut(kids, i)
    }

    pub fn children_mut(entry: &mut Element) -> Option<&mut Element> {
        let data = data_of_mut(entry);
        prop_mut(data, "Children")
    }

    pub fn find<'a>(entry: &'a Element, ident: &str) -> Vec<&'a Element> {
        let mut found = Vec::new();
        if element_id(entry) == Some(ident) {
            found.push(entry);
        }
        for child in children_of(entry) {
            found.extend(find(child, ident));
        }
        found
    }

    pub fn find_paths(root: &Element, ident: &str) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        fn rec(el: &Element, path: &[usize], ident: &str, out: &mut Vec<Vec<usize>>) {
            if element_id(el) == Some(ident) {
                out.push(path.to_vec());
            }
            if let Some(kids) = prop(data_of(el), "Children") {
                for (i, child) in element_children(kids).enumerate() {
                    let mut p = path.to_vec();
                    p.push(i);
                    rec(child, &p, ident, out);
                }
            }
        }
        rec(root, &[], ident, &mut out);
        out
    }

    pub fn along_children<'a>(root: &'a Element, idxs: &[usize]) -> Option<&'a Element> {
        let mut cur = root;
        for &i in idxs {
            let kids = prop(data_of(cur), "Children")?;
            cur = nth_el(kids, i)?;
        }
        Some(cur)
    }

    pub fn along_children_mut<'a>(root: &'a mut Element, idxs: &[usize]) -> Option<&'a mut Element> {
        let mut cur = root;
        for &i in idxs {
            let data = data_of_mut(cur);
            let kids = prop_mut(data, "Children")?;
            cur = nth_el_mut(kids, i)?;
        }
        Some(cur)
    }

    pub fn subtree_has_id(el: &Element, id: &str) -> bool {
        if attr(el, "name") == Some("ID") && attr(el, "value") == Some(id) {
            return true;
        }
        element_children(el).any(|c| subtree_has_id(c, id))
    }

    pub fn insert_child(parent: &mut Element, at: usize, child: Element) {
        let mut seen = 0usize;
        let mut idx = parent.children.len();
        for (i, n) in parent.children.iter().enumerate() {
            if n.as_element().is_some() {
                if seen == at {
                    idx = i;
                    break;
                }
                seen += 1;
            }
        }
        if seen < at {
            idx = parent.children.len();
        }
        parent.children.insert(idx, XMLNode::Element(child));
    }

    pub fn clear_children(parent: &mut Element) {
        parent.children.clear();
    }

    pub fn append_child(parent: &mut Element, child: Element) {
        parent.children.push(XMLNode::Element(child));
    }

    pub fn set_id(entry: &mut Element, value: &str) -> Result<(), String> {
        let data = data_of_mut(entry);
        let element = prop_mut(data, "Element Data").ok_or("no Element Data")?;
        let ident = prop_mut(element, "ID").ok_or("no ID")?;
        set_value(ident, value.to_string());
        Ok(())
    }

    pub fn set_flag(entry: &mut Element, name: &str, value: &str) {
        let data = data_of_mut(entry);
        if let Some(element) = prop_mut(data, "Element Data") {
            if let Some(flag) = prop_mut(element, name) {
                set_value(flag, value.to_string());
            }
        }
    }

    pub fn pin(entry: &mut Element, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
        {
            let lay = layout_mut(entry)?;
            if let Some(align) = prop_mut(lay, "Align") {
                if let Some(h) = prop_mut(align, "Horizontal") {
                    set_value(h, "Left".into());
                }
                if let Some(v) = prop_mut(align, "Vertical") {
                    set_value(v, "Top".into());
                }
            }
        }
        {
            let lay = layout_mut(entry)?;
            if let Some(ap) = prop_mut(lay, "AnchorPercent") {
                set_value(ap, "false".into());
            }
            for name in ["Width Percentage", "Height Percentage"] {
                if let Some(flag) = prop_mut(lay, name) {
                    set_value(flag, "false".into());
                }
            }
        }
        set_layout_field(entry, "Position X", x)?;
        set_layout_field(entry, "Position Y", y)?;
        set_layout_field(entry, "Width", width)?;
        set_layout_field(entry, "Height", height)?;
        Ok(())
    }
}

use xml::{attr, data_of, element_id, fmt_f, prop, prop_mut, set_box, set_named};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tools;
    use std::path::PathBuf;

    #[test]
    fn page_drops_only_the_picking_layer() {
        let layer = |id: &str| {
            format!(
                "<Property name=\"Children\" value=\"GcNGuiLayerData\"><Property name=\"GcNGuiLayerData\">\
                 <Property name=\"Element Data\" value=\"GcNGuiElementData\"><Property name=\"ID\" value=\"{id}\" />\
                 </Property></Property></Property>"
            )
        };
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Data template=\"cGcNGuiLayerData\">\
             <Property name=\"Children\">{}{}{}</Property></Data>",
            layer("NORMAL"),
            layer("ALT"),
            layer("PICKING")
        );
        let mut doc = Mxml::parse(&text).expect("parse");
        assert_eq!(drop_picking_layer(doc.root_mut()), 1);
        let kids = xml::prop(doc.root(), "Children").unwrap();
        let ids: Vec<&str> = xml::element_children(kids).filter_map(element_id).collect();
        assert_eq!(ids, ["NORMAL", "ALT"]);
        assert_eq!(drop_picking_layer(doc.root_mut()), 0, "older pages have none");
    }

    #[test]
    fn plan_for_numbers() {
        let p = plan_for(1.0);
        assert!(p.vanilla());
        assert_eq!(p.slot, 90.0);
        assert_eq!(p.pitch, 96.0);
        assert_eq!(p.tech_rows, 3);
        assert_eq!(p.cargo_rows, 5);
        assert_eq!(p.tech_h, 290.0);
        assert_eq!(p.cargo_h, 480.0);
        assert_eq!(p.cargo_y, 1070.0);
        assert_eq!(p.panel_w, 1008.0);
        assert_eq!(p.box_w, 1014.0);
        assert_eq!(p.describe(), "slots 90 (100%), tech 3 rows, cargo 5 rows");

        let p = plan_for(0.7);
        assert_eq!(p.slot, 63.0);
        assert_eq!(p.pitch, 69.0);
        assert_eq!(p.tech_rows, 4);
        assert_eq!(p.cargo_rows, 7);
        assert_eq!(p.cargo_y, 1058.0);
        let (slot, pitch, panel_w, rows, window_h, root_w, root_h) = chest_metrics(&p);
        assert_eq!(slot, 52.0);
        assert_eq!(pitch, 58.0);
        assert_eq!(panel_w, 609.0);
        assert_eq!(rows, 10);
        assert_eq!(window_h, 580.0);
        assert_eq!(root_w, 660.0);
        assert_eq!(root_h, 584.0);

        let p = plan_for(0.5);
        assert_eq!(p.slot, 45.0);
        assert_eq!(p.tech_rows, 6);
        assert_eq!(p.cargo_rows, 9);
    }

    #[test]
    fn scale_from_clamps() {
        assert_eq!(scale_from(0.7).unwrap(), 0.7);
        assert!(scale_from(0.4).is_err());
        assert!(scale_from(1.1).is_err());
    }

    #[test]
    fn vanilla_100_without_filters_removes_mod_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "nms-slots-vanilla-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("keep.txt"), b"x").unwrap();
        let tools = Tools {
            mbincompiler: PathBuf::from("missing-compiler"),
            hgpaktool: PathBuf::from("missing-pak"),
        };
        let msg = build_slots(1.0, false, &tmp, Path::new("."), &tools, Path::new("."))
            .expect("vanilla remove");
        assert_eq!(msg, VANILLA_MSG);
        assert!(!tmp.exists());
    }
}
