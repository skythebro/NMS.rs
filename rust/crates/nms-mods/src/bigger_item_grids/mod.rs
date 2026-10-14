//! Bigger Item Grids: more cards per page on Transfer Items and Craft Product (ported from Python).
//!
//! The UI files in the mod folder draw the bigger grid. The game code keeps its own page size
//! (8 items) in the slot filler and the page dots of both screens; this moves all of them to
//! columns * rows, or none if any site fails a check. Applied once at start, like Python.

pub mod sigscan;

use std::path::Path;
use std::sync::Mutex;

use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, FileImage, GameMod, Kind, SettingSpec, Settings};
use serde_json::{json, Value};

use sigscan::ScaleRedirect;

const VANILLA_ITEMS: u32 = 8;
const MAX_ITEMS: u32 = 99;
const SCREENS: &[(&str, &str)] = &[("TRANSFER_RECEIVE_DESC", "Transfer Items dots"), ("PRODUCTS_ALL", "Craft Product dots")];

const SPECS: &[SettingSpec] = &[
    SettingSpec { key: "grid", label: "Grid", kind: Kind::Status },
    SettingSpec { key: "window", label: "Window", kind: Kind::Status },
    SettingSpec { key: "patch", label: "Code patch", kind: Kind::Status },
    SettingSpec { key: "write_logs", label: "Write log", kind: Kind::Bool },
];

struct Preset {
    columns: u32,
    rows: u32,
    window_scale: f64,
}

fn load_preset(dir: &Path) -> Result<Preset, String> {
    if !dir.join("UI").join("POPUP_INVENTORY.MBIN").is_file() {
        return Err("UI\\POPUP_INVENTORY.MBIN is missing: extract the whole mod folder".into());
    }
    let text = std::fs::read_to_string(dir.join("bigger_item_grids_preset.json")).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let int = |k: &str| v.get(k).and_then(Value::as_u64).map(|n| n as u32).ok_or(format!("{k} missing"));
    let (columns, rows) = (int("columns")?, int("rows")?);
    if columns == 0 || rows == 0 || columns * rows > MAX_ITEMS {
        return Err(format!("bigger_item_grids_preset.json has {columns} by {rows} cards; at most {MAX_ITEMS} fit"));
    }
    let window_scale = v.get("window_scale").and_then(Value::as_f64).unwrap_or(1.0);
    Ok(Preset { columns, rows, window_scale })
}

fn plan_all(image: &FileImage, items: u32) -> (Vec<(&'static str, ScaleRedirect)>, Vec<String>) {
    let mut plans = Vec::new();
    let mut failures = Vec::new();
    match sigscan::plan_slot_filler(image, items) {
        Some(p) => plans.push(("Slot filler", p)),
        None => failures.push("slot filler (called from TRANSFER_RECEIVE_DESC, uses SLOT%02d) was not found".to_string()),
    }
    for (anchor, label) in SCREENS {
        match sigscan::plan_scale_redirect(image, anchor, items) {
            Some(p) => plans.push((*label, p)),
            None => failures.push(format!("{label}: anchor {anchor} was not unique")),
        }
    }
    (plans, failures)
}

/// Patch every page-size site, or none if any one fails a check. Returns the status.
fn apply_page_patch(image: &ProcessImage, items: u32, log: &Log) -> Result<String, String> {
    if items == VANILLA_ITEMS {
        log.info("8 items per page, the game default. Nothing patched.");
        return Ok("Not needed".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let file = FileImage::load(&exe)?;
    let (plans, failures) = plan_all(&file, items);
    for reason in &failures {
        log.info(&format!("Left vanilla: {reason}"));
    }
    if !failures.is_empty() {
        return Ok("Off, see log".into());
    }
    for (label, plan) in &plans {
        for (rva, old, _) in &plan.patches {
            if image.read(*rva, old.len()).as_deref() != Some(old.as_slice()) {
                log.info(&format!("Left vanilla: {label} code at 0x{rva:X} did not match NMS.exe"));
                return Ok("Off, see log".into());
            }
        }
    }
    let mut live = image.clone();
    for (_, plan) in &plans {
        live.write(plan.cave_rva, &plan.scale.to_le_bytes())?;
        for (rva, _, new) in &plan.patches {
            live.write(*rva, new)?;
        }
    }
    log.info(&format!("{items} items per page on Transfer Items and Craft Product"));
    Ok("Applied".into())
}

pub struct BiggerItemGrids {
    settings: Mutex<Settings>,
    log: Log,
    preset: Option<Preset>,
    patch: String,
}

impl BiggerItemGrids {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "BiggerItemGrids");
        let settings = Settings::load(&dir.join("bigger_item_grids.json"), json!({"write_logs": false}));
        let log = Log::open(&dir.join("bigger_item_grids.log"), settings.is_not_false("write_logs"));
        let mut patch = "Off, see log".to_string();
        let preset = match load_preset(&dir) {
            Ok(p) => {
                log.info(&format!("Grid {} by {}, window x{}", p.columns, p.rows, p.window_scale));
                match apply_page_patch(&ctx.image, p.columns * p.rows, &log) {
                    Ok(status) => patch = status,
                    Err(e) => log.info(&format!("Page size was left vanilla: {e}")),
                }
                Some(p)
            }
            Err(e) => {
                log.info(&format!("Page size was left vanilla: {e}"));
                None
            }
        };
        Self { settings: Mutex::new(settings), log, preset, patch }
    }
}

impl GameMod for BiggerItemGrids {
    fn name(&self) -> &'static str {
        "BiggerItemGrids"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        let p = self.preset.as_ref();
        match key {
            "grid" => Some(json!(p.map_or("-".into(), |p| format!("{} x {} = {}", p.columns, p.rows, p.columns * p.rows)))),
            "window" => Some(json!(p.map_or("-".into(), |p| format!("x{:.2}", p.window_scale)))),
            "patch" => Some(json!(self.patch)),
            "write_logs" => Some(json!(self.settings.lock().ok()?.is_not_false("write_logs"))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "write_logs" => self.settings.lock().ok()?.default(key).cloned(),
            _ => None,
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        if key != "write_logs" {
            return Err(format!("unknown setting {key:?}"));
        }
        let on = value.as_bool().ok_or("expected true or false")?;
        self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
        self.log.set_enabled(on);
        Ok(format!("Write log {}", if on { "on" } else { "off" }))
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(BiggerItemGrids::new(ctx))
}
